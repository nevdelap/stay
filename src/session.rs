use crate::config::Config;
use crate::relay;
use crate::tmux::{self, Tmux};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

/// Creates a new stay-managed tmux session.
///
/// The session inherits the configured tmux defaults, then starts the chosen
/// command after the explicit command preflight passes.
///
/// # Errors
///
/// Returns an error when tmux cannot be started, the explicit command
/// preflight fails, or tmux reports a failure creating the new session.
pub fn create_session(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    cwd: Option<&str>,
    command_words: &[String],
) -> Result<(), String> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
    let user_tmux_config = dirs::home_dir().map(|home| home.join(".tmux.conf"));
    create_session_with_shell(
        tmux,
        config,
        session_name,
        cwd,
        command_words,
        Path::new(&shell),
        user_tmux_config.as_deref(),
    )
}

/// Creates a new stay-managed tmux session using an explicitly selected shell.
///
/// This is the same operation as [`create_session`], with the shell supplied
/// by the caller instead of read from the process environment. It keeps tests
/// from needing to mutate the process-global `SHELL` variable or depend on the
/// process's home directory.
///
/// # Errors
///
/// Returns an error when tmux cannot be started, the explicit command
/// preflight fails, or tmux reports a failure creating the new session.
pub fn create_session_with_shell(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    cwd: Option<&str>,
    command_words: &[String],
    shell: &Path,
    user_tmux_config: Option<&Path>,
) -> Result<(), String> {
    let command_tail = build_command_tail(config, command_words, shell.as_os_str())?;
    let bootstrap_name = format!(
        "__stay-bootstrap-{}-{}",
        std::process::id(),
        current_timestamp()
    );

    tmux.run([
        "new-session",
        "-d",
        "-s",
        bootstrap_name.as_str(),
        "--",
        "/bin/sh",
        "-c",
        "sleep 1000000",
    ])?;
    let bootstrap_guard = BootstrapGuard {
        tmux: tmux.clone(),
        session_name: bootstrap_name,
    };

    tmux.run(["set-option", "-g", "remain-on-exit", "on"])?;
    let history_limit = config.history_lines.to_string();
    tmux.run(["set-option", "-g", "history-limit", history_limit.as_str()])?;
    if !user_tmux_config_exists(user_tmux_config) {
        apply_builtin_tmux_settings(tmux)?;
    }

    let mut arguments = vec![
        OsString::from("new-session"),
        OsString::from("-d"),
        OsString::from("-s"),
        OsString::from(session_name),
    ];
    if let Some(cwd) = cwd {
        arguments.push(OsString::from("-c"));
        arguments.push(OsString::from(cwd));
    }
    arguments.push(OsString::from("-e"));
    arguments.push(OsString::from(format!("STAY_SESSION_NAME={session_name}")));
    arguments.push(OsString::from("--"));
    arguments.extend(command_tail);

    let output = tmux.run(arguments)?;
    ensure_success(output)?;
    drop(bootstrap_guard);
    Ok(())
}

fn user_tmux_config_exists(path: Option<&Path>) -> bool {
    path.is_some_and(Path::exists)
}

fn apply_builtin_tmux_settings(tmux: &Tmux) -> Result<(), String> {
    let status_right = format!("stay (wrapping tmux) v{} ", env!("CARGO_PKG_VERSION"));
    tmux.run([
        "set-option",
        "-g",
        "status-style",
        "bg=darkblue,fg=white,bold",
    ])?;
    tmux.run(["set-option", "-g", "status-left-length", "200"])?;
    tmux.run([
        "set-option",
        "-g",
        "status-left",
        " #{session_name}  #{pane_current_path}",
    ])?;
    tmux.run(["set-option", "-g", "status-right", status_right.as_str()])?;
    tmux.run(["set-window-option", "-g", "window-status-format", ""])?;
    tmux.run([
        "set-window-option",
        "-g",
        "window-status-current-format",
        "",
    ])?;
    Ok(())
}

/// Kills an existing stay-managed tmux session.
///
/// # Errors
///
/// Returns an error when tmux cannot be started or reports a failure killing
/// the named session.
pub fn kill_session(tmux: &Tmux, session_name: &str) -> Result<(), String> {
    let output = tmux.run(["kill-session", "-t", session_name])?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8(output.stderr)
        .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
    if is_expected_last_session_shutdown(&stderr) {
        return Ok(());
    }

    Err(format_tmux_failure(output.status, &stderr))
}

/// Recreates a session after removing any existing stay-managed session.
///
/// # Errors
///
/// Returns an error when the existing session cannot be removed or the new
/// session cannot be created.
pub fn force_recreate_session(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    cwd: Option<&str>,
    command_words: &[String],
) -> Result<(), String> {
    match kill_session(tmux, session_name) {
        Ok(()) => {}
        Err(error) if is_missing_session_error(&error) => {}
        Err(error) => return Err(error),
    }

    create_session(tmux, config, session_name, cwd, command_words)
}

/// Attaches to an existing session through stay's interactive relay.
///
/// Control calls remain bounded, while the relay's tmux attach child remains
/// alive for the duration of the user's attachment. `read_only` and
/// `low_priority` map onto tmux's `attach-session -f` client flags
/// independently.
///
/// # Errors
///
/// Returns an error when trailing command words were supplied or when the
/// platform cannot allocate the relay PTY.
pub fn attach_session(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    command_words: &[String],
    read_only: bool,
    low_priority: bool,
) -> Result<u8, String> {
    attach_session_with_input(
        tmux,
        config,
        session_name,
        command_words,
        read_only,
        low_priority,
        &[],
    )
}

/// Attaches to an existing session after forwarding input captured during an
/// interactive picker handoff.
///
/// `read_only` and `low_priority` map onto tmux's `attach-session -f` client
/// flags independently.
///
/// # Errors
///
/// Returns an error when trailing command words are supplied or when the
/// relay cannot allocate or operate its attachment PTY.
pub fn attach_session_with_input(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    command_words: &[String],
    read_only: bool,
    low_priority: bool,
    initial_input: &[u8],
) -> Result<u8, String> {
    if !command_words.is_empty() {
        return Err(format!(
            "existing session {session_name:?} cannot be combined with \
             trailing command words; use -f/--force-recreate"
        ));
    }

    relay::attach_with_input(
        tmux,
        config,
        session_name,
        read_only,
        low_priority,
        initial_input,
    )
}

struct BootstrapGuard {
    tmux: Tmux,
    session_name: String,
}

impl Drop for BootstrapGuard {
    fn drop(&mut self) {
        let _ = self
            .tmux
            .run(["kill-session", "-t", self.session_name.as_str()]);
    }
}

fn build_command_tail(
    config: &Config,
    command_words: &[String],
    shell: &OsStr,
) -> Result<Vec<OsString>, String> {
    if command_words.is_empty() {
        return Ok(default_command_tail(config, shell));
    }

    preflight_explicit_command(&command_words[0])?;
    Ok(command_words.iter().cloned().map(OsString::from).collect())
}

fn default_command_tail(config: &Config, shell: &OsStr) -> Vec<OsString> {
    match &config.default_command {
        Some(default_command) => vec![
            shell.to_owned(),
            OsString::from("-c"),
            OsString::from(default_command),
        ],
        None => vec![shell.to_owned()],
    }
}

fn preflight_explicit_command(command: &str) -> Result<(), String> {
    match resolve_command_path(command) {
        Ok(path) => validate_executable(&path)
            .map_err(|reason| format!("command {command:?} is not a regular executable: {reason}")),
        Err(reason) => Err(format!("command {command:?} cannot be executed: {reason}")),
    }
}

fn resolve_command_path(command: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(command);
    if has_path_separator(candidate) {
        return Ok(candidate.to_path_buf());
    }

    let Some(path_variable) = std::env::var_os("PATH") else {
        return Err("PATH is unset".to_owned());
    };

    let mut saw_non_executable = None;
    for directory in std::env::split_paths(&path_variable) {
        let candidate = if directory.as_os_str().is_empty() {
            PathBuf::from(command)
        } else {
            directory.join(command)
        };
        match validate_executable(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(ExecutableError::NotFound) => {}
            Err(ExecutableError::NotExecutable) => {
                saw_non_executable.get_or_insert(candidate);
            }
            Err(ExecutableError::Metadata(error)) => return Err(error),
        }
    }

    if let Some(candidate) = saw_non_executable {
        return Err(format!(
            "{} exists but is not executable",
            candidate.display()
        ));
    }

    Err(format!("{command:?} was not found in PATH"))
}

fn has_path_separator(path: &Path) -> bool {
    path.components().count() > 1
}

enum ExecutableError {
    NotFound,
    NotExecutable,
    Metadata(String),
}

fn validate_executable(path: &Path) -> Result<(), ExecutableError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ExecutableError::NotFound)
        }
        Err(error) => {
            return Err(ExecutableError::Metadata(format!(
                "failed to inspect {}: {error}",
                path.display()
            )))
        }
    };

    if !metadata.is_file() {
        return Err(ExecutableError::NotExecutable);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(ExecutableError::NotExecutable);
        }
    }

    Ok(())
}

impl std::fmt::Display for ExecutableError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "not found"),
            Self::NotExecutable => write!(formatter, "not executable"),
            Self::Metadata(error) => write!(formatter, "{error}"),
        }
    }
}

fn ensure_success(output: crate::tmux::CommandOutput) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8(output.stderr)
        .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
    if stderr.trim().is_empty() {
        Err(format!("tmux command failed with status {}", output.status))
    } else {
        Err(format!(
            "tmux command failed with status {}: {}",
            output.status,
            stderr.trim()
        ))
    }
}

fn format_tmux_failure(status: std::process::ExitStatus, stderr: &str) -> String {
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("tmux command failed with status {status}")
    } else {
        format!("tmux command failed with status {status}: {detail}")
    }
}

fn is_expected_last_session_shutdown(stderr: &str) -> bool {
    // This intentionally matches tmux's English diagnostic. tmux ships no
    // translations today, but a wording change would require updating this
    // classifier. Do not force a C locale here: tmux copies its environment
    // into created sessions.
    stderr.contains("server exited unexpectedly")
}

fn is_missing_session_error(error: &str) -> bool {
    // These English diagnostics are intentionally matched verbatim. tmux
    // ships no translations today, but a wording change would require
    // updating this classifier. Forcing a C locale on the wrapper is
    // deliberately avoided because tmux copies its environment into created
    // sessions.
    error.contains("can't find session")
        || error.contains("no such session")
        || tmux::is_missing_server_error(error)
}

fn current_timestamp() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn config(default_command: &str) -> Config {
        Config {
            default_command: Some(default_command.to_owned()),
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 1234,
        }
    }

    fn temp_script(contents: &str, executable: bool) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("stay-session-{stamp}"));
        fs::write(&path, contents).expect("write temp script");
        if executable {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let mut permissions = fs::metadata(&path).expect("metadata").permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(&path, permissions).expect("mark executable");
            }
        }
        path
    }

    #[test]
    fn default_command_uses_shell_and_configured_command_string() {
        let config = config("echo hi");
        let tail = default_command_tail(&config, OsStr::new("/bin/sh"));
        assert_eq!(tail[1], OsString::from("-c"));
        assert_eq!(tail[2], OsString::from("echo hi"));
    }

    #[test]
    fn no_default_command_uses_the_shell_without_a_command_argument() {
        let config = Config {
            default_command: None,
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 1234,
        };
        let tail = default_command_tail(&config, OsStr::new("/bin/sh"));
        assert_eq!(tail.len(), 1);
        assert!(!tail[0].is_empty());
    }

    #[test]
    fn explicit_command_is_passed_through_after_preflight() {
        let script = temp_script("#!/bin/sh\nexit 0\n", true);
        let command = vec![script.to_string_lossy().into_owned(), "arg one".into()];
        let tail = build_command_tail(&config("ignored"), &command, OsStr::new("/bin/sh")).unwrap();
        assert_eq!(tail[0], OsString::from(script.as_os_str()));
        assert_eq!(tail[1], OsString::from("arg one"));
    }

    #[test]
    fn rejects_missing_or_non_executable_commands() {
        let missing = build_command_tail(
            &config("ignored"),
            &[String::from("definitely-not-stay-executable-12345")],
            OsStr::new("/bin/sh"),
        );
        assert!(missing.is_err());

        let script = temp_script("#!/bin/sh\nexit 0\n", false);
        let non_executable = build_command_tail(
            &config("ignored"),
            &[script.to_string_lossy().into_owned()],
            OsStr::new("/bin/sh"),
        );
        assert!(non_executable.is_err());
        let _ = fs::remove_file(script);
    }

    #[test]
    fn attach_rejects_trailing_command_words_before_exec() {
        let tmux = Tmux::for_test_namespace("stay-test-attach");
        let error = attach_session(
            &tmux,
            &config("ignored"),
            "work",
            &["echo".to_owned()],
            false,
            false,
        )
        .unwrap_err();
        assert!(error.contains("trailing command words"), "{error}");
        assert!(error.contains("-f/--force-recreate"), "{error}");
    }

    #[test]
    fn direct_paths_are_preflighted_without_searching_path() {
        let path = temp_script("#!/bin/sh\nexit 0\n", true);
        assert!(preflight_explicit_command(path.to_str().unwrap()).is_ok());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn user_tmux_config_predicate_only_accepts_existing_paths() {
        let path = temp_script("# tmux settings\n", false);
        let missing = path.with_extension("missing");

        assert!(user_tmux_config_exists(Some(&path)));
        assert!(!user_tmux_config_exists(Some(&missing)));
        assert!(!user_tmux_config_exists(None));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn built_in_tmux_settings_apply_without_a_user_config() {
        let guard = TestServerGuard::new("builtin");
        let config = config("ignored");
        create_session_with_shell(
            &guard.tmux,
            &config,
            "builtin",
            None,
            &[],
            Path::new("/bin/sh"),
            None,
        )
        .unwrap();

        assert_eq!(
            show_global_option(&guard.tmux, "status-style"),
            "bg=darkblue,fg=white,bold"
        );
        assert_eq!(show_global_option(&guard.tmux, "status-left-length"), "200");
        assert_eq!(show_global_option(&guard.tmux, "remain-on-exit"), "on");
        assert_eq!(show_global_option(&guard.tmux, "history-limit"), "1234");
        assert_eq!(
            show_global_option(&guard.tmux, "status-left"),
            " #{session_name}  #{pane_current_path}"
        );
        assert_eq!(
            show_global_option(&guard.tmux, "status-right"),
            format!("stay (wrapping tmux) v{}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(show_window_option(&guard.tmux, "window-status-format"), "");
        assert_eq!(
            show_window_option(&guard.tmux, "window-status-current-format"),
            ""
        );
    }

    #[test]
    fn built_in_tmux_settings_do_not_apply_with_a_user_config() {
        let config_path = temp_script("# user tmux settings\n", false);
        let guard = TestServerGuard::new("user-config");
        let config = config("ignored");
        create_session_with_shell(
            &guard.tmux,
            &config,
            "user-config",
            None,
            &[],
            Path::new("/bin/sh"),
            Some(&config_path),
        )
        .unwrap();

        assert_ne!(
            show_global_option(&guard.tmux, "status-style"),
            "bg=darkblue,fg=white,bold"
        );
        assert_ne!(show_global_option(&guard.tmux, "status-left-length"), "200");
        assert_eq!(show_global_option(&guard.tmux, "remain-on-exit"), "on");
        assert_eq!(show_global_option(&guard.tmux, "history-limit"), "1234");
        assert_ne!(
            show_global_option(&guard.tmux, "status-left"),
            " #{session_name}  #{pane_current_path}"
        );
        assert_ne!(
            show_global_option(&guard.tmux, "status-right"),
            format!("stay (wrapping tmux) v{}", env!("CARGO_PKG_VERSION"))
        );
        let _ = fs::remove_file(config_path);
    }

    struct TestServerGuard {
        tmux: Tmux,
    }

    impl TestServerGuard {
        fn new(suffix: &str) -> Self {
            Self {
                tmux: Tmux::for_test_namespace(unique_test_namespace(suffix)),
            }
        }
    }

    impl Drop for TestServerGuard {
        fn drop(&mut self) {
            let _ = self
                .tmux
                .command(["kill-server"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    fn unique_test_namespace(suffix: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!(
            "stay-test-session-{suffix}-{}-{counter}",
            std::process::id()
        )
    }

    fn show_global_option(tmux: &Tmux, option: &str) -> String {
        let output = tmux.run(["show-options", "-g", "-v", option]).unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }

    fn show_window_option(tmux: &Tmux, option: &str) -> String {
        let output = tmux
            .run(["show-window-options", "-g", "-v", option])
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }
}

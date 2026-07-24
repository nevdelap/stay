use crate::config::Config;
use crate::relay;
use crate::tmux::{self, Tmux};
use std::ffi::OsString;
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
    let command_tail = build_command_tail(config, command_words)?;
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
/// alive for the duration of the user's attachment.
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
) -> Result<u8, String> {
    if !command_words.is_empty() {
        return Err(format!(
            "existing session {session_name:?} cannot be combined with \
             trailing command words; use -f/--force-recreate"
        ));
    }

    relay::attach(tmux, config, session_name)
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

fn build_command_tail(config: &Config, command_words: &[String]) -> Result<Vec<OsString>, String> {
    if command_words.is_empty() {
        return Ok(default_command_tail(config));
    }

    preflight_explicit_command(&command_words[0])?;
    Ok(command_words.iter().cloned().map(OsString::from).collect())
}

fn default_command_tail(config: &Config) -> Vec<OsString> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh"));
    vec![
        shell,
        OsString::from("-c"),
        OsString::from(config.default_command.clone()),
    ]
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
    stderr.contains("server exited unexpectedly")
}

fn is_missing_session_error(error: &str) -> bool {
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
            default_command: default_command.to_owned(),
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
        let tail = default_command_tail(&config);
        assert_eq!(tail[1], OsString::from("-c"));
        assert_eq!(tail[2], OsString::from("echo hi"));
    }

    #[test]
    fn explicit_command_is_passed_through_after_preflight() {
        let script = temp_script("#!/bin/sh\nexit 0\n", true);
        let command = vec![script.to_string_lossy().into_owned(), "arg one".into()];
        let tail = build_command_tail(&config("ignored"), &command).unwrap();
        assert_eq!(tail[0], OsString::from(script.as_os_str()));
        assert_eq!(tail[1], OsString::from("arg one"));
    }

    #[test]
    fn rejects_missing_or_non_executable_commands() {
        let missing = build_command_tail(
            &config("ignored"),
            &[String::from("definitely-not-stay-executable-12345")],
        );
        assert!(missing.is_err());

        let script = temp_script("#!/bin/sh\nexit 0\n", false);
        let non_executable =
            build_command_tail(&config("ignored"), &[script.to_string_lossy().into_owned()]);
        assert!(non_executable.is_err());
        let _ = fs::remove_file(script);
    }

    #[test]
    fn attach_rejects_trailing_command_words_before_exec() {
        let tmux = Tmux::for_test_namespace("stay-test-attach");
        let error =
            attach_session(&tmux, &config("ignored"), "work", &["echo".to_owned()]).unwrap_err();
        assert!(error.contains("trailing command words"), "{error}");
        assert!(error.contains("-f/--force-recreate"), "{error}");
    }

    #[test]
    fn direct_paths_are_preflighted_without_searching_path() {
        let path = temp_script("#!/bin/sh\nexit 0\n", true);
        assert!(preflight_explicit_command(path.to_str().unwrap()).is_ok());
        let _ = fs::remove_file(path);
    }
}

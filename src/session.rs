use crate::config::Config;
use crate::relay;
pub use crate::relay::AttachOptions;
use crate::tmux::{self, Tmux};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write as _;
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
    crate::session_name::parse_session_name(session_name)?;
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
    crate::session_name::parse_session_name(session_name)?;
    let command_tail = build_command_tail(config, command_words, shell.as_os_str())?;
    let tmux_config = TemporaryTmuxConfig::create(user_tmux_config, config.history_lines)?;

    let mut arguments = vec![
        OsString::from("-f"),
        tmux_config.path.clone().into_os_string(),
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
    ensure_success(tmux.run(["set-option", "-g", "remain-on-exit", "on"])?)?;
    let history_limit = config.history_lines.to_string();
    ensure_success(tmux.run(["set-option", "-g", "history-limit", history_limit.as_str()])?)?;
    if !user_tmux_config_exists(user_tmux_config) {
        apply_builtin_tmux_settings(tmux)?;
    }
    Ok(())
}

fn user_tmux_config_exists(path: Option<&Path>) -> bool {
    path.is_some_and(Path::exists)
}

const BUILTIN_STATUS_LEFT: &str = " #{session_name}  #{pane_current_path} \
#{?client_readonly,#{?#{m/r:(^|\\|)ignore-size(\\||$),#{s/,/|:#{client_flags}}},(view only / low priority),(view only)},#{?#{m/r:(^|\\|)ignore-size(\\||$),#{s/,/|:#{client_flags}}},(low priority),}}";

fn apply_builtin_tmux_settings(tmux: &Tmux) -> Result<(), String> {
    let status_right = format!("stay (wrapping tmux) v{} ", env!("CARGO_PKG_VERSION"));
    ensure_success(tmux.run([
        "set-option",
        "-g",
        "status-style",
        "bg=darkblue,fg=white,bold",
    ])?)?;
    ensure_success(tmux.run(["set-option", "-g", "status-left-length", "200"])?)?;
    ensure_success(tmux.run(["set-option", "-g", "status-left", BUILTIN_STATUS_LEFT])?)?;
    ensure_success(tmux.run(["set-option", "-g", "status-right", status_right.as_str()])?)?;
    ensure_success(tmux.run(["set-window-option", "-g", "window-status-format", ""])?)?;
    ensure_success(tmux.run([
        "set-window-option",
        "-g",
        "window-status-current-format",
        "",
    ])?)?;
    Ok(())
}

/// Kills an existing stay-managed tmux session.
///
/// # Errors
///
/// Returns an error when tmux cannot be started or reports a failure killing
/// the named session.
pub fn kill_session(tmux: &Tmux, session_name: &str) -> Result<(), String> {
    crate::session_name::parse_session_name(session_name)?;
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

/// Kills the terminated session identifiers captured by the picker.
///
/// A target may disappear after the picker snapshot and before its kill is
/// attempted; that race is treated as success so the remaining targets are
/// still processed. Other tmux failures stop the operation and are returned.
/// The caller supplies the snapshot, so sessions that terminate later are not
/// added to this operation.
///
/// # Errors
///
/// Returns the first non-race tmux failure encountered while killing a target.
pub fn kill_terminated_sessions(tmux: &Tmux, session_names: &[String]) -> Result<(), String> {
    for session_name in session_names {
        match kill_session(tmux, session_name) {
            Ok(()) => {}
            Err(error) if is_missing_session_error(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Recreates a session after removing any existing stay-managed session.
///
/// If the targeted session is currently terminated, its exit code is
/// printed to stderr first, so force-recreating never silently discards it.
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
    force_recreate_session_inner(tmux, config, session_name, cwd, command_words, true).map(|_| ())
}

/// Recreates a session for the interactive picker without writing a notice to
/// stderr. The returned notice is meant to be rendered in the picker row.
pub(crate) fn force_recreate_session_for_picker(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    cwd: Option<&str>,
    command_words: &[String],
) -> Result<Option<TerminatedRecreateNotice>, String> {
    force_recreate_session_inner(tmux, config, session_name, cwd, command_words, false)
}

fn force_recreate_session_inner(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    cwd: Option<&str>,
    command_words: &[String],
    emit_notice: bool,
) -> Result<Option<TerminatedRecreateNotice>, String> {
    let sessions = tmux.list_sessions()?;
    if let Some(notice) = terminated_recreate_notice(&sessions, session_name)
        && emit_notice
    {
        // quality: intentional-output
        eprintln!("{notice}");
    }

    match kill_session(tmux, session_name) {
        Ok(()) => {}
        Err(error) if is_missing_session_error(&error) => {}
        Err(error) => return Err(error),
    }

    create_session(tmux, config, session_name, cwd, command_words)?;
    Ok(terminated_recreate_notice(&sessions, session_name))
}

/// Returns the stderr notice for a terminated session about to be
/// force-recreated, or `None` when the session doesn't exist or isn't
/// terminated (force-recreating a live or nonexistent session is
/// unchanged).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminatedRecreateNotice {
    session_name: String,
    pub(crate) exit_code: u8,
}

impl TerminatedRecreateNotice {
    pub(crate) fn row_detail(&self) -> String {
        format!(
            "[terminated with exit code {} before recreate]",
            self.exit_code
        )
    }
}

impl std::fmt::Display for TerminatedRecreateNotice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "session {:?} terminated with exit code {} before recreate",
            self.session_name, self.exit_code
        )
    }
}

fn terminated_recreate_notice(
    sessions: &[tmux::SessionRecord],
    session_name: &str,
) -> Option<TerminatedRecreateNotice> {
    let session = sessions
        .iter()
        .find(|session| session.name == session_name)?;
    if session.status_word() != "terminated" {
        return None;
    }
    Some(TerminatedRecreateNotice {
        session_name: session_name.to_owned(),
        exit_code: session.exit_code.unwrap_or(0),
    })
}

/// Attaches to an existing session through stay's interactive relay.
///
/// Control calls remain bounded, while the relay's tmux attach child remains
/// alive for the duration of the user's attachment. See [`AttachOptions`]
/// for the attach modifiers.
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
    options: AttachOptions<'_>,
) -> Result<u8, String> {
    attach_session_with_input(tmux, config, session_name, command_words, options, &[])
}

/// Attaches to an existing session after forwarding input captured during an
/// interactive picker handoff. See [`AttachOptions`] for the attach
/// modifiers.
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
    options: AttachOptions<'_>,
    initial_input: &[u8],
) -> Result<u8, String> {
    if !command_words.is_empty() {
        return Err(format!(
            "existing session {session_name:?} cannot be combined with \
             trailing command words; use -f/--force-recreate"
        ));
    }

    let user_tmux_config = dirs::home_dir().map(|home| home.join(".tmux.conf"));
    if !user_tmux_config_exists(user_tmux_config.as_deref()) {
        apply_builtin_tmux_settings(tmux)?;
    }

    relay::attach_with_input(tmux, config, session_name, options, initial_input)
}

/// Size of each `-p/--pass-through` chunk forwarded to the session.
const PASS_THROUGH_CHUNK_BYTES: usize = 8192;

/// Forwards stay's own stdin into `session_name`'s pane incrementally, via
/// `load-buffer`/`paste-buffer -d`, without ever attaching. Stops at EOF.
///
/// Forwarding happens as each chunk arrives (not buffered until EOF), so a
/// continuous producer (e.g. `tail -f data | stay attach session -p`)
/// delivers input as it is produced.
///
/// # Errors
///
/// Returns an error when reading stdin fails or a tmux control command
/// fails.
pub fn pass_through(tmux: &Tmux, session_name: &str) -> Result<(), String> {
    pass_through_from(tmux, session_name, &mut std::io::stdin().lock())
}

fn pass_through_from<R: std::io::Read>(
    tmux: &Tmux,
    session_name: &str,
    input: &mut R,
) -> Result<(), String> {
    let mut buffer = [0_u8; PASS_THROUGH_CHUNK_BYTES];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| format!("failed to read stdin: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        tmux.paste_stdin_chunk(session_name, &buffer[..read])?;
    }
}

struct TemporaryTmuxConfig {
    path: PathBuf,
}

impl TemporaryTmuxConfig {
    fn create(user_tmux_config: Option<&Path>, history_lines: usize) -> Result<Self, String> {
        use std::io::ErrorKind;

        let contents = tmux_config_contents(user_tmux_config, history_lines);
        for attempt in 0..100_u32 {
            let path = std::env::temp_dir().join(format!(
                "stay-tmux-{}-{}-{attempt}.conf",
                std::process::id(),
                current_timestamp()
            ));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;

                options.mode(0o600);
            }
            let mut file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "failed to create temporary tmux config {}: {error}",
                        path.display()
                    ));
                }
            };

            let config = Self { path };
            if let Err(error) = file.write_all(contents.as_bytes()) {
                drop(config);
                return Err(format!("failed to write temporary tmux config: {error}"));
            }
            return Ok(config);
        }
        Err("failed to create a unique temporary tmux config".to_owned())
    }
}

impl Drop for TemporaryTmuxConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn tmux_config_contents(user_tmux_config: Option<&Path>, history_lines: usize) -> String {
    use std::fmt::Write as _;

    let mut contents = String::new();
    if let Some(path) = user_tmux_config {
        contents.push_str("source-file -q ");
        contents.push_str(&tmux_config_argument(path));
        contents.push('\n');
    }
    contents.push_str("set-option -g remain-on-exit on\n");
    let _ = writeln!(contents, "set-option -g history-limit {history_lines}");
    contents
}

fn tmux_config_argument(path: &Path) -> String {
    let mut argument = String::from("\"");
    for character in path.to_string_lossy().chars() {
        if matches!(character, '\\' | '"' | '$') {
            argument.push('\\');
        }
        argument.push(character);
    }
    argument.push('"');
    argument
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
            return Err(ExecutableError::NotFound);
        }
        Err(error) => {
            return Err(ExecutableError::Metadata(format!(
                "failed to inspect {}: {error}",
                path.display()
            )));
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
    use crate::test_support::TempPath;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn config(default_command: &str) -> Config {
        Config {
            default_command: Some(default_command.to_owned()),
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 1234,
            log_capture_interval_seconds: 5,
        }
    }

    fn temp_script(contents: &str, executable: bool) -> TempPath {
        let path = TempPath::file("stay-session");
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
            log_capture_interval_seconds: 5,
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
            AttachOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("trailing command words"), "{error}");
        assert!(error.contains("-f/--force-recreate"), "{error}");
    }

    #[test]
    fn kill_terminated_sessions_ignores_a_missing_target_and_continues() {
        let log = TempPath::file("stay-kill-all-log");
        let script = format!(
            "printf '%s:%s\\n' \"$2\" \"$4\" >> '{}'; \\
             if test \"$4\" = gone; then printf '%s\\n' \"can't find session\" >&2; exit 1; fi",
            log.display()
        );
        let tmux = Tmux::for_test_shell_script(script);
        let names = vec!["gone".to_owned(), "kept".to_owned()];

        kill_terminated_sessions(&tmux, &names).expect("missing target should be ignored");

        let calls = fs::read_to_string(&log).expect("read kill log");
        assert_eq!(
            calls.lines().collect::<Vec<_>>(),
            ["kill-session:gone", "kill-session:kept"]
        );
        let _ = fs::remove_file(log);
    }

    #[test]
    fn kill_terminated_sessions_surfaces_real_tmux_failures() {
        let script =
            "if test \"$4\" = broken; then printf '%s\\n' 'permission denied' >&2; exit 1; fi";
        let tmux = Tmux::for_test_shell_script(script);
        let names = vec!["broken".to_owned(), "later".to_owned()];

        let error =
            kill_terminated_sessions(&tmux, &names).expect_err("real failure should surface");
        assert!(error.contains("permission denied"), "{error}");
    }

    fn session_creation_surfaces_tmux_failure(script: &str, expected: &str) {
        let tmux = Tmux::for_test_shell_script(script);
        let error = create_session_with_shell(
            &tmux,
            &config("ignored"),
            "failed",
            None,
            &[],
            Path::new("/bin/sh"),
            None,
        )
        .expect_err("tmux failure should stop session creation");
        assert!(error.contains(expected), "{error}");
    }

    #[test]
    fn session_creation_surfaces_remain_on_exit_failure() {
        session_creation_surfaces_tmux_failure(
            "if test \"$2\" = set-option && test \"$4\" = remain-on-exit; then \
             printf '%s\\n' 'remain-on-exit failed' >&2; exit 1; fi",
            "remain-on-exit failed",
        );
    }

    #[test]
    fn session_creation_surfaces_history_limit_failure() {
        session_creation_surfaces_tmux_failure(
            "if test \"$2\" = set-option && test \"$4\" = history-limit; then \
             printf '%s\\n' 'history-limit failed' >&2; exit 1; fi",
            "history-limit failed",
        );
    }

    #[test]
    fn session_creation_surfaces_builtin_setting_failure() {
        session_creation_surfaces_tmux_failure(
            "if test \"$2\" = set-option && test \"$4\" = status-right; then \
             printf '%s\\n' 'status-right failed' >&2; exit 1; fi",
            "status-right failed",
        );
    }

    #[test]
    fn session_operations_reject_invalid_names_before_running_tmux() {
        let marker = TempPath::file("stay-invalid-session-name");
        let script = format!(
            "printf invoked > {}",
            shell_quote(&marker.to_string_lossy())
        );
        let tmux = Tmux::for_test_shell_script(script);
        let config = config("ignored");

        let error = create_session(&tmux, &config, "bad.name", None, &[])
            .expect_err("create_session must reject invalid names");
        assert!(error.contains("invalid session name"), "{error}");
        assert!(!marker.exists(), "create_session invoked tmux");

        let error = create_session_with_shell(
            &tmux,
            &config,
            "bad.name",
            None,
            &[],
            Path::new("/bin/sh"),
            None,
        )
        .expect_err("create_session_with_shell must reject invalid names");
        assert!(error.contains("invalid session name"), "{error}");
        assert!(!marker.exists(), "create_session_with_shell invoked tmux");

        let error =
            kill_session(&tmux, "bad.name").expect_err("kill_session must reject invalid names");
        assert!(error.contains("invalid session name"), "{error}");
        assert!(!marker.exists(), "kill_session invoked tmux");
    }

    #[test]
    fn pass_through_delivers_a_bounded_multiline_chunk_in_order_without_attaching() {
        let guard = TestServerGuard::new("passthrough");
        let root = TempPath::directory("stay-passthrough-marker");
        let marker = root.path().join("received.txt");
        // Reads exactly three lines into a marker file, sidestepping the
        // pane's own terminal echo (which would otherwise show the input
        // twice: once from the pty's canonical-mode echo, once from a
        // command that itself echoes stdin) and letting this test check
        // "delivered once, in order" directly.
        let script = format!(
            "for i in 1 2 3; do IFS= read -r line; printf '%s\\n' \"$line\" >> {}; done; sleep 30",
            shell_quote(&marker.to_string_lossy())
        );
        let status = guard
            .tmux
            .command([
                "new-session",
                "-d",
                "-s",
                "target",
                "--",
                "sh",
                "-c",
                &script,
            ])
            .status()
            .expect("create pass-through target session");
        assert!(status.success());

        let mut input = std::io::Cursor::new(b"first\nsecond\nthird\n".to_vec());
        pass_through_from(&guard.tmux, "target", &mut input).expect("pass input through");

        let mut content = String::new();
        for _ in 0..150 {
            if let Ok(read) = fs::read_to_string(&marker) {
                content = read;
                if content.lines().count() >= 3 {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(content, "first\nsecond\nthird\n");

        assert!(
            !guard
                .tmux
                .list_sessions()
                .expect("list sessions")
                .iter()
                .any(|session| session.attached),
            "pass-through must never attach"
        );
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn session_record(name: &str, terminated: bool, exit_code: Option<u8>) -> tmux::SessionRecord {
        tmux::SessionRecord {
            name: name.to_owned(),
            attached: false,
            created: 0,
            terminated,
            exit_code,
            dead_signal: None,
            dead_time: terminated.then_some(0),
            current_directory: None,
            current_command: None,
        }
    }

    #[test]
    fn terminated_recreate_notice_names_the_session_and_exit_code() {
        let sessions = [session_record("work", true, Some(7))];
        let notice = terminated_recreate_notice(&sessions, "work").expect("expected a notice");
        let notice = notice.to_string();
        assert!(notice.contains("\"work\""), "{notice}");
        assert!(notice.contains("exit code 7"), "{notice}");
        let row_detail = terminated_recreate_notice(&sessions, "work")
            .expect("expected a row detail")
            .row_detail();
        assert_eq!(row_detail, "[terminated with exit code 7 before recreate]");
    }

    #[test]
    fn terminated_recreate_notice_defaults_a_missing_exit_code_to_zero() {
        let sessions = [session_record("work", true, None)];
        let notice = terminated_recreate_notice(&sessions, "work").expect("expected a notice");
        let notice = notice.to_string();
        assert!(notice.contains("exit code 0"), "{notice}");
        assert_eq!(
            terminated_recreate_notice(&sessions, "work")
                .expect("expected a row detail")
                .row_detail(),
            "[terminated with exit code 0 before recreate]"
        );
    }

    #[test]
    fn terminated_recreate_notice_is_none_for_a_live_or_missing_session() {
        let sessions = [session_record("work", false, None)];
        assert!(terminated_recreate_notice(&sessions, "work").is_none());
        assert!(terminated_recreate_notice(&sessions, "missing").is_none());
        assert!(terminated_recreate_notice(&[], "work").is_none());
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
            BUILTIN_STATUS_LEFT
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
    fn session_creation_does_not_leave_a_bootstrap_session() {
        let guard = TestServerGuard::new("no-bootstrap");
        create_session_with_shell(
            &guard.tmux,
            &config("ignored"),
            "real-session",
            None,
            &[],
            Path::new("/bin/sh"),
            None,
        )
        .unwrap();

        let output = guard
            .tmux
            .command(["list-sessions", "-F", "#{session_name}"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            ["real-session"]
        );
    }

    #[test]
    fn user_tmux_config_settings_survive_session_creation() {
        let config_path = TempPath::file("stay-user-tmux-config");
        fs::write(&config_path, "set-option -g @stay-user-option user-value\n").unwrap();
        let guard = TestServerGuard::new("user-option");

        create_session_with_shell(
            &guard.tmux,
            &config("ignored"),
            "user-option",
            None,
            &[],
            Path::new("/bin/sh"),
            Some(&config_path),
        )
        .unwrap();

        assert_eq!(
            show_global_option(&guard.tmux, "@stay-user-option"),
            "user-value"
        );
    }

    #[test]
    fn temporary_tmux_config_is_owner_only_and_removed_on_drop() {
        let config = TemporaryTmuxConfig::create(None, 1234).unwrap();
        let path = config.path.clone();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(config);
        assert!(!path.exists());
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
            BUILTIN_STATUS_LEFT
        );
        assert_ne!(
            show_global_option(&guard.tmux, "status-right"),
            format!("stay (wrapping tmux) v{}", env!("CARGO_PKG_VERSION"))
        );
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

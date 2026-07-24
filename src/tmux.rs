use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Deadline for short-lived tmux control commands.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

const PRODUCTION_NAMESPACE: &str = "stay";

/// A tmux server namespace and the command boundary used to access it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tmux {
    namespace: String,
    program: std::ffi::OsString,
    prefix_arguments: Vec<std::ffi::OsString>,
}

/// A parsed stay-managed tmux session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecord {
    pub name: String,
    pub attached: bool,
    pub created: u64,
}

impl SessionRecord {
    /// Returns the plain-list marker for this session.
    #[must_use]
    pub fn marker(&self) -> char {
        if self.attached {
            'a'
        } else {
            'd'
        }
    }
}

/// Renders a session inventory in stay's plain-list format.
#[must_use]
pub fn render_session_inventory(sessions: &[SessionRecord]) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    for session in sessions {
        let _ = writeln!(output, "{}\t{}", session.marker(), session.name);
    }
    output
}

#[derive(Debug)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Tmux {
    /// Creates the production wrapper. Production dispatch cannot select a
    /// different tmux server namespace.
    #[must_use]
    pub fn production() -> Self {
        Self {
            namespace: PRODUCTION_NAMESPACE.to_owned(),
            program: "tmux".into(),
            prefix_arguments: Vec::new(),
        }
    }

    /// Creates a wrapper for an isolated test namespace.
    ///
    /// The namespace must begin with `stay-test-`; production dispatch uses
    /// [`Tmux::production`] and never accepts a namespace from its callers.
    ///
    /// # Panics
    ///
    /// Panics when the namespace does not begin with `stay-test-`.
    #[must_use]
    pub fn for_test_namespace(namespace: impl Into<String>) -> Self {
        let namespace = namespace.into();
        assert!(
            namespace.starts_with("stay-test-"),
            "test namespaces must begin with stay-test-"
        );
        Self {
            namespace,
            program: "tmux".into(),
            prefix_arguments: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_shell_script(script: impl Into<std::ffi::OsString>) -> Self {
        Self {
            namespace: "stay-test-program".to_owned(),
            program: "/bin/sh".into(),
            prefix_arguments: vec!["-c".into(), script.into()],
        }
    }

    /// Builds a tmux command with this wrapper's namespace.
    #[must_use]
    pub fn command<I, S>(&self, arguments: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new(&self.program);
        command
            .args(&self.prefix_arguments)
            .arg("-L")
            .arg(&self.namespace)
            .args(arguments);
        command
    }

    /// Builds the attach command used by wrapper-argument tests.
    ///
    /// Production attachments use [`Tmux::attach_program_and_arguments`]
    /// because the relay owns the attach child's PTY.
    #[must_use]
    pub fn attach_command(&self, session_name: &str) -> Command {
        self.command(["attach-session", "-t", session_name])
    }

    /// Detaches the client attached to a session.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be started or rejects the target.
    pub fn detach_client(&self, session_name: &str) -> Result<(), String> {
        ensure_command_success(self.run(["detach-client", "-s", session_name])?)
    }

    /// Enters tmux copy mode for the named session.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be started or rejects the target.
    pub fn copy_mode(&self, session_name: &str) -> Result<(), String> {
        ensure_command_success(self.run(["copy-mode", "-t", session_name])?)
    }

    /// Returns the retained pane exit status, or `None` while it is alive.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux returns malformed status data or another
    /// control-command failure.
    pub fn pane_exit_status(&self, session_name: &str) -> Result<Option<u8>, String> {
        let output = self.run([
            "list-panes",
            "-t",
            session_name,
            "-F",
            "#{pane_dead}:#{pane_dead_status}",
        ])?;
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)
                .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
            // These English diagnostics are intentionally matched verbatim.
            // tmux ships no translations today, but changing its wording
            // would require updating this classifier.  Forcing a C locale on
            // the wrapper is deliberately avoided because tmux copies its
            // environment into created sessions.
            if is_missing_server_error(&stderr) || stderr.contains("can't find session") {
                return Ok(None);
            }
            return Err(format_tmux_failure(output.status, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| "tmux list-panes returned invalid UTF-8".to_owned())?;
        let row = stdout
            .lines()
            .next()
            .ok_or_else(|| "tmux returned no pane status".to_owned())?;
        let (dead, status) = row
            .split_once(':')
            .ok_or_else(|| format!("malformed tmux pane status: {row:?}"))?;
        if dead != "1" {
            return Ok(None);
        }
        let status = status
            .parse::<i16>()
            .map_err(|_| format!("invalid tmux pane exit status: {row:?}"))?;
        if !(0..=255).contains(&status) {
            return Err(format!("invalid tmux pane exit status: {row:?}"));
        }
        let status =
            u8::try_from(status).map_err(|_| format!("invalid tmux pane exit status: {row:?}"))?;
        Ok(Some(status))
    }

    /// Returns the executable and separate arguments for a relay child.
    #[must_use]
    pub(crate) fn attach_program_and_arguments(
        &self,
        session_name: &str,
    ) -> (std::ffi::OsString, Vec<std::ffi::OsString>) {
        let mut arguments = self.prefix_arguments.clone();
        arguments.extend([
            std::ffi::OsString::from("-L"),
            self.namespace.clone().into(),
            "attach-session".into(),
            "-t".into(),
            session_name.into(),
        ]);
        (self.program.clone(), arguments)
    }

    /// Lists sessions from this wrapper's tmux server.
    ///
    /// A server that has not started yet is an empty inventory.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be started, times out, returns an
    /// unexpected failure, or emits malformed session data.
    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, String> {
        let output = self.run([
            "list-sessions",
            "-F",
            "#{session_name}:#{session_attached}:#{session_created}",
        ])?;

        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)
                .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
            if is_missing_server_error(&stderr) {
                return Ok(Vec::new());
            }
            return Err(format_tmux_failure(output.status, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| "tmux list-sessions returned invalid UTF-8".to_owned())?;
        let mut sessions = stdout
            .lines()
            .map(parse_session_row)
            .collect::<Result<Vec<_>, _>>()?;
        sessions.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.created.cmp(&right.created))
        });
        Ok(sessions)
    }

    /// Runs a tmux command and captures its output.
    ///
    /// The command is bounded by the same timeout used for tmux list and
    /// version checks.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be spawned, times out, or the child
    /// process itself cannot be waited on cleanly.
    pub fn run<I, S>(&self, arguments: I) -> Result<CommandOutput, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut child = self
            .command(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to start tmux: {error}"))?;

        wait_with_timeout(&mut child, COMMAND_TIMEOUT)
    }
}

fn ensure_command_success(output: CommandOutput) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8(output.stderr)
        .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
    Err(format_tmux_failure(output.status, &stderr))
}

fn parse_session_row(row: &str) -> Result<SessionRecord, String> {
    let mut fields = row.split(':');
    let name = fields
        .next()
        .ok_or_else(|| "tmux session row is missing its name".to_owned())?;
    let attached = fields
        .next()
        .ok_or_else(|| format!("tmux session row is missing attachment count: {row:?}"))?
        .parse::<u32>()
        .map_err(|_| format!("invalid tmux attachment count in row: {row:?}"))?;
    let created = fields
        .next()
        .ok_or_else(|| format!("tmux session row is missing creation time: {row:?}"))?
        .parse::<u64>()
        .map_err(|_| format!("invalid tmux creation time in row: {row:?}"))?;
    if fields.next().is_some() || name.is_empty() {
        return Err(format!("malformed tmux session row: {row:?}"));
    }
    Ok(SessionRecord {
        name: name.to_owned(),
        attached: attached > 0,
        created,
    })
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Result<CommandOutput, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = read_pipe(child.stdout.take())?;
                let stderr = read_pipe(child.stderr.take())?;
                return Ok(CommandOutput {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                terminate(child);
                return Err(format!(
                    "tmux command timed out after {} seconds; tmux may be unresponsive",
                    timeout.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                terminate(child);
                return Err(format!("failed while waiting for tmux: {error}"));
            }
        }
    }
}

fn read_pipe<R: Read>(pipe: Option<R>) -> Result<Vec<u8>, String> {
    let Some(mut pipe) = pipe else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)
        .map_err(|error| format!("failed to read tmux output: {error}"))?;
    Ok(output)
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) fn is_missing_server_error(stderr: &str) -> bool {
    // This intentionally matches tmux's English diagnostics. tmux ships no
    // translations today, but a wording change would require updating this
    // classifier. Do not force a C locale here: tmux copies its environment
    // into created sessions.
    stderr.contains("no server running")
        || stderr.contains("no sessions")
        || stderr.contains("server exited unexpectedly")
        || stderr.contains("error connecting") && stderr.contains("No such file or directory")
}

fn format_tmux_failure(status: ExitStatus, stderr: &str) -> String {
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("tmux command failed with status {status}")
    } else {
        format!("tmux command failed with status {status}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_namespace() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let pid = std::process::id();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("stay-test-{pid}-{nanos}-{counter}")
    }

    struct ServerGuard {
        tmux: Tmux,
    }

    impl ServerGuard {
        fn new() -> Self {
            Self {
                tmux: Tmux::for_test_namespace(unique_namespace()),
            }
        }
    }

    impl Drop for ServerGuard {
        fn drop(&mut self) {
            let _ = self
                .tmux
                .command(["kill-server"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    #[test]
    fn production_namespace_is_fixed_and_test_namespace_is_injected() {
        let production = Tmux::production().command(["list-sessions"]);
        assert_eq!(production.get_args().collect::<Vec<_>>()[1], "stay");

        let test = Tmux::for_test_namespace("stay-test-example").command(["list-sessions"]);
        assert_eq!(test.get_args().collect::<Vec<_>>()[1], "stay-test-example");
    }

    #[test]
    fn attach_command_uses_the_injected_namespace_and_separate_target() {
        let tmux = Tmux::for_test_namespace("stay-test-attach");
        let command = tmux.attach_command("work space");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                std::ffi::OsStr::new("-L"),
                std::ffi::OsStr::new("stay-test-attach"),
                std::ffi::OsStr::new("attach-session"),
                std::ffi::OsStr::new("-t"),
                std::ffi::OsStr::new("work space"),
            ]
        );
    }

    #[test]
    fn relay_attach_argv_includes_argv_zero_and_injected_namespace() {
        let tmux = Tmux::for_test_namespace("stay-test-relay");
        let (program, arguments) = tmux.attach_program_and_arguments("work space");
        assert_eq!(program, std::ffi::OsString::from("tmux"));
        assert_eq!(
            arguments,
            [
                std::ffi::OsString::from("-L"),
                std::ffi::OsString::from("stay-test-relay"),
                std::ffi::OsString::from("attach-session"),
                std::ffi::OsString::from("-t"),
                std::ffi::OsString::from("work space"),
            ]
        );
    }

    #[test]
    fn parses_and_derives_session_rows() {
        assert_eq!(
            parse_session_row("work space:2:42"),
            Ok(SessionRecord {
                name: "work space".to_owned(),
                attached: true,
                created: 42,
            })
        );
        assert_eq!(parse_session_row("idle:0:43").unwrap().marker(), 'd');
    }

    #[test]
    fn rejects_malformed_session_rows() {
        for row in ["", "name", "name:attached:42", "name:0", ":0:42"] {
            assert!(parse_session_row(row).is_err(), "accepted {row:?}");
        }
    }

    #[test]
    fn sorts_by_name_then_creation_time() {
        let mut sessions = [
            SessionRecord {
                name: "zeta".to_owned(),
                attached: false,
                created: 1,
            },
            SessionRecord {
                name: "alpha".to_owned(),
                attached: false,
                created: 9,
            },
            SessionRecord {
                name: "alpha".to_owned(),
                attached: true,
                created: 2,
            },
        ];
        sessions.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.created.cmp(&right.created))
        });
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.created)
                .collect::<Vec<_>>(),
            [2, 9, 1]
        );
    }

    #[test]
    fn missing_server_is_an_empty_inventory() {
        let guard = ServerGuard::new();
        assert!(guard.tmux.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn real_tmux_inventory_is_sorted() {
        let guard = ServerGuard::new();
        let first = guard
            .tmux
            .command(["new-session", "-d", "-s", "zeta", "--", "sleep", "10"])
            .status()
            .expect("start first test session");
        assert!(first.success());
        let second = guard
            .tmux
            .command(["new-session", "-d", "-s", "alpha", "--", "sleep", "10"])
            .status()
            .expect("start second test session");
        assert!(second.success());

        let sessions = guard.tmux.list_sessions().unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert!(sessions.iter().all(|session| !session.attached));
    }

    #[test]
    fn timeout_terminates_a_wedged_command() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1"]);
        let mut child = command.spawn().expect("spawn sleep");
        let error = wait_with_timeout(&mut child, Duration::from_millis(20))
            .expect_err("wedged command should time out");
        assert!(error.contains("timed out"), "{error}");
        assert!(
            child
                .try_wait()
                .expect("check timed-out child status")
                .is_some(),
            "timed-out child was not reaped"
        );
    }

    #[test]
    fn reports_non_missing_tmux_failures() {
        let error = Tmux::for_test_shell_script("exit 1")
            .list_sessions()
            .expect_err("failing command must fail");
        assert!(error.contains("tmux command failed"), "{error}");
    }

    #[test]
    fn rejects_invalid_utf8_from_tmux_stdout_and_stderr() {
        let stdout_error = Tmux::for_test_shell_script("printf '\\377'")
            .list_sessions()
            .expect_err("invalid stdout must fail");
        assert!(stdout_error.contains("invalid UTF-8"), "{stdout_error}");

        let stderr_error = Tmux::for_test_shell_script("printf '\\377' >&2; exit 1")
            .list_sessions()
            .expect_err("invalid stderr must fail");
        assert!(stderr_error.contains("invalid UTF-8"));
    }

    #[test]
    fn wrapper_timeout_reaps_the_child() {
        let tmux = Tmux::for_test_shell_script("exec sleep 3");
        let started = Instant::now();
        let error = tmux
            .list_sessions()
            .expect_err("sleeping command must time out");
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_millis(2500));
    }
}

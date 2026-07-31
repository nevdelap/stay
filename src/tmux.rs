use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs;
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use crate::session_name::parse_session_name;
use serde::Serialize;
use unicode_width::UnicodeWidthStr;

/// Deadline for short-lived tmux control commands.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

const PRODUCTION_NAMESPACE: &str = "stay";

/// Buffer name used for `-p/--pass-through` chunks, kept stay-specific so
/// it can never collide with a buffer the user's own tmux usage creates.
const PASSTHROUGH_BUFFER_NAME: &str = "stay-passthrough";

/// The result of sweeping stale test-server sockets.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TestServerSweepReport {
    /// Namespaces whose live servers were terminated.
    pub killed_live: Vec<String>,
    /// Namespaces whose dead socket files were removed.
    pub removed_dead: Vec<String>,
}

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
    pub terminated: bool,
    pub exit_code: Option<u8>,
    pub dead_time: Option<u64>,
    pub current_directory: Option<String>,
    pub current_command: Option<String>,
}

impl SessionRecord {
    /// Returns the status word for this session.
    #[must_use]
    pub fn status_word(&self) -> &'static str {
        if self.terminated {
            "terminated"
        } else if self.attached {
            "attached"
        } else {
            "detached"
        }
    }

    /// Returns structured suffix spans shared by plain and picker listings.
    #[must_use]
    pub fn status_detail(&self) -> Vec<SuffixSpan> {
        if !self.terminated {
            return vec![SuffixSpan {
                text: format!(" [{}]", self.status_word()),
                emphasis: false,
            }];
        }

        let exit_code = self.exit_code.unwrap_or(0);
        vec![
            SuffixSpan {
                text: " [terminated exit=".to_owned(),
                emphasis: false,
            },
            SuffixSpan {
                text: exit_code.to_string(),
                emphasis: exit_code != 0,
            },
            SuffixSpan {
                text: format!(" @{}]", format_dead_time(self.dead_time.unwrap_or(0))),
                emphasis: false,
            },
        ]
    }
}

/// A rendered session suffix segment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuffixSpan {
    pub text: String,
    pub emphasis: bool,
}

/// Renders a session inventory in stay's plain-list format.
#[must_use]
pub fn render_session_inventory(sessions: &[SessionRecord], colour: bool) -> String {
    use std::fmt::Write as _;

    let name_width = sessions
        .iter()
        .map(|session| UnicodeWidthStr::width(session.name.as_str()))
        .max()
        .unwrap_or(0);
    let mut output = String::new();
    for session in sessions {
        let _ = write!(output, "{}", session.name);
        let name_padding = name_width.saturating_sub(UnicodeWidthStr::width(session.name.as_str()));
        let _ = write!(output, "{}", " ".repeat(name_padding));
        for span in session.status_detail() {
            if colour && span.emphasis {
                let _ = write!(output, "\x1b[31m{}\x1b[0m", span.text);
            } else {
                let _ = write!(output, "{}", span.text);
            }
        }
        output.push('\n');
    }
    output
}

/// A stable, machine-readable session row.
#[derive(Clone, Debug, Serialize)]
pub struct JsonSession {
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub current_directory: Option<String>,
    pub current_command: Option<String>,
    pub terminated_at: Option<String>,
    pub exit_code: Option<u8>,
}

/// The stable envelope emitted by `stay list --json`.
#[derive(Clone, Debug, Serialize)]
pub struct JsonEnvelope {
    pub sessions: Vec<JsonSession>,
}

impl From<&SessionRecord> for JsonSession {
    fn from(session: &SessionRecord) -> Self {
        Self {
            name: session.name.clone(),
            status: session.status_word().to_owned(),
            created_at: format_utc_timestamp(session.created),
            current_directory: session.current_directory.clone(),
            current_command: session.current_command.clone(),
            terminated_at: session
                .terminated
                .then_some(session.dead_time)
                .flatten()
                .map(format_utc_timestamp),
            exit_code: session.exit_code,
        }
    }
}

/// Renders a deterministic JSON inventory without ANSI decoration.
#[must_use]
pub fn render_session_json(sessions: &[SessionRecord]) -> String {
    use std::fmt::Write as _;

    let mut ordered = sessions.to_vec();
    ordered.sort_by(|left, right| {
        left.created
            .cmp(&right.created)
            .then_with(|| left.name.cmp(&right.name))
    });
    let envelope = JsonEnvelope {
        sessions: ordered.iter().map(JsonSession::from).collect(),
    };

    let mut output = String::from("{\"sessions\":[");
    for (index, session) in envelope.sessions.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push('{');
        write_json_string_field(&mut output, "name", &session.name, true);
        write_json_string_field(&mut output, "status", &session.status, false);
        write_json_string_field(&mut output, "created_at", &session.created_at, false);
        write_json_optional_string_field(
            &mut output,
            "current_directory",
            session.current_directory.as_deref(),
            false,
        );
        write_json_optional_string_field(
            &mut output,
            "current_command",
            session.current_command.as_deref(),
            false,
        );
        write_json_optional_string_field(
            &mut output,
            "terminated_at",
            session.terminated_at.as_deref(),
            false,
        );
        write!(&mut output, ",\"exit_code\":").expect("writing to a String cannot fail");
        match session.exit_code {
            Some(code) => write!(&mut output, "{code}").expect("writing to a String cannot fail"),
            None => output.push_str("null"),
        }
        output.push('}');
    }
    output.push_str("]}\n");
    output
}

fn write_json_string_field(output: &mut String, name: &str, value: &str, first: bool) {
    if !first {
        output.push(',');
    }
    output.push('"');
    escape_json_string(output, name);
    output.push_str("\":\"");
    escape_json_string(output, value);
    output.push('"');
}

fn write_json_optional_string_field(
    output: &mut String,
    name: &str,
    value: Option<&str>,
    first: bool,
) {
    if !first {
        output.push(',');
    }
    output.push('"');
    escape_json_string(output, name);
    output.push_str("\":");
    match value {
        Some(value) => {
            output.push('"');
            escape_json_string(output, value);
            output.push('"');
        }
        None => output.push_str("null"),
    }
}

fn escape_json_string(output: &mut String, value: &str) {
    use std::fmt::Write as _;

    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0C}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1F}' => {
                write!(output, "\\u{:04x}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => output.push(character),
        }
    }
}

fn format_dead_time(dead_time: u64) -> String {
    use time::format_description::well_known::Rfc3339;
    use time::{OffsetDateTime, UtcOffset};

    let seconds = i64::try_from(dead_time).unwrap_or(i64::MAX);
    let timestamp =
        OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let local = UtcOffset::local_offset_at(timestamp)
        .map_or(timestamp, |offset| timestamp.to_offset(offset));
    local
        .format(&Rfc3339)
        .unwrap_or_else(|_| local.unix_timestamp().to_string())
}

fn format_utc_timestamp(seconds: u64) -> String {
    use time::format_description::well_known::Rfc3339;
    use time::{OffsetDateTime, UtcOffset};

    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    let timestamp =
        OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    timestamp
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .unwrap_or_else(|_| timestamp.unix_timestamp().to_string())
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
        sweep_orphaned_test_servers_once()
            .unwrap_or_else(|error| panic!("failed to sweep orphaned test servers: {error}"));
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

    /// Detaches the client whose tmux process has `client_pid`.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be started, the client cannot be
    /// resolved, or tmux rejects the target.
    pub fn detach_client(&self, session_name: &str, client_pid: i32) -> Result<(), String> {
        let output = self.run([
            "list-clients",
            "-t",
            session_name,
            "-F",
            "#{client_pid}:#{client_tty}",
        ])?;
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)
                .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
            return Err(format_tmux_failure(output.status, &stderr));
        }

        let client_target = find_client_target(&output.stdout, client_pid)?;
        ensure_command_success(self.run(["detach-client", "-t", client_target.as_str()])?)
    }

    /// Enters tmux copy mode for the named session.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be started or rejects the target.
    pub fn copy_mode(&self, session_name: &str) -> Result<(), String> {
        ensure_command_success(self.run(["copy-mode", "-t", session_name])?)
    }

    /// Renames an existing stay-managed tmux session.
    ///
    /// # Errors
    ///
    /// Returns an error when either name is invalid, tmux cannot be started,
    /// or tmux rejects the rename.
    pub fn rename_session(&self, session_name: &str, new_name: &str) -> Result<(), String> {
        let session_name = parse_session_name(session_name)?;
        let new_name = parse_session_name(new_name)?;
        ensure_command_success(self.run([
            "rename-session",
            "-t",
            session_name.as_str(),
            new_name.as_str(),
        ])?)
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
        if status.is_empty() {
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
    ///
    /// `read_only` and `low_priority` map independently onto tmux's
    /// `attach-session -f` client flags (`read-only` and `ignore-size`)
    /// rather than tmux's bundled `-r` shorthand, which always sets both.
    /// When neither flag is set, the resulting argv is byte-identical to
    /// the plain attach argv used before these modifiers existed.
    #[must_use]
    pub(crate) fn attach_program_and_arguments(
        &self,
        session_name: &str,
        read_only: bool,
        low_priority: bool,
    ) -> (std::ffi::OsString, Vec<std::ffi::OsString>) {
        let mut arguments = self.prefix_arguments.clone();
        arguments.extend([
            std::ffi::OsString::from("-L"),
            self.namespace.clone().into(),
            "attach-session".into(),
            "-t".into(),
            session_name.into(),
        ]);
        if let Some(flags) = attach_client_flags(read_only, low_priority) {
            arguments.push("-f".into());
            arguments.push(flags.into());
        }
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
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{session_attached}:#{session_created}:#{pane_dead}:#{pane_dead_status}:#{pane_dead_time}:#{pane_id}",
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
            .map_err(|_| "tmux list-panes returned invalid UTF-8".to_owned())?;
        let mut grouped = BTreeMap::<String, SessionAccumulator>::new();
        let panes = stdout
            .lines()
            .map(parse_session_row)
            .map(|pane| pane.and_then(|pane| self.enrich_pane(pane)))
            .collect::<Result<Vec<_>, _>>()?;
        for pane in panes {
            let session = grouped
                .entry(pane.name.clone())
                .or_insert_with(|| SessionAccumulator {
                    attached: pane.attached,
                    created: pane.created,
                    live_panes: 0,
                    most_recent_dead: None,
                    current_directory: None,
                    current_command: None,
                });
            if pane.dead {
                let dead_time = pane.dead_time.unwrap_or(0);
                if session
                    .most_recent_dead
                    .as_ref()
                    .is_none_or(|current| dead_time >= current.dead_time.unwrap_or(0))
                {
                    session.most_recent_dead = Some(DeadPane {
                        exit_code: pane.exit_code,
                        dead_time: pane.dead_time,
                        current_command: pane.current_command.clone(),
                    });
                }
            } else {
                session.live_panes += 1;
                if session.current_directory.is_none() {
                    session
                        .current_directory
                        .clone_from(&pane.current_directory);
                }
                if session.current_command.is_none() {
                    session.current_command.clone_from(&pane.current_command);
                }
            }
        }

        let mut sessions = grouped
            .into_iter()
            .map(|(name, session)| {
                let terminated = session.live_panes == 0;
                let (exit_code, dead_time) = if terminated {
                    session
                        .most_recent_dead
                        .as_ref()
                        .map_or((None, None), |dead| (dead.exit_code, dead.dead_time))
                } else {
                    (None, None)
                };
                let current_command = if terminated {
                    session
                        .most_recent_dead
                        .and_then(|dead| dead.current_command)
                } else {
                    session.current_command
                };
                SessionRecord {
                    name,
                    attached: session.attached,
                    created: session.created,
                    terminated,
                    exit_code,
                    dead_time,
                    current_directory: if terminated {
                        None
                    } else {
                        session.current_directory
                    },
                    current_command,
                }
            })
            .collect::<Vec<_>>();
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

    /// Runs a tmux command, feeding `input` to its stdin before waiting.
    ///
    /// `input` is written in full and the pipe is then closed (EOF), so a
    /// command reading its whole stdin (e.g. `load-buffer ... -`) sees a
    /// bounded, complete write rather than blocking on a still-open pipe.
    /// Bounded and short-lived like [`Tmux::run`], not a long-lived child.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be spawned, the input cannot be
    /// written, times out, or the child process cannot be waited on cleanly.
    pub(crate) fn run_with_stdin<I, S>(
        &self,
        arguments: I,
        input: &[u8],
    ) -> Result<CommandOutput, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut child = self
            .command(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to start tmux: {error}"))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open tmux stdin".to_owned())?;
        let write_result = stdin.write_all(input);
        drop(stdin);

        // Reap the child regardless of whether the write succeeded, so a
        // failed write never leaves a zombie behind; the wait's own error
        // (if any) takes priority, then the write's.
        let output = wait_with_timeout(&mut child, COMMAND_TIMEOUT)?;
        write_result.map_err(|error| format!("failed to write tmux stdin: {error}"))?;
        Ok(output)
    }

    /// Loads `chunk` into stay's own pass-through buffer, then immediately
    /// pastes it into `session_name`'s pane and deletes the buffer.
    ///
    /// The buffer name is stay-specific so this can never collide with a
    /// buffer the user's own tmux usage might create.
    ///
    /// # Errors
    ///
    /// Returns an error when tmux cannot be started, times out, or rejects
    /// either command.
    pub(crate) fn paste_stdin_chunk(&self, session_name: &str, chunk: &[u8]) -> Result<(), String> {
        ensure_command_success(self.run_with_stdin(
            [
                "load-buffer",
                "-b",
                PASSTHROUGH_BUFFER_NAME,
                "-t",
                session_name,
                "-",
            ],
            chunk,
        )?)?;
        ensure_command_success(self.run([
            "paste-buffer",
            "-b",
            PASSTHROUGH_BUFFER_NAME,
            "-t",
            session_name,
            "-d",
        ])?)
    }

    fn pane_value(&self, pane_id: &str, format: &str) -> Result<Option<String>, String> {
        let output = self.run(["display-message", "-p", "-t", pane_id, format])?;
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)
                .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
            if is_missing_server_error(&stderr) || stderr.contains("can't find pane") {
                return Ok(None);
            }
            return Err(format_tmux_failure(output.status, &stderr));
        }

        let value = String::from_utf8(output.stdout)
            .map_err(|_| "tmux display-message returned invalid UTF-8".to_owned())?;
        let value = value.strip_suffix('\n').unwrap_or(&value);
        Ok((!value.is_empty()).then(|| value.to_owned()))
    }

    fn enrich_pane(&self, pane: PaneRecord) -> Result<PaneRecord, String> {
        let current_directory = self.pane_value(&pane.pane_id, "#{pane_current_path}")?;
        let current_command = self.pane_value(&pane.pane_id, "#{pane_current_command}")?;
        Ok(PaneRecord {
            current_directory,
            current_command,
            ..pane
        })
    }
}

/// Sweep live or dead sockets left by an abnormal test-process exit.
///
/// Only Unix tmux socket entries whose names begin with `stay-test-` are
/// inspected. A responding server is killed through `kill-server`; a socket
/// that reports tmux's missing-server diagnostic is removed as stale. Other
/// probe failures are left untouched for a later run to diagnose.
///
/// # Errors
///
/// Returns an error when the socket directory cannot be read, tmux cannot be
/// started, or a socket cannot be removed.
pub fn sweep_orphaned_test_servers() -> Result<TestServerSweepReport, String> {
    #[cfg(unix)]
    {
        sweep_orphaned_test_servers_unix()
    }

    #[cfg(not(unix))]
    {
        Ok(TestServerSweepReport::default())
    }
}

fn sweep_orphaned_test_servers_once() -> Result<(), String> {
    use std::sync::OnceLock;

    static SWEEP: OnceLock<Result<(), String>> = OnceLock::new();
    SWEEP
        .get_or_init(|| sweep_orphaned_test_servers().map(|_| ()))
        .clone()
}

#[cfg(unix)]
fn sweep_orphaned_test_servers_unix() -> Result<TestServerSweepReport, String> {
    let directory = tmux_socket_directory();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TestServerSweepReport::default());
        }
        Err(error) => {
            return Err(format!(
                "failed to read tmux socket directory {}: {error}",
                directory.display()
            ));
        }
    };
    let mut report = TestServerSweepReport::default();
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to inspect tmux socket: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("failed to inspect tmux socket: {error}"))?
            .is_socket()
        {
            continue;
        }
        let namespace = entry.file_name();
        let Some(namespace) = namespace.to_str() else {
            continue;
        };
        if !namespace.starts_with("stay-test-") {
            continue;
        }

        let Ok(probe) = run_namespace_tmux(namespace, ["list-sessions"]) else {
            continue;
        };
        if probe.status.success() {
            let Ok(killed) = run_namespace_tmux(namespace, ["kill-server"]) else {
                continue;
            };
            if killed.status.success() {
                report.killed_live.push(namespace.to_owned());
                remove_socket(&entry.path(), namespace)?;
            }
        } else if is_missing_server_error(&String::from_utf8_lossy(&probe.stderr)) {
            remove_socket(&entry.path(), namespace)?;
            report.removed_dead.push(namespace.to_owned());
        }
    }
    Ok(report)
}

#[cfg(unix)]
fn tmux_socket_directory() -> PathBuf {
    let root = std::env::var_os("TMUX_TMPDIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    root.join(format!("tmux-{}", nix::unistd::Uid::current().as_raw()))
}

#[cfg(unix)]
fn run_namespace_tmux<I, S>(namespace: &str, arguments: I) -> Result<CommandOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut child = Command::new("tmux")
        .arg("-L")
        .arg(namespace)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start tmux: {error}"))?;
    wait_with_timeout(&mut child, COMMAND_TIMEOUT)
}

#[cfg(unix)]
fn remove_socket(path: &std::path::Path, namespace: &str) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to remove stale tmux socket {namespace}: {error}"
        )),
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

struct SessionAccumulator {
    attached: bool,
    created: u64,
    live_panes: usize,
    most_recent_dead: Option<DeadPane>,
    current_directory: Option<String>,
    current_command: Option<String>,
}

struct DeadPane {
    exit_code: Option<u8>,
    dead_time: Option<u64>,
    current_command: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
struct PaneRecord {
    name: String,
    pane_id: String,
    attached: bool,
    created: u64,
    dead: bool,
    exit_code: Option<u8>,
    dead_time: Option<u64>,
    current_directory: Option<String>,
    current_command: Option<String>,
}

fn parse_session_row(row: &str) -> Result<PaneRecord, String> {
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
    let dead = fields
        .next()
        .ok_or_else(|| format!("tmux pane row is missing its dead flag: {row:?}"))?;
    let dead = match dead {
        "0" => false,
        "1" => true,
        value => {
            return Err(format!(
                "invalid tmux pane dead flag in row: {row:?}: {value:?}"
            ));
        }
    };
    let exit_code = parse_optional_field(fields.next(), row, "exit code")?;
    let dead_time = parse_optional_field(fields.next(), row, "dead time")?;
    let pane_id = fields
        .next()
        .ok_or_else(|| format!("tmux pane row is missing its pane ID: {row:?}"))?;
    if fields.next().is_some() || name.is_empty() || pane_id.is_empty() {
        return Err(format!("malformed tmux session row: {row:?}"));
    }
    Ok(PaneRecord {
        name: name.to_owned(),
        pane_id: pane_id.to_owned(),
        attached: attached > 0,
        created,
        dead,
        exit_code: exit_code
            .map(str::parse::<u8>)
            .transpose()
            .map_err(|_| format!("invalid tmux pane exit code in row: {row:?}"))?,
        dead_time: dead_time
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| format!("invalid tmux pane dead time in row: {row:?}"))?,
        current_directory: None,
        current_command: None,
    })
}

fn parse_optional_field<'a>(
    field: Option<&'a str>,
    row: &str,
    name: &str,
) -> Result<Option<&'a str>, String> {
    let field = field.ok_or_else(|| format!("tmux pane row is missing its {name}: {row:?}"))?;
    Ok((!field.is_empty()).then_some(field))
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

/// Maps the two independent attach modifiers onto tmux's `-f` client flags.
///
/// tmux's own `-r` shorthand always bundles `read-only,ignore-size`; stay
/// sets each flag independently so a low-priority read-write client and a
/// full-priority read-only client are both representable.
fn attach_client_flags(read_only: bool, low_priority: bool) -> Option<&'static str> {
    match (read_only, low_priority) {
        (true, true) => Some("read-only,ignore-size"),
        (true, false) => Some("read-only"),
        (false, true) => Some("ignore-size"),
        (false, false) => None,
    }
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

fn find_client_target(output: &[u8], client_pid: i32) -> Result<String, String> {
    let output = String::from_utf8(output.to_vec())
        .map_err(|_| "tmux list-clients returned invalid UTF-8".to_owned())?;
    let expected_pid = client_pid.to_string();
    let mut target = None;
    for row in output.lines() {
        let mut fields = row.split(':');
        let pid = fields
            .next()
            .ok_or_else(|| format!("malformed tmux client row: {row:?}"))?;
        let client_target = fields
            .next()
            .ok_or_else(|| format!("malformed tmux client row: {row:?}"))?;
        if fields.next().is_some() || client_target.is_empty() {
            return Err(format!("malformed tmux client row: {row:?}"));
        }
        if pid == expected_pid {
            if target.is_some() {
                return Err(format!(
                    "multiple tmux clients found for attach PID {client_pid}"
                ));
            }
            target = Some(client_target.to_owned());
        }
    }

    target.ok_or_else(|| format!("tmux client for attach PID {client_pid} was not found"))
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
        let (program, arguments) = tmux.attach_program_and_arguments("work space", false, false);
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
    fn relay_attach_argv_omits_flag_argument_when_no_modifier_is_set() {
        let tmux = Tmux::for_test_namespace("stay-test-relay");
        let (_, plain) = tmux.attach_program_and_arguments("work", false, false);
        assert!(!plain.iter().any(|argument| argument == "-f"));
    }

    #[test]
    fn relay_attach_argv_maps_read_only_to_the_read_only_client_flag() {
        let tmux = Tmux::for_test_namespace("stay-test-relay");
        let (_, arguments) = tmux.attach_program_and_arguments("work", true, false);
        assert_eq!(
            arguments[arguments.len() - 2..],
            [
                std::ffi::OsString::from("-f"),
                std::ffi::OsString::from("read-only"),
            ]
        );
    }

    #[test]
    fn relay_attach_argv_maps_low_priority_to_the_ignore_size_client_flag() {
        let tmux = Tmux::for_test_namespace("stay-test-relay");
        let (_, arguments) = tmux.attach_program_and_arguments("work", false, true);
        assert_eq!(
            arguments[arguments.len() - 2..],
            [
                std::ffi::OsString::from("-f"),
                std::ffi::OsString::from("ignore-size"),
            ]
        );
    }

    #[test]
    fn relay_attach_argv_composes_both_client_flags_independently() {
        let tmux = Tmux::for_test_namespace("stay-test-relay");
        let (_, arguments) = tmux.attach_program_and_arguments("work", true, true);
        assert_eq!(
            arguments[arguments.len() - 2..],
            [
                std::ffi::OsString::from("-f"),
                std::ffi::OsString::from("read-only,ignore-size"),
            ]
        );
    }

    #[test]
    fn detach_client_resolves_the_attach_pid_to_one_client_target() {
        let tmux = Tmux::for_test_shell_script(
            "if [ \"$2\" = \"list-clients\" ]; then printf '42:/dev/pts/9\\n'; exit 0; fi; \
             if [ \"$2\" = \"detach-client\" ] && [ \"$3\" = \"-t\" ] && [ \"$4\" = \"/dev/pts/9\" ]; then exit 0; fi; \
             exit 9",
        );
        tmux.detach_client("work", 42)
            .expect("resolved client should be detached");
    }

    #[test]
    fn detach_client_does_not_fall_back_to_detaching_the_session() {
        let tmux = Tmux::for_test_shell_script(
            "if [ \"$2\" = \"list-clients\" ]; then printf '41:/dev/pts/8\\n'; exit 0; fi; \
             printf 'detach-client unexpectedly invoked\\n' >&2; exit 9",
        );
        let error = tmux
            .detach_client("work", 42)
            .expect_err("an unknown attach PID must fail");
        assert!(error.contains("attach PID 42 was not found"), "{error}");
    }

    #[test]
    fn parses_and_derives_session_rows() {
        assert_eq!(
            parse_session_row("work space:2:42:0:::%0"),
            Ok(PaneRecord {
                name: "work space".to_owned(),
                pane_id: "%0".to_owned(),
                attached: true,
                created: 42,
                dead: false,
                exit_code: None,
                dead_time: None,
                current_directory: None,
                current_command: None,
            })
        );
        assert_eq!(parse_session_row("idle:0:43:0:::%1").unwrap().name, "idle");
        assert_eq!(
            parse_session_row("dead:0:44:1:7:12345:%2").unwrap(),
            PaneRecord {
                name: "dead".to_owned(),
                pane_id: "%2".to_owned(),
                attached: false,
                created: 44,
                dead: true,
                exit_code: Some(7),
                dead_time: Some(12345),
                current_directory: None,
                current_command: None,
            }
        );
    }

    #[test]
    fn status_word_prioritizes_termination_then_attachment() {
        let mut session = SessionRecord {
            name: "job".to_owned(),
            attached: true,
            created: 0,
            terminated: true,
            exit_code: Some(1),
            dead_time: Some(1),
            current_directory: None,
            current_command: None,
        };
        assert_eq!(session.status_word(), "terminated");

        session.terminated = false;
        assert_eq!(session.status_word(), "attached");

        session.attached = false;
        assert_eq!(session.status_word(), "detached");
    }

    #[test]
    fn renders_json_with_stable_fields_statuses_and_order() {
        let sessions = [
            SessionRecord {
                name: "terminated".to_owned(),
                attached: false,
                created: 2,
                terminated: true,
                exit_code: Some(7),
                dead_time: Some(10),
                current_directory: None,
                current_command: Some("make".to_owned()),
            },
            SessionRecord {
                name: "zeta".to_owned(),
                attached: false,
                created: 1,
                terminated: false,
                exit_code: None,
                dead_time: None,
                current_directory: Some("/tmp".to_owned()),
                current_command: Some("vim".to_owned()),
            },
            SessionRecord {
                name: "alpha".to_owned(),
                attached: true,
                created: 1,
                terminated: false,
                exit_code: None,
                dead_time: None,
                current_directory: Some("/workspace".to_owned()),
                current_command: Some("bash".to_owned()),
            },
        ];

        assert_eq!(
            render_session_json(&sessions),
            concat!(
                "{\"sessions\":[",
                "{\"name\":\"alpha\",\"status\":\"attached\",",
                "\"created_at\":\"1970-01-01T00:00:01Z\",",
                "\"current_directory\":\"/workspace\",",
                "\"current_command\":\"bash\",\"terminated_at\":null,\"exit_code\":null},",
                "{\"name\":\"zeta\",\"status\":\"detached\",",
                "\"created_at\":\"1970-01-01T00:00:01Z\",",
                "\"current_directory\":\"/tmp\",\"current_command\":\"vim\",",
                "\"terminated_at\":null,\"exit_code\":null},",
                "{\"name\":\"terminated\",\"status\":\"terminated\",",
                "\"created_at\":\"1970-01-01T00:00:02Z\",",
                "\"current_directory\":null,\"current_command\":\"make\",",
                "\"terminated_at\":\"1970-01-01T00:00:10Z\",\"exit_code\":7}",
                "]}\n"
            )
        );
    }

    #[test]
    fn formats_termination_time_using_the_recorded_local_offset() {
        use time::format_description::well_known::Rfc3339;
        use time::{OffsetDateTime, UtcOffset};

        let dst_boundary = 1_615_704_000;
        for seconds in [dst_boundary - 1, dst_boundary, dst_boundary + 1] {
            let timestamp = OffsetDateTime::from_unix_timestamp(seconds).unwrap();
            let expected = UtcOffset::local_offset_at(timestamp)
                .map_or(timestamp, |offset| timestamp.to_offset(offset))
                .format(&Rfc3339)
                .unwrap();
            assert_eq!(format_dead_time(u64::try_from(seconds).unwrap()), expected);
        }
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
                terminated: false,
                exit_code: None,
                dead_time: None,
                current_directory: None,
                current_command: None,
            },
            SessionRecord {
                name: "alpha".to_owned(),
                attached: false,
                created: 9,
                terminated: false,
                exit_code: None,
                dead_time: None,
                current_directory: None,
                current_command: None,
            },
            SessionRecord {
                name: "alpha".to_owned(),
                attached: true,
                created: 2,
                terminated: false,
                exit_code: None,
                dead_time: None,
                current_directory: None,
                current_command: None,
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
    fn rename_rejects_invalid_names_before_running_tmux() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let error = tmux
            .rename_session("old", "bad:name")
            .expect_err("invalid new name must be rejected");
        assert!(error.contains("invalid session name"), "{error}");
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

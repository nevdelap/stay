//! Attach-mode logging: `-l/--log`, `-t/--truncate`, `--raw`.
//!
//! Default (clean) mode restricts every capture to tmux's history range
//! (`-E -1`), never the volatile visible screen. A bounded overlap anchor
//! derived from one atomic capture identifies the append point even when the
//! retained history window moves. `--raw` instead opens
//! a continuous `pipe-pane` stream, which keeps producing output while the
//! session is detached.
//!
//! Clean mode re-captures the whole retained range on every tick and finds
//! the already-captured overlap locally, rather than querying
//! `#{history_size}` separately and addressing a later capture with a
//! relative `-N` offset: a separate history-size query is racy against a
//! still-growing pane, because `-N` is relative to whatever is retained at
//! the moment `capture-pane` itself runs, which can have moved past what
//! the earlier, separate query observed. Re-capturing the full range every
//! time is accepted for this atomicity, and is only affordable because
//! `Tmux::run` no longer deadlocks on output larger than the OS pipe
//! capacity (`wait_with_timeout` in `src/tmux.rs` drains both pipes
//! concurrently with the wait, rather than after it).
//!
//! Honest gap: tmux has no hook or event for "a `remain-on-exit` pane's
//! command exited" (confirmed against `tmux show-hooks -g`'s full list of
//! recognized hook names, and by observing a registered `pane-exited`
//! hook — not a real tmux event name — never fire); only a persistent
//! daemon watching every logged session could react to that moment
//! immediately, which conflicts with stay's no-daemon design. Clean mode's
//! final increment for a session that terminates while nobody is attached
//! is therefore captured on the next attach (or force-recreate), not at
//! the moment the command exits — the same trade-off already accepted for
//! the periodic-while-unattended gap, extended to cover termination too.

#[cfg(unix)]
mod unix {
    use crate::tmux::Tmux;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::ExitStatus;

    /// Resolves a `-l` path against `cwd` and validates the log target.
    ///
    /// A relative path resolves against the invoking client's own working
    /// directory, not stay's (they are the same process, but this is the
    /// caller's cwd, never the tmux session's `pane_current_path`). When
    /// the resolved target already exists, it is canonicalized so aliased
    /// spellings of the same file (a `..`-relative path, a symlinked parent
    /// directory) collapse to one path — the single resolved target every
    /// capture in this attach reuses, rather than re-resolving (and
    /// re-racing a symlink swap) on every capture.
    ///
    /// # Errors
    ///
    /// Returns an error when a pre-existing target is a symlink, is not a
    /// regular file, is not owned by the current user, or grants any
    /// group/other permission bit.
    pub fn resolve_log_path(raw_path: &str, cwd: &Path) -> Result<PathBuf, String> {
        let candidate = Path::new(raw_path);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            cwd.join(candidate)
        };
        validate_log_target(&joined)?;
        Ok(fs::canonicalize(&joined).unwrap_or(joined))
    }

    fn validate_log_target(path: &Path) -> Result<(), String> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "failed to check log target {}: {error}",
                    path.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "log target {} is a symlink; refusing to log through it",
                path.display()
            ));
        }
        if !metadata.is_file() {
            return Err(format!(
                "log target {} is not a regular file",
                path.display()
            ));
        }
        if metadata.uid() != nix::unistd::Uid::current().as_raw() {
            return Err(format!(
                "log target {} is not owned by the current user",
                path.display()
            ));
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(format!(
                "log target {} grants group or other permissions",
                path.display()
            ));
        }
        Ok(())
    }

    /// A `-l`-logged attach's live state: either a `--raw` `pipe-pane`
    /// stream or a clean-mode incremental/truncating `capture-pane` cycle.
    pub struct LogSession {
        path: PathBuf,
        mode: Mode,
        warned: bool,
    }

    enum Mode {
        Clean { truncate: bool },
        Raw,
    }

    impl LogSession {
        /// Resolves, validates, and opens logging for one attach.
        ///
        /// Both modes back-fill everything currently retained before their
        /// ongoing mechanism starts, so the log reads as complete from
        /// session start (bounded only by whatever `history-limit` already
        /// evicted) — but only the *first* time: `--raw`'s backfill only
        /// runs when the pane has no pipe already active (see
        /// [`pane_has_active_pipe`]), since re-running it against an
        /// already-piping session would truncate away everything that pipe
        /// has appended since. `--raw` then hands ongoing capture to a
        /// server-side `pipe-pane`; clean mode relies on the caller driving
        /// [`LogSession::on_attach_open`], [`LogSession::on_tick`], and
        /// [`LogSession::on_detach`] at the relay's boundaries (there is no
        /// tmux hook for an unattended session's command exiting — see the
        /// module-level doc comment).
        ///
        /// # Errors
        ///
        /// Returns an error when the path fails validation or a tmux
        /// control command fails. A failure writing the log itself is
        /// non-fatal and surfaces once via [`LogSession::on_tick`] et al.
        pub fn start(
            tmux: &Tmux,
            session_name: &str,
            raw_path: &str,
            cwd: &Path,
            truncate: bool,
            raw: bool,
        ) -> Result<Self, String> {
            let path = resolve_log_path(raw_path, cwd)?;
            let mut session = Self {
                path,
                mode: if raw {
                    Mode::Raw
                } else {
                    Mode::Clean { truncate }
                },
                warned: false,
            };
            if raw {
                // A second `--raw` attach against a session that already has
                // an active pipe must not repeat the backfill: `write_full`
                // truncates, which would destroy everything the running
                // pipe has appended beyond tmux's own (usually much
                // smaller) history retention. The pipe itself is always
                // replaced below so a newly requested path takes effect.
                if !pane_has_active_pipe(tmux, session_name)? {
                    let dump = run_capture_pane(tmux, session_name, "-", "-", true)?;
                    if let Err(error) = write_full(&session.path, &dump) {
                        session.warn_once(&write_failure_message(&session.path, &error));
                    }
                }
                start_pipe_pane(tmux, session_name, &session.path)?;
            } else if !truncate {
                // A generously raised limit makes eviction rare in practice,
                // without changing the anchor-based correctness guarantee.
                raise_history_limit(tmux, session_name)?;
            }
            Ok(session)
        }

        /// Runs the one-shot capture due when an attach opens.
        ///
        /// # Errors
        ///
        /// Returns an error when a tmux control command fails.
        pub fn on_attach_open(&mut self, tmux: &Tmux, session_name: &str) -> Result<(), String> {
            self.tick(tmux, session_name)
        }

        /// Runs the periodic capture due while a client stays attached.
        ///
        /// # Errors
        ///
        /// Returns an error when a tmux control command fails.
        pub fn on_tick(&mut self, tmux: &Tmux, session_name: &str) -> Result<(), String> {
            self.tick(tmux, session_name)
        }

        /// Runs the one-shot capture due when the relay is about to detach.
        ///
        /// # Errors
        ///
        /// Returns an error when a tmux control command fails.
        pub fn on_detach(&mut self, tmux: &Tmux, session_name: &str) -> Result<(), String> {
            self.tick(tmux, session_name)
        }

        fn tick(&mut self, tmux: &Tmux, session_name: &str) -> Result<(), String> {
            match self.mode {
                Mode::Raw => {
                    // No relay-driven capture is needed: pipe-pane already
                    // streams server-side. This only re-verifies the log
                    // target is still accepting writes, so a removed or
                    // now-unwritable path still surfaces the one-time
                    // warning this task requires.
                    if let Err(error) = OpenOptions::new().append(true).open(&self.path) {
                        let message = format!(
                            "log target {} is no longer writable: {error}",
                            self.path.display()
                        );
                        self.warn_once(&message);
                    }
                    Ok(())
                }
                Mode::Clean { truncate } => {
                    let warning = capture_once(tmux, session_name, &self.path, truncate)?;
                    if let Some(warning) = warning {
                        self.warn_once(&warning);
                    }
                    Ok(())
                }
            }
        }

        fn warn_once(&mut self, message: &str) {
            if !self.warned {
                self.warned = true;
                eprintln!("stay: {message}");
            }
        }
    }

    /// Runs one capture against `log_path`, invoked by the relay's own
    /// boundary/periodic ticks.
    ///
    /// Returns `Ok(Some(message))` when the tmux side of the capture
    /// succeeded but the local file write failed (a non-fatal condition the
    /// caller surfaces once); a tmux control-command failure is a genuine
    /// error and is returned as `Err`.
    ///
    /// # Errors
    ///
    /// Returns an error when a tmux control command fails.
    fn capture_once(
        tmux: &Tmux,
        session_name: &str,
        path: &Path,
        truncate: bool,
    ) -> Result<Option<String>, String> {
        if truncate {
            let dump = run_capture_pane(tmux, session_name, "-", "-", false)?;
            return Ok(write_full(path, &dump)
                .err()
                .map(|error| write_failure_message(path, &error)));
        }

        // A single atomic `capture-pane` call. Querying `#{history_size}`
        // separately and then addressing the capture with a relative `-N`
        // offset would be racy against a still-growing pane: `-N` is relative
        // to the CURRENT bottom of the retained window at the moment
        // capture-pane itself runs, which can have moved past what the
        // earlier query observed. Requesting the full retained range every
        // time and finding the previously captured overlap in Rust has no
        // such window.
        let dump = run_capture_pane(tmux, session_name, "-", "-1", false)?;
        let current_lines = count_lines(&dump);
        let cursor = read_cursor(path, session_name);
        let mut warning = None;

        let (payload, dump_offset, fallback, previous_lines, previous_anchor) = match cursor {
            CursorState::Missing => (dump.clone(), 0, false, 0, None),
            CursorState::Invalid => (fallback_payload(&dump), 0, true, 0, None),
            CursorState::Valid(cursor) => match cursor.anchor.as_deref() {
                Some(anchor) => match unique_subslice(&dump, anchor) {
                    Some(offset) => (
                        dump[offset + anchor.len()..].to_vec(),
                        offset + anchor.len(),
                        false,
                        cursor.line_count,
                        cursor.anchor,
                    ),
                    None => (
                        fallback_payload(&dump),
                        0,
                        true,
                        cursor.line_count,
                        cursor.anchor,
                    ),
                },
                None => (
                    fallback_payload(&dump),
                    0,
                    true,
                    cursor.line_count,
                    cursor.anchor,
                ),
            },
        };

        let append_result = append_bytes(path, &payload);
        let append_succeeded = append_result.is_ok();
        let (captured_lines, anchor_dump) = match append_result {
            Ok(_) => (current_lines, dump.as_slice()),
            Err(error) => {
                warning = Some(write_failure_message(path, &error.error));
                // A partial append may end in the middle of a line. Keep the
                // metadata conservative so the next capture retries rather
                // than treating an unrecorded byte as durable progress.
                if error.written == 0 {
                    (previous_lines, &[] as &[u8])
                } else {
                    let durable_payload = &payload[..error.written];
                    let durable_dump_end = if fallback {
                        durable_payload
                            .strip_prefix(EVICTION_MARKER.as_bytes())
                            .map_or(0, <[u8]>::len)
                    } else {
                        dump_offset + durable_payload.len()
                    };
                    let durable_dump_end = durable_dump_end.min(dump.len());
                    let durable_dump = &dump[..durable_dump_end];
                    (count_lines(durable_dump), durable_dump)
                }
            }
        };

        let anchor = if append_succeeded {
            make_anchor(anchor_dump)
        } else if anchor_dump.is_empty() {
            previous_anchor
        } else {
            None
        };
        if let Err(error) = write_cursor(path, session_name, captured_lines, anchor) {
            warning = warning.or_else(|| Some(cursor_failure_message(path, &error)));
        }
        Ok(warning)
    }

    // A capture runs at most a few times a second; a dedicated
    // byte-counting crate isn't worth a new dependency for this.
    #[allow(clippy::naive_bytecount)]
    fn count_lines(dump: &[u8]) -> u64 {
        dump.iter().filter(|&&byte| byte == b'\n').count() as u64
    }

    const MAX_ANCHOR_LINES: usize = 64;
    const MAX_ANCHOR_BYTES: usize = 8192;
    const EVICTION_MARKER: &str = "--- history evicted before capture ---\n";

    fn fallback_payload(dump: &[u8]) -> Vec<u8> {
        let mut payload = EVICTION_MARKER.as_bytes().to_vec();
        payload.extend_from_slice(dump);
        payload
    }

    fn make_anchor(dump: &[u8]) -> Option<Vec<u8>> {
        let mut line_starts = vec![0];
        for (index, &byte) in dump.iter().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        let complete_lines = line_starts.len().checked_sub(1)?;
        if complete_lines == 0 {
            return None;
        }
        let newest_end = line_starts[complete_lines];
        let newest_start = line_starts[complete_lines - 1];
        if newest_end - newest_start > MAX_ANCHOR_BYTES {
            return None;
        }

        let mut start = newest_start;
        let mut selected_lines = 1;
        while selected_lines < MAX_ANCHOR_LINES && selected_lines < complete_lines {
            let previous_start = line_starts[complete_lines - selected_lines - 1];
            if newest_end - previous_start > MAX_ANCHOR_BYTES {
                break;
            }
            start = previous_start;
            selected_lines += 1;
        }
        Some(dump[start..newest_end].to_vec())
    }

    fn unique_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return None;
        }
        let mut match_offset = None;
        for (offset, window) in haystack.windows(needle.len()).enumerate() {
            if window == needle {
                if match_offset.is_some() {
                    return None;
                }
                match_offset = Some(offset);
            }
        }
        match_offset
    }

    fn write_failure_message(path: &Path, error: &io::Error) -> String {
        format!("failed to write log {}: {error}", path.display())
    }

    struct AppendError {
        error: io::Error,
        written: usize,
    }

    fn append_bytes(path: &Path, bytes: &[u8]) -> Result<usize, AppendError> {
        if bytes.is_empty() {
            return Ok(0);
        }
        // `.mode(0o600)` only applies when this call actually creates the
        // file, but that is exactly the case that matters: it keeps a
        // freshly created log passing this module's own no-group/other-bits
        // validation on the next attach, regardless of the process umask.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| AppendError { error, written: 0 })?;
        let mut written = 0;
        while written < bytes.len() {
            match file.write(&bytes[written..]) {
                Ok(0) => {
                    return Err(AppendError {
                        error: io::Error::new(io::ErrorKind::WriteZero, "write returned zero"),
                        written,
                    });
                }
                Ok(count) => written += count,
                Err(error) => return Err(AppendError { error, written }),
            }
        }
        Ok(written)
    }

    fn write_full(path: &Path, bytes: &[u8]) -> io::Result<()> {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(bytes)
    }

    fn offset_sidecar_path(path: &Path) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(".offset");
        PathBuf::from(name)
    }

    struct StoredCursor {
        session_name: String,
        log_size: u64,
        line_count: u64,
        anchor: Option<Vec<u8>>,
    }

    enum CursorState {
        Missing,
        Invalid,
        Valid(StoredCursor),
    }

    fn current_log_size(path: &Path) -> u64 {
        fs::metadata(path).map_or(0, |metadata| metadata.len())
    }

    fn parse_cursor(contents: &str) -> Option<StoredCursor> {
        let mut lines = contents.lines();
        let session_name = lines.next()?.strip_prefix("session=")?.to_owned();
        let log_size = lines.next()?.strip_prefix("log_size=")?.parse().ok()?;
        let line_count = lines.next()?.strip_prefix("line_count=")?.parse().ok()?;
        let anchor = match lines.next()?.strip_prefix("anchor=")? {
            "none" => None,
            encoded if !encoded.is_empty() => Some(decode_hex(encoded)?),
            _ => return None,
        };
        Some(StoredCursor {
            session_name,
            log_size,
            line_count,
            anchor,
        })
    }

    fn decode_hex(encoded: &str) -> Option<Vec<u8>> {
        if !encoded.len().is_multiple_of(2) {
            return None;
        }
        (0..encoded.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).ok())
            .collect()
    }

    fn encode_hex(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        })
    }

    fn read_cursor(path: &Path, session_name: &str) -> CursorState {
        let sidecar = offset_sidecar_path(path);
        if validate_log_target(&sidecar).is_err() {
            return if fs::symlink_metadata(&sidecar).is_ok() {
                CursorState::Invalid
            } else {
                CursorState::Missing
            };
        }
        let contents = match fs::read_to_string(sidecar) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return CursorState::Missing;
            }
            Err(_) => return CursorState::Invalid,
        };
        let Some(cursor) = parse_cursor(&contents) else {
            return CursorState::Invalid;
        };
        if cursor.session_name != session_name || cursor.log_size != current_log_size(path) {
            return CursorState::Invalid;
        }
        CursorState::Valid(cursor)
    }

    fn cursor_failure_message(path: &Path, error: &str) -> String {
        format!("failed to update log cursor {}: {error}", path.display())
    }

    fn write_cursor(
        path: &Path,
        session_name: &str,
        line_count: u64,
        anchor: Option<Vec<u8>>,
    ) -> Result<(), String> {
        // Write-then-rename so a crash mid-write can never leave a
        // truncated/corrupt sidecar in place: the old (or no) file stays
        // valid until the new one is atomically swapped in. Failures are
        // returned to the caller so they remain visible and the next
        // capture retries from a stale (or absent) cursor rather than
        // losing log content.
        let sidecar = offset_sidecar_path(path);
        let mut temp_name = sidecar.as_os_str().to_owned();
        temp_name.push(".tmp");
        let temp = PathBuf::from(temp_name);
        validate_log_target(&sidecar)?;
        validate_log_target(&temp)?;
        let contents = format!(
            "session={session_name}\nlog_size={}\nline_count={line_count}\nanchor={}\n",
            current_log_size(path),
            anchor.map_or_else(|| "none".to_owned(), |bytes| encode_hex(&bytes))
        );
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&temp)
            .map_err(|error| format!("failed to open cursor temporary file: {error}"))?;
        file.write_all(contents.as_bytes())
            .map_err(|error| format!("failed to write cursor temporary file: {error}"))?;
        validate_log_target(&sidecar)?;
        fs::rename(&temp, &sidecar)
            .map_err(|error| format!("failed to replace cursor sidecar: {error}"))
    }

    /// Runs `capture-pane`. `escapes` requests `-e` (ANSI escape sequences
    /// included) for `--raw`'s backfill; every clean-mode capture omits it,
    /// since tmux's default `capture-pane` output is already ANSI-free.
    fn run_capture_pane(
        tmux: &Tmux,
        session_name: &str,
        start: &str,
        end: &str,
        escapes: bool,
    ) -> Result<Vec<u8>, String> {
        let mut arguments = vec!["capture-pane", "-p"];
        if escapes {
            arguments.push("-e");
        }
        arguments.extend(["-t", session_name, "-S", start, "-E", end]);
        let output = tmux.run(arguments)?;
        ensure_tmux_success(output.status, &output.stderr)?;
        Ok(output.stdout)
    }

    fn ensure_tmux_success(status: ExitStatus, stderr: &[u8]) -> Result<(), String> {
        if status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(stderr);
        let detail = detail.trim();
        if detail.is_empty() {
            Err(format!("tmux command failed with status {status}"))
        } else {
            Err(format!(
                "tmux command failed with status {status}: {detail}"
            ))
        }
    }

    fn start_pipe_pane(tmux: &Tmux, session_name: &str, path: &Path) -> Result<(), String> {
        let command = format!("umask 077; cat >> {}", shell_quote(&path.to_string_lossy()));
        let output = tmux.run(["pipe-pane", "-t", session_name, &command])?;
        ensure_tmux_success(output.status, &output.stderr)
    }

    /// Reports whether this session's pane already has a `pipe-pane`
    /// stream attached (tmux's `#{pane_pipe}` format, `1`/`0`).
    fn pane_has_active_pipe(tmux: &Tmux, session_name: &str) -> Result<bool, String> {
        let output = tmux.run(["display-message", "-p", "-t", session_name, "#{pane_pipe}"])?;
        ensure_tmux_success(output.status, &output.stderr)?;
        let text = String::from_utf8(output.stdout)
            .map_err(|_| "tmux display-message returned invalid UTF-8".to_owned())?;
        Ok(text.trim() == "1")
    }

    /// Widens this session's retained scrollback well past the process
    /// default so append-mode captures rarely have to use their
    /// (still-correct, still-marked) eviction fallback.
    const LOGGED_SESSION_HISTORY_LIMIT: &str = "50000";

    fn raise_history_limit(tmux: &Tmux, session_name: &str) -> Result<(), String> {
        let output = tmux.run([
            "set-option",
            "-t",
            session_name,
            "history-limit",
            LOGGED_SESSION_HISTORY_LIMIT,
        ])?;
        ensure_tmux_success(output.status, &output.stderr)
    }

    /// Quotes `value` as one POSIX shell word (`'...'`, with embedded
    /// single quotes closed/reopened/escaped) — the standard technique for
    /// safely embedding arbitrary, untrusted content in a shell command
    /// line.
    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    #[cfg(test)]
    mod tests {
        use super::{
            CursorState, capture_once, make_anchor, offset_sidecar_path, read_cursor,
            resolve_log_path, shell_quote, validate_log_target, write_cursor,
        };
        use crate::test_support::TempPath;
        use crate::tmux::Tmux;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        fn unique_path() -> TempPath {
            TempPath::file("stay-logging-test")
        }

        fn line_count(path: &std::path::Path, session_name: &str) -> u64 {
            match read_cursor(path, session_name) {
                CursorState::Valid(cursor) => cursor.line_count,
                CursorState::Missing | CursorState::Invalid => 0,
            }
        }

        fn stored_anchor(path: &std::path::Path, session_name: &str) -> Option<Vec<u8>> {
            match read_cursor(path, session_name) {
                CursorState::Valid(cursor) => cursor.anchor,
                CursorState::Missing | CursorState::Invalid => None,
            }
        }

        #[test]
        fn a_fresh_path_is_accepted() {
            let path = unique_path();
            assert!(validate_log_target(&path).is_ok());
        }

        #[test]
        fn a_symlink_target_is_rejected() {
            let real = unique_path();
            fs::write(&real, "").expect("write symlink target");
            let link = unique_path();
            std::os::unix::fs::symlink(&real, &link).expect("create symlink");

            let error = validate_log_target(&link).expect_err("symlink should be rejected");
            assert!(error.contains("symlink"), "{error}");

            let _ = fs::remove_file(&link);
            let _ = fs::remove_file(&real);
        }

        #[test]
        fn a_world_readable_file_is_rejected_and_owner_only_is_accepted() {
            let path = unique_path();
            fs::write(&path, "").expect("write log target");
            let mut permissions = fs::metadata(&path).expect("stat log target").permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(&path, permissions).expect("chmod log target");
            let error =
                validate_log_target(&path).expect_err("group/other bits should be rejected");
            assert!(error.contains("group or other"), "{error}");

            let mut permissions = fs::metadata(&path).expect("stat log target").permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(&path, permissions).expect("chmod log target");
            assert!(validate_log_target(&path).is_ok());

            let _ = fs::remove_file(&path);
        }

        #[test]
        fn aliased_paths_resolve_to_the_same_canonical_target() {
            let directory = unique_path();
            fs::create_dir(&directory).expect("create test directory");
            let subdirectory = directory.join("sub");
            fs::create_dir(&subdirectory).expect("create test subdirectory");
            let file = directory.join("session.log");
            fs::write(&file, "").expect("write log target");
            let mut permissions = fs::metadata(&file).expect("stat log target").permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(&file, permissions).expect("chmod log target");

            let direct = resolve_log_path("session.log", &directory).expect("resolve direct path");
            let dotted =
                resolve_log_path("../session.log", &subdirectory).expect("resolve dotted path");
            assert_eq!(direct, dotted);

            let alias = directory.join("alias");
            std::os::unix::fs::symlink(&directory, &alias).expect("create directory alias symlink");
            let via_alias =
                resolve_log_path(&format!("{}/session.log", alias.display()), &directory)
                    .expect("resolve path through a symlinked directory alias");
            assert_eq!(direct, via_alias);

            let _ = fs::remove_file(&alias);
            let _ = fs::remove_dir(&subdirectory);
            let _ = fs::remove_file(&file);
            let _ = fs::remove_dir(&directory);
        }

        #[test]
        fn cursor_round_trips_through_the_sidecar_file() {
            let path = unique_path();
            assert_eq!(line_count(&path, "session"), 0);
            write_cursor(&path, "session", 42, Some(vec![0, b'\n', 0xff])).expect("write cursor");
            assert_eq!(line_count(&path, "session"), 42);
            assert_eq!(stored_anchor(&path, "session"), Some(vec![0, b'\n', 0xff]));
            assert_eq!(
                fs::read_to_string(offset_sidecar_path(&path))
                    .expect("read cursor sidecar")
                    .lines()
                    .last(),
                Some("anchor=000aff")
            );
            let _ = fs::remove_file(offset_sidecar_path(&path));
        }

        #[test]
        fn anchor_keeps_only_recent_complete_lines_with_both_caps() {
            use std::fmt::Write as _;
            let lines = (0..65).fold(String::new(), |mut lines, index| {
                let _ = writeln!(lines, "line-{index}");
                lines
            });
            let anchor = make_anchor(lines.as_bytes()).expect("anchor for numbered lines");
            let expected = (1..65).fold(String::new(), |mut lines, index| {
                let _ = writeln!(lines, "line-{index}");
                lines
            });
            assert_eq!(anchor, expected.as_bytes());

            let bytes = format!("{}{}{}", "a".repeat(4000), "\n", "b".repeat(4000) + "\n");
            assert_eq!(make_anchor(bytes.as_bytes()), Some(bytes.into_bytes()));

            assert_eq!(make_anchor(b"partial"), None);
            assert_eq!(make_anchor(b""), None);
            assert_eq!(make_anchor(b"oversized\n"), Some(b"oversized\n".to_vec()));
            assert_eq!(
                make_anchor(format!("{}\n", "x".repeat(8192)).as_bytes()),
                None
            );
            assert_eq!(
                make_anchor(format!("{}\n", "x".repeat(8191)).as_bytes()),
                Some(format!("{}\n", "x".repeat(8191)).into_bytes())
            );
            assert_eq!(
                make_anchor(b"first\n\nlast\n"),
                Some(b"first\n\nlast\n".to_vec())
            );
        }

        #[test]
        fn overlapping_anchor_appends_each_new_line_once() {
            let path = unique_path();
            fs::write(&path, "old\n").expect("write initial log");
            write_cursor(&path, "session", 1, Some(b"old\n".to_vec()))
                .expect("write initial cursor");

            let tmux = Tmux::for_test_shell_script("printf 'old\\nnew\\n'");
            capture_once(&tmux, "session", &path, false).expect("capture overlap");
            assert_eq!(fs::read_to_string(&path).expect("read log"), "old\nnew\n");
            assert_eq!(
                stored_anchor(&path, "session"),
                Some(b"old\nnew\n".to_vec())
            );

            let _ = fs::remove_file(offset_sidecar_path(&path));
        }

        #[test]
        fn an_evicted_overlap_uses_a_marker_and_advances_the_cursor() {
            let path = unique_path();
            fs::write(&path, "old\n").expect("write initial log");
            write_cursor(&path, "session", 1, Some(b"old\n".to_vec()))
                .expect("write initial cursor");

            let tmux = Tmux::for_test_shell_script("printf 'new\\n'");
            capture_once(&tmux, "session", &path, false).expect("capture evicted history");
            let contents = fs::read_to_string(&path).expect("read log");
            assert!(contents.starts_with("old\n--- history evicted before capture"));
            assert!(contents.ends_with("new\n"));
            assert_eq!(line_count(&path, "session"), 1);
            assert_eq!(stored_anchor(&path, "session"), Some(b"new\n".to_vec()));

            let _ = fs::remove_file(offset_sidecar_path(&path));
        }

        #[test]
        fn an_ambiguous_anchor_uses_the_marked_full_dump() {
            let path = unique_path();
            fs::write(&path, "old\n").expect("write initial log");
            write_cursor(&path, "session", 1, Some(b"anchor\n".to_vec()))
                .expect("write initial cursor");

            let tmux = Tmux::for_test_shell_script("printf 'anchor\\nx\\nanchor\\n'");
            capture_once(&tmux, "session", &path, false).expect("capture ambiguous anchor");
            let contents = fs::read_to_string(&path).expect("read log");
            assert_eq!(
                contents,
                "old\n--- history evicted before capture ---\nanchor\nx\nanchor\n"
            );

            let _ = fs::remove_file(offset_sidecar_path(&path));
        }

        #[test]
        fn failed_append_leaves_the_cursor_for_a_retry() {
            let path = unique_path();
            fs::create_dir(&path).expect("create unwritable log directory");
            let recorded_size = fs::metadata(&path).expect("stat log directory").len();
            write_cursor(&path, "session", 1, Some(b"old\n".to_vec()))
                .expect("write initial cursor");

            let tmux = Tmux::for_test_shell_script("printf 'old\\nnew\\n'");
            let warning = capture_once(&tmux, "session", &path, false)
                .expect("capture with a failed append")
                .expect("failed append should produce a warning");
            assert!(warning.contains("failed to write log"), "{warning}");
            assert_eq!(line_count(&path, "session"), 1);

            fs::remove_dir(&path).expect("remove unwritable log directory");
            let filler_length =
                usize::try_from(recorded_size).expect("test directory size fits in usize");
            fs::write(&path, vec![b'x'; filler_length]).expect("create log file");
            capture_once(&tmux, "session", &path, false)
                .expect("retry capture after restoring the log");
            let contents = fs::read_to_string(&path).expect("read retried log");
            assert!(contents.ends_with("new\n"), "{contents:?}");
            assert_eq!(line_count(&path, "session"), 2);

            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(offset_sidecar_path(&path));
        }

        #[test]
        fn cursor_session_or_log_size_mismatch_forces_a_full_capture() {
            for mismatch in ["session", "log_size"] {
                let path = unique_path();
                fs::write(&path, "old\n").expect("write existing log");
                write_cursor(&path, "session", 1, Some(b"old\n".to_vec()))
                    .expect("write initial cursor");
                let sidecar = offset_sidecar_path(&path);
                let contents = if mismatch == "session" {
                    "session=other\nlog_size=4\nline_count=1\n".to_owned()
                } else {
                    "session=session\nlog_size=99\nline_count=1\n".to_owned()
                };
                fs::write(&sidecar, contents).expect("tamper with cursor metadata");

                let tmux = Tmux::for_test_shell_script("printf 'new\\n'");
                capture_once(&tmux, "session", &path, false)
                    .expect("capture after cursor mismatch");
                assert_eq!(
                    fs::read_to_string(&path).expect("read full recapture"),
                    "old\n--- history evicted before capture ---\nnew\n"
                );

                let _ = fs::remove_file(&sidecar);
                let _ = fs::remove_file(&path);
            }
        }

        #[test]
        fn legacy_and_corrupt_sidecars_use_the_marked_fallback() {
            for contents in [
                "session=session\nlog_size=4\nline_count=1\n",
                "session=session\nlog_size=4\nline_count=1\nanchor=not-hex\n",
            ] {
                let path = unique_path();
                fs::write(&path, "old\n").expect("write existing log");
                fs::write(offset_sidecar_path(&path), contents).expect("write bad cursor");
                let mut permissions = fs::metadata(offset_sidecar_path(&path))
                    .expect("stat bad cursor")
                    .permissions();
                permissions.set_mode(0o600);
                fs::set_permissions(offset_sidecar_path(&path), permissions)
                    .expect("secure bad cursor");

                let tmux = Tmux::for_test_shell_script("printf 'new\\n'");
                capture_once(&tmux, "session", &path, false).expect("capture after bad cursor");
                assert_eq!(
                    fs::read_to_string(&path).expect("read marked recapture"),
                    "old\n--- history evicted before capture ---\nnew\n"
                );
                assert!(
                    fs::read_to_string(offset_sidecar_path(&path))
                        .expect("read rewritten cursor")
                        .contains("anchor=6e65770a")
                );

                let _ = fs::remove_file(offset_sidecar_path(&path));
                let _ = fs::remove_file(&path);
            }
        }

        #[test]
        fn cursor_sidecar_and_temporary_symlinks_are_rejected() {
            let path = unique_path();
            fs::write(&path, "").expect("write log");
            let real = unique_path();
            fs::write(&real, "untouched").expect("write symlink target");
            let sidecar = offset_sidecar_path(&path);

            std::os::unix::fs::symlink(&real, &sidecar).expect("create sidecar symlink");
            let error =
                write_cursor(&path, "session", 1, None).expect_err("reject sidecar symlink");
            assert!(error.contains("symlink"), "{error}");
            assert_eq!(
                fs::read_to_string(&real).expect("read symlink target"),
                "untouched"
            );
            fs::remove_file(&sidecar).expect("remove sidecar symlink");

            let mut temporary = sidecar.as_os_str().to_owned();
            temporary.push(".tmp");
            let temporary = std::path::PathBuf::from(temporary);
            std::os::unix::fs::symlink(&real, &temporary).expect("create temporary symlink");
            let error = write_cursor(&path, "session", 1, None)
                .expect_err("reject temporary cursor symlink");
            assert!(error.contains("symlink"), "{error}");
            assert_eq!(
                fs::read_to_string(&real).expect("read symlink target"),
                "untouched"
            );

            let _ = fs::remove_file(&temporary);
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(&real);
        }

        #[test]
        fn shell_quote_escapes_embedded_single_quotes() {
            assert_eq!(shell_quote("plain"), "'plain'");
            assert_eq!(shell_quote("it's"), "'it'\\''s'");
        }
    }
}

#[cfg(unix)]
pub use unix::{LogSession, resolve_log_path};

#[cfg(not(unix))]
pub fn resolve_log_path(_: &str, _: &std::path::Path) -> Result<std::path::PathBuf, String> {
    Err("attach-mode logging is unsupported on this platform".to_owned())
}

#[cfg(not(unix))]
pub struct LogSession;

#[cfg(not(unix))]
impl LogSession {
    pub fn start(
        _: &crate::tmux::Tmux,
        _: &str,
        _: &str,
        _: &std::path::Path,
        _: bool,
        _: bool,
    ) -> Result<Self, String> {
        Err("attach-mode logging is unsupported on this platform".to_owned())
    }

    pub fn on_attach_open(&mut self, _: &crate::tmux::Tmux, _: &str) -> Result<(), String> {
        Ok(())
    }

    pub fn on_tick(&mut self, _: &crate::tmux::Tmux, _: &str) -> Result<(), String> {
        Ok(())
    }

    pub fn on_detach(&mut self, _: &crate::tmux::Tmux, _: &str) -> Result<(), String> {
        Ok(())
    }
}

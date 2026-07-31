//! Attach-mode logging: `-l/--log`, `-t/--truncate`, `--raw`.
//!
//! Default (clean) mode restricts every capture to tmux's history range
//! (`-E -1`), never the volatile visible screen, so a line count derived
//! from one atomic capture is a stable append point. `--raw` instead opens
//! a continuous `pipe-pane` stream, which keeps producing output while the
//! session is detached.
//!
//! Clean mode re-captures the whole retained range on every tick and skips
//! the already-captured prefix locally, rather than querying
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
                // smaller) history retention — and tmux's own `-o` already
                // makes re-running pipe-pane a no-op here, so there is
                // nothing for either call to do. Only a session with no
                // pipe yet (first `-l --raw`, or one whose pipe was closed)
                // needs the one-shot backfill-then-start sequence.
                if !pane_has_active_pipe(tmux, session_name)? {
                    let dump = run_capture_pane(tmux, session_name, "-", "-", true)?;
                    if let Err(error) = write_full(&session.path, &dump) {
                        session.warn_once(&write_failure_message(&session.path, &error));
                    }
                    start_pipe_pane(tmux, session_name, &session.path)?;
                }
            } else if !truncate {
                // Append mode's incremental accounting only loses content
                // when tmux evicts history faster than this session's
                // captures run; a generously raised limit makes that rare
                // in practice without changing correctness (eviction is
                // still detected and marked explicitly either way).
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

        // A single atomic `capture-pane` call, with the line count derived
        // from its own output. Querying `#{history_size}` separately and
        // then addressing the capture with a relative `-N` offset would be
        // racy against a still-growing pane: `-N` is relative to the
        // CURRENT bottom of the retained window at the moment capture-pane
        // itself runs, which can have moved past what a prior, separate
        // history-size query observed, silently dropping the oldest lines
        // in the requested range. Requesting the full retained range every
        // time and skipping the already-captured prefix in Rust has no such
        // window.
        let dump = run_capture_pane(tmux, session_name, "-", "-1", false)?;
        let current_lines = count_lines(&dump);
        let previous = read_cursor(path);
        let mut warning = None;

        if current_lines < previous {
            let lost = previous - current_lines;
            let marker =
                format!("--- history evicted before capture, {lost} lines possibly lost ---\n");
            if let Err(error) = append_bytes(path, marker.as_bytes()) {
                warning = Some(write_failure_message(path, &error));
            }
            if let Err(error) = append_bytes(path, &dump) {
                warning = warning.or_else(|| Some(write_failure_message(path, &error)));
            }
        } else {
            let new_bytes = skip_lines(&dump, previous);
            if let Err(error) = append_bytes(path, new_bytes) {
                warning = Some(write_failure_message(path, &error));
            }
        }

        write_cursor(path, current_lines);
        Ok(warning)
    }

    // A capture runs at most a few times a second; a dedicated
    // byte-counting crate isn't worth a new dependency for this.
    #[allow(clippy::naive_bytecount)]
    fn count_lines(dump: &[u8]) -> u64 {
        dump.iter().filter(|&&byte| byte == b'\n').count() as u64
    }

    /// Returns the suffix of `dump` after skipping its first `skip` lines.
    fn skip_lines(dump: &[u8], skip: u64) -> &[u8] {
        let mut index = 0;
        for _ in 0..skip {
            match dump[index..].iter().position(|&byte| byte == b'\n') {
                Some(offset) => index += offset + 1,
                None => return &[],
            }
        }
        &dump[index..]
    }

    fn write_failure_message(path: &Path, error: &io::Error) -> String {
        format!("failed to write log {}: {error}", path.display())
    }

    fn append_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        // `.mode(0o600)` only applies when this call actually creates the
        // file, but that is exactly the case that matters: it keeps a
        // freshly created log passing this module's own no-group/other-bits
        // validation on the next attach, regardless of the process umask.
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?
            .write_all(bytes)
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

    fn read_cursor(path: &Path) -> u64 {
        fs::read_to_string(offset_sidecar_path(path))
            .ok()
            .and_then(|contents| contents.trim().parse().ok())
            .unwrap_or(0)
    }

    fn write_cursor(path: &Path, value: u64) {
        // Write-then-rename so a crash mid-write can never leave a
        // truncated/corrupt sidecar in place: the old (or no) file stays
        // valid until the new one is atomically swapped in. Best-effort
        // otherwise — a failure here only means the next attach re-derives
        // from a stale (or absent) cursor, which self-corrects via a full
        // history capture rather than losing log content.
        let sidecar = offset_sidecar_path(path);
        let mut temp_name = sidecar.as_os_str().to_owned();
        temp_name.push(".tmp");
        let temp = PathBuf::from(temp_name);
        if fs::write(&temp, value.to_string()).is_ok() {
            let _ = fs::rename(&temp, &sidecar);
        }
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
        let output = tmux.run(["pipe-pane", "-o", "-t", session_name, &command])?;
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
    /// default so append-mode's incremental capture rarely has to fall back
    /// to its (still-correct, still-marked) eviction path.
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
            offset_sidecar_path, read_cursor, resolve_log_path, shell_quote, validate_log_target,
            write_cursor,
        };
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        fn unique_path() -> std::path::PathBuf {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos();
            let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
            std::env::temp_dir().join(format!("stay-logging-test-{nanos}-{counter}"))
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
            assert_eq!(read_cursor(&path), 0);
            write_cursor(&path, 42);
            assert_eq!(read_cursor(&path), 42);
            let _ = fs::remove_file(offset_sidecar_path(&path));
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

//! The interactive PTY relay used for tmux attachments.

use crate::config::Config;
use crate::tmux::Tmux;

/// Attach-mode modifiers threaded from the CLI/picker through to the relay.
///
/// Bundled into one struct (rather than growing the attach functions'
/// positional bool parameters further) once a fourth independent modifier
/// (`-l/--log`'s `truncate`/`raw` alongside `-r`/`-L`) made the plain
/// parameter list both too long and too easy to mis-order at a call site.
/// These four flags are genuinely independent CLI toggles, not states of
/// one state machine, so they stay as plain bools rather than being folded
/// into enums.
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct AttachOptions<'a> {
    pub read_only: bool,
    pub low_priority: bool,
    pub log_path: Option<&'a str>,
    pub truncate: bool,
    pub raw: bool,
}

#[cfg(unix)]
mod unix {
    use super::{AttachOptions, Config, Tmux};
    use crate::logging::LogSession;
    use crate::tmux;
    use nix::errno::Errno;
    use nix::libc;
    use nix::poll::{PollFd, PollFlags, poll};
    use nix::pty::{ForkptyResult, Winsize, forkpty};
    use nix::sys::signal::{self, SaFlags, SigAction, SigHandler, SigSet, Signal, kill};
    use nix::sys::termios::{self, SetArg, Termios};
    use nix::sys::wait::{WaitStatus, waitpid};
    use nix::unistd::execvp;
    use std::ffi::CString;
    use std::io;
    use std::os::fd::{AsFd, AsRawFd, OwnedFd};
    use std::panic;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

    const PANE_POLL_INTERVAL: Duration = Duration::from_millis(500);

    static TERMINATE_REQUESTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn request_termination(_: libc::c_int) {
        TERMINATE_REQUESTED.store(true, Ordering::Relaxed);
    }

    /// Attaches through a real PTY and returns the retained pane status.
    ///
    /// # Errors
    ///
    /// Returns an error when PTY allocation, signal/terminal setup, tmux
    /// control, or relay I/O fails.
    pub fn attach(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        options: AttachOptions<'_>,
    ) -> Result<u8, String> {
        attach_with_input(tmux, config, session_name, options, &[])
    }

    /// Attaches through the relay after forwarding input captured during an
    /// interactive picker handoff.
    ///
    /// `options.read_only`/`options.low_priority` map onto tmux's
    /// `attach-session -f` client flags independently.
    /// `options.log_path` opens `-l/--log` logging for this attach
    /// (`options.truncate`/`options.raw` select its mode); `None` disables
    /// logging entirely.
    ///
    /// # Errors
    ///
    /// Returns an error when PTY allocation, signal/terminal setup, tmux
    /// control, or relay I/O fails.
    pub fn attach_with_input(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        options: AttachOptions<'_>,
        initial_input: &[u8],
    ) -> Result<u8, String> {
        let (program, arguments) = tmux.attach_program_and_arguments(
            session_name,
            options.read_only,
            options.low_priority,
        );
        let attach_start = epoch_seconds()?;
        let log_session = match options.log_path {
            Some(path) => {
                let cwd = std::env::current_dir()
                    .map_err(|error| format!("failed to resolve the current directory: {error}"))?;
                Some(LogSession::start(
                    tmux,
                    session_name,
                    path,
                    &cwd,
                    options.truncate,
                    options.raw,
                )?)
            }
            None => None,
        };
        let program = CString::new(program.as_encoded_bytes())
            .map_err(|_| "tmux executable contains a NUL byte".to_owned())?;
        let mut exec_arguments = vec![
            CString::new(program.as_bytes())
                .map_err(|_| "tmux executable contains a NUL byte".to_owned())?,
        ];
        exec_arguments.extend(
            arguments
                .into_iter()
                .map(|argument| CString::new(argument.as_encoded_bytes()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "tmux attach arguments contain a NUL byte".to_owned())?,
        );
        let winsize = current_winsize();
        let child = spawn_attach_child(&program, &exec_arguments, winsize)?;
        let _signals = SignalGuard::install()?;
        let _terminal = TerminalGuard::new()?;
        relay_loop(
            tmux,
            config,
            session_name,
            &child,
            initial_input,
            attach_start,
            log_session,
        )
    }

    struct AttachChild {
        pid: nix::unistd::Pid,
        master: OwnedFd,
    }

    #[allow(unsafe_code)]
    fn spawn_attach_child(
        program: &CString,
        arguments: &[CString],
        winsize: Option<Winsize>,
    ) -> Result<AttachChild, String> {
        // forkpty performs the required setsid/TIOCSCTTY setup in the child,
        // and gives tmux a controlling terminal even when stay has pipes.
        let result = unsafe { forkpty(winsize.as_ref(), None::<&Termios>) }
            .map_err(|error| format!("failed to allocate attach PTY: {error}"))?;
        match result {
            ForkptyResult::Parent { child, master } => Ok(AttachChild { pid: child, master }),
            ForkptyResult::Child => {
                let _ = execvp(program.as_c_str(), arguments);
                unsafe { libc::_exit(127) }
            }
        }
    }

    fn relay_loop(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        child: &AttachChild,
        initial_input: &[u8],
        attach_start: u64,
        mut log_session: Option<LogSession>,
    ) -> Result<u8, String> {
        if let Some(log_session) = log_session.as_mut() {
            log_session.on_attach_open(tmux, session_name)?;
        }
        let log_interval = Duration::from_secs(config.log_capture_interval_seconds.max(1));
        let mut last_log_tick = Instant::now();

        handle_child_input(tmux, config, session_name, child, initial_input)?;
        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        let mut stdin_open = true;
        let mut child_output_open = true;
        let mut attach_child_stopped = false;
        let mut last_winsize = current_winsize();
        let mut last_pane_poll = Instant::now();

        while child_output_open {
            if TERMINATE_REQUESTED.swap(false, Ordering::Relaxed) {
                attach_child_stopped = !detach_client(tmux, session_name, child.pid);
                child_output_open = !attach_child_stopped;
                stdin_open = false;
            }

            if child_output_open && last_pane_poll.elapsed() >= PANE_POLL_INTERVAL {
                last_pane_poll = Instant::now();
                if pane_state(tmux, session_name)?.is_some_and(|state| {
                    state.dead && state.dead_time.is_some_and(|time| time >= attach_start)
                }) {
                    attach_child_stopped = !detach_client(tmux, session_name, child.pid);
                    child_output_open = !attach_child_stopped;
                    stdin_open = false;
                }
            }

            if let Some(log_session) = log_session.as_mut()
                && last_log_tick.elapsed() >= log_interval
            {
                last_log_tick = Instant::now();
                log_session.on_tick(tmux, session_name)?;
            }

            propagate_winsize(
                child.master.as_raw_fd(),
                &mut last_winsize,
                current_winsize(),
            );

            let mut pollfds = Vec::with_capacity(2);
            if stdin_open {
                pollfds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLIN));
            }
            let master_index = pollfds.len();
            pollfds.push(PollFd::new(
                child.master.as_fd(),
                PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR,
            ));

            match poll(&mut pollfds, 100u16) {
                Ok(_) => {}
                Err(Errno::EINTR) => continue,
                Err(error) => return Err(format!("relay poll failed: {error}")),
            }

            let events = pollfds[master_index]
                .revents()
                .unwrap_or_else(PollFlags::empty);
            let master_closed = events.intersects(PollFlags::POLLHUP | PollFlags::POLLERR);
            if events.intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR) {
                let mut output = [0_u8; 8192];
                match nix::unistd::read(&child.master, &mut output) {
                    Ok(0) | Err(Errno::EIO) => child_output_open = false,
                    Ok(length) => {
                        forward_output(&mut stdout, &output[..length])?;
                    }
                    Err(Errno::EINTR) => {}
                    Err(error) => return Err(format!("relay PTY read failed: {error}")),
                }
            }

            if child_output_open
                && stdin_open
                && !master_closed
                && pollfds[0]
                    .revents()
                    .unwrap_or_else(PollFlags::empty)
                    .intersects(PollFlags::POLLIN | PollFlags::POLLHUP)
            {
                let mut input = [0_u8; 4096];
                match nix::unistd::read(stdin.as_fd(), &mut input) {
                    Ok(0) => stdin_open = false,
                    Ok(length) => {
                        handle_child_input(tmux, config, session_name, child, &input[..length])?;
                    }
                    Err(Errno::EINTR) => {}
                    Err(error) => return Err(format!("relay input failed: {error}")),
                }
            }
        }

        if let Some(log_session) = log_session.as_mut() {
            log_session.on_detach(tmux, session_name)?;
        }
        let attach_status = reap_child(child.pid)?;
        if !attach_child_stopped {
            attach_failure(attach_status).map_or(Ok(()), Err)?;
        }
        Ok(exit_status_for_attach(
            pane_state(tmux, session_name)?.as_ref(),
            attach_start,
        ))
    }

    fn detach_client(tmux: &Tmux, session_name: &str, child: nix::unistd::Pid) -> bool {
        if tmux.detach_client(session_name, child.as_raw()).is_err() {
            stop_attach_child(child);
            false
        } else {
            true
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct PaneState {
        dead: bool,
        dead_time: Option<u64>,
        dead_status: Option<u8>,
    }

    fn pane_state(tmux: &Tmux, session_name: &str) -> Result<Option<PaneState>, String> {
        let output = tmux.run([
            "list-panes",
            "-t",
            session_name,
            "-F",
            "#{pane_dead}:#{pane_dead_time}:#{pane_dead_status}",
        ])?;
        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)
                .map_err(|_| "tmux returned invalid UTF-8 on stderr".to_owned())?;
            if tmux::is_missing_server_error(&stderr)
                || stderr.contains("can't find session")
                || stderr.contains("no such session")
            {
                return Ok(None);
            }
            return Err(format_tmux_failure(output.status, &stderr));
        }

        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| "tmux list-panes returned invalid UTF-8".to_owned())?;
        let row = stdout
            .lines()
            .next()
            .ok_or_else(|| "tmux returned no pane state".to_owned())?;
        parse_pane_state_row(row).map(Some)
    }

    fn parse_pane_state_row(row: &str) -> Result<PaneState, String> {
        let mut fields = row.split(':');
        let dead = match fields.next() {
            Some("0") => false,
            Some("1") => true,
            Some(value) => return Err(format!("invalid tmux pane dead flag: {value:?}")),
            None => return Err("tmux pane state is missing its dead flag".to_owned()),
        };
        let dead_time = fields
            .next()
            .ok_or_else(|| format!("tmux pane state is missing its dead time: {row:?}"))?;
        let dead_status = fields
            .next()
            .ok_or_else(|| format!("tmux pane state is missing its dead status: {row:?}"))?;
        if fields.next().is_some() {
            return Err(format!("malformed tmux pane state: {row:?}"));
        }
        if !dead {
            if !dead_time.is_empty() || !dead_status.is_empty() {
                return Err(format!("live tmux pane has dead fields: {row:?}"));
            }
            return Ok(PaneState {
                dead,
                dead_time: None,
                dead_status: None,
            });
        }

        let dead_time = dead_time
            .parse::<u64>()
            .map_err(|_| format!("invalid tmux pane dead time: {row:?}"))?;
        let dead_status = dead_status
            .parse::<u8>()
            .map_err(|_| format!("invalid tmux pane dead status: {row:?}"))?;
        Ok(PaneState {
            dead,
            dead_time: Some(dead_time),
            dead_status: Some(dead_status),
        })
    }

    fn exit_status_for_attach(state: Option<&PaneState>, attach_start: u64) -> u8 {
        state
            .filter(|state| state.dead && state.dead_time.is_some_and(|time| time >= attach_start))
            .and_then(|state| state.dead_status)
            .unwrap_or(0)
    }

    fn attach_failure(status: WaitStatus) -> Option<String> {
        match status {
            WaitStatus::Exited(_, 0) => None,
            WaitStatus::Exited(_, code) => Some(format!("tmux attach failed with status {code}")),
            WaitStatus::Signaled(_, signal, _) => {
                Some(format!("tmux attach terminated by {signal}"))
            }
            _ => Some(format!("tmux attach ended unexpectedly: {status:?}")),
        }
    }

    fn epoch_seconds() -> Result<u64, String> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|error| format!("system clock is before the Unix epoch: {error}"))
    }

    fn format_tmux_failure(status: std::process::ExitStatus, stderr: &str) -> String {
        let detail = stderr.trim();
        if detail.is_empty() {
            format!("tmux command failed with status {status}")
        } else {
            format!("tmux command failed with status {status}: {detail}")
        }
    }

    /// Writes one chunk of attach-PTY output to the terminal, then flushes.
    ///
    /// `io::stdout()` is line-buffered, so without the flush a chunk that
    /// carries no trailing newline (a shell prompt is the canonical case)
    /// would be held in its buffer until some later newline passes through.
    /// Flushing on every chunk keeps partial-line output visible at once.
    fn forward_output<W: io::Write>(stdout: &mut W, output: &[u8]) -> Result<(), String> {
        stdout
            .write_all(output)
            .map_err(|error| format!("relay output failed: {error}"))?;
        stdout
            .flush()
            .map_err(|error| format!("relay output failed: {error}"))
    }

    fn handle_input(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        child: nix::unistd::Pid,
        master: &OwnedFd,
        input: &[u8],
    ) -> Result<(), String> {
        let mut forwarded = Vec::with_capacity(input.len());
        for &byte in input {
            if byte == config.detach_key {
                write_input(master, &forwarded)?;
                forwarded.clear();
                tmux.detach_client(session_name, child.as_raw())?;
            } else if byte == config.copy_mode_key {
                write_input(master, &forwarded)?;
                forwarded.clear();
                tmux.copy_mode(session_name)?;
            } else {
                forwarded.push(byte);
            }
        }
        write_input(master, &forwarded)
    }

    fn handle_child_input(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        child: &AttachChild,
        input: &[u8],
    ) -> Result<(), String> {
        let result = handle_input(tmux, config, session_name, child.pid, &child.master, input);
        if let Err(error) = result {
            stop_attach_child(child.pid);
            return match reap_child(child.pid) {
                Ok(_) => Err(error),
                Err(reap_error) => Err(format!("{error}; {reap_error}")),
            };
        }
        Ok(())
    }

    fn write_input(master: &OwnedFd, input: &[u8]) -> Result<(), String> {
        if input.is_empty() {
            return Ok(());
        }
        let mut written = 0;
        while written < input.len() {
            match nix::unistd::write(master, &input[written..]) {
                Ok(length) => written += length,
                Err(Errno::EINTR) => {}
                Err(Errno::EIO | Errno::EPIPE) => return Ok(()),
                Err(error) => return Err(format!("relay input write failed: {error}")),
            }
        }
        Ok(())
    }

    fn stop_attach_child(pid: nix::unistd::Pid) {
        let _ = kill(pid, Signal::SIGTERM);
        let _ = kill(pid, Signal::SIGKILL);
    }

    fn reap_child(pid: nix::unistd::Pid) -> Result<WaitStatus, String> {
        loop {
            match waitpid(pid, None) {
                Ok(status) => return Ok(status),
                Err(Errno::EINTR) => {}
                Err(error) => return Err(format!("failed to reap tmux attach: {error}")),
            }
        }
    }

    fn current_winsize() -> Option<Winsize> {
        let stdin = io::stdin();
        if !nix::unistd::isatty(stdin.as_fd()).unwrap_or(false) {
            return None;
        }
        let mut winsize = Winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if ioctl_get_winsize(stdin.as_fd().as_raw_fd(), &mut winsize).is_ok() {
            Some(winsize)
        } else {
            None
        }
    }

    #[allow(unsafe_code)]
    fn ioctl_get_winsize(fd: libc::c_int, winsize: &mut Winsize) -> Result<(), Errno> {
        let result = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, winsize) };
        Errno::result(result).map(|_| ())
    }

    #[allow(unsafe_code)]
    fn set_winsize(fd: libc::c_int, winsize: Winsize) {
        let _ = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &winsize) };
    }

    fn propagate_winsize(
        fd: libc::c_int,
        last_winsize: &mut Option<Winsize>,
        winsize: Option<Winsize>,
    ) {
        if let Some(winsize) = winsize
            && Some(winsize) != *last_winsize
        {
            set_winsize(fd, winsize);
            *last_winsize = Some(winsize);
        }
    }

    struct TerminalGuard {
        original: Option<Termios>,
        panic_state: Arc<Mutex<Option<Termios>>>,
        previous_hook: Option<Arc<Mutex<Option<PanicHook>>>>,
    }

    impl TerminalGuard {
        fn new() -> Result<Self, String> {
            let stdin = io::stdin();
            if !nix::unistd::isatty(stdin.as_fd()).unwrap_or(false) {
                return Ok(Self {
                    original: None,
                    panic_state: Arc::new(Mutex::new(None)),
                    previous_hook: None,
                });
            }

            let original = termios::tcgetattr(stdin.as_fd())
                .map_err(|error| format!("failed to read terminal settings: {error}"))?;
            let mut raw = original.clone();
            termios::cfmakeraw(&mut raw);
            termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw)
                .map_err(|error| format!("failed to enter raw terminal mode: {error}"))?;

            let panic_state = Arc::new(Mutex::new(Some(original.clone())));
            let previous = Arc::new(Mutex::new(Some(panic::take_hook())));
            let hook_state = Arc::clone(&panic_state);
            let hook_previous = Arc::clone(&previous);
            panic::set_hook(Box::new(move |info| {
                if let Ok(mut state) = hook_state.lock()
                    && let Some(original) = state.take()
                {
                    let stdin = io::stdin();
                    let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &original);
                }
                if let Ok(previous) = hook_previous.lock()
                    && let Some(previous) = previous.as_ref()
                {
                    previous(info);
                }
            }));

            Ok(Self {
                original: Some(original),
                panic_state,
                previous_hook: Some(previous),
            })
        }
    }

    impl Drop for TerminalGuard {
        fn drop(&mut self) {
            if let Some(original) = self.original.take() {
                let stdin = io::stdin();
                let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &original);
                if let Ok(mut state) = self.panic_state.lock() {
                    state.take();
                }
            }
            if let Some(previous) = self.previous_hook.take() {
                let _ = panic::take_hook();
                let previous = previous.lock().ok().and_then(|mut hook| hook.take());
                if let Some(previous) = previous {
                    panic::set_hook(previous);
                }
            }
        }
    }

    struct SignalGuard {
        previous_term: SigAction,
        previous_pipe: SigAction,
    }

    impl SignalGuard {
        #[allow(unsafe_code)]
        fn install() -> Result<Self, String> {
            TERMINATE_REQUESTED.store(false, Ordering::Relaxed);
            let term_action = SigAction::new(
                SigHandler::Handler(request_termination),
                SaFlags::empty(),
                SigSet::empty(),
            );
            let pipe_action = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
            let previous_term = unsafe { signal::sigaction(Signal::SIGTERM, &term_action) }
                .map_err(|error| format!("failed to install SIGTERM handler: {error}"))?;
            let previous_pipe = unsafe { signal::sigaction(Signal::SIGPIPE, &pipe_action) }
                .map_err(|error| format!("failed to ignore SIGPIPE: {error}"))?;
            Ok(Self {
                previous_term,
                previous_pipe,
            })
        }
    }

    impl Drop for SignalGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            let _ = unsafe { signal::sigaction(Signal::SIGTERM, &self.previous_term) };
            let _ = unsafe { signal::sigaction(Signal::SIGPIPE, &self.previous_pipe) };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::CString;
        use std::os::fd::AsRawFd;
        use std::time::{Duration, Instant};

        fn relay_global_state_lock() -> std::sync::MutexGuard<'static, ()> {
            crate::test_global_state_lock()
        }

        #[test]
        fn configured_control_bytes_are_distinct() {
            let config = crate::config::Config {
                default_command: Some("sh".to_owned()),
                detach_key: 0x1c,
                copy_mode_key: 0,
                history_lines: 1,
                log_capture_interval_seconds: 5,
            };
            assert_ne!(config.detach_key, config.copy_mode_key);
        }

        #[test]
        fn parses_live_and_dead_pane_state() {
            assert_eq!(
                parse_pane_state_row("0::"),
                Ok(PaneState {
                    dead: false,
                    dead_time: None,
                    dead_status: None,
                })
            );
            assert_eq!(
                parse_pane_state_row("1:12345:7"),
                Ok(PaneState {
                    dead: true,
                    dead_time: Some(12345),
                    dead_status: Some(7),
                })
            );
        }

        #[test]
        fn only_deaths_during_attach_propagate_their_status() {
            let live = PaneState {
                dead: false,
                dead_time: None,
                dead_status: None,
            };
            let before_attach = PaneState {
                dead: true,
                dead_time: Some(99),
                dead_status: Some(5),
            };
            let during_attach = PaneState {
                dead: true,
                dead_time: Some(100),
                dead_status: Some(7),
            };

            assert_eq!(exit_status_for_attach(None, 100), 0);
            assert_eq!(exit_status_for_attach(Some(&live), 100), 0);
            assert_eq!(exit_status_for_attach(Some(&before_attach), 100), 0);
            assert_eq!(exit_status_for_attach(Some(&during_attach), 100), 7);
        }

        #[test]
        fn partial_line_output_is_flushed_past_the_line_buffer() {
            // io::stdout() is line-buffered: without the flush in
            // forward_output, a chunk with no trailing newline would sit in
            // the buffer until a later newline arrived (the original relay
            // bug, where a shell prompt did not appear until Enter). A
            // LineWriter mirrors that buffering, so this asserts the
            // partial-line bytes reach the sink at once.
            let emitted = Arc::new(Mutex::new(Vec::new()));
            let mut stdout = io::LineWriter::new(RecordingSink(Arc::clone(&emitted)));
            forward_output(&mut stdout, b"no-newline-marker").expect("forward output");
            // Clone out of the lock before asserting so a failed assertion
            // does not hold (and poison) the sink's mutex while `stdout`
            // unwinds and flushes the buffer on drop.
            let emitted = emitted.lock().expect("lock emitted output").clone();
            assert!(
                String::from_utf8_lossy(&emitted).contains("no-newline-marker"),
                "partial-line output was not flushed past the line buffer: {emitted:?}"
            );
        }

        struct RecordingSink(Arc<Mutex<Vec<u8>>>);

        impl io::Write for RecordingSink {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0
                    .lock()
                    .expect("lock recording sink")
                    .extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        #[test]
        fn closed_attach_pty_input_is_a_normal_shutdown() {
            let pair = nix::pty::openpty(None, None).expect("allocate test PTY");
            drop(pair.slave);
            assert!(write_input(&pair.master, b"input").is_ok());
        }

        #[test]
        fn window_size_round_trips_through_the_attach_pty() {
            let pair = nix::pty::openpty(None, None).expect("allocate test PTY");
            let expected = Winsize {
                ws_row: 37,
                ws_col: 119,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            set_winsize(pair.master.as_raw_fd(), expected);
            let mut actual = Winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            ioctl_get_winsize(pair.slave.as_raw_fd(), &mut actual).expect("read test PTY size");
            assert_eq!(actual.ws_row, expected.ws_row);
            assert_eq!(actual.ws_col, expected.ws_col);
        }

        #[test]
        fn relay_resize_event_updates_the_attach_pty_size() {
            let pair = nix::pty::openpty(None, None).expect("allocate test PTY");
            let initial = Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let expected = Winsize {
                ws_row: 37,
                ws_col: 119,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            set_winsize(pair.master.as_raw_fd(), initial);
            let mut last_winsize = Some(initial);
            propagate_winsize(pair.master.as_raw_fd(), &mut last_winsize, Some(expected));
            assert_eq!(last_winsize, Some(expected));
            let mut actual = Winsize {
                ws_row: 0,
                ws_col: 0,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            ioctl_get_winsize(pair.slave.as_raw_fd(), &mut actual)
                .expect("read resized test PTY size");
            assert_eq!(actual.ws_row, expected.ws_row);
            assert_eq!(actual.ws_col, expected.ws_col);
        }

        #[allow(unsafe_code)]
        #[test]
        fn signal_guard_ignores_and_restores_sigpipe() {
            let _lock = relay_global_state_lock();
            let default = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
            let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
            let original = unsafe { signal::sigaction(Signal::SIGPIPE, &default) }
                .expect("set SIGPIPE default disposition");
            let guard = SignalGuard::install().expect("install relay signal handlers");
            let during = unsafe { signal::sigaction(Signal::SIGPIPE, &ignore) }
                .expect("read relay SIGPIPE disposition");
            assert!(matches!(during.handler(), SigHandler::SigIgn));
            drop(guard);
            let restored = unsafe { signal::sigaction(Signal::SIGPIPE, &ignore) }
                .expect("read restored SIGPIPE disposition");
            assert!(matches!(restored.handler(), SigHandler::SigDfl));
            unsafe { signal::sigaction(Signal::SIGPIPE, &original) }
                .expect("restore test SIGPIPE disposition");
        }

        #[test]
        fn termination_fallback_stops_a_wedged_attach_child() {
            let _lock = relay_global_state_lock();
            let program = CString::new("/bin/sh").expect("program C string");
            let arguments = [
                CString::new("sh").expect("argv zero C string"),
                CString::new("-c").expect("shell flag C string"),
                CString::new("sleep 30").expect("shell command C string"),
            ];
            let child = spawn_attach_child(&program, &arguments, None).expect("spawn test child");
            TERMINATE_REQUESTED.store(true, Ordering::Relaxed);
            let tmux = Tmux::for_test_shell_script("exit 1");
            let config = Config {
                default_command: Some("sh".to_owned()),
                detach_key: 0x1c,
                copy_mode_key: 0,
                history_lines: 1,
                log_capture_interval_seconds: 5,
            };
            let started = Instant::now();
            let error = relay_loop(
                &tmux,
                &config,
                "test",
                &child,
                &[],
                epoch_seconds().expect("read test attach time"),
                None,
            )
            .expect_err("pane status shim should fail");
            assert!(error.contains("tmux command failed"), "{error}");
            assert!(started.elapsed() < Duration::from_secs(1));
        }

        #[test]
        fn detach_key_failure_stops_and_reaps_the_attach_child() {
            let program = CString::new("/bin/sh").expect("program C string");
            let arguments = [
                CString::new("sh").expect("argv zero C string"),
                CString::new("-c").expect("shell flag C string"),
                CString::new("sleep 30").expect("shell command C string"),
            ];
            let child = spawn_attach_child(&program, &arguments, None).expect("spawn test child");
            let tmux = Tmux::for_test_shell_script(
                "if [ \"$2\" = \"list-clients\" ]; then printf '41:/dev/pts/8\\n'; exit 0; fi; \
                 exit 97",
            );
            let config = Config {
                default_command: Some("sh".to_owned()),
                detach_key: 0x1c,
                copy_mode_key: 0,
                history_lines: 1,
                log_capture_interval_seconds: 5,
            };
            let error = handle_child_input(&tmux, &config, "test", &child, &[config.detach_key])
                .expect_err("missing client PID should fail");
            assert!(error.contains("attach PID"), "{error}");
            assert!(error.contains("was not found"), "{error}");
            assert!(matches!(
                waitpid(child.pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
                Err(Errno::ECHILD)
            ));
        }

        #[test]
        fn non_tty_terminal_guard_does_not_install_a_panic_hook() {
            let guard = TerminalGuard::new().expect("read test terminal state");
            assert!(guard.original.is_none());
        }

        #[cfg(not(target_os = "macos"))]
        #[allow(unsafe_code)]
        #[test]
        fn panic_hook_restores_the_attach_terminal_state() {
            let _lock = relay_global_state_lock();
            let result = unsafe { forkpty(None::<&Winsize>, None::<&Termios>) }
                .expect("allocate panic-hook PTY");
            match result {
                ForkptyResult::Child => {
                    // The libtest panic hook may hold a process-global lock in
                    // another thread when forkpty returns.  Avoid invoking it
                    // from the child, where that lock can never be released.
                    panic::set_hook(Box::new(|_| {}));
                    let panic_result = std::panic::catch_unwind(|| {
                        let _guard = TerminalGuard::new().expect("enter child raw mode");
                        panic!("exercise terminal panic hook");
                    });
                    assert!(panic_result.is_err());
                    unsafe { libc::_exit(0) };
                }
                ForkptyResult::Parent { child, master } => {
                    let before = termios::tcgetattr(master.as_fd()).expect("read PTY state");
                    waitpid(child, None).expect("reap panic-hook child");
                    let after = termios::tcgetattr(master.as_fd()).expect("read restored state");
                    assert_eq!(before, after);
                }
            }
        }
    }
}

#[cfg(unix)]
pub use unix::{attach, attach_with_input};

#[cfg(not(unix))]
pub fn attach(_: &Tmux, _: &Config, _: &str, _: AttachOptions<'_>) -> Result<u8, String> {
    Err("interactive PTY attachment is unsupported on this platform".to_owned())
}

#[cfg(not(unix))]
pub fn attach_with_input(
    tmux: &Tmux,
    config: &Config,
    session_name: &str,
    options: AttachOptions<'_>,
    _: &[u8],
) -> Result<u8, String> {
    attach(tmux, config, session_name, options)
}

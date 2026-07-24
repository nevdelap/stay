//! The interactive PTY relay used for tmux attachments.

use crate::config::Config;
use crate::tmux::Tmux;

#[cfg(unix)]
mod unix {
    use super::{Config, Tmux};
    use nix::errno::Errno;
    use nix::libc;
    use nix::poll::{poll, PollFd, PollFlags};
    use nix::pty::{forkpty, ForkptyResult, Winsize};
    use nix::sys::signal::{self, kill, SaFlags, SigAction, SigHandler, SigSet, Signal};
    use nix::sys::termios::{self, SetArg, Termios};
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::execvp;
    use std::ffi::CString;
    use std::io::{self, Write};
    use std::os::fd::{AsFd, AsRawFd, OwnedFd};
    use std::panic;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

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
    pub fn attach(tmux: &Tmux, config: &Config, session_name: &str) -> Result<u8, String> {
        let (program, arguments) = tmux.attach_program_and_arguments(session_name);
        let program = CString::new(program.as_encoded_bytes())
            .map_err(|_| "tmux executable contains a NUL byte".to_owned())?;
        let mut exec_arguments = vec![CString::new(program.as_bytes())
            .map_err(|_| "tmux executable contains a NUL byte".to_owned())?];
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
        relay_loop(tmux, config, session_name, &child)
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
    ) -> Result<u8, String> {
        let stdin = io::stdin();
        let mut stdin_open = true;
        let mut child_output_open = true;
        let mut last_winsize = current_winsize();

        while child_output_open {
            if TERMINATE_REQUESTED.swap(false, Ordering::Relaxed) {
                if tmux.detach_client(session_name).is_err() {
                    stop_attach_child(child.pid);
                    child_output_open = false;
                }
                stdin_open = false;
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
                    Ok(length) => io::stdout()
                        .write_all(&output[..length])
                        .map_err(|error| format!("relay output failed: {error}"))?,
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
                        handle_input(tmux, config, session_name, &child.master, &input[..length])?;
                    }
                    Err(Errno::EINTR) => {}
                    Err(error) => return Err(format!("relay input failed: {error}")),
                }
            }
        }

        reap_child(child.pid)?;
        Ok(tmux.pane_exit_status(session_name)?.unwrap_or(0))
    }

    fn handle_input(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        master: &OwnedFd,
        input: &[u8],
    ) -> Result<(), String> {
        let mut forwarded = Vec::with_capacity(input.len());
        for &byte in input {
            if byte == config.detach_key {
                write_input(master, &forwarded)?;
                forwarded.clear();
                tmux.detach_client(session_name)?;
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
        if let Some(winsize) = winsize {
            if Some(winsize) != *last_winsize {
                set_winsize(fd, winsize);
                *last_winsize = Some(winsize);
            }
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
                if let Ok(mut state) = hook_state.lock() {
                    if let Some(original) = state.take() {
                        let stdin = io::stdin();
                        let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &original);
                    }
                }
                if let Ok(previous) = hook_previous.lock() {
                    if let Some(previous) = previous.as_ref() {
                        previous(info);
                    }
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

        #[test]
        fn configured_control_bytes_are_distinct() {
            let config = crate::config::Config {
                default_command: "sh".to_owned(),
                detach_key: 0x1c,
                copy_mode_key: 0,
                history_lines: 1,
            };
            assert_ne!(config.detach_key, config.copy_mode_key);
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
                default_command: "sh".to_owned(),
                detach_key: 0x1c,
                copy_mode_key: 0,
                history_lines: 1,
            };
            let started = Instant::now();
            let error = relay_loop(&tmux, &config, "test", &child)
                .expect_err("pane status shim should fail");
            assert!(error.contains("tmux command failed"), "{error}");
            assert!(started.elapsed() < Duration::from_secs(1));
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
pub use unix::attach;

#[cfg(not(unix))]
pub fn attach(_: &Tmux, _: &Config, _: &str) -> Result<u8, String> {
    Err("interactive PTY attachment is unsupported on this platform".to_owned())
}

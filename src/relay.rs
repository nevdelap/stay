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
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
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

    const MAX_PENDING_INPUT: usize = 64 * 1024;
    const PANE_POLL_INTERVAL: Duration = Duration::from_millis(500);

    static TERMINATE_REQUESTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn request_termination(_: libc::c_int) {
        TERMINATE_REQUESTED.store(true, Ordering::Relaxed);
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
        let _signals = match SignalGuard::install() {
            Ok(guard) => guard,
            Err(error) => return Err(abort_attach_child(child.pid, error)),
        };
        let _terminal = match TerminalGuard::new() {
            Ok(guard) => guard,
            Err(error) => return Err(abort_attach_child(child.pid, error)),
        };
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
            ForkptyResult::Parent { child, master } => {
                if let Err(error) = set_nonblocking(&master) {
                    return Err(abort_attach_child(child, error));
                }
                Ok(AttachChild { pid: child, master })
            }
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
        log_session: Option<LogSession>,
    ) -> Result<u8, String> {
        let mut cleanup = AttachCleanup::new(child.pid);
        let input = RelayLoopInput {
            tmux,
            config,
            session_name,
            child,
            initial_input,
            attach_start,
            log_session,
        };
        match relay_loop_inner(input, &mut cleanup) {
            Ok(status) => Ok(status),
            Err(error) => Err(cleanup.abort(error)),
        }
    }

    struct RelayLoopInput<'a> {
        tmux: &'a Tmux,
        config: &'a Config,
        session_name: &'a str,
        child: &'a AttachChild,
        initial_input: &'a [u8],
        attach_start: u64,
        log_session: Option<LogSession>,
    }

    fn relay_loop_inner(
        input: RelayLoopInput<'_>,
        cleanup: &mut AttachCleanup,
    ) -> Result<u8, String> {
        let RelayLoopInput {
            tmux,
            config,
            session_name,
            child,
            initial_input,
            attach_start,
            mut log_session,
        } = input;
        if let Some(log_session) = log_session.as_mut() {
            log_session.on_attach_open(tmux, session_name)?;
        }
        let log_interval = Duration::from_secs(config.log_capture_interval_seconds.max(1));
        let mut last_log_tick = Instant::now();

        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        let mut stdin_open = true;
        let mut child_output_open = true;
        let mut last_winsize = current_winsize();
        let mut last_pane_poll = Instant::now();
        let mut pending_input = PendingInput::default();
        queue_input(&mut pending_input, config, initial_input);

        while child_output_open {
            if TERMINATE_REQUESTED.swap(false, Ordering::Relaxed) {
                if !detach_client(tmux, session_name, child.pid) {
                    cleanup.stop();
                    child_output_open = false;
                }
                stdin_open = false;
            }

            if child_output_open && last_pane_poll.elapsed() >= PANE_POLL_INTERVAL {
                last_pane_poll = Instant::now();
                if pane_state(tmux, session_name)?.is_some_and(|state| {
                    state.dead && state.dead_time.is_some_and(|time| time >= attach_start)
                }) {
                    if !detach_client(tmux, session_name, child.pid) {
                        cleanup.stop();
                        child_output_open = false;
                    }
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

            drain_pending_input(tmux, session_name, child, &mut pending_input)?;

            let events = poll_relay(&stdin, &child.master, stdin_open, &pending_input)?;
            if events.master_readable {
                let mut output = [0_u8; 8192];
                match read_master_output(&child.master, &mut output)? {
                    MasterRead::Closed => child_output_open = false,
                    MasterRead::NoData => {}
                    MasterRead::Data(length) => {
                        forward_output(&mut stdout, &output[..length])?;
                    }
                }
            }

            if child_output_open
                && !events.master_closed
                && pending_input.wants_write()
                && events.master_writable
                && let Err(error) =
                    drain_pending_input(tmux, session_name, child, &mut pending_input)
            {
                return Err(error);
            }

            if child_output_open && stdin_open && events.stdin_ready {
                let mut input = [0_u8; 4096];
                match nix::unistd::read(stdin.as_fd(), &mut input) {
                    Ok(0) => stdin_open = false,
                    Ok(length) => queue_input(&mut pending_input, config, &input[..length]),
                    Err(Errno::EINTR) => {}
                    Err(error) => return Err(format!("relay input failed: {error}")),
                }
            }
        }

        if let Some(log_session) = log_session.as_mut() {
            log_session.on_detach(tmux, session_name)?;
        }
        let attach_status = cleanup.reap()?;
        if !cleanup.stopped() {
            attach_failure(attach_status).map_or(Ok(()), Err)?;
        }
        Ok(exit_status_for_attach(
            pane_state(tmux, session_name)?.as_ref(),
            attach_start,
        ))
    }

    #[derive(Default)]
    #[allow(clippy::struct_excessive_bools)]
    struct RelayPoll {
        stdin_ready: bool,
        master_readable: bool,
        master_writable: bool,
        master_closed: bool,
    }

    fn poll_relay(
        stdin: &io::Stdin,
        master: &OwnedFd,
        stdin_open: bool,
        pending_input: &PendingInput,
    ) -> Result<RelayPoll, String> {
        let mut pollfds = Vec::with_capacity(2);
        let stdin_index = if stdin_open && pending_input.len() < MAX_PENDING_INPUT {
            let index = pollfds.len();
            pollfds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLIN));
            Some(index)
        } else {
            None
        };
        let master_index = pollfds.len();
        let mut master_events = PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR;
        if pending_input.wants_write() {
            master_events.insert(PollFlags::POLLOUT);
        }
        pollfds.push(PollFd::new(master.as_fd(), master_events));

        match poll(&mut pollfds, 100u16) {
            Ok(_) => {}
            Err(Errno::EINTR) => return Ok(RelayPoll::default()),
            Err(error) => return Err(format!("relay poll failed: {error}")),
        }

        let master_events = pollfds[master_index]
            .revents()
            .unwrap_or_else(PollFlags::empty);
        let stdin_ready = stdin_index.is_some_and(|index| {
            pollfds[index]
                .revents()
                .unwrap_or_else(PollFlags::empty)
                .intersects(PollFlags::POLLIN | PollFlags::POLLHUP)
        });
        Ok(RelayPoll {
            stdin_ready,
            master_readable: master_events
                .intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR),
            master_writable: master_events.intersects(PollFlags::POLLOUT),
            master_closed: master_events.intersects(PollFlags::POLLHUP | PollFlags::POLLERR),
        })
    }

    fn detach_client(tmux: &Tmux, session_name: &str, child: nix::unistd::Pid) -> bool {
        tmux.detach_client(session_name, child.as_raw()).is_ok()
    }

    #[derive(Debug, Eq, PartialEq)]
    struct PaneState {
        dead: bool,
        dead_time: Option<u64>,
        dead_status: Option<u8>,
        dead_signal: Option<u8>,
    }

    fn pane_state(tmux: &Tmux, session_name: &str) -> Result<Option<PaneState>, String> {
        let output = tmux.run([
            "list-panes",
            "-t",
            session_name,
            "-F",
            "#{pane_dead}:#{pane_dead_time}:#{pane_dead_status}:#{pane_dead_signal}",
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
        let dead_signal = fields
            .next()
            .ok_or_else(|| format!("tmux pane state is missing its dead signal: {row:?}"))?;
        if fields.next().is_some() {
            return Err(format!("malformed tmux pane state: {row:?}"));
        }
        if !dead {
            if !dead_time.is_empty() || !dead_status.is_empty() || !dead_signal.is_empty() {
                return Err(format!("live tmux pane has dead fields: {row:?}"));
            }
            return Ok(PaneState {
                dead,
                dead_time: None,
                dead_status: None,
                dead_signal: None,
            });
        }

        // tmux can report `pane_dead` `1` for one or more polls before it
        // finishes stamping the death fields, leaving `dead_time` (and, in
        // the same window, `dead_status`/`dead_signal`) transiently empty -
        // not only when a normal exit leaves `dead_status` empty and a
        // signalled one leaves `dead_signal` empty instead. Treat all three
        // as optional whenever `dead` is true, the way
        // `Tmux::pane_exit_status` (and `parse_session_row`'s `dead_time`)
        // already treat `dead_status`: an unreported field this poll just
        // means the next poll, 500 ms later, sees the fully-stamped row,
        // rather than the relay treating a transient shape as a hard parse
        // error and aborting the attach (review_docs/TASK-055.md R001).
        let dead_time = (!dead_time.is_empty())
            .then(|| dead_time.parse::<u64>())
            .transpose()
            .map_err(|_| format!("invalid tmux pane dead time: {row:?}"))?;
        let dead_status = (!dead_status.is_empty())
            .then(|| dead_status.parse::<u8>())
            .transpose()
            .map_err(|_| format!("invalid tmux pane dead status: {row:?}"))?;
        let dead_signal = if dead_signal.is_empty() {
            None
        } else {
            Some(
                tmux::parse_dead_signal(dead_signal)
                    .ok_or_else(|| format!("invalid tmux pane dead signal: {row:?}"))?,
            )
        };
        Ok(PaneState {
            dead,
            dead_time,
            dead_status,
            dead_signal,
        })
    }

    fn exit_status_for_attach(state: Option<&PaneState>, attach_start: u64) -> u8 {
        let Some(state) = state
            .filter(|state| state.dead && state.dead_time.is_some_and(|time| time >= attach_start))
        else {
            return 0;
        };
        if let Some(signal) = state.dead_signal {
            return 128_u8.saturating_add(signal);
        }
        state.dead_status.unwrap_or(0)
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

    #[derive(Debug, Eq, PartialEq)]
    enum MasterRead {
        Data(usize),
        NoData,
        Closed,
    }

    fn read_master_output(master: &OwnedFd, output: &mut [u8]) -> Result<MasterRead, String> {
        loop {
            match nix::unistd::read(master, output) {
                Ok(0) | Err(Errno::EIO) => return Ok(MasterRead::Closed),
                Ok(length) => return Ok(MasterRead::Data(length)),
                Err(Errno::EINTR) => {}
                Err(error) if error == Errno::EAGAIN || error == Errno::EWOULDBLOCK => {
                    return Ok(MasterRead::NoData);
                }
                Err(error) => return Err(format!("relay PTY read failed: {error}")),
            }
        }
    }

    #[derive(Debug, Default)]
    struct PendingInput {
        items: std::collections::VecDeque<PendingItem>,
        length: usize,
    }

    #[derive(Debug)]
    enum PendingItem {
        Bytes { bytes: Vec<u8>, offset: usize },
        Detach,
        CopyMode,
    }

    impl PendingInput {
        fn len(&self) -> usize {
            self.length
        }

        fn wants_write(&self) -> bool {
            matches!(self.items.front(), Some(PendingItem::Bytes { .. }))
        }

        #[cfg(test)]
        fn is_empty(&self) -> bool {
            self.items.is_empty()
        }

        fn push_bytes(&mut self, bytes: Vec<u8>) {
            if bytes.is_empty() {
                return;
            }
            self.length += bytes.len();
            self.items
                .push_back(PendingItem::Bytes { bytes, offset: 0 });
        }

        fn push_action(&mut self, action: PendingItem) {
            self.length += 1;
            self.items.push_back(action);
        }

        fn remove_front_action(&mut self) -> PendingItem {
            self.length -= 1;
            self.items.pop_front().expect("pending action is present")
        }

        fn discard_front_bytes(&mut self) {
            let Some(PendingItem::Bytes { bytes, offset }) = self.items.pop_front() else {
                return;
            };
            self.length -= bytes.len() - offset;
        }
    }

    fn queue_input(pending: &mut PendingInput, config: &Config, input: &[u8]) {
        let mut forwarded = Vec::new();
        for &byte in input {
            let action = if byte == config.detach_key {
                Some(PendingItem::Detach)
            } else if byte == config.copy_mode_key {
                Some(PendingItem::CopyMode)
            } else {
                None
            };
            if let Some(action) = action {
                pending.push_bytes(std::mem::take(&mut forwarded));
                pending.push_action(action);
            } else {
                forwarded.push(byte);
            }
        }
        pending.push_bytes(forwarded);
    }

    fn drain_pending_input(
        tmux: &Tmux,
        session_name: &str,
        child: &AttachChild,
        pending: &mut PendingInput,
    ) -> Result<(), String> {
        loop {
            let Some(item) = pending.items.front_mut() else {
                return Ok(());
            };
            match item {
                PendingItem::Bytes { bytes, offset } => {
                    match write_input(&child.master, &bytes[*offset..])? {
                        WriteInput::Written(length) => {
                            *offset += length;
                            pending.length -= length;
                            if *offset == bytes.len() {
                                pending.items.pop_front();
                            }
                        }
                        WriteInput::WouldBlock => return Ok(()),
                        WriteInput::Closed => {
                            pending.discard_front_bytes();
                        }
                    }
                }
                PendingItem::Detach | PendingItem::CopyMode => {
                    let action = pending.remove_front_action();
                    match action {
                        PendingItem::Detach => {
                            tmux.detach_client(session_name, child.pid.as_raw())?;
                        }
                        PendingItem::CopyMode => tmux.copy_mode(session_name)?,
                        PendingItem::Bytes { .. } => unreachable!("pending bytes are not actions"),
                    }
                }
            }
        }
    }

    #[cfg(test)]
    fn handle_child_input(
        tmux: &Tmux,
        config: &Config,
        session_name: &str,
        child: &AttachChild,
        input: &[u8],
    ) -> Result<(), String> {
        let mut cleanup = AttachCleanup::new(child.pid);
        let mut pending = PendingInput::default();
        queue_input(&mut pending, config, input);
        while !pending.is_empty() {
            if let Err(error) = drain_pending_input(tmux, session_name, child, &mut pending) {
                return Err(cleanup.abort(error));
            }
            if pending.is_empty() {
                break;
            }
            let mut pollfd = [PollFd::new(
                child.master.as_fd(),
                PollFlags::POLLOUT | PollFlags::POLLHUP | PollFlags::POLLERR,
            )];
            match poll(&mut pollfd, 100u16) {
                Ok(_) | Err(Errno::EINTR) => {}
                Err(error) => return Err(cleanup.abort(format!("relay poll failed: {error}"))),
            }
        }
        Ok(())
    }

    enum WriteInput {
        Written(usize),
        WouldBlock,
        Closed,
    }

    fn write_input(master: &OwnedFd, input: &[u8]) -> Result<WriteInput, String> {
        if input.is_empty() {
            return Ok(WriteInput::Written(0));
        }
        loop {
            match nix::unistd::write(master, input) {
                Ok(length) => return Ok(WriteInput::Written(length)),
                Err(Errno::EINTR) => {}
                Err(error) if error == Errno::EAGAIN || error == Errno::EWOULDBLOCK => {
                    return Ok(WriteInput::WouldBlock);
                }
                Err(Errno::EIO | Errno::EPIPE) => return Ok(WriteInput::Closed),
                Err(error) => return Err(format!("relay input write failed: {error}")),
            }
        }
    }

    fn set_nonblocking(master: &OwnedFd) -> Result<(), String> {
        let flags = fcntl(master, FcntlArg::F_GETFL)
            .map_err(|error| format!("failed to read attach PTY flags: {error}"))?;
        let flags = OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK;
        fcntl(master, FcntlArg::F_SETFL(flags))
            .map_err(|error| format!("failed to set attach PTY nonblocking: {error}"))?;
        Ok(())
    }

    struct AttachCleanup {
        pid: nix::unistd::Pid,
        stopped: bool,
        reaped: bool,
    }

    impl AttachCleanup {
        fn new(pid: nix::unistd::Pid) -> Self {
            Self {
                pid,
                stopped: false,
                reaped: false,
            }
        }

        fn stopped(&self) -> bool {
            self.stopped
        }

        fn stop(&mut self) {
            if !self.stopped {
                stop_attach_child(self.pid);
                self.stopped = true;
            }
        }

        fn reap(&mut self) -> Result<WaitStatus, String> {
            if self.reaped {
                return Err("tmux attach child was already reaped".to_owned());
            }
            self.reaped = true;
            reap_child(self.pid)
        }

        fn abort(&mut self, error: String) -> String {
            if self.reaped {
                return error;
            }
            self.stop();
            match self.reap() {
                Ok(_) => error,
                Err(reap_error) => format!("{error}; {reap_error}"),
            }
        }
    }

    fn abort_attach_child(pid: nix::unistd::Pid, error: String) -> String {
        AttachCleanup::new(pid).abort(error)
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
        term: SigAction,
        int: SigAction,
        hup: SigAction,
        pipe: SigAction,
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
            let previous_int = match unsafe { signal::sigaction(Signal::SIGINT, &term_action) } {
                Ok(previous) => previous,
                Err(error) => {
                    let _ = unsafe { signal::sigaction(Signal::SIGTERM, &previous_term) };
                    return Err(format!("failed to install SIGINT handler: {error}"));
                }
            };
            let previous_hup = match unsafe { signal::sigaction(Signal::SIGHUP, &term_action) } {
                Ok(previous) => previous,
                Err(error) => {
                    let _ = unsafe { signal::sigaction(Signal::SIGINT, &previous_int) };
                    let _ = unsafe { signal::sigaction(Signal::SIGTERM, &previous_term) };
                    return Err(format!("failed to install SIGHUP handler: {error}"));
                }
            };
            let previous_pipe = unsafe { signal::sigaction(Signal::SIGPIPE, &pipe_action) }
                .map_err(|error| {
                    let _ = unsafe { signal::sigaction(Signal::SIGHUP, &previous_hup) };
                    let _ = unsafe { signal::sigaction(Signal::SIGINT, &previous_int) };
                    let _ = unsafe { signal::sigaction(Signal::SIGTERM, &previous_term) };
                    format!("failed to ignore SIGPIPE: {error}")
                })?;
            Ok(Self {
                term: previous_term,
                int: previous_int,
                hup: previous_hup,
                pipe: previous_pipe,
            })
        }
    }

    impl Drop for SignalGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            let _ = unsafe { signal::sigaction(Signal::SIGTERM, &self.term) };
            let _ = unsafe { signal::sigaction(Signal::SIGINT, &self.int) };
            let _ = unsafe { signal::sigaction(Signal::SIGHUP, &self.hup) };
            let _ = unsafe { signal::sigaction(Signal::SIGPIPE, &self.pipe) };
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
                parse_pane_state_row("0:::"),
                Ok(PaneState {
                    dead: false,
                    dead_time: None,
                    dead_status: None,
                    dead_signal: None,
                })
            );
            assert_eq!(
                parse_pane_state_row("1:12345:7:"),
                Ok(PaneState {
                    dead: true,
                    dead_time: Some(12345),
                    dead_status: Some(7),
                    dead_signal: None,
                })
            );
        }

        #[test]
        fn a_signalled_pane_has_no_exit_status_but_parses_its_signal() {
            // tmux never publishes both fields for the same pane; the
            // exit-status field is empty exactly when a pane died from a
            // signal, matching the real row shape from a `SIGKILL`ed
            // command (verified against tmux 3.6a).
            assert_eq!(
                parse_pane_state_row("1:1785465643::"),
                Ok(PaneState {
                    dead: true,
                    dead_time: Some(1_785_465_643),
                    dead_status: None,
                    dead_signal: None,
                })
            );
            assert_eq!(
                parse_pane_state_row("1:1785465643::9"),
                Ok(PaneState {
                    dead: true,
                    dead_time: Some(1_785_465_643),
                    dead_status: None,
                    dead_signal: Some(9),
                })
            );
        }

        #[test]
        fn a_named_dead_signal_resolves_to_the_same_number() {
            // Some tmux builds render `pane_dead_signal` as the platform's
            // short signal name (`sig2name()`) rather than the raw number
            // - verified empirically against a real `SIGKILL`ed pane on
            // tmux 3.7b (macOS), which reports "kill" where tmux 3.4
            // (Linux) reports "9".
            assert_eq!(
                parse_pane_state_row("1:1785465643::kill"),
                Ok(PaneState {
                    dead: true,
                    dead_time: Some(1_785_465_643),
                    dead_status: None,
                    dead_signal: Some(9),
                })
            );
        }

        #[test]
        fn a_dead_pane_with_no_fields_stamped_yet_parses_instead_of_erroring() {
            // tmux can report `pane_dead` `1` for one or more polls before
            // it finishes stamping `pane_dead_time`, `pane_dead_status`,
            // and `pane_dead_signal` - reproduced under concurrent load
            // during TASK-055 review (review_docs/TASK-055.md R001), which
            // made the relay's poll error out and abort the attach instead
            // of simply waiting for the next poll to see the stamped row.
            assert_eq!(
                parse_pane_state_row("1:::"),
                Ok(PaneState {
                    dead: true,
                    dead_time: None,
                    dead_status: None,
                    dead_signal: None,
                })
            );
        }

        #[test]
        fn only_deaths_during_attach_propagate_their_status() {
            let live = PaneState {
                dead: false,
                dead_time: None,
                dead_status: None,
                dead_signal: None,
            };
            let before_attach = PaneState {
                dead: true,
                dead_time: Some(99),
                dead_status: Some(5),
                dead_signal: None,
            };
            let during_attach = PaneState {
                dead: true,
                dead_time: Some(100),
                dead_status: Some(7),
                dead_signal: None,
            };
            let not_yet_stamped = PaneState {
                dead: true,
                dead_time: None,
                dead_status: None,
                dead_signal: None,
            };

            assert_eq!(exit_status_for_attach(None, 100), 0);
            assert_eq!(exit_status_for_attach(Some(&live), 100), 0);
            assert_eq!(exit_status_for_attach(Some(&before_attach), 100), 0);
            assert_eq!(exit_status_for_attach(Some(&during_attach), 100), 7);
            assert_eq!(exit_status_for_attach(Some(&not_yet_stamped), 100), 0);
        }

        #[test]
        fn a_death_by_signal_during_attach_reports_128_plus_the_signal() {
            let killed_during_attach = PaneState {
                dead: true,
                dead_time: Some(100),
                dead_status: None,
                dead_signal: Some(9),
            };
            let killed_before_attach = PaneState {
                dead: true,
                dead_time: Some(99),
                dead_status: None,
                dead_signal: Some(9),
            };

            assert_eq!(
                exit_status_for_attach(Some(&killed_during_attach), 100),
                137
            );
            assert_eq!(exit_status_for_attach(Some(&killed_before_attach), 100), 0);
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
        fn a_stale_master_readiness_event_with_no_data_keeps_the_relay_alive() {
            let pair = nix::pty::openpty(None, None).expect("allocate test PTY");
            set_nonblocking(&pair.master).expect("set test PTY nonblocking");
            nix::unistd::write(&pair.slave, b"ready").expect("write test PTY data");
            let mut pollfds = [PollFd::new(pair.master.as_fd(), PollFlags::POLLIN)];
            assert_eq!(poll(&mut pollfds, 100u16).expect("poll test PTY"), 1);

            let mut output = [0_u8; 16];
            assert_eq!(
                read_master_output(&pair.master, &mut output),
                Ok(MasterRead::Data(5))
            );
            // Model another consumer draining the bytes after poll reported
            // readiness. The stale event must be treated as no data, not as a
            // fatal relay error.
            assert_eq!(
                read_master_output(&pair.master, &mut output),
                Ok(MasterRead::NoData)
            );
        }

        #[test]
        fn closed_byte_writes_preserve_queued_controls_in_fifo_order() {
            let pair = nix::pty::openpty(None, None).expect("allocate test PTY");
            drop(pair.slave);
            let log = crate::test_support::TempPath::file("stay-relay-control-order");
            let script = format!(
                "if [ \"$2\" = \"list-clients\" ]; then printf '41:/dev/pts/8\\n'; exit 0; fi; printf '%s\\n' \"$2\" >> '{}'; exit 0",
                log.display()
            );
            let tmux = Tmux::for_test_shell_script(script);
            let config = Config {
                default_command: Some("sh".to_owned()),
                detach_key: 0x1c,
                copy_mode_key: 0,
                history_lines: 1,
                log_capture_interval_seconds: 5,
            };
            let child = AttachChild {
                pid: nix::unistd::Pid::from_raw(41),
                master: pair.master,
            };
            let mut input = b" first ".to_vec();
            input.push(config.detach_key);
            input.extend_from_slice(b" second ");
            input.push(config.copy_mode_key);

            handle_child_input(&tmux, &config, "test", &child, &input)
                .expect("queued controls should survive a closed PTY write");
            assert_eq!(
                std::fs::read_to_string(&log)
                    .expect("read control-order log")
                    .lines()
                    .collect::<Vec<_>>(),
                ["detach-client", "copy-mode"]
            );
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
            let original_term = unsafe { signal::sigaction(Signal::SIGTERM, &default) }
                .expect("set SIGTERM default disposition");
            let original_int = unsafe { signal::sigaction(Signal::SIGINT, &default) }
                .expect("set SIGINT default disposition");
            let original_hup = unsafe { signal::sigaction(Signal::SIGHUP, &default) }
                .expect("set SIGHUP default disposition");
            let original = unsafe { signal::sigaction(Signal::SIGPIPE, &default) }
                .expect("set SIGPIPE default disposition");
            let guard = SignalGuard::install().expect("install relay signal handlers");
            let during_term = unsafe { signal::sigaction(Signal::SIGTERM, &default) }
                .expect("read relay SIGTERM disposition");
            let during_int = unsafe { signal::sigaction(Signal::SIGINT, &default) }
                .expect("read relay SIGINT disposition");
            let during_hup = unsafe { signal::sigaction(Signal::SIGHUP, &default) }
                .expect("read relay SIGHUP disposition");
            let during = unsafe { signal::sigaction(Signal::SIGPIPE, &ignore) }
                .expect("read relay SIGPIPE disposition");
            assert!(matches!(during_term.handler(), SigHandler::Handler(_)));
            assert!(matches!(during_int.handler(), SigHandler::Handler(_)));
            assert!(matches!(during_hup.handler(), SigHandler::Handler(_)));
            assert!(matches!(during.handler(), SigHandler::SigIgn));
            drop(guard);
            let restored_term = unsafe { signal::sigaction(Signal::SIGTERM, &ignore) }
                .expect("read restored SIGTERM disposition");
            let restored_int = unsafe { signal::sigaction(Signal::SIGINT, &ignore) }
                .expect("read restored SIGINT disposition");
            let restored_hup = unsafe { signal::sigaction(Signal::SIGHUP, &ignore) }
                .expect("read restored SIGHUP disposition");
            let restored = unsafe { signal::sigaction(Signal::SIGPIPE, &ignore) }
                .expect("read restored SIGPIPE disposition");
            assert!(matches!(restored_term.handler(), SigHandler::SigDfl));
            assert!(matches!(restored_int.handler(), SigHandler::SigDfl));
            assert!(matches!(restored_hup.handler(), SigHandler::SigDfl));
            assert!(matches!(restored.handler(), SigHandler::SigDfl));
            unsafe { signal::sigaction(Signal::SIGTERM, &original_term) }
                .expect("restore test SIGTERM disposition");
            unsafe { signal::sigaction(Signal::SIGINT, &original_int) }
                .expect("restore test SIGINT disposition");
            unsafe { signal::sigaction(Signal::SIGHUP, &original_hup) }
                .expect("restore test SIGHUP disposition");
            unsafe { signal::sigaction(Signal::SIGPIPE, &original) }
                .expect("restore test SIGPIPE disposition");
        }

        #[test]
        fn aborting_a_reaped_attach_does_not_stop_it_again() {
            let mut cleanup = AttachCleanup::new(nix::unistd::Pid::from_raw(41));
            cleanup.reaped = true;
            assert_eq!(
                cleanup.abort("post-reap failure".to_owned()),
                "post-reap failure"
            );
            assert!(!cleanup.stopped());
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
            assert!(matches!(
                waitpid(child.pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)),
                Err(Errno::ECHILD)
            ));
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
pub use unix::attach_with_input;

#[cfg(not(unix))]
pub fn attach_with_input(
    _: &Tmux,
    _: &Config,
    _: &str,
    _: AttachOptions<'_>,
    _: &[u8],
) -> Result<u8, String> {
    Err("interactive PTY attachment is unsupported on this platform".to_owned())
}

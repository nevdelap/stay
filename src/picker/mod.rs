//! The interactive session picker.

use crate::config::Config;
use crate::session;
use crate::session_name::parse_session_name;
use crate::tmux::{SessionRecord, Tmux};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::panic::{self, PanicHookInfo};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const ESCAPE_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(20);
const IDLE_STATUS: &str = "↑/↓ select · Enter attach · c create · k kill · r recreate · e edit name · v view-only · l low-priority · Esc quit";
const EMPTY_STATUS: &str = "c create · Esc quit";

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

/// How the caller wants the picker screen set up.
#[derive(Clone, Copy)]
pub enum ScreenPreference {
    /// Probe the terminal and use the alternate screen only if it works.
    Auto,
    /// `--no-alt-screen`: draw on the main screen, never the alternate.
    ForceMainScreen,
    /// `--alt-screen`: use the alternate screen, skipping the probe.
    ForceAlternateScreen,
}

/// Outcome of probing the terminal for alternate-screen support.
struct ProbeOutcome {
    alternate_screen: bool,
    /// Bytes read from stdin that were not part of a cursor-position
    /// reply (for example keystrokes that arrived during the probe). They
    /// are forwarded to the picker's input reader so they are not lost.
    leftover_input: Vec<u8>,
}

/// How long to wait for the terminal's cursor-position reply.
///
/// Generous enough to survive a mobile/SSH round trip; the probe makes at
/// most two queries, so the worst-case added startup latency is twice this.
const CURSOR_QUERY_TIMEOUT: Duration = Duration::from_millis(400);

/// Probe whether the terminal truly honours the alternate-screen escape
/// sequences, rather than merely advertising a `TERM` that claims it does.
///
/// Sends "enter alt screen → move cursor → leave alt screen" as a single
/// batched write (so a conformant terminal never renders the intermediate,
/// empty alt buffer), then compares the cursor position reported before
/// and after. A conformant terminal restores the cursor to where it was; a
/// terminal that silently ignores `?1049h`/`?1049l` (Termius, Conduit on
/// Android) leaves it where we moved it.
///
/// Requires raw mode to already be enabled. Any bytes that arrive on stdin
/// while waiting for a reply — keystrokes the user typed during the probe —
/// are captured in `leftover_input` rather than discarded.
///
/// Conservative on uncertainty: if the terminal does not answer a
/// cursor query (or the probe bytes cannot be written), the terminal is
/// treated as *not* supporting the alternate screen, since that is the
/// universally-safe mode. `--alt-screen` exists to override that.
#[cfg(unix)]
fn probe_alternate_screen() -> ProbeOutcome {
    // Report cursor position: ESC [ row ; col R
    const CURSOR_QUERY: &[u8] = b"\x1b[6n";
    // Enter alt screen, jump somewhere distinctive, leave — batched so a
    // conformant terminal processes the whole write before rendering and
    // never shows the intermediate alt buffer.
    const PROBE: &[u8] = b"\x1b[?1049h\x1b[3;3H\x1b[?1049l";

    let unsupported = |leftover: Vec<u8>| ProbeOutcome {
        alternate_screen: false,
        leftover_input: leftover,
    };

    let mut stdout = io::stdout();
    let send = |stdout: &mut io::Stdout, bytes: &[u8]| {
        stdout.write_all(bytes).is_ok() && stdout.flush().is_ok()
    };

    let mut leftover = Vec::new();

    if !send(&mut stdout, CURSOR_QUERY) {
        return unsupported(leftover);
    }
    let Some(before) = query_cursor(&mut leftover) else {
        return unsupported(leftover);
    };

    if !send(&mut stdout, PROBE) {
        return unsupported(leftover);
    }
    if !send(&mut stdout, CURSOR_QUERY) {
        return unsupported(leftover);
    }
    let Some(after) = query_cursor(&mut leftover) else {
        return unsupported(leftover);
    };

    // Conformant terminal: leaving alt screen restores the cursor. A
    // terminal that ignored the sequences left it at (3, 3).
    ProbeOutcome {
        alternate_screen: after == before,
        leftover_input: leftover,
    }
}

/// Read one cursor-position reply from stdin within `CURSOR_QUERY_TIMEOUT`.
///
/// Only a validated `ESC [ <row> ; <col> R` run is consumed; every other
/// byte (keystrokes, unrelated escape sequences) is appended to `leftover`
/// so it reaches the picker's input reader. Returns `None` when no reply
/// arrives in time, leaving whatever arrived in `leftover`.
#[cfg(unix)]
fn query_cursor(leftover: &mut Vec<u8>) -> Option<(u16, u16)> {
    use nix::errno::Errno;
    use nix::poll::{poll, PollFd, PollFlags};
    use nix::unistd::read;
    use std::os::fd::AsFd;

    let stdin = io::stdin();
    let fd = stdin.as_fd();
    let mut buf: Vec<u8> = Vec::with_capacity(16);
    let deadline = Instant::now() + CURSOR_QUERY_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let timeout =
            u16::try_from(remaining.as_millis().min(u128::from(u16::MAX))).unwrap_or(u16::MAX);
        let mut poll_fds = [PollFd::new(fd, PollFlags::POLLIN)];
        match poll(&mut poll_fds, timeout) {
            Ok(0) => break,
            Ok(_) => {}
            Err(Errno::EINTR) => continue,
            Err(_) => break,
        }
        let mut byte = [0_u8; 1];
        match read(fd, &mut byte) {
            Ok(0) => break,
            Ok(_) => buf.push(byte[0]),
            Err(Errno::EINTR) => continue,
            Err(_) => break,
        }
        if let Some((position, start, end)) = extract_cursor_response(&buf) {
            // Preserve anything the terminal sent that was not the cursor
            // reply itself, then hand back the position.
            leftover.extend_from_slice(&buf[..start]);
            leftover.extend_from_slice(&buf[end..]);
            return Some(position);
        }
        if buf.len() > 32 {
            break;
        }
    }
    // Timed out with no reply: whatever arrived is the user's input.
    leftover.extend_from_slice(&buf);
    None
}

#[cfg(not(unix))]
fn probe_alternate_screen() -> ProbeOutcome {
    // No raw byte-level probe on non-Unix; assume supported.
    ProbeOutcome {
        alternate_screen: true,
        leftover_input: Vec::new(),
    }
}

/// Find the first complete cursor-position report in `buf`.
///
/// Matches `ESC [ <digits> ; <digits> R` and returns the decoded
/// `(row, col)` plus the byte range `[start, end)` it occupies, so the
/// caller can preserve any bytes before or after it as user input. Returns
/// `None` when no complete, well-formed report is present.
fn extract_cursor_response(buf: &[u8]) -> Option<((u16, u16), usize, usize)> {
    for (esc_idx, &byte) in buf.iter().enumerate() {
        if byte != 0x1b {
            continue;
        }
        if buf.get(esc_idx + 1) != Some(&b'[') {
            continue;
        }
        // Try to parse a complete report starting at this ESC. On any
        // mismatch, continue to the next ESC rather than giving up — the
        // buffer may contain an unrelated escape sequence (e.g. an arrow
        // key) before the real cursor reply.
        let body = &buf[esc_idx + 2..];
        let Some(r_pos) = body.iter().position(|&b| b == b'R') else {
            continue;
        };
        let report = &body[..r_pos];
        let Some(semi) = report.iter().position(|&b| b == b';') else {
            continue;
        };
        let Some(row) = parse_ascii_u16(&report[..semi]) else {
            continue;
        };
        let Some(col) = parse_ascii_u16(&report[semi + 1..]) else {
            continue;
        };
        let end = esc_idx + 2 + r_pos + 1;
        return Some(((row, col), esc_idx, end));
    }
    None
}

/// Decode a slice of ASCII digit bytes as a `u16`, rejecting empties and
/// overflow. Used by [`extract_cursor_response`].
fn parse_ascii_u16(digits: &[u8]) -> Option<u16> {
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let mut value: u32 = 0;
    for &digit in digits {
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(digit - b'0'))?;
    }
    u16::try_from(value).ok()
}

/// Opens the picker and, when the user attaches, hands off to the relay.
///
/// `preference` controls screen setup: [`ScreenPreference::Auto`] probes the
/// terminal and uses the alternate screen only when it actually works;
/// `ForceMainScreen` (`--no-alt-screen`) and `ForceAlternateScreen`
/// (`--alt-screen`) override the probe.
///
/// # Errors
///
/// Returns an error when terminal setup, picker input/rendering, or the
/// selected session's attach operation fails.
pub fn run(tmux: &Tmux, config: &Config, preference: ScreenPreference) -> Result<u8, String> {
    if !io::stdout().is_terminal() {
        return Err("the interactive picker requires a terminal".to_owned());
    }

    let outcome = run_picker(tmux, config, preference)?;
    match outcome {
        PickerOutcome::Quit => Ok(0),
        PickerOutcome::Attach {
            session_name,
            residual_input,
        } => session::attach_session_with_input(tmux, config, &session_name, &[], &residual_input),
    }
}

enum PickerOutcome {
    Quit,
    Attach {
        session_name: String,
        residual_input: Vec<u8>,
    },
}

fn run_picker(
    tmux: &Tmux,
    config: &Config,
    preference: ScreenPreference,
) -> Result<PickerOutcome, String> {
    let (_terminal_guard, leftover) = TerminalGuard::enter(preference)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|error| format!("failed to initialize picker terminal: {error}"))?;
    // Seed the reader with anything the probe captured off stdin, so
    // keystrokes typed while the terminal was being probed are not lost.
    let mut input = InputReader::with_pending(leftover);
    let mut state = PickerState::default();
    let mut next_poll = Instant::now();

    loop {
        if Instant::now() >= next_poll {
            state.poll(tmux);
            next_poll = Instant::now() + POLL_INTERVAL;
        }

        terminal
            .draw(|frame| render(frame, &state))
            .map_err(|error| format!("failed to render picker: {error}"))?;

        if let Some(key) = input.next(Duration::from_millis(50))? {
            if let Some(outcome) = handle_key(&mut state, key, tmux, config, &mut input)? {
                return Ok(outcome);
            }
        }
    }
}

fn handle_key(
    state: &mut PickerState,
    key: PickerKey,
    tmux: &Tmux,
    config: &Config,
    input: &mut InputReader,
) -> Result<Option<PickerOutcome>, String> {
    match state.mode.clone() {
        PickerMode::Idle => handle_idle_key(state, key, tmux, config, input),
        PickerMode::Create { .. } => handle_create_key(state, key, tmux, config, input),
        PickerMode::EditName { .. } => Ok(handle_edit_name_key(state, key, tmux)),
        PickerMode::KillConfirm { .. } => Ok(handle_kill_key(state, key, tmux)),
    }
}

fn handle_idle_key(
    state: &mut PickerState,
    key: PickerKey,
    tmux: &Tmux,
    config: &Config,
    input: &mut InputReader,
) -> Result<Option<PickerOutcome>, String> {
    match key {
        PickerKey::Escape | PickerKey::Char('q') => Ok(Some(PickerOutcome::Quit)),
        PickerKey::Up => {
            state.clear_feedback();
            state.move_up();
            Ok(None)
        }
        PickerKey::Down => {
            state.clear_feedback();
            state.move_down();
            Ok(None)
        }
        PickerKey::Enter => {
            state.clear_feedback();
            state
                .selected_name
                .clone()
                .map(|session_name| {
                    input.drain_available().map(|residual_input| {
                        Some(PickerOutcome::Attach {
                            session_name,
                            residual_input,
                        })
                    })
                })
                .transpose()
                .map(Option::flatten)
        }
        PickerKey::Char('c') => {
            state.clear_feedback();
            state.mode = PickerMode::Create {
                input: String::new(),
            };
            Ok(None)
        }
        PickerKey::Char('k') => {
            state.clear_feedback();
            if let Some(session_name) = state.selected_name.clone() {
                state.mode = PickerMode::KillConfirm {
                    session_name,
                    selector: YesNoSelector::new(true),
                };
            }
            Ok(None)
        }
        PickerKey::Char('r') => {
            state.clear_feedback();
            if let Some(session_name) = state.selected_name.clone() {
                state.recreate(tmux, config, &session_name);
            }
            Ok(None)
        }
        PickerKey::Char('e') => {
            if let Some(session_name) = state.selected_name.clone() {
                state.clear_feedback();
                state.mode = PickerMode::EditName {
                    session_name,
                    input: String::new(),
                };
            }
            Ok(None)
        }
        PickerKey::Char('v') => {
            if state.selected_name.is_some() {
                state.clear_feedback();
                state.action_error = Some("v: not yet implemented".to_owned());
            }
            Ok(None)
        }
        PickerKey::Char('l') => {
            if state.selected_name.is_some() {
                state.clear_feedback();
                state.action_error = Some("l: not yet implemented".to_owned());
            }
            Ok(None)
        }
        PickerKey::Left
        | PickerKey::Right
        | PickerKey::Other
        | PickerKey::Backspace
        | PickerKey::Char(_) => {
            state.clear_feedback();
            Ok(None)
        }
    }
}

fn handle_create_key(
    state: &mut PickerState,
    key: PickerKey,
    tmux: &Tmux,
    config: &Config,
    input: &mut InputReader,
) -> Result<Option<PickerOutcome>, String> {
    match key {
        PickerKey::Escape => {
            state.mode = PickerMode::Idle;
            Ok(None)
        }
        PickerKey::Enter => {
            let name = state.create_name();
            match parse_session_name(&name) {
                Ok(session_name) => {
                    match session::create_session(tmux, config, &session_name, None, &[]) {
                        Ok(()) => input.drain_available().map(|residual_input| {
                            Some(PickerOutcome::Attach {
                                session_name,
                                residual_input,
                            })
                        }),
                        Err(error) => {
                            state.action_error = Some(error);
                            state.mode = PickerMode::Idle;
                            state.poll(tmux);
                            Ok(None)
                        }
                    }
                }
                Err(error) => {
                    state.action_error = Some(error);
                    state.mode = PickerMode::Idle;
                    Ok(None)
                }
            }
        }
        PickerKey::Backspace => {
            state.delete_create_character();
            Ok(None)
        }
        PickerKey::Char(character) => {
            state.push_create_character(character);
            Ok(None)
        }
        PickerKey::Up | PickerKey::Down | PickerKey::Left | PickerKey::Right | PickerKey::Other => {
            Ok(None)
        }
    }
}

fn handle_edit_name_key(
    state: &mut PickerState,
    key: PickerKey,
    tmux: &Tmux,
) -> Option<PickerOutcome> {
    match key {
        PickerKey::Escape => {
            state.mode = PickerMode::Idle;
            None
        }
        PickerKey::Enter => {
            let (old_name, new_name) = state.edit_name();
            match parse_session_name(&new_name) {
                Ok(new_name) => match tmux.rename_session(&old_name, &new_name) {
                    Ok(()) => {
                        state.selected_name = Some(new_name);
                        state.action_error = None;
                        state.mode = PickerMode::Idle;
                        state.poll(tmux);
                        None
                    }
                    Err(error) => {
                        state.action_error = Some(error);
                        state.mode = PickerMode::Idle;
                        None
                    }
                },
                Err(error) => {
                    state.action_error = Some(error);
                    state.mode = PickerMode::Idle;
                    None
                }
            }
        }
        PickerKey::Backspace => {
            state.delete_edit_name_character();
            None
        }
        PickerKey::Char(character) => {
            state.push_edit_name_character(character);
            None
        }
        PickerKey::Up | PickerKey::Down | PickerKey::Left | PickerKey::Right | PickerKey::Other => {
            None
        }
    }
}

fn handle_kill_key(state: &mut PickerState, key: PickerKey, tmux: &Tmux) -> Option<PickerOutcome> {
    let action = match &mut state.mode {
        PickerMode::KillConfirm { selector, .. } => selector.handle_key(key),
        PickerMode::Idle | PickerMode::Create { .. } | PickerMode::EditName { .. } => {
            YesNoAction::Cancel
        }
    };

    match action {
        YesNoAction::Confirm(YesNoOption::Yes) => {
            let session_name = state.confirm_name();
            match session::kill_session(tmux, &session_name) {
                Ok(()) => state.action_error = None,
                Err(error) => state.action_error = Some(error),
            }
            state.mode = PickerMode::Idle;
            state.poll(tmux);
        }
        YesNoAction::Confirm(YesNoOption::No) | YesNoAction::Cancel => {
            state.mode = PickerMode::Idle;
        }
        YesNoAction::Continue => {}
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum YesNoOption {
    Yes,
    No,
}

impl YesNoOption {
    fn label(self) -> &'static str {
        match self {
            Self::Yes => "Yes",
            Self::No => "No",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum YesNoAction {
    Continue,
    Confirm(YesNoOption),
    Cancel,
}

/// A reusable inline yes/no selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct YesNoSelector {
    focused: YesNoOption,
}

impl YesNoSelector {
    /// Create a selector, defaulting destructive actions to `No`.
    fn new(destructive: bool) -> Self {
        Self {
            focused: if destructive {
                YesNoOption::No
            } else {
                YesNoOption::Yes
            },
        }
    }

    fn focused_option(self) -> YesNoOption {
        self.focused
    }

    fn handle_key(&mut self, key: PickerKey) -> YesNoAction {
        match key {
            PickerKey::Char('y') => {
                self.focused = YesNoOption::Yes;
                YesNoAction::Confirm(YesNoOption::Yes)
            }
            PickerKey::Char('n') => {
                self.focused = YesNoOption::No;
                YesNoAction::Confirm(YesNoOption::No)
            }
            PickerKey::Left => {
                self.focused = YesNoOption::Yes;
                YesNoAction::Continue
            }
            PickerKey::Right => {
                self.focused = YesNoOption::No;
                YesNoAction::Continue
            }
            PickerKey::Enter => YesNoAction::Confirm(self.focused_option()),
            PickerKey::Escape
            | PickerKey::Up
            | PickerKey::Down
            | PickerKey::Backspace
            | PickerKey::Other
            | PickerKey::Char(_) => YesNoAction::Cancel,
        }
    }

    fn render(self) -> Line<'static> {
        Line::from(vec![
            self.option_span(YesNoOption::Yes),
            Span::raw(" "),
            self.option_span(YesNoOption::No),
        ])
    }

    fn option_span(self, option: YesNoOption) -> Span<'static> {
        let style = if self.focused == option {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        Span::styled(option.label(), style)
    }

    #[cfg(test)]
    fn text() -> &'static str {
        "Yes No"
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum PickerMode {
    #[default]
    Idle,
    Create {
        input: String,
    },
    EditName {
        session_name: String,
        input: String,
    },
    KillConfirm {
        session_name: String,
        selector: YesNoSelector,
    },
}

#[derive(Default)]
struct PickerState {
    sessions: Vec<SessionRecord>,
    selected_name: Option<String>,
    poll_error: Option<String>,
    action_error: Option<String>,
    mode: PickerMode,
}

impl PickerState {
    fn poll(&mut self, tmux: &Tmux) {
        self.apply_poll_result(tmux.list_sessions());
    }

    fn apply_poll_result(&mut self, result: Result<Vec<SessionRecord>, String>) {
        match result {
            Ok(sessions) => {
                if let Some(selected_name) = &self.selected_name {
                    if !sessions
                        .iter()
                        .any(|session| &session.name == selected_name)
                    {
                        self.selected_name = None;
                    }
                }
                self.sessions = sessions;
                self.poll_error = None;
            }
            Err(error) => self.poll_error = Some(error),
        }
    }

    fn clear_feedback(&mut self) {
        self.poll_error = None;
        self.action_error = None;
    }

    fn create_name(&self) -> String {
        match &self.mode {
            PickerMode::Create { input } => input.clone(),
            PickerMode::Idle | PickerMode::EditName { .. } | PickerMode::KillConfirm { .. } => {
                String::new()
            }
        }
    }

    fn push_create_character(&mut self, character: char) {
        if let PickerMode::Create { input } = &mut self.mode {
            input.push(character);
        }
    }

    fn delete_create_character(&mut self) {
        if let PickerMode::Create { input } = &mut self.mode {
            let _ = input.pop();
        }
    }

    fn edit_name(&self) -> (String, String) {
        match &self.mode {
            PickerMode::EditName {
                session_name,
                input,
            } => (session_name.clone(), input.clone()),
            PickerMode::Idle | PickerMode::Create { .. } | PickerMode::KillConfirm { .. } => {
                (String::new(), String::new())
            }
        }
    }

    fn push_edit_name_character(&mut self, character: char) {
        if let PickerMode::EditName { input, .. } = &mut self.mode {
            input.push(character);
        }
    }

    fn delete_edit_name_character(&mut self) {
        if let PickerMode::EditName { input, .. } = &mut self.mode {
            let _ = input.pop();
        }
    }

    fn confirm_name(&self) -> String {
        match &self.mode {
            PickerMode::KillConfirm { session_name, .. } => session_name.clone(),
            PickerMode::Idle | PickerMode::Create { .. } | PickerMode::EditName { .. } => {
                String::new()
            }
        }
    }

    fn recreate(&mut self, tmux: &Tmux, config: &Config, session_name: &str) {
        match session::force_recreate_session(tmux, config, session_name, None, &[]) {
            Ok(()) => self.action_error = None,
            Err(error) => self.action_error = Some(error),
        }
        self.poll(tmux);
    }

    fn move_up(&mut self) {
        let Some(first) = self.sessions.first() else {
            return;
        };
        let index = self.selected_index().unwrap_or(self.sessions.len());
        let next = index.saturating_sub(1);
        self.selected_name = Some(self.sessions.get(next).unwrap_or(first).name.clone());
    }

    fn move_down(&mut self) {
        let Some(first) = self.sessions.first() else {
            return;
        };
        let index = self.selected_index().unwrap_or(usize::MAX);
        let next = if index == usize::MAX {
            0
        } else {
            index.saturating_add(1).min(self.sessions.len() - 1)
        };
        self.selected_name = Some(self.sessions.get(next).unwrap_or(first).name.clone());
    }

    fn selected_index(&self) -> Option<usize> {
        self.selected_name.as_ref().and_then(|name| {
            self.sessions
                .iter()
                .position(|session| &session.name == name)
        })
    }

    fn status(&self) -> &str {
        if let Some(error) = &self.action_error {
            return error;
        }
        if let Some(error) = &self.poll_error {
            return error;
        }
        if self.sessions.is_empty() {
            EMPTY_STATUS
        } else {
            IDLE_STATUS
        }
    }

    #[cfg(test)]
    fn prompt(&self) -> Option<String> {
        match &self.mode {
            PickerMode::Create { input } => Some(format!("New session name: {input}█")),
            PickerMode::EditName {
                session_name,
                input,
            } => Some(format!("Edit name \"{session_name}\" to: {input}█")),
            PickerMode::KillConfirm { session_name, .. } => Some(format!(
                "Kill session \"{session_name}\"? {}",
                YesNoSelector::text()
            )),
            PickerMode::Idle => None,
        }
    }

    fn prompt_line(&self) -> Option<Line<'static>> {
        match &self.mode {
            PickerMode::Create { input } => Some(Line::from(format!("New session name: {input}█"))),
            PickerMode::EditName {
                session_name,
                input,
            } => Some(Line::from(format!(
                "Edit name \"{session_name}\" to: {input}█"
            ))),
            PickerMode::KillConfirm {
                session_name,
                selector,
            } => {
                let mut line = Line::from(format!("Kill session \"{session_name}\"? "));
                for span in selector.render().spans {
                    line.push_span(span);
                }
                Some(line)
            }
            PickerMode::Idle => None,
        }
    }
}

fn render(frame: &mut Frame<'_>, state: &PickerState) {
    let area = frame.area();
    let block = Block::default()
        .title(" stay ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let status_line = state
        .prompt_line()
        .unwrap_or_else(|| Line::from(state.status()));
    let inner_width = inner.width as usize;
    let status_height = u16::try_from(
        status_line
            .width()
            .saturating_add(inner_width.saturating_sub(1))
            / inner_width,
    )
    .unwrap_or(u16::MAX)
    .max(1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(status_height),
        ])
        .split(inner);
    let list_area = chunks[0];
    let separator_area = chunks[1];
    let status_area = chunks[2];

    for (index, session) in state.sessions.iter().enumerate() {
        if index >= list_area.height as usize {
            break;
        }
        let row_area = Rect {
            x: list_area.x,
            y: list_area.y + u16::try_from(index).unwrap_or(u16::MAX),
            width: list_area.width,
            height: 1,
        };
        let selected = state.selected_index() == Some(index);
        let text = session_row(session, selected, row_area.width);
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        frame.render_widget(Paragraph::new(text).style(style), row_area);
    }

    if state.sessions.is_empty() {
        let text = "(no sessions)";
        let text_width = u16::try_from(text.len()).unwrap_or(u16::MAX);
        let x = inner
            .x
            .saturating_add(inner.width.saturating_sub(text_width) / 2);
        let row_area = Rect {
            x,
            y: list_area.y + list_area.height / 2,
            width: list_area.width.saturating_sub(x.saturating_sub(inner.x)),
            height: 1,
        };
        frame.render_widget(Paragraph::new(text), row_area);
    }

    frame.render_widget(
        Paragraph::new("─".repeat(separator_area.width as usize)),
        separator_area,
    );
    frame.render_widget(
        Paragraph::new(status_line).wrap(Wrap { trim: false }),
        status_area,
    );
}

fn session_row(session: &SessionRecord, selected: bool, width: u16) -> Line<'static> {
    let width = width as usize;
    let suffix = fitted_suffix(session, width);
    let suffix_width = suffix_display_width(&suffix);
    let available = width.saturating_sub(suffix_width);
    let mut row = truncate_to_width(&session.name, available);
    let row_width = UnicodeWidthStr::width(row.as_str());
    row.push_str(&" ".repeat(width.saturating_sub(row_width + suffix_width)));
    let mut spans = vec![Span::raw(row)];
    for suffix_span in suffix {
        let style = if !selected && suffix_span.emphasis {
            Style::default().fg(Color::Red)
        } else {
            Style::default()
        };
        spans.push(Span::styled(suffix_span.text, style));
    }
    Line::from(spans)
}

fn fitted_suffix(session: &SessionRecord, width: usize) -> Vec<crate::tmux::SuffixSpan> {
    let full = session.status_detail();
    if !session.terminated || suffix_display_width(&full) <= width {
        return full;
    }

    let without_time = vec![
        full[0].clone(),
        full[1].clone(),
        crate::tmux::SuffixSpan {
            text: "]".to_owned(),
            emphasis: false,
        },
    ];
    if suffix_display_width(&without_time) <= width {
        return without_time;
    }

    let marker = vec![crate::tmux::SuffixSpan {
        text: format!(" [{}]", session.status_word()),
        emphasis: false,
    }];
    if suffix_display_width(&marker) <= width {
        return marker;
    }
    truncate_suffix(&marker, width)
}

fn suffix_display_width(suffix: &[crate::tmux::SuffixSpan]) -> usize {
    suffix
        .iter()
        .map(|span| UnicodeWidthStr::width(span.text.as_str()))
        .sum()
}

fn truncate_suffix(
    suffix: &[crate::tmux::SuffixSpan],
    width: usize,
) -> Vec<crate::tmux::SuffixSpan> {
    let mut remaining = width;
    let mut result = Vec::new();
    for span in suffix {
        if remaining == 0 {
            break;
        }
        let text = truncate_to_width(&span.text, remaining);
        let used = UnicodeWidthStr::width(text.as_str());
        if used > 0 {
            result.push(crate::tmux::SuffixSpan {
                text,
                emphasis: span.emphasis,
            });
            remaining -= used;
        }
    }
    result
}

fn truncate_to_width(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PickerKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Char(char),
    Other,
}

struct InputReader {
    pending: VecDeque<u8>,
}

impl InputReader {
    #[cfg(test)]
    fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }

    /// Build a reader preloaded with `pending` bytes (in arrival order),
    /// e.g. keystrokes the screen probe captured before the reader existed.
    fn with_pending(pending: Vec<u8>) -> Self {
        let mut queue = VecDeque::with_capacity(pending.len());
        queue.extend(pending);
        Self { pending: queue }
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<PickerKey>, String> {
        let Some(byte) = self.read_byte(timeout)? else {
            return Ok(None);
        };
        let key = match byte {
            b'\r' | b'\n' => PickerKey::Enter,
            0x1b => self.escape_or_quit()?,
            0x08 | 0x7f => PickerKey::Backspace,
            0x01 => PickerKey::Up,
            0x02 => PickerKey::Down,
            0x03 => PickerKey::Left,
            0x04 => PickerKey::Right,
            byte if byte.is_ascii() => PickerKey::Char(char::from(byte)),
            byte => self.read_utf8(byte, timeout)?,
        };
        Ok(Some(key))
    }

    fn escape_or_quit(&mut self) -> Result<PickerKey, String> {
        let Some(next) = self.read_byte(ESCAPE_SEQUENCE_TIMEOUT)? else {
            return Ok(PickerKey::Escape);
        };
        if next != b'[' && next != b'O' {
            self.pending.push_front(next);
            return Ok(PickerKey::Escape);
        }
        let Some(direction) = self.read_byte(ESCAPE_SEQUENCE_TIMEOUT)? else {
            self.pending.push_front(next);
            return Ok(PickerKey::Escape);
        };
        match direction {
            b'A' => Ok(PickerKey::Up),
            b'B' => Ok(PickerKey::Down),
            b'C' => Ok(PickerKey::Right),
            b'D' => Ok(PickerKey::Left),
            _ => Ok(PickerKey::Other),
        }
    }

    fn read_utf8(&mut self, first: u8, timeout: Duration) -> Result<PickerKey, String> {
        let length = if first & 0xe0 == 0xc0 {
            2
        } else if first & 0xf0 == 0xe0 {
            3
        } else if first & 0xf8 == 0xf0 {
            4
        } else {
            return Ok(PickerKey::Other);
        };
        let mut bytes = vec![first];
        for _ in 1..length {
            let Some(byte) = self.read_byte(timeout)? else {
                return Ok(PickerKey::Other);
            };
            if byte & 0xc0 != 0x80 {
                self.pending.push_front(byte);
                return Ok(PickerKey::Other);
            }
            bytes.push(byte);
        }
        match std::str::from_utf8(&bytes) {
            Ok(character) => Ok(PickerKey::Char(
                character.chars().next().unwrap_or('\u{fffd}'),
            )),
            Err(_) => Ok(PickerKey::Other),
        }
    }

    #[cfg(unix)]
    fn read_byte(&mut self, timeout: Duration) -> Result<Option<u8>, String> {
        use nix::poll::{poll, PollFd, PollFlags};
        use std::os::fd::AsFd;

        if let Some(byte) = self.pending.pop_front() {
            return Ok(Some(byte));
        }
        let stdin = io::stdin();
        let mut poll_fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
        let timeout =
            u16::try_from(timeout.as_millis().min(u128::from(u16::MAX))).unwrap_or(u16::MAX);
        poll(&mut poll_fds, timeout)
            .map_err(|error| format!("picker input poll failed: {error}"))?;
        if !poll_fds[0]
            .revents()
            .unwrap_or_else(PollFlags::empty)
            .contains(PollFlags::POLLIN)
        {
            return Ok(None);
        }
        let mut byte = [0_u8; 1];
        match nix::unistd::read(stdin.as_fd(), &mut byte) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(byte[0])),
            Err(error) => Err(format!("picker input read failed: {error}")),
        }
    }

    #[cfg(unix)]
    fn drain_available(&mut self) -> Result<Vec<u8>, String> {
        use nix::poll::{poll, PollFd, PollFlags};
        use std::os::fd::AsFd;

        let mut residual = self.pending.drain(..).collect::<Vec<_>>();
        let stdin = io::stdin();
        loop {
            let mut poll_fds = [PollFd::new(stdin.as_fd(), PollFlags::POLLIN)];
            poll(&mut poll_fds, 0u16)
                .map_err(|error| format!("picker input poll failed: {error}"))?;
            if !poll_fds[0]
                .revents()
                .unwrap_or_else(PollFlags::empty)
                .contains(PollFlags::POLLIN)
            {
                break;
            }
            let mut bytes = [0_u8; 4096];
            match nix::unistd::read(stdin.as_fd(), &mut bytes) {
                Ok(0) => break,
                Ok(length) => residual.extend_from_slice(&bytes[..length]),
                Err(error) => return Err(format!("picker input read failed: {error}")),
            }
        }
        Ok(residual)
    }

    #[cfg(not(unix))]
    fn read_byte(&mut self, timeout: Duration) -> Result<Option<u8>, String> {
        use crossterm::event::{poll, read, Event, KeyCode};

        if let Some(byte) = self.pending.pop_front() {
            return Ok(Some(byte));
        }
        if !poll(timeout).map_err(|error| format!("picker input poll failed: {error}"))? {
            return Ok(None);
        }
        match read().map_err(|error| format!("picker input read failed: {error}"))? {
            Event::Key(event) => match event.code {
                KeyCode::Enter => Ok(Some(b'\r')),
                KeyCode::Esc => Ok(Some(0x1b)),
                KeyCode::Char(character) if character.is_ascii() => Ok(Some(character as u8)),
                KeyCode::Backspace => Ok(Some(0x7f)),
                KeyCode::Up => Ok(Some(b'\x01')),
                KeyCode::Down => Ok(Some(b'\x02')),
                KeyCode::Left => Ok(Some(b'\x03')),
                KeyCode::Right => Ok(Some(b'\x04')),
                _ => Ok(Some(0)),
            },
            _ => Ok(Some(0)),
        }
    }

    #[cfg(not(unix))]
    fn drain_available(&mut self) -> Result<Vec<u8>, String> {
        Ok(self.pending.drain(..).collect())
    }
}

#[derive(Clone, Copy)]
enum ScreenMode {
    Alternate,
    MainScreen,
}

struct TerminalGuard {
    active: Arc<Mutex<bool>>,
    previous_hook: Option<Arc<Mutex<Option<PanicHook>>>>,
    screen_mode: ScreenMode,
}

impl TerminalGuard {
    /// Set up the picker terminal and return the guard plus any stdin bytes
    /// the probe swallowed (to be fed back into the input reader).
    fn enter(preference: ScreenPreference) -> Result<(Self, Vec<u8>), String> {
        enable_raw_mode().map_err(|error| format!("failed to enter raw terminal mode: {error}"))?;

        // Probe while raw mode is active: the cursor-position reply is
        // read byte-by-byte and must not be canonical-buffered or echoed.
        let outcome = resolve_screen_mode(preference);
        let screen_mode = outcome.screen_mode;

        let mut stdout = io::stdout();
        match screen_mode {
            ScreenMode::Alternate => {
                if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
                    let _ = disable_raw_mode();
                    return Err(format!("failed to enter alternate screen: {error}"));
                }
            }
            ScreenMode::MainScreen => {
                if let Err(error) = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0), Hide) {
                    let _ = disable_raw_mode();
                    return Err(format!("failed to initialize main screen mode: {error}"));
                }
            }
        }

        let active = Arc::new(Mutex::new(true));
        let previous = Arc::new(Mutex::new(Some(panic::take_hook())));
        let hook_active = Arc::clone(&active);
        let hook_previous = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            restore_if_active(&hook_active, screen_mode);
            if let Ok(previous) = hook_previous.lock() {
                if let Some(previous) = previous.as_ref() {
                    previous(info);
                }
            }
        }));

        Ok((
            Self {
                active,
                previous_hook: Some(previous),
                screen_mode,
            },
            outcome.leftover_input,
        ))
    }
}

/// Resolve the caller's preference into a concrete mode, probing the
/// terminal when [`ScreenPreference::Auto`]. Carries back any stdin bytes
/// the probe captured so they can be forwarded to the input reader.
fn resolve_screen_mode(preference: ScreenPreference) -> ResolvedScreenMode {
    let (screen_mode, leftover_input) = match preference {
        ScreenPreference::ForceMainScreen => (ScreenMode::MainScreen, Vec::new()),
        ScreenPreference::ForceAlternateScreen => (ScreenMode::Alternate, Vec::new()),
        ScreenPreference::Auto => {
            let outcome = probe_alternate_screen();
            let mode = if outcome.alternate_screen {
                ScreenMode::Alternate
            } else {
                ScreenMode::MainScreen
            };
            (mode, outcome.leftover_input)
        }
    };
    ResolvedScreenMode {
        screen_mode,
        leftover_input,
    }
}

struct ResolvedScreenMode {
    screen_mode: ScreenMode,
    leftover_input: Vec<u8>,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_if_active(&self.active, self.screen_mode);
        if let Some(previous) = self.previous_hook.take() {
            let _ = panic::take_hook();
            if let Ok(mut previous) = previous.lock() {
                if let Some(previous) = previous.take() {
                    panic::set_hook(previous);
                }
            }
        }
    }
}

fn restore_if_active(active: &Arc<Mutex<bool>>, screen_mode: ScreenMode) {
    let should_restore = active.lock().map_or(true, |mut active| {
        if *active {
            *active = false;
            true
        } else {
            false
        }
    });
    if should_restore {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();

        match screen_mode {
            ScreenMode::Alternate => {
                let _ = execute!(stdout, Show, LeaveAlternateScreen);
            }
            ScreenMode::MainScreen => {
                let _ = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0), Show);
            }
        }
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn forced_preferences_skip_the_probe() {
        // Both force paths short-circuit the probe, so they are
        // deterministic regardless of the controlling terminal.
        let main = resolve_screen_mode(ScreenPreference::ForceMainScreen);
        assert!(matches!(main.screen_mode, ScreenMode::MainScreen));
        assert!(main.leftover_input.is_empty());
        let alt = resolve_screen_mode(ScreenPreference::ForceAlternateScreen);
        assert!(matches!(alt.screen_mode, ScreenMode::Alternate));
        assert!(alt.leftover_input.is_empty());
    }

    #[test]
    fn cursor_report_round_trips() {
        // `\x1b[1;1R` is 6 bytes: ESC [ 1 ; 1 R.
        assert_eq!(extract_cursor_response(b"\x1b[1;1R"), Some(((1, 1), 0, 6)));
        assert_eq!(
            extract_cursor_response(b"\x1b[24;80R"),
            Some(((24, 80), 0, 8))
        );
        assert_eq!(extract_cursor_response(b"\x1b[3;3R"), Some(((3, 3), 0, 6)));
    }

    #[test]
    fn cursor_report_preserves_surrounding_input() {
        // A keystroke before the reply and one after it must be recoverable
        // via the returned byte range.
        let buf = b"q\x1b[2;5Rz";
        let ((row, col), start, end) = extract_cursor_response(buf).expect("parsed");
        assert_eq!((row, col), (2, 5));
        assert_eq!(&buf[..start], b"q");
        assert_eq!(&buf[end..], b"z");
    }

    #[test]
    fn cursor_report_skips_unrelated_escape() {
        // An arrow-key report (ESC[A) before the real reply must not be
        // mistaken for the cursor reply, and must be left intact.
        let buf = b"\x1b[A\x1b[10;20R";
        let ((row, col), start, end) = extract_cursor_response(buf).expect("parsed");
        assert_eq!((row, col), (10, 20));
        assert_eq!(&buf[..start], b"\x1b[A");
        assert_eq!(&buf[end..], b"");
    }

    #[test]
    fn malformed_cursor_reports_are_rejected() {
        assert_eq!(extract_cursor_response(b"\x1b[6n"), None);
        assert_eq!(extract_cursor_response(b"garbage"), None);
        assert_eq!(extract_cursor_response(b"\x1b[ab"), None);
        assert_eq!(extract_cursor_response(b""), None);
        assert_eq!(extract_cursor_response(b"\x1b[;3R"), None);
        assert_eq!(extract_cursor_response(b"\x1b[1x3R"), None);
    }

    fn session(name: &str, attached: bool) -> SessionRecord {
        SessionRecord {
            name: name.to_owned(),
            attached,
            created: 0,
            terminated: false,
            exit_code: None,
            dead_time: None,
        }
    }

    #[test]
    fn selection_moves_and_clamps_by_name() {
        let mut state = PickerState {
            sessions: vec![session("alpha", false), session("beta", true)],
            ..PickerState::default()
        };
        state.move_down();
        assert_eq!(state.selected_name.as_deref(), Some("alpha"));
        state.move_down();
        assert_eq!(state.selected_name.as_deref(), Some("beta"));
        state.move_down();
        assert_eq!(state.selected_name.as_deref(), Some("beta"));
        state.move_up();
        assert_eq!(state.selected_name.as_deref(), Some("alpha"));
        state.move_up();
        assert_eq!(state.selected_name.as_deref(), Some("alpha"));
    }

    #[test]
    fn a_missing_selected_name_is_cleared_after_poll() {
        let mut state = PickerState {
            sessions: vec![session("alpha", false), session("beta", false)],
            selected_name: Some("beta".to_owned()),
            ..PickerState::default()
        };
        state.apply_poll_result(Ok(vec![session("alpha", false)]));
        assert_eq!(state.selected_index(), None);
    }

    #[test]
    fn poll_failures_keep_the_last_list_and_show_the_error() {
        let mut state = PickerState {
            sessions: vec![session("alpha", false)],
            selected_name: Some("alpha".to_owned()),
            ..PickerState::default()
        };
        state.apply_poll_result(Err("tmux is unavailable".to_owned()));
        assert_eq!(state.sessions, vec![session("alpha", false)]);
        assert_eq!(state.selected_name.as_deref(), Some("alpha"));
        assert_eq!(state.status(), "tmux is unavailable");
    }

    #[test]
    fn status_text_matches_this_milestone() {
        let state = PickerState::default();
        assert_eq!(state.status(), EMPTY_STATUS);
        let state = PickerState {
            sessions: vec![session("work", false)],
            ..PickerState::default()
        };
        assert_eq!(
            state.status(),
            "↑/↓ select · Enter attach · c create · k kill · r recreate · e edit name · v view-only · l low-priority · Esc quit"
        );
    }

    #[test]
    fn create_mode_renders_the_name_prompt_and_supports_editing() {
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: String::new(),
            },
            ..PickerState::default()
        };
        state.push_create_character('w');
        state.push_create_character('o');
        state.push_create_character('r');
        state.push_create_character('k');
        assert_eq!(state.prompt().as_deref(), Some("New session name: work█"));
        state.delete_create_character();
        assert_eq!(state.create_name(), "wor");
    }

    #[test]
    fn edit_name_mode_renders_the_name_prompt_and_supports_editing() {
        let mut state = PickerState {
            mode: PickerMode::EditName {
                session_name: "build".to_owned(),
                input: String::new(),
            },
            ..PickerState::default()
        };
        state.push_edit_name_character('r');
        state.push_edit_name_character('e');
        state.push_edit_name_character('n');
        assert_eq!(
            state.prompt().as_deref(),
            Some("Edit name \"build\" to: ren█")
        );
        state.delete_edit_name_character();
        assert_eq!(state.edit_name(), ("build".to_owned(), "re".to_owned()));
    }

    #[test]
    fn yes_no_selector_defaults_to_the_safe_option() {
        assert_eq!(YesNoSelector::new(true).focused_option(), YesNoOption::No);
        assert_eq!(YesNoSelector::new(false).focused_option(), YesNoOption::Yes);
    }

    #[test]
    fn yes_no_selector_moves_focus_and_selects_directly() {
        let mut selector = YesNoSelector::new(true);
        assert_eq!(selector.handle_key(PickerKey::Left), YesNoAction::Continue);
        assert_eq!(selector.focused_option(), YesNoOption::Yes);
        assert_eq!(selector.handle_key(PickerKey::Right), YesNoAction::Continue);
        assert_eq!(selector.focused_option(), YesNoOption::No);
        assert_eq!(
            selector.handle_key(PickerKey::Char('y')),
            YesNoAction::Confirm(YesNoOption::Yes)
        );
        assert_eq!(selector.focused_option(), YesNoOption::Yes);
        assert_eq!(
            selector.handle_key(PickerKey::Char('n')),
            YesNoAction::Confirm(YesNoOption::No)
        );
        assert_eq!(selector.focused_option(), YesNoOption::No);
        assert_eq!(
            selector.handle_key(PickerKey::Enter),
            YesNoAction::Confirm(YesNoOption::No)
        );
    }

    #[test]
    fn yes_no_selector_renders_only_the_focused_option_reversed() {
        let line = YesNoSelector::new(true).render();
        assert_eq!(line.spans[0].content, "Yes");
        assert!(!line.spans[0]
            .style
            .add_modifier
            .contains(Modifier::REVERSED));
        assert!(line.spans[2]
            .style
            .add_modifier
            .contains(Modifier::REVERSED));
    }

    #[test]
    fn input_reader_parses_left_and_right_arrows() {
        let mut input = InputReader::with_pending(b"\x1b[C\x1b[D".to_vec());
        assert_eq!(
            input.next(Duration::ZERO).expect("read right arrow"),
            Some(PickerKey::Right)
        );
        assert_eq!(
            input.next(Duration::ZERO).expect("read left arrow"),
            Some(PickerKey::Left)
        );
    }

    #[test]
    fn view_only_and_low_priority_guards_do_not_call_tmux() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = Config {
            default_command: None,
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 10_000,
        };
        for (key, expected) in [
            (PickerKey::Char('v'), "v: not yet implemented"),
            (PickerKey::Char('l'), "l: not yet implemented"),
        ] {
            let mut state = PickerState {
                sessions: vec![session("work", false)],
                selected_name: Some("work".to_owned()),
                ..PickerState::default()
            };
            let mut input = InputReader::new();
            handle_idle_key(&mut state, key, &tmux, &config, &mut input)
                .expect("guard key should be handled");
            assert_eq!(state.status(), expected);
            assert_eq!(state.selected_name.as_deref(), Some("work"));
        }
    }

    #[test]
    fn kill_confirmation_captures_the_selected_name() {
        let state = PickerState {
            selected_name: Some("original".to_owned()),
            mode: PickerMode::KillConfirm {
                session_name: "original".to_owned(),
                selector: YesNoSelector::new(true),
            },
            ..PickerState::default()
        };
        assert_eq!(
            state.prompt().as_deref(),
            Some("Kill session \"original\"? Yes No")
        );
        assert_eq!(
            match &state.mode {
                PickerMode::KillConfirm { selector, .. } => selector.focused_option(),
                PickerMode::Idle | PickerMode::Create { .. } | PickerMode::EditName { .. } => {
                    panic!("expected kill confirmation")
                }
            },
            YesNoOption::No
        );
        assert_eq!(state.confirm_name(), "original");
    }

    #[test]
    fn action_errors_take_precedence_over_poll_errors() {
        let state = PickerState {
            poll_error: Some("poll error".to_owned()),
            action_error: Some("action error".to_owned()),
            ..PickerState::default()
        };
        assert_eq!(state.status(), "action error");
    }

    #[test]
    fn selected_rows_keep_the_marker_and_fill_the_row() {
        let selected = session_row(&session("build", false), true, 24);
        let ordinary = session_row(&session("build", false), false, 24);
        assert_eq!(selected, ordinary);
        assert_eq!(selected.spans[0].content, "build        ");
        assert_eq!(selected.spans[1].content, " [detached]");
    }

    #[test]
    fn wide_names_are_padded_by_terminal_display_width() {
        let row = session_row(&session("東京", false), false, 12);
        let text = row
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(UnicodeWidthStr::width(text.as_str()), 12);
        assert!(text.ends_with("[detached]"));
    }

    #[test]
    fn terminated_rows_keep_details_and_only_unfocused_exit_is_red() {
        let terminated = SessionRecord {
            name: "build".to_owned(),
            attached: false,
            created: 0,
            terminated: true,
            exit_code: Some(7),
            dead_time: Some(0),
        };
        let unfocused = session_row(&terminated, false, 80);
        let selected = session_row(&terminated, true, 80);
        assert!(unfocused
            .spans
            .iter()
            .any(|span| span.content == "7" && span.style.fg == Some(Color::Red)));
        assert!(selected
            .spans
            .iter()
            .any(|span| span.content == "7" && span.style.fg != Some(Color::Red)));
        assert!(
            selected
                .spans
                .iter()
                .any(|span| span.content.contains("@ 1970-01-01T")),
            "selected spans: {:?}",
            selected.spans
        );
    }

    #[test]
    fn terminated_rows_drop_time_then_exit_code_when_narrow() {
        let terminated = SessionRecord {
            name: "build-session".to_owned(),
            attached: false,
            created: 0,
            terminated: true,
            exit_code: Some(7),
            dead_time: Some(0),
        };
        let with_exit = session_row(&terminated, false, 25);
        let with_exit_text = with_exit
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(UnicodeWidthStr::width(with_exit_text.as_str()), 25);
        assert!(with_exit_text.contains("[terminated exit=7]"));
        assert!(!with_exit_text.contains('@'));

        let marker_only = session_row(&terminated, false, 19);
        let marker_only_text = marker_only
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(UnicodeWidthStr::width(marker_only_text.as_str()), 19);
        assert!(marker_only_text.ends_with("[terminated]"));
        assert!(!marker_only_text.contains("exit="));
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn panic_restores_the_picker_terminal_state() {
        use nix::pty::{forkpty, ForkptyResult, Winsize};
        use nix::sys::termios;
        use nix::sys::wait::waitpid;
        use std::os::fd::AsFd;

        let _lock = crate::test_global_state_lock();
        let result = unsafe { forkpty(None::<&Winsize>, None) }.expect("allocate picker PTY");
        match result {
            ForkptyResult::Child => {
                panic::set_hook(Box::new(|_| {}));
                let panic_result = panic::catch_unwind(|| {
                    let _guard = TerminalGuard::enter(ScreenPreference::Auto)
                        .expect("enter picker terminal");
                    panic!("exercise picker terminal panic hook");
                });
                assert!(panic_result.is_err());
                unsafe { nix::libc::_exit(0) };
            }
            ForkptyResult::Parent { child, master } => {
                let before = termios::tcgetattr(master.as_fd()).expect("read picker PTY state");
                waitpid(child, None).expect("reap picker panic test");
                let after = termios::tcgetattr(master.as_fd()).expect("read restored state");
                assert_eq!(before, after);
            }
        }
    }

    // ----- PTY terminal-emulator fixture for the screen probe -----

    /// A configurable fake terminal that drives a child over a PTY. It models
    /// exactly the behaviour the probe relies on: answering cursor-position
    /// requests, optionally honouring `?1049h`/`?1049l`, and honouring CUP.
    #[cfg(unix)]
    struct EmulatorSpec {
        /// Whether `ESC[?1049h` saves the cursor and `ESC[?1049l` restores it.
        honors_alt_screen: bool,
        /// Whether to answer `ESC[6n` (cursor-position request) at all.
        responds_to_dsr: bool,
        /// Bytes to inject into the child's stdin once a trigger fires.
        inject: Vec<u8>,
        /// Inject `inject` after this much wall-clock time has elapsed.
        inject_after: Duration,
        /// Inject `inject` the moment the first cursor-position request is
        /// seen — so the bytes land while the probe is still reading.
        inject_on_first_dsr: bool,
    }

    #[cfg(unix)]
    struct Emulation {
        saw_enter_alt_screen: bool,
        output: Vec<u8>,
    }

    /// Run `probe_alternate_screen()` in a child process behind a PTY driven
    /// by `spec`, returning the probe's outcome and what the emulator saw.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn run_probe_in_pty(spec: &EmulatorSpec) -> (ProbeOutcome, Emulation) {
        use nix::pty::{forkpty, ForkptyResult, Winsize};
        use nix::sys::wait::waitpid;
        use std::ffi::CString;
        use std::os::fd::AsFd;
        use std::os::unix::ffi::OsStrExt;

        // Serialize with any other fork-touching test to keep the test
        // process single-threaded across the fork.
        let _lock = crate::test_global_state_lock();

        let executable = CString::new(std::env::current_exe().unwrap().as_os_str().as_bytes())
            .expect("test executable path contains no NUL");
        let arguments = [
            executable.as_c_str(),
            c"--exact",
            c"picker::tests::picker_probe_helper",
            c"--nocapture",
        ];
        let result = unsafe { forkpty(None::<&Winsize>, None) }.expect("allocate probe PTY");
        match result {
            ForkptyResult::Child => {
                let _ = nix::unistd::execv(&executable, &arguments);
                unsafe { nix::libc::_exit(0) };
            }
            ForkptyResult::Parent { child, master } => {
                let emu = emulate(spec, master.as_fd());
                let _ = waitpid(child, None);
                let outcome = decode_probe_report(&emu.output);
                (outcome, emu)
            }
        }
    }

    /// Decode the probe helper's report from its PTY output.
    #[cfg(unix)]
    fn decode_probe_report(output: &[u8]) -> ProbeOutcome {
        let marker = b"__STAY_PROBE_RESULT__";
        let start = output
            .windows(marker.len())
            .position(|window| window == marker)
            .map_or_else(
                || {
                    panic!(
                        "probe helper report missing from {:?}",
                        String::from_utf8_lossy(output)
                    )
                },
                |index| index + marker.len(),
            );
        let supported = *output.get(start).expect("probe report status");
        assert_eq!(
            output.get(start + 1),
            Some(&b':'),
            "probe report output: {:?}",
            String::from_utf8_lossy(output)
        );
        let mut leftover_input = Vec::new();
        let mut cursor = start + 2;
        while cursor + 1 < output.len()
            && output[cursor].is_ascii_hexdigit()
            && output[cursor + 1].is_ascii_hexdigit()
        {
            let text = std::str::from_utf8(&output[cursor..cursor + 2]).expect("probe report hex");
            leftover_input.push(u8::from_str_radix(text, 16).expect("probe report byte"));
            cursor += 2;
        }
        ProbeOutcome {
            alternate_screen: supported == b'1',
            leftover_input,
        }
    }

    /// Drive the PTY master as a fake terminal until the child exits (master
    /// read returns EOF/EIO), answering DSR, tracking the cursor, and
    /// recording `?1049h`.
    #[cfg(unix)]
    fn emulate(spec: &EmulatorSpec, master: std::os::fd::BorrowedFd<'_>) -> Emulation {
        use nix::errno::Errno;
        use nix::poll::{poll, PollFd, PollFlags};
        use nix::unistd::{read, write};

        let mut cursor: (u16, u16) = (1, 1);
        let mut saved: Option<(u16, u16)> = None;
        let mut saw_enter_alt_screen = false;
        let mut saw_dsr = false;
        let mut seq: Vec<u8> = Vec::new();
        let mut state = EmuParseState::Ground;
        let start = Instant::now();
        let mut injected = false;
        let deadline = start + Duration::from_secs(10);
        let mut output = Vec::new();

        loop {
            if Instant::now() > deadline {
                break;
            }

            if !injected && !spec.inject.is_empty() {
                let elapsed = Instant::now().duration_since(start);
                let due = elapsed >= spec.inject_after || (spec.inject_on_first_dsr && saw_dsr);
                if due {
                    let _ = write(master, &spec.inject);
                    injected = true;
                }
            }

            let mut pfd = [PollFd::new(master, PollFlags::POLLIN)];
            match poll(&mut pfd, 50u16) {
                Ok(0) | Err(Errno::EINTR) => continue,
                Ok(_) => {}
                Err(_) => break,
            }
            let mut buf = [0u8; 4096];
            let n = match read(master, &mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(Errno::EINTR) => continue,
                Err(_) => break,
            };
            output.extend_from_slice(&buf[..n]);
            for &byte in &buf[..n] {
                // A fresh ESC always starts a new sequence, abandoning any
                // in-progress one.
                if byte == 0x1b {
                    seq.clear();
                    seq.push(byte);
                    state = EmuParseState::Esc;
                    continue;
                }
                match state {
                    EmuParseState::Ground => {}
                    EmuParseState::Esc => {
                        if byte == b'[' {
                            seq.push(byte);
                            state = EmuParseState::Csi;
                        } else {
                            // ESC <x> is a non-CSI escape we do not model.
                            state = EmuParseState::Ground;
                        }
                    }
                    EmuParseState::Csi => {
                        seq.push(byte);
                        if (0x40..=0x7e).contains(&byte) {
                            if let Some(reply) = handle_seq(
                                &seq,
                                spec,
                                &mut cursor,
                                &mut saved,
                                &mut saw_enter_alt_screen,
                                &mut saw_dsr,
                            ) {
                                let _ = write(master, reply.as_bytes());
                            }
                            state = EmuParseState::Ground;
                        } else if seq.len() > 32 {
                            // Malformed / runaway sequence; resync.
                            state = EmuParseState::Ground;
                        }
                    }
                }
            }
        }

        Emulation {
            saw_enter_alt_screen,
            output,
        }
    }

    /// Minimal CSI parser state for the terminal emulator.
    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum EmuParseState {
        Ground,
        Esc,
        Csi,
    }

    /// Dispatch one complete escape sequence, mutating emulator state. Returns
    /// bytes to write back to the child (a cursor reply) when applicable.
    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    fn handle_seq(
        seq: &[u8],
        spec: &EmulatorSpec,
        cursor: &mut (u16, u16),
        saved: &mut Option<(u16, u16)>,
        saw_enter_alt_screen: &mut bool,
        saw_dsr: &mut bool,
    ) -> Option<String> {
        if seq == b"\x1b[6n" {
            *saw_dsr = true;
            return if spec.responds_to_dsr {
                Some(format!("\x1b[{};{}R", cursor.0, cursor.1))
            } else {
                None
            };
        }
        if seq == b"\x1b[?1049h" {
            *saw_enter_alt_screen = true;
            if spec.honors_alt_screen {
                *saved = Some(*cursor);
            }
            return None;
        }
        if seq == b"\x1b[?1049l" {
            if spec.honors_alt_screen && saved.is_some() {
                *cursor = saved.take().expect("saved cursor present");
            }
            return None;
        }
        if seq.starts_with(b"\x1b[") && seq.last() == Some(&b'H') {
            let body = &seq[2..seq.len() - 1];
            if let Some((row, col)) = parse_cup(body) {
                *cursor = (row, col);
            }
        }
        None
    }

    #[cfg(unix)]
    fn parse_cup(body: &[u8]) -> Option<(u16, u16)> {
        let semi = body.iter().position(|&b| b == b';')?;
        let row = parse_ascii_u16(&body[..semi])?;
        let col = parse_ascii_u16(&body[semi + 1..])?;
        Some((row, col))
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn probe_detects_a_conformant_alternate_screen() {
        let spec = EmulatorSpec {
            honors_alt_screen: true,
            responds_to_dsr: true,
            inject: Vec::new(),
            inject_after: Duration::from_secs(60),
            inject_on_first_dsr: false,
        };
        let (outcome, emu) = run_probe_in_pty(&spec);
        assert!(
            outcome.alternate_screen,
            "an honouring terminal should be detected as supported; output={:?}",
            String::from_utf8_lossy(&emu.output)
        );
        assert!(outcome.leftover_input.is_empty());
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn probe_rejects_a_terminal_that_ignores_alt_screen() {
        let spec = EmulatorSpec {
            honors_alt_screen: false,
            responds_to_dsr: true,
            inject: Vec::new(),
            inject_after: Duration::from_secs(60),
            inject_on_first_dsr: false,
        };
        let (outcome, _) = run_probe_in_pty(&spec);
        assert!(
            !outcome.alternate_screen,
            "a terminal that ignores ?1049 must fall back to the main screen"
        );
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn probe_treats_a_silent_terminal_as_unsupported() {
        let spec = EmulatorSpec {
            honors_alt_screen: true,
            responds_to_dsr: false,
            inject: Vec::new(),
            inject_after: Duration::from_secs(60),
            inject_on_first_dsr: false,
        };
        let (outcome, _) = run_probe_in_pty(&spec);
        assert!(
            !outcome.alternate_screen,
            "no cursor reply must be treated conservatively as unsupported"
        );
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn probe_preserves_input_that_arrives_during_the_query() {
        let spec = EmulatorSpec {
            honors_alt_screen: true,
            responds_to_dsr: true,
            inject: b"q".to_vec(),
            inject_after: Duration::from_secs(60),
            inject_on_first_dsr: true,
        };
        let (outcome, _) = run_probe_in_pty(&spec);
        assert!(
            outcome.alternate_screen,
            "an honouring terminal should still be detected"
        );
        assert!(
            outcome.leftover_input.contains(&b'q'),
            "a keystroke typed during the probe must survive it, got {:?}",
            outcome.leftover_input
        );
    }

    /// Run the whole picker in a child behind a PTY driven by `spec`, for the
    /// given preference, returning the child's exit code and what the emulator
    /// saw. Inject `Esc` via `spec` so the picker quits; the reaper force-kills
    /// a stuck child rather than hanging the suite.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn run_picker_in_pty(spec: &EmulatorSpec, preference: ScreenPreference) -> (i32, Emulation) {
        use nix::pty::{forkpty, ForkptyResult, Winsize};
        use std::ffi::CString;
        use std::os::fd::AsFd;
        use std::os::unix::ffi::OsStrExt;

        let _lock = crate::test_global_state_lock();
        let executable = CString::new(std::env::current_exe().unwrap().as_os_str().as_bytes())
            .expect("test executable path contains no NUL");
        let helper_name = match preference {
            ScreenPreference::Auto => "picker_run_auto_helper",
            ScreenPreference::ForceMainScreen => "picker_run_main_helper",
            ScreenPreference::ForceAlternateScreen => "picker_run_alternate_helper",
        };
        let helper_test_name = CString::new(format!("picker::tests::{helper_name}"))
            .expect("helper test name contains no NUL");
        let arguments = [
            executable.as_c_str(),
            c"--exact",
            helper_test_name.as_c_str(),
            c"--nocapture",
        ];
        let result = unsafe { forkpty(None::<&Winsize>, None) }.expect("allocate picker PTY");
        match result {
            ForkptyResult::Child => {
                let _ = nix::unistd::execv(&executable, &arguments);
                unsafe { nix::libc::_exit(127) };
            }
            ForkptyResult::Parent { child, master } => {
                let emu = emulate(spec, master.as_fd());
                let code = reap_or_kill(child);
                (decode_picker_report(&emu.output).unwrap_or(code), emu)
            }
        }
    }

    #[cfg(unix)]
    fn decode_picker_report(output: &[u8]) -> Option<i32> {
        let marker = b"__STAY_PICKER_RESULT__";
        let Some(start) = output
            .windows(marker.len())
            .position(|window| window == marker)
            .map(|index| index + marker.len())
        else {
            panic!(
                "picker helper report missing from {:?}",
                String::from_utf8_lossy(output)
            );
        };
        let report = output[start..]
            .split(|&byte| byte == b'\n' || byte == b'\r')
            .next()?;
        std::str::from_utf8(report).ok()?.parse().ok()
    }

    #[cfg(unix)]
    #[test]
    fn picker_probe_helper() {
        if !std::env::args().any(|argument| argument.contains("picker_probe_helper")) {
            return;
        }
        let _ = disable_raw_mode();
        let _ = enable_raw_mode();
        let outcome = probe_alternate_screen();
        let _ = disable_raw_mode();
        let mut leftover = String::new();
        for byte in &outcome.leftover_input {
            write!(&mut leftover, "{byte:02x}").expect("write probe report");
        }
        println!(
            "__STAY_PROBE_RESULT__{}:{leftover}",
            u8::from(outcome.alternate_screen)
        );
    }

    #[cfg(unix)]
    fn run_picker_helper(preference: ScreenPreference) {
        let _ = disable_raw_mode();
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = Config {
            default_command: None,
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 10_000,
        };
        let status = run(&tmux, &config, preference).unwrap_or(1);
        println!("__STAY_PICKER_RESULT__{status}");
    }

    #[cfg(unix)]
    #[test]
    fn picker_run_auto_helper() {
        if std::env::args().any(|argument| argument.contains("picker_run_auto_helper")) {
            run_picker_helper(ScreenPreference::Auto);
        }
    }

    #[cfg(unix)]
    #[test]
    fn picker_run_main_helper() {
        if std::env::args().any(|argument| argument.contains("picker_run_main_helper")) {
            run_picker_helper(ScreenPreference::ForceMainScreen);
        }
    }

    #[cfg(unix)]
    #[test]
    fn picker_run_alternate_helper() {
        if std::env::args().any(|argument| argument.contains("picker_run_alternate_helper")) {
            run_picker_helper(ScreenPreference::ForceAlternateScreen);
        }
    }

    /// Reap the child if it has already exited; otherwise it is stuck, so
    /// SIGKILL it and reap the corpse. Never blocks indefinitely.
    #[cfg(unix)]
    fn reap_or_kill(child: nix::unistd::Pid) -> i32 {
        use nix::sys::signal::{kill, Signal};
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

        if let Ok(WaitStatus::Exited(_, code)) = waitpid(child, Some(WaitPidFlag::WNOHANG)) {
            code
        } else {
            let _ = kill(child, Signal::SIGKILL);
            let _ = waitpid(child, None);
            -1
        }
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn picker_main_screen_never_enters_the_alternate_buffer() {
        // A terminal that never answers the probe falls back to the main
        // screen, so the picker must not emit ?1049h/?1049l, yet must still
        // start, render, and quit cleanly on Esc.
        let spec = EmulatorSpec {
            honors_alt_screen: false,
            responds_to_dsr: false,
            inject: vec![0x1b],
            inject_after: Duration::from_millis(1_500),
            inject_on_first_dsr: false,
        };
        let (code, emu) = run_picker_in_pty(&spec, ScreenPreference::Auto);
        assert_eq!(
            code, 0,
            "picker should quit cleanly on Esc in main-screen mode"
        );
        assert!(
            !emu.saw_enter_alt_screen,
            "main-screen fallback must not emit the alternate-screen sequence"
        );
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn picker_alt_preference_enters_the_alternate_buffer() {
        let spec = EmulatorSpec {
            honors_alt_screen: true,
            responds_to_dsr: false,
            inject: vec![0x1b],
            inject_after: Duration::from_millis(100),
            inject_on_first_dsr: false,
        };
        let (code, emu) = run_picker_in_pty(&spec, ScreenPreference::ForceAlternateScreen);
        assert_eq!(
            code, 0,
            "picker should quit cleanly on Esc in alt-screen mode"
        );
        assert!(
            emu.saw_enter_alt_screen,
            "--alt-screen must emit the alternate-screen sequence"
        );
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn picker_acts_on_input_that_arrived_during_the_probe() {
        // End-to-end: a keystroke that lands while the probe is reading must
        // survive into the picker's input loop and drive a real action. Only
        // 'q' is injected, and only during the probe (inject_on_first_dsr);
        // no Esc is ever sent afterwards. The picker quitting with exit 0 is
        // therefore only possible if the probe-preserved 'q' reached the
        // input reader — otherwise it would run until the SIGKILL backstop.
        let spec = EmulatorSpec {
            honors_alt_screen: true,
            responds_to_dsr: true,
            inject: b"q".to_vec(),
            inject_after: Duration::from_secs(60),
            inject_on_first_dsr: true,
        };
        let (code, _) = run_picker_in_pty(&spec, ScreenPreference::Auto);
        assert_eq!(
            code, 0,
            "picker should quit on the probe-preserved 'q' byte"
        );
    }
}

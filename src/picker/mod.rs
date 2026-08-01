//! The interactive session picker.

use crate::config::Config;
use crate::session;
use crate::session_name::parse_session_name;
use crate::tmux::{SessionRecord, Tmux};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear as ClearWidget, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::panic::{self, PanicHookInfo};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const ESCAPE_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(20);
const LIST_GUTTER_WIDTH: u16 = 1;
const IDLE_STATUS: &str = "↑/↓ select · v toggle view-only · l toggle low-priority · Enter attach · r recreate · e edit name · k kill · K kill all terminated · Esc quit";
const EMPTY_STATUS: &str = "Esc quit";

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

/// How the caller wants the picker screen set up.
#[derive(Clone, Copy)]
pub enum ScreenPreference {
    /// Probe the terminal and use the alternate screen only if it works.
    Auto,
    /// `--no-alt-screen`: draw on the main screen, never the alternate.
    ForceMainScreen,
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
/// universally-safe mode; the Auto probe is the only alternate-screen path.
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
    use nix::poll::{PollFd, PollFlags, poll};
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
/// `ForceMainScreen` (`--no-alt-screen`) overrides the probe.
///
/// # Errors
///
/// Returns an error when terminal setup, picker input/rendering, or the
/// selected session's attach operation fails.
pub fn run(tmux: &Tmux, config: &Config, preference: ScreenPreference) -> Result<u8, String> {
    if !io::stdout().is_terminal() {
        return Err("the interactive picker requires a terminal".to_owned());
    }

    loop {
        match run_picker(tmux, config, preference)? {
            PickerOutcome::Quit => return Ok(0),
            PickerOutcome::Attach {
                session_name,
                residual_input,
                read_only,
                low_priority,
            } => {
                session::attach_session_with_input(
                    tmux,
                    config,
                    &session_name,
                    &[],
                    session::AttachOptions {
                        read_only,
                        low_priority,
                        ..session::AttachOptions::default()
                    },
                    &residual_input,
                )?;
            }
        }
    }
}

enum PickerOutcome {
    Quit,
    Attach {
        session_name: String,
        residual_input: Vec<u8>,
        read_only: bool,
        low_priority: bool,
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
            .draw(|frame| render(frame, &mut state))
            .map_err(|error| format!("failed to render picker: {error}"))?;

        if let Some(key) = input.next(Duration::from_millis(50))?
            && let Some(outcome) = handle_key(&mut state, key, tmux, config, &mut input)?
        {
            return Ok(outcome);
        }
    }
}

/// Drains any input typed ahead of an attach and builds the outcome for it.
fn attach_outcome(
    input: &mut InputReader,
    session_name: String,
    read_only: bool,
    low_priority: bool,
) -> Result<Option<PickerOutcome>, String> {
    input.drain_available().map(|residual_input| {
        Some(PickerOutcome::Attach {
            session_name,
            residual_input,
            read_only,
            low_priority,
        })
    })
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
        PickerMode::KillAllConfirm { .. } => Ok(handle_kill_all_key(state, key, tmux)),
        PickerMode::RecreateConfirm { .. } => Ok(handle_recreate_key(state, key, tmux, config)),
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
        PickerKey::Escape | PickerKey::Char('q') => {
            state.clear_pending_attach();
            Ok(Some(PickerOutcome::Quit))
        }
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
            let modifiers = state.take_pending_attach();
            let Some(session_name) = state.selected_name.clone() else {
                state.mode = PickerMode::Create {
                    input: String::new(),
                    cursor: 0,
                };
                return Ok(None);
            };
            attach_outcome(
                input,
                session_name,
                modifiers.read_only,
                modifiers.low_priority,
            )
        }
        PickerKey::Char('c') => {
            state.clear_feedback();
            state.clear_pending_attach();
            state.selected_name = None;
            state.mode = PickerMode::Create {
                input: String::new(),
                cursor: 0,
            };
            Ok(None)
        }
        PickerKey::Char('k') => {
            state.clear_feedback();
            state.clear_pending_attach();
            if let Some(session_name) = state.selected_name.clone() {
                state.mode = PickerMode::KillConfirm {
                    session_name,
                    selector: YesNoSelector::new(true),
                };
            }
            Ok(None)
        }
        PickerKey::Char('K') => {
            begin_kill_all_confirmation(state);
            Ok(None)
        }
        PickerKey::Char('r') => {
            state.clear_feedback();
            state.clear_pending_attach();
            if let Some(session_name) = state.selected_name.clone() {
                if state
                    .sessions
                    .iter()
                    .find(|session| session.name == session_name)
                    .is_some_and(|session| session.terminated)
                {
                    state.mode = PickerMode::RecreateConfirm {
                        session_name,
                        selector: YesNoSelector::new(true),
                    };
                } else {
                    state.recreate(tmux, config, &session_name);
                }
            }
            Ok(None)
        }
        PickerKey::Char('e') => {
            state.clear_pending_attach();
            if let Some(session_name) = state.selected_name.clone() {
                state.clear_feedback();
                state.mode = PickerMode::EditName {
                    input: session_name.clone(),
                    cursor: session_name.len(),
                    session_name,
                };
            }
            Ok(None)
        }
        PickerKey::Char('v' | 'l') => {
            toggle_attach_modifier(state, key);
            Ok(None)
        }
        _ => {
            state.clear_feedback();
            Ok(None)
        }
    }
}

fn begin_kill_all_confirmation(state: &mut PickerState) {
    state.clear_feedback();
    state.clear_pending_attach();
    let session_names = state
        .sessions
        .iter()
        .filter(|session| session.terminated)
        .map(|session| session.name.clone())
        .collect::<Vec<_>>();
    if session_names.is_empty() {
        state.action_error = Some("No terminated sessions to kill.".to_owned());
    } else {
        state.mode = PickerMode::KillAllConfirm {
            session_names,
            selector: YesNoSelector::new(true),
        };
    }
}

fn toggle_attach_modifier(state: &mut PickerState, key: PickerKey) {
    if state.selected_name.is_none() {
        return;
    }
    state.clear_feedback();
    match key {
        PickerKey::Char('v') => state.pending_attach.read_only = !state.pending_attach.read_only,
        PickerKey::Char('l') => {
            state.pending_attach.low_priority = !state.pending_attach.low_priority;
        }
        _ => unreachable!("only attach modifier keys reach this helper"),
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
                        Ok(()) => attach_outcome(input, session_name, false, false),
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
        PickerKey::Left | PickerKey::Right | PickerKey::Home | PickerKey::End => {
            state.move_create_cursor(key);
            Ok(None)
        }
        PickerKey::DeleteForward => {
            state.delete_create_character_forward();
            Ok(None)
        }
        PickerKey::DeleteToStart => {
            state.delete_create_to_start();
            Ok(None)
        }
        PickerKey::DeleteToEnd => {
            state.delete_create_to_end();
            Ok(None)
        }
        PickerKey::DeletePreviousWord => {
            state.delete_create_previous_word();
            Ok(None)
        }
        PickerKey::Char(character) => {
            state.push_create_character(character);
            Ok(None)
        }
        PickerKey::Up | PickerKey::Down | PickerKey::Other => Ok(None),
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
        PickerKey::Left | PickerKey::Right | PickerKey::Home | PickerKey::End => {
            state.move_edit_name_cursor(key);
            None
        }
        PickerKey::DeleteForward => {
            state.delete_edit_name_character_forward();
            None
        }
        PickerKey::DeleteToStart => {
            state.delete_edit_name_to_start();
            None
        }
        PickerKey::DeleteToEnd => {
            state.delete_edit_name_to_end();
            None
        }
        PickerKey::DeletePreviousWord => {
            state.delete_edit_name_previous_word();
            None
        }
        PickerKey::Char(character) => {
            state.push_edit_name_character(character);
            None
        }
        PickerKey::Up | PickerKey::Down | PickerKey::Other => None,
    }
}

fn format_name_prompt_line(label: &str, input: &str, cursor: usize) -> Line<'static> {
    debug_assert!(input.is_char_boundary(cursor));
    let (before, after) = input.split_at(cursor);
    let mut line = Line::default();
    line.push_span(Span::raw(label.to_owned()));
    if !before.is_empty() {
        line.push_span(Span::raw(before.to_owned()));
    }
    let cursor_style = Style::default().add_modifier(Modifier::REVERSED);
    if let Some(character) = after.chars().next() {
        let character_length = character.len_utf8();
        line.push_span(Span::styled(character.to_string(), cursor_style));
        if character_length < after.len() {
            line.push_span(Span::raw(after[character_length..].to_owned()));
        }
    } else {
        line.push_span(Span::styled(" ", cursor_style));
    }
    line
}

fn insert_name_character(input: &mut String, cursor: &mut usize, character: char) {
    debug_assert!(input.is_char_boundary(*cursor));
    input.insert(*cursor, character);
    *cursor += character.len_utf8();
}

fn delete_name_character(input: &mut String, cursor: &mut usize) {
    debug_assert!(input.is_char_boundary(*cursor));
    if let Some((index, _)) = input[..*cursor].char_indices().next_back() {
        input.drain(index..*cursor);
        *cursor = index;
    }
}

fn delete_name_character_forward(input: &mut String, cursor: usize) {
    debug_assert!(input.is_char_boundary(cursor));
    if let Some(character) = input[cursor..].chars().next() {
        input.drain(cursor..cursor + character.len_utf8());
    }
}

fn delete_name_to_start(input: &mut String, cursor: &mut usize) {
    debug_assert!(input.is_char_boundary(*cursor));
    input.drain(..*cursor);
    *cursor = 0;
}

fn delete_name_to_end(input: &mut String, cursor: usize) {
    debug_assert!(input.is_char_boundary(cursor));
    input.truncate(cursor);
}

fn delete_name_previous_word(input: &mut String, cursor: &mut usize) {
    debug_assert!(input.is_char_boundary(*cursor));
    let mut index = *cursor;
    while let Some(character) = input[..index].chars().next_back() {
        if !character.is_whitespace() {
            break;
        }
        index -= character.len_utf8();
    }
    while let Some(character) = input[..index].chars().next_back() {
        if character.is_whitespace() {
            break;
        }
        index -= character.len_utf8();
    }
    input.drain(index..*cursor);
    *cursor = index;
}

fn move_name_cursor(input: &str, cursor: &mut usize, key: PickerKey) {
    debug_assert!(input.is_char_boundary(*cursor));
    match key {
        PickerKey::Home => *cursor = 0,
        PickerKey::End => *cursor = input.len(),
        PickerKey::Left => {
            *cursor = input[..*cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
        }
        PickerKey::Right => {
            if let Some(character) = input[*cursor..].chars().next() {
                *cursor += character.len_utf8();
            }
        }
        _ => unreachable!("only cursor movement keys reach the name cursor"),
    }
}

fn handle_kill_key(state: &mut PickerState, key: PickerKey, tmux: &Tmux) -> Option<PickerOutcome> {
    let action = match &mut state.mode {
        PickerMode::KillConfirm { selector, .. } => selector.handle_key(key),
        PickerMode::Idle
        | PickerMode::Create { .. }
        | PickerMode::EditName { .. }
        | PickerMode::KillAllConfirm { .. }
        | PickerMode::RecreateConfirm { .. } => YesNoAction::Cancel,
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

fn handle_kill_all_key(
    state: &mut PickerState,
    key: PickerKey,
    tmux: &Tmux,
) -> Option<PickerOutcome> {
    let action = match &mut state.mode {
        PickerMode::KillAllConfirm { selector, .. } => selector.handle_key(key),
        PickerMode::Idle
        | PickerMode::Create { .. }
        | PickerMode::EditName { .. }
        | PickerMode::KillConfirm { .. }
        | PickerMode::RecreateConfirm { .. } => YesNoAction::Cancel,
    };

    match action {
        YesNoAction::Confirm(YesNoOption::Yes) => {
            let session_names = match &state.mode {
                PickerMode::KillAllConfirm { session_names, .. } => session_names.clone(),
                _ => Vec::new(),
            };
            state.mode = PickerMode::Idle;
            match session::kill_terminated_sessions(tmux, &session_names) {
                Ok(()) => state.action_error = None,
                Err(error) => state.action_error = Some(error),
            }
            state.poll(tmux);
        }
        YesNoAction::Confirm(YesNoOption::No) | YesNoAction::Cancel => {
            state.mode = PickerMode::Idle;
        }
        YesNoAction::Continue => {}
    }
    None
}

fn handle_recreate_key(
    state: &mut PickerState,
    key: PickerKey,
    tmux: &Tmux,
    config: &Config,
) -> Option<PickerOutcome> {
    let action = match &mut state.mode {
        PickerMode::RecreateConfirm { selector, .. } => selector.handle_key(key),
        PickerMode::Idle
        | PickerMode::Create { .. }
        | PickerMode::EditName { .. }
        | PickerMode::KillConfirm { .. }
        | PickerMode::KillAllConfirm { .. } => YesNoAction::Cancel,
    };

    match action {
        YesNoAction::Confirm(YesNoOption::Yes) => {
            let session_name = state.confirm_name();
            state.mode = PickerMode::Idle;
            state.recreate(tmux, config, &session_name);
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
            | PickerKey::Home
            | PickerKey::End
            | PickerKey::DeleteForward
            | PickerKey::DeleteToStart
            | PickerKey::DeleteToEnd
            | PickerKey::DeletePreviousWord
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
        cursor: usize,
    },
    EditName {
        session_name: String,
        input: String,
        cursor: usize,
    },
    KillConfirm {
        session_name: String,
        selector: YesNoSelector,
    },
    KillAllConfirm {
        session_names: Vec<String>,
        selector: YesNoSelector,
    },
    RecreateConfirm {
        session_name: String,
        selector: YesNoSelector,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PendingAttachModifiers {
    read_only: bool,
    low_priority: bool,
}

impl PendingAttachModifiers {
    fn row_detail(self) -> Option<&'static str> {
        match (self.read_only, self.low_priority) {
            (true, true) => Some("attach with view-only + low-priority"),
            (true, false) => Some("attach with view-only"),
            (false, true) => Some("attach with low-priority"),
            (false, false) => None,
        }
    }
}

#[derive(Default)]
struct PickerState {
    sessions: Vec<SessionRecord>,
    selected_name: Option<String>,
    list_offset: usize,
    list_viewport_height: usize,
    recreate_notice: Option<PickerRecreateNotice>,
    poll_error: Option<String>,
    action_error: Option<String>,
    pending_attach: PendingAttachModifiers,
    mode: PickerMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PickerRecreateNotice {
    session_name: String,
    notice: session::TerminatedRecreateNotice,
}

impl PickerState {
    fn poll(&mut self, tmux: &Tmux) {
        self.apply_poll_result(tmux.list_sessions());
    }

    fn apply_poll_result(&mut self, result: Result<Vec<SessionRecord>, String>) {
        match result {
            Ok(sessions) => {
                if let Some(selected_name) = &self.selected_name
                    && !sessions
                        .iter()
                        .any(|session| &session.name == selected_name)
                {
                    self.selected_name = None;
                    self.pending_attach = PendingAttachModifiers::default();
                }
                self.sessions = sessions;
                self.ensure_selected_visible();
                self.poll_error = None;
            }
            Err(error) => self.poll_error = Some(error),
        }
    }

    fn clear_feedback(&mut self) {
        self.poll_error = None;
        self.action_error = None;
        self.recreate_notice = None;
    }

    fn set_list_viewport_height(&mut self, height: usize) {
        self.list_viewport_height = height;
        self.ensure_selected_visible();
    }

    fn ensure_selected_visible(&mut self) {
        let height = self.list_viewport_height;
        if height == 0 {
            self.list_offset = 0;
            return;
        }

        let selected = self.selected_index();
        if selected < self.list_offset {
            self.list_offset = selected;
        } else if selected >= self.list_offset.saturating_add(height) {
            self.list_offset = selected + 1 - height;
        }

        let last_offset = self.sessions.len().saturating_add(1).saturating_sub(height);
        self.list_offset = self.list_offset.min(last_offset);
    }

    fn create_name(&self) -> String {
        match &self.mode {
            PickerMode::Create { input, .. } => input.clone(),
            PickerMode::Idle
            | PickerMode::EditName { .. }
            | PickerMode::KillConfirm { .. }
            | PickerMode::KillAllConfirm { .. }
            | PickerMode::RecreateConfirm { .. } => String::new(),
        }
    }

    fn push_create_character(&mut self, character: char) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            insert_name_character(input, cursor, character);
        }
    }

    fn delete_create_character(&mut self) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            delete_name_character(input, cursor);
        }
    }

    fn delete_create_character_forward(&mut self) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            delete_name_character_forward(input, *cursor);
        }
    }

    fn delete_create_to_start(&mut self) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            delete_name_to_start(input, cursor);
        }
    }

    fn delete_create_to_end(&mut self) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            delete_name_to_end(input, *cursor);
        }
    }

    fn delete_create_previous_word(&mut self) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            delete_name_previous_word(input, cursor);
        }
    }

    fn move_create_cursor(&mut self, key: PickerKey) {
        if let PickerMode::Create { input, cursor } = &mut self.mode {
            move_name_cursor(input, cursor, key);
        }
    }

    fn edit_name(&self) -> (String, String) {
        match &self.mode {
            PickerMode::EditName {
                session_name,
                input,
                ..
            } => (session_name.clone(), input.clone()),
            PickerMode::Idle
            | PickerMode::Create { .. }
            | PickerMode::KillConfirm { .. }
            | PickerMode::KillAllConfirm { .. }
            | PickerMode::RecreateConfirm { .. } => (String::new(), String::new()),
        }
    }

    fn push_edit_name_character(&mut self, character: char) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            insert_name_character(input, cursor, character);
        }
    }

    fn delete_edit_name_character(&mut self) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            delete_name_character(input, cursor);
        }
    }

    fn delete_edit_name_character_forward(&mut self) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            delete_name_character_forward(input, *cursor);
        }
    }

    fn delete_edit_name_to_start(&mut self) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            delete_name_to_start(input, cursor);
        }
    }

    fn delete_edit_name_to_end(&mut self) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            delete_name_to_end(input, *cursor);
        }
    }

    fn delete_edit_name_previous_word(&mut self) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            delete_name_previous_word(input, cursor);
        }
    }

    fn move_edit_name_cursor(&mut self, key: PickerKey) {
        if let PickerMode::EditName { input, cursor, .. } = &mut self.mode {
            move_name_cursor(input, cursor, key);
        }
    }

    fn confirm_name(&self) -> String {
        match &self.mode {
            PickerMode::KillConfirm { session_name, .. }
            | PickerMode::RecreateConfirm { session_name, .. } => session_name.clone(),
            PickerMode::Idle
            | PickerMode::Create { .. }
            | PickerMode::EditName { .. }
            | PickerMode::KillAllConfirm { .. } => String::new(),
        }
    }

    fn recreate(&mut self, tmux: &Tmux, config: &Config, session_name: &str) {
        match session::force_recreate_session_for_picker(tmux, config, session_name, None, &[]) {
            Ok(notice) => {
                self.action_error = None;
                self.recreate_notice = notice.map(|notice| PickerRecreateNotice {
                    session_name: session_name.to_owned(),
                    notice,
                });
            }
            Err(error) => self.action_error = Some(error),
        }
        self.poll(tmux);
    }

    fn clear_pending_attach(&mut self) {
        self.pending_attach = PendingAttachModifiers::default();
    }

    fn take_pending_attach(&mut self) -> PendingAttachModifiers {
        let pending = self.pending_attach;
        self.clear_pending_attach();
        pending
    }

    fn move_up(&mut self) {
        self.clear_pending_attach();
        let Some(selected_name) = self.selected_name.as_deref() else {
            return;
        };
        let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.name == selected_name)
        else {
            self.selected_name = None;
            return;
        };
        if index == 0 {
            self.selected_name = None;
        } else {
            self.selected_name = Some(self.sessions[index - 1].name.clone());
        }
        self.ensure_selected_visible();
    }

    fn move_down(&mut self) {
        self.clear_pending_attach();
        if self.selected_name.is_none() {
            if let Some(first) = self.sessions.first() {
                self.selected_name = Some(first.name.clone());
            }
            self.ensure_selected_visible();
            return;
        }
        let Some(selected_name) = self.selected_name.as_deref() else {
            return;
        };
        let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.name == selected_name)
        else {
            self.selected_name = None;
            return;
        };
        if let Some(next) = self.sessions.get(index + 1) {
            self.selected_name = Some(next.name.clone());
        }
        self.ensure_selected_visible();
    }

    fn selected_index(&self) -> usize {
        self.selected_name
            .as_ref()
            .and_then(|name| {
                self.sessions
                    .iter()
                    .position(|session| &session.name == name)
            })
            .map_or(0, |index| index + 1)
    }

    fn status(&self) -> String {
        if let Some(error) = &self.action_error {
            return error.clone();
        }
        if let Some(error) = &self.poll_error {
            return error.clone();
        }
        if self.sessions.is_empty() {
            EMPTY_STATUS.to_owned()
        } else {
            IDLE_STATUS.to_owned()
        }
    }

    #[cfg(test)]
    fn prompt(&self) -> Option<String> {
        match &self.mode {
            PickerMode::Create { input, .. } => Some(format!("New session name: {input}")),
            PickerMode::EditName { input, .. } => Some(format!("Edit session name: {input}")),
            PickerMode::KillConfirm { session_name, .. } => Some(format!(
                "Kill session \"{session_name}\"? {}",
                YesNoSelector::text()
            )),
            PickerMode::KillAllConfirm { session_names, .. } => {
                let count = session_names.len();
                let noun = if count == 1 { "session" } else { "sessions" };
                Some(format!(
                    "Kill {count} terminated {noun}? {}",
                    YesNoSelector::text()
                ))
            }
            PickerMode::RecreateConfirm { session_name, .. } => Some(format!(
                "Recreate session \"{session_name}\"? {}",
                YesNoSelector::text()
            )),
            PickerMode::Idle => None,
        }
    }

    fn prompt_line(&self) -> Option<Line<'static>> {
        match &self.mode {
            PickerMode::Create { input, cursor } => Some(format_name_prompt_line(
                "New session name: ",
                input,
                *cursor,
            )),
            PickerMode::EditName { input, cursor, .. } => Some(format_name_prompt_line(
                "Edit session name: ",
                input,
                *cursor,
            )),
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
            PickerMode::KillAllConfirm {
                session_names,
                selector,
            } => {
                let count = session_names.len();
                let noun = if count == 1 { "session" } else { "sessions" };
                let mut line = Line::from(format!("Kill {count} terminated {noun}? "));
                for span in selector.render().spans {
                    line.push_span(span);
                }
                Some(line)
            }
            PickerMode::RecreateConfirm {
                session_name,
                selector,
            } => {
                let mut line = Line::from(format!("Recreate session \"{session_name}\"? "));
                for span in selector.render().spans {
                    line.push_span(span);
                }
                Some(line)
            }
            PickerMode::Idle => None,
        }
    }
}

fn render(frame: &mut Frame<'_>, state: &mut PickerState) {
    let frame_area = frame.area();
    if frame_area.width == 0 || frame_area.height == 0 {
        return;
    }
    let status_line = state
        .prompt_line()
        .unwrap_or_else(|| Line::from(state.status()));
    let area = picker_area(frame_area, state, &status_line);
    frame.render_widget(ClearWidget, frame_area);
    let block = Block::default()
        .title(picker_title_line(area.width))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().fg(Color::White).bg(Color::Indexed(235)))
        .border_style(Style::default().fg(Color::Indexed(68)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let (list_area, separator_area, status_area) = picker_chunks(inner, &status_line);
    state.set_list_viewport_height(list_area.height as usize);
    let list_offset = state.list_offset;
    let total_rows = state.sessions.len().saturating_add(1);
    let rows_above = list_offset > 0;
    let rows_below = list_offset.saturating_add(list_area.height as usize) < total_rows;
    let text_width = list_area.width.saturating_sub(LIST_GUTTER_WIDTH);
    let name_width = state
        .sessions
        .iter()
        .map(|session| UnicodeWidthStr::width(session.name.as_str()))
        .max()
        .unwrap_or(0);

    for visible_row in 0..list_area.height {
        let logical_row = list_offset.saturating_add(visible_row as usize);
        let row_area = Rect {
            x: list_area.x,
            y: list_area.y.saturating_add(visible_row),
            width: text_width,
            height: 1,
        };
        if logical_row == 0 {
            let selected = state.selected_index() == 0;
            frame.render_widget(Paragraph::new(create_row(selected, text_width)), row_area);
        } else if let Some(session) = state.sessions.get(logical_row - 1) {
            let selected = state.selected_index() == logical_row;
            let attach_detail = selected
                .then(|| state.pending_attach.row_detail())
                .flatten();
            let suffix =
                picker_status_detail(session, state.recreate_notice.as_ref(), attach_detail);
            let text = session_row_with_suffix(session, selected, text_width, name_width, suffix);
            frame.render_widget(Paragraph::new(text), row_area);
        }

        let marker = if visible_row == 0 && rows_above {
            Some("↑")
        } else if visible_row + 1 == list_area.height && rows_below {
            Some("↓")
        } else {
            None
        };
        if let Some(marker) = marker {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    marker,
                    Style::default().fg(Color::Gray),
                ))),
                Rect {
                    x: list_area.x + text_width,
                    y: list_area.y.saturating_add(visible_row),
                    width: LIST_GUTTER_WIDTH,
                    height: 1,
                },
            );
        }
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

fn picker_chunks(inner: Rect, status_line: &Line<'_>) -> (Rect, Rect, Rect) {
    let inner_width = inner.width as usize;
    let status_height = wrapped_line_count(status_line.width(), inner_width);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(status_height),
        ])
        .split(inner);
    (chunks[0], chunks[1], chunks[2])
}

fn picker_title_text() -> String {
    format!("stay v{}", env!("CARGO_PKG_VERSION"))
}

fn picker_title_line(area_width: u16) -> Line<'static> {
    let title = picker_title_text();
    let available_width = usize::from(area_width.saturating_sub(2));
    let title_width = available_width.saturating_sub(2);
    let title = truncate_to_width(&title, title_width);
    Line::from(format!(" {title} "))
}

fn picker_area(frame_area: Rect, state: &PickerState, status_line: &Line<'_>) -> Rect {
    let shortcut_width = UnicodeWidthStr::width(IDLE_STATUS);
    let name_width = state
        .sessions
        .iter()
        .map(|session| UnicodeWidthStr::width(session.name.as_str()))
        .max()
        .unwrap_or(0);
    let selected_index = state.selected_index();
    let row_width = state
        .sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let selected = selected_index == index + 1;
            let attach_detail = selected
                .then(|| state.pending_attach.row_detail())
                .flatten();
            suffix_display_width(&picker_status_detail(
                session,
                state.recreate_notice.as_ref(),
                attach_detail,
            ))
            .saturating_add(name_width)
        })
        .max()
        .unwrap_or(0);
    let content_width = row_width
        .max(UnicodeWidthStr::width("create new session"))
        .max(shortcut_width)
        .max(status_line.width())
        .max(picker_title_text().width().saturating_add(2))
        .saturating_add(usize::from(LIST_GUTTER_WIDTH));
    let width = u16::try_from(content_width.saturating_add(2))
        .unwrap_or(u16::MAX)
        .min(frame_area.width);
    let inner_width = usize::from(width.saturating_sub(2)).max(1);
    let status_height = wrapped_line_count(status_line.width(), inner_width);
    let list_height = u16::try_from(state.sessions.len().saturating_add(1)).unwrap_or(u16::MAX);
    let desired_height = list_height
        .saturating_add(1)
        .saturating_add(status_height)
        .saturating_add(2);
    let height = desired_height.min(frame_area.height);
    Rect {
        x: frame_area
            .x
            .saturating_add(frame_area.width.saturating_sub(width) / 2),
        y: frame_area
            .y
            .saturating_add(frame_area.height.saturating_sub(height) / 2),
        width,
        height,
    }
}

fn create_row(selected: bool, width: u16) -> Line<'static> {
    let width = usize::from(width);
    let text = truncate_to_width("create new session", width);
    let padding = " ".repeat(width.saturating_sub(UnicodeWidthStr::width(text.as_str())));
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let mut line = Line::from(Span::styled(text, style));
    if !padding.is_empty() {
        line.push_span(Span::raw(padding));
    }
    line
}

fn wrapped_line_count(width: usize, available_width: usize) -> u16 {
    if width == 0 || available_width == 0 {
        return 1;
    }
    u16::try_from(width.saturating_add(available_width.saturating_sub(1)) / available_width)
        .unwrap_or(u16::MAX)
        .max(1)
}

#[cfg(test)]
fn session_row_with_name_width(
    session: &SessionRecord,
    selected: bool,
    width: u16,
    name_width: usize,
) -> Line<'static> {
    session_row_with_suffix(
        session,
        selected,
        width,
        name_width,
        session.status_detail(),
    )
}

fn session_row_with_suffix(
    session: &SessionRecord,
    selected: bool,
    width: u16,
    name_width: usize,
    suffix: Vec<crate::tmux::SuffixSpan>,
) -> Line<'static> {
    let width = width as usize;
    let suffix = fitted_suffix(session, suffix, width);
    let suffix_width = suffix_display_width(&suffix);
    let name_width = name_width.min(width.saturating_sub(suffix_width));
    let name = truncate_to_width(&session.name, name_width);
    let name_padding = name_width.saturating_sub(UnicodeWidthStr::width(name.as_str()));
    let name_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let mut spans = vec![Span::styled(name, name_style)];
    if name_padding > 0 {
        spans.push(Span::styled(" ".repeat(name_padding), name_style));
    }
    spans.extend(picker_suffix_spans(session, selected, suffix));
    let used_width = name_width.saturating_add(suffix_width);
    if used_width < width {
        spans.push(Span::raw(" ".repeat(width - used_width)));
    }
    Line::from(spans)
}

fn picker_status_detail(
    session: &SessionRecord,
    recreate_notice: Option<&PickerRecreateNotice>,
    attach_detail: Option<&str>,
) -> Vec<crate::tmux::SuffixSpan> {
    let mut detail = session.status_detail();
    let Some(recreate_notice) =
        recreate_notice.filter(|notice| notice.session_name == session.name)
    else {
        if let Some(attach_detail) = attach_detail {
            detail.push(crate::tmux::SuffixSpan {
                text: format!(" [{attach_detail}]"),
                emphasis: false,
            });
        }
        return detail;
    };
    let notice = recreate_notice.notice.row_detail();
    let notice = notice
        .strip_prefix('[')
        .and_then(|notice| notice.strip_suffix(']'))
        .expect("recreate row detail has brackets");
    if let Some(last) = detail.last_mut() {
        if let Some(prefix) = last.text.strip_suffix(']') {
            last.text = format!("{prefix} - {notice}]");
        } else {
            detail.push(crate::tmux::SuffixSpan {
                text: format!(" [{notice}]"),
                emphasis: false,
            });
        }
    } else {
        detail.push(crate::tmux::SuffixSpan {
            text: format!(" [{notice}]"),
            emphasis: false,
        });
    }
    if let Some(attach_detail) = attach_detail {
        detail.push(crate::tmux::SuffixSpan {
            text: format!(" [{attach_detail}]"),
            emphasis: false,
        });
    }
    detail
}

fn picker_suffix_spans(
    session: &SessionRecord,
    selected: bool,
    suffix: Vec<crate::tmux::SuffixSpan>,
) -> Vec<Span<'static>> {
    let emphasize_exit =
        !selected && session.terminated && session.exit_code.is_some_and(|code| code != 0);
    let muted = Style::default().fg(Color::Gray);
    let red = Style::default().fg(Color::Red);
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let mut spans = Vec::new();
    let mut first_suffix = true;
    for suffix_span in suffix {
        let style = if selected {
            selected_style
        } else if suffix_span.emphasis {
            red
        } else {
            muted
        };
        if emphasize_exit
            && !suffix_span.emphasis
            && let Some(index) = suffix_span.text.find("exit=")
        {
            let (before, after) = suffix_span.text.split_at(index);
            if !before.is_empty() {
                push_picker_suffix_span(
                    &mut spans,
                    before.to_owned(),
                    style,
                    selected,
                    first_suffix,
                );
            }
            spans.push(Span::styled("exit=", red));
            first_suffix = false;
            let after = &after["exit=".len()..];
            if !after.is_empty() {
                spans.push(Span::styled(after.to_owned(), style));
            }
            continue;
        }
        push_picker_suffix_span(&mut spans, suffix_span.text, style, selected, first_suffix);
        first_suffix = false;
    }
    spans
}

fn push_picker_suffix_span(
    spans: &mut Vec<Span<'static>>,
    text: String,
    style: Style,
    selected: bool,
    first_suffix: bool,
) {
    if !selected
        && first_suffix
        && let Some(rest) = text.strip_prefix(' ')
    {
        spans.push(Span::raw(" "));
        if !rest.is_empty() {
            spans.push(Span::styled(rest.to_owned(), style));
        }
        return;
    }
    spans.push(Span::styled(text, style));
}

fn fitted_suffix(
    session: &SessionRecord,
    full: Vec<crate::tmux::SuffixSpan>,
    width: usize,
) -> Vec<crate::tmux::SuffixSpan> {
    if suffix_display_width(&full) <= width {
        return full;
    }
    if !session.terminated {
        if let Some(compact) = compact_recreate_suffix(&full, width) {
            return compact;
        }
        return truncate_suffix(&full, width);
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

fn compact_recreate_suffix(
    full: &[crate::tmux::SuffixSpan],
    width: usize,
) -> Option<Vec<crate::tmux::SuffixSpan>> {
    let text = full
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    let exit_code_start = text.find("exit code ")? + "exit code ".len();
    let exit_code_end = text[exit_code_start..]
        .find(|character: char| !character.is_ascii_digit())
        .map_or(text.len(), |offset| exit_code_start + offset);
    if exit_code_start == exit_code_end || !text.contains("recreate") {
        return None;
    }
    let exit_code = &text[exit_code_start..exit_code_end];
    let candidates = [
        format!(" [detached - exit code {exit_code} - recreate]"),
        format!(" [exit code {exit_code} - recreate]"),
        format!(" [exit code {exit_code} recreate]"),
    ];
    candidates
        .into_iter()
        .find(|candidate| UnicodeWidthStr::width(candidate.as_str()) <= width)
        .map(|text| {
            vec![crate::tmux::SuffixSpan {
                text,
                emphasis: false,
            }]
        })
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
    Home,
    End,
    DeleteForward,
    DeleteToStart,
    DeleteToEnd,
    DeletePreviousWord,
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
            0x01 => PickerKey::Home,
            0x02 => PickerKey::Left,
            0x03 => PickerKey::Escape,
            0x04 => PickerKey::DeleteForward,
            0x05 => PickerKey::End,
            0x06 => PickerKey::Right,
            0x0b => PickerKey::DeleteToEnd,
            0x15 => PickerKey::DeleteToStart,
            0x17 => PickerKey::DeletePreviousWord,
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
        let mut sequence = vec![direction];
        while sequence
            .last()
            .is_some_and(|byte| !byte.is_ascii_alphabetic() && *byte != b'~')
        {
            let Some(byte) = self.read_byte(ESCAPE_SEQUENCE_TIMEOUT)? else {
                return Ok(PickerKey::Other);
            };
            sequence.push(byte);
        }
        match sequence.as_slice() {
            [b'A'] => Ok(PickerKey::Up),
            [b'B'] => Ok(PickerKey::Down),
            [b'C'] => Ok(PickerKey::Right),
            [b'D'] => Ok(PickerKey::Left),
            [b'H'] | [b'1' | b'7', b'~'] => Ok(PickerKey::Home),
            [b'F'] | [b'4' | b'8', b'~'] => Ok(PickerKey::End),
            [b'3', b'~'] => Ok(PickerKey::DeleteForward),
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
        use nix::poll::{PollFd, PollFlags, poll};
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
        use nix::poll::{PollFd, PollFlags, poll};
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
        use crossterm::event::{Event, KeyCode, KeyModifiers, poll, read};

        if let Some(byte) = self.pending.pop_front() {
            return Ok(Some(byte));
        }
        if !poll(timeout).map_err(|error| format!("picker input poll failed: {error}"))? {
            return Ok(None);
        }
        match read().map_err(|error| format!("picker input read failed: {error}"))? {
            Event::Key(event) => {
                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    if let KeyCode::Char(character) = event.code {
                        let control = match character.to_ascii_lowercase() {
                            'a' => Some(0x01),
                            'b' => Some(0x02),
                            'c' => Some(0x03),
                            'd' => Some(0x04),
                            'e' => Some(0x05),
                            'f' => Some(0x06),
                            'h' => Some(0x08),
                            'k' => Some(0x0b),
                            'u' => Some(0x15),
                            'w' => Some(0x17),
                            _ => None,
                        };
                        if let Some(control) = control {
                            return Ok(Some(control));
                        }
                    }
                }
                match event.code {
                    KeyCode::Enter => Ok(Some(b'\r')),
                    KeyCode::Esc => Ok(Some(0x1b)),
                    KeyCode::Char(character) if character.is_ascii() => Ok(Some(character as u8)),
                    KeyCode::Backspace => Ok(Some(0x7f)),
                    KeyCode::Up => Ok(self.queue_sequence(b"\x1b[A")),
                    KeyCode::Down => Ok(self.queue_sequence(b"\x1b[B")),
                    KeyCode::Left => Ok(self.queue_sequence(b"\x1b[D")),
                    KeyCode::Right => Ok(self.queue_sequence(b"\x1b[C")),
                    KeyCode::Home => Ok(self.queue_sequence(b"\x1b[H")),
                    KeyCode::End => Ok(self.queue_sequence(b"\x1b[F")),
                    KeyCode::Delete => Ok(self.queue_sequence(b"\x1b[3~")),
                    _ => Ok(Some(0)),
                }
            }
            _ => Ok(Some(0)),
        }
    }

    #[cfg(not(unix))]
    fn queue_sequence(&mut self, sequence: &[u8]) -> Option<u8> {
        self.pending.extend(sequence);
        self.pending.pop_front()
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
            if let Ok(previous) = hook_previous.lock()
                && let Some(previous) = previous.as_ref()
            {
                previous(info);
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
            if let Ok(mut previous) = previous.lock()
                && let Some(previous) = previous.take()
            {
                panic::set_hook(previous);
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
    use crate::session_name::MAX_SESSION_NAME_CHARS;
    use std::fmt::Write;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn forced_main_preference_skips_the_probe() {
        // The force-main path short-circuits the probe, so it is
        // deterministic regardless of the controlling terminal.
        let main = resolve_screen_mode(ScreenPreference::ForceMainScreen);
        assert!(matches!(main.screen_mode, ScreenMode::MainScreen));
        assert!(main.leftover_input.is_empty());
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
            dead_signal: None,
            dead_time: None,
            current_directory: None,
            current_command: None,
        }
    }

    fn test_config() -> Config {
        Config {
            default_command: None,
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 10_000,
            log_capture_interval_seconds: 5,
        }
    }

    #[test]
    fn create_row_is_selected_by_default() {
        let state = PickerState::default();
        assert_eq!(state.selected_name, None);
        assert_eq!(state.selected_index(), 0);
        let row = create_row(true, 20);
        assert_eq!(row.spans[0].content, "create new session");
        assert!(row.spans[0].style.add_modifier.contains(Modifier::REVERSED));
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
        assert_eq!(state.selected_name, None);
        state.move_up();
        assert_eq!(state.selected_name, None);
    }

    #[test]
    fn selection_scrolls_to_keep_every_logical_row_visible() {
        let mut state = PickerState {
            sessions: vec![
                session("alpha", false),
                session("beta", false),
                session("gamma", false),
                session("delta", false),
            ],
            ..PickerState::default()
        };
        state.set_list_viewport_height(3);

        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.list_offset, 0);
        state.move_down();
        assert_eq!(state.selected_index(), 1);
        assert_eq!(state.list_offset, 0);
        state.move_down();
        assert_eq!(state.selected_index(), 2);
        assert_eq!(state.list_offset, 0);
        state.move_down();
        assert_eq!(state.selected_index(), 3);
        assert_eq!(state.list_offset, 1);
        state.move_down();
        assert_eq!(state.selected_index(), 4);
        assert_eq!(state.list_offset, 2);

        state.move_up();
        assert_eq!(state.selected_index(), 3);
        assert_eq!(state.list_offset, 2);
        state.move_up();
        assert_eq!(state.selected_index(), 2);
        assert_eq!(state.list_offset, 2);
        state.move_up();
        assert_eq!(state.selected_index(), 1);
        assert_eq!(state.list_offset, 1);
        state.move_up();
        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.list_offset, 0);
    }

    #[test]
    fn a_missing_selected_name_is_cleared_after_poll() {
        let mut state = PickerState {
            sessions: vec![session("alpha", false), session("beta", false)],
            selected_name: Some("beta".to_owned()),
            ..PickerState::default()
        };
        state.apply_poll_result(Ok(vec![session("alpha", false)]));
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn enter_on_create_row_opens_the_name_prompt() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("alpha", false)],
            ..PickerState::default()
        };
        let mut input = InputReader::new();
        handle_idle_key(&mut state, PickerKey::Enter, &tmux, &config, &mut input)
            .expect("Enter on create row should be handled");
        assert!(matches!(&state.mode, PickerMode::Create { input, cursor: 0 } if input.is_empty()));
        assert_eq!(state.prompt().as_deref(), Some("New session name: "));
        assert_eq!(state.selected_name, None);
    }

    #[test]
    fn c_focuses_create_row_and_opens_the_name_prompt() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("alpha", false)],
            selected_name: Some("alpha".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::new();
        handle_idle_key(&mut state, PickerKey::Char('c'), &tmux, &config, &mut input)
            .expect("c should be handled");
        assert!(matches!(&state.mode, PickerMode::Create { input, cursor: 0 } if input.is_empty()));
        assert_eq!(state.prompt().as_deref(), Some("New session name: "));
        assert_eq!(state.selected_name, None);
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn empty_create_submission_is_rejected_without_a_default_name() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: String::new(),
                cursor: 0,
            },
            ..PickerState::default()
        };
        let mut input = InputReader::new();
        assert!(
            handle_create_key(&mut state, PickerKey::Enter, &tmux, &config, &mut input,)
                .expect("empty create should be handled")
                .is_none()
        );
        assert!(matches!(&state.mode, PickerMode::Idle));
        assert!(
            state
                .action_error
                .as_deref()
                .is_some_and(|error| error.contains("empty"))
        );
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
            "↑/↓ select · v toggle view-only · l toggle low-priority · Enter attach · r recreate · e edit name · k kill · K kill all terminated · Esc quit"
        );
    }

    #[test]
    fn picker_area_shrink_wraps_and_centers_content() {
        let state = PickerState {
            sessions: vec![session("alpha", false)],
            ..PickerState::default()
        };
        let status_line = Line::from(IDLE_STATUS);
        let frame = Rect::new(0, 0, 160, 40);
        let area = picker_area(frame, &state, &status_line);
        assert_eq!(
            area.width,
            u16::try_from(status_line.width()).unwrap() + 2 + LIST_GUTTER_WIDTH
        );
        assert_eq!(area.height, 6);
        assert!(area.x > frame.x);
        assert!(area.y > frame.y);
        assert!(area.x + area.width < frame.x + frame.width);
        assert!(area.y + area.height < frame.y + frame.height);

        let small_frame = Rect::new(0, 0, 20, 5);
        assert_eq!(picker_area(small_frame, &state, &status_line), small_frame);
    }

    #[test]
    fn picker_prompts_keep_the_idle_shortcut_width_as_a_minimum() {
        let minimum_width =
            u16::try_from(UnicodeWidthStr::width(IDLE_STATUS)).unwrap() + 2 + LIST_GUTTER_WIDTH;
        let states = [
            PickerState {
                sessions: vec![session("alpha", false)],
                mode: PickerMode::Create {
                    input: String::new(),
                    cursor: 0,
                },
                ..PickerState::default()
            },
            PickerState {
                sessions: vec![session("alpha", false)],
                mode: PickerMode::EditName {
                    session_name: "alpha".to_owned(),
                    input: String::new(),
                    cursor: 0,
                },
                ..PickerState::default()
            },
            PickerState {
                sessions: vec![session("alpha", false)],
                mode: PickerMode::KillConfirm {
                    session_name: "alpha".to_owned(),
                    selector: YesNoSelector::new(true),
                },
                ..PickerState::default()
            },
        ];

        for state in states {
            let prompt = state.prompt_line().expect("prompt should be rendered");
            let area = picker_area(Rect::new(0, 0, 160, 40), &state, &prompt);
            assert_eq!(area.width, minimum_width);
        }
    }

    #[test]
    fn empty_picker_uses_the_full_shortcut_panel_width_as_a_minimum() {
        let state = PickerState {
            mode: PickerMode::Create {
                input: String::new(),
                cursor: 0,
            },
            ..PickerState::default()
        };
        let prompt = Line::from("x");
        let area = picker_area(Rect::new(0, 0, 160, 40), &state, &prompt);
        assert_eq!(
            area.width,
            u16::try_from(UnicodeWidthStr::width(IDLE_STATUS)).unwrap() + 2 + LIST_GUTTER_WIDTH
        );
        assert_eq!(area.height, 5);
    }

    #[test]
    fn picker_height_wraps_status_exactly_and_clamps_to_the_frame() {
        let state = PickerState::default();
        let status_line = Line::from("x".repeat(50));
        let frame = Rect::new(0, 0, 20, 20);
        let area = picker_area(frame, &state, &status_line);
        assert_eq!(area.width, frame.width);
        assert_eq!(area.height, 7);

        let short_frame = Rect::new(0, 0, 20, 5);
        assert_eq!(picker_area(short_frame, &state, &status_line), short_frame);
    }

    #[test]
    fn picker_width_accounts_for_maximum_and_unicode_names() {
        let name = "界".repeat(MAX_SESSION_NAME_CHARS);
        let state = PickerState {
            sessions: vec![session(&name, false)],
            ..PickerState::default()
        };
        let frame = Rect::new(0, 0, 500, 40);
        let area = picker_area(frame, &state, &Line::from(IDLE_STATUS));
        let row_width = UnicodeWidthStr::width(name.as_str())
            + suffix_display_width(&session(&name, false).status_detail());
        let expected_width = row_width + usize::from(LIST_GUTTER_WIDTH) + 2;
        assert_eq!(usize::from(area.width), expected_width);
        assert!(area.width < frame.width);
    }

    #[test]
    fn picker_create_and_rename_reject_over_limit_names() {
        let too_long = "x".repeat(MAX_SESSION_NAME_CHARS + 1);
        let tmux = Tmux::for_test_shell_script("exit 0");
        let config = test_config();
        let mut create_state = PickerState {
            mode: PickerMode::Create {
                input: too_long.clone(),
                cursor: too_long.len(),
            },
            ..PickerState::default()
        };
        handle_create_key(
            &mut create_state,
            PickerKey::Enter,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("over-limit create should be handled");
        assert!(
            create_state
                .action_error
                .as_deref()
                .is_some_and(|error| error.contains("must not exceed 128 Unicode characters"))
        );

        let mut edit_state = PickerState {
            sessions: vec![session("build", false)],
            selected_name: Some("build".to_owned()),
            mode: PickerMode::EditName {
                session_name: "build".to_owned(),
                input: too_long.clone(),
                cursor: too_long.len(),
            },
            ..PickerState::default()
        };
        handle_key(
            &mut edit_state,
            PickerKey::Enter,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("over-limit rename should be handled");
        assert_eq!(edit_state.sessions[0].name, "build");
        assert_eq!(edit_state.selected_name.as_deref(), Some("build"));
        assert!(
            edit_state
                .action_error
                .as_deref()
                .is_some_and(|error| error.contains("must not exceed 128 Unicode characters"))
        );
    }

    #[test]
    fn small_picker_viewport_shows_scroll_markers_and_selected_rows() {
        use ratatui::backend::TestBackend;

        for (selected_name, expected_offset, expected_above, expected_below) in [
            (None, 0, None, Some("↓")),
            (Some("gamma"), 1, Some("↑"), Some("↓")),
            (Some("delta"), 2, Some("↑"), None),
        ] {
            let mut state = PickerState {
                sessions: vec![
                    session("alpha", false),
                    session("beta", false),
                    session("gamma", false),
                    session("delta", false),
                ],
                selected_name: selected_name.map(str::to_owned),
                ..PickerState::default()
            };
            let frame = Rect::new(0, 0, 160, 7);
            let backend = TestBackend::new(frame.width, frame.height);
            let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
            terminal
                .draw(|frame| render(frame, &mut state))
                .expect("render picker viewport");

            let area = picker_area(frame, &state, &Line::from(IDLE_STATUS));
            let inner = Block::default().borders(Borders::ALL).inner(area);
            let (list_area, _, _) = picker_chunks(inner, &Line::from(IDLE_STATUS));
            assert_eq!(state.list_offset, expected_offset);
            assert_eq!(list_area.height, 3);
            let gutter_x = list_area.x + list_area.width - LIST_GUTTER_WIDTH;
            let buffer = terminal.backend().buffer();
            let first_gutter = buffer
                .cell((gutter_x, list_area.y))
                .expect("first gutter cell");
            let last_gutter = buffer
                .cell((gutter_x, list_area.y + list_area.height - 1))
                .expect("last gutter cell");
            assert_eq!(first_gutter.symbol(), expected_above.unwrap_or(" "));
            assert_eq!(last_gutter.symbol(), expected_below.unwrap_or(" "));
            if expected_above.is_some() {
                assert_eq!(first_gutter.fg, Color::Gray);
            }
            if expected_below.is_some() {
                assert_eq!(last_gutter.fg, Color::Gray);
            }

            let selected_row = state.selected_index() - state.list_offset;
            let selected_cell = buffer
                .cell((
                    list_area.x,
                    list_area.y + u16::try_from(selected_row).unwrap(),
                ))
                .expect("selected row cell");
            assert!(selected_cell.modifier.contains(Modifier::REVERSED));
            assert!(!first_gutter.modifier.contains(Modifier::REVERSED));
            assert!(!last_gutter.modifier.contains(Modifier::REVERSED));
        }
    }

    #[test]
    fn picker_render_styles_the_box_and_clears_its_surroundings() {
        use ratatui::backend::TestBackend;

        let mut state = PickerState {
            sessions: vec![session("alpha", false)],
            ..PickerState::default()
        };
        let backend = TestBackend::new(160, 40);
        let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
        terminal
            .draw(|frame| render(frame, &mut state))
            .expect("render picker");
        let area = picker_area(Rect::new(0, 0, 160, 40), &state, &Line::from(IDLE_STATUS));
        let buffer = terminal.backend().buffer();
        let border = buffer.cell((area.x, area.y)).expect("top-left border cell");
        assert_eq!(border.fg, Color::Indexed(68));
        let interior = buffer
            .cell((area.x + 1, area.y + 1))
            .expect("interior cell");
        assert_eq!(interior.bg, Color::Indexed(235));
        let surround = buffer.cell((0, 0)).expect("surround cell");
        assert_eq!(surround.bg, Color::Reset);
    }

    #[test]
    fn picker_title_uses_the_package_version_and_contributes_to_width() {
        use ratatui::backend::TestBackend;

        let title = picker_title_text();
        assert_eq!(title, format!("stay v{}", env!("CARGO_PKG_VERSION")));
        let title_line = picker_title_line(160);
        let title_text = title_line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(title_text, format!(" {title} "));

        let state = PickerState::default();
        let area = picker_area(Rect::new(0, 0, 160, 40), &state, &Line::from("x"));
        let expected_inner_width = UnicodeWidthStr::width("create new session")
            .max(UnicodeWidthStr::width(IDLE_STATUS))
            .max(UnicodeWidthStr::width(title.as_str()).saturating_add(2))
            .saturating_add(usize::from(LIST_GUTTER_WIDTH));
        assert_eq!(
            usize::from(area.width.saturating_sub(2)),
            expected_inner_width
        );

        let backend = TestBackend::new(160, 40);
        let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
        let mut state = PickerState::default();
        terminal
            .draw(|frame| render(frame, &mut state))
            .expect("render titled picker");
        let buffer = terminal.backend().buffer();
        let top = (area.x..area.x + area.width)
            .map(|x| buffer.cell((x, area.y)).expect("title cell").symbol())
            .collect::<String>();
        assert!(top.contains(title.as_str()));
    }

    #[test]
    fn picker_title_truncates_without_overwriting_narrow_borders() {
        use ratatui::backend::TestBackend;

        let frame = Rect::new(0, 0, 8, 5);
        let backend = TestBackend::new(frame.width, frame.height);
        let mut terminal = ratatui::Terminal::new(backend).expect("create test terminal");
        let mut state = PickerState::default();
        terminal
            .draw(|frame| render(frame, &mut state))
            .expect("render narrow picker");

        let buffer = terminal.backend().buffer();
        let top = (0..frame.width)
            .map(|x| buffer.cell((x, 0)).expect("top title cell").symbol())
            .collect::<String>();
        assert!(top.starts_with("╭"));
        assert!(top.ends_with("╮"));
        assert!(top.contains("stay"));
    }

    #[test]
    fn create_mode_renders_the_name_prompt_and_supports_editing() {
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: String::new(),
                cursor: 0,
            },
            ..PickerState::default()
        };
        assert_eq!(state.prompt().as_deref(), Some("New session name: "));
        state.push_create_character('w');
        state.push_create_character('o');
        state.push_create_character('r');
        state.push_create_character('k');
        assert_eq!(state.prompt().as_deref(), Some("New session name: work"));
        state.delete_create_character();
        assert_eq!(state.create_name(), "wor");
    }

    #[test]
    fn create_cursor_moves_clamps_and_inserts_at_each_position() {
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: String::new(),
                cursor: 0,
            },
            ..PickerState::default()
        };

        for character in "build".chars() {
            state.push_create_character(character);
        }
        state.move_create_cursor(PickerKey::Left);
        state.push_create_character('X');
        assert_eq!(state.create_name(), "builXd");
        for _ in 0..10 {
            state.move_create_cursor(PickerKey::Left);
        }
        state.push_create_character('^');
        assert_eq!(state.create_name(), "^builXd");
        for _ in 0..10 {
            state.move_create_cursor(PickerKey::Right);
        }
        state.push_create_character('$');
        assert_eq!(state.create_name(), "^builXd$");
        assert_eq!(
            state.prompt().as_deref(),
            Some("New session name: ^builXd$")
        );
    }

    #[test]
    fn create_home_and_end_move_to_the_prompt_boundaries() {
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: "build".to_owned(),
                cursor: "build".len(),
            },
            ..PickerState::default()
        };

        state.move_create_cursor(PickerKey::Home);
        assert_eq!(state.prompt().as_deref(), Some("New session name: build"));
        state.move_create_cursor(PickerKey::Home);
        state.push_create_character('^');
        state.move_create_cursor(PickerKey::End);
        state.push_create_character('$');
        assert_eq!(state.create_name(), "^build$");
        assert_eq!(state.prompt().as_deref(), Some("New session name: ^build$"));
    }

    #[test]
    fn create_backspace_deletes_one_unicode_scalar() {
        let input = "a東京b".to_owned();
        let mut state = PickerState {
            mode: PickerMode::Create {
                cursor: input.len(),
                input,
            },
            ..PickerState::default()
        };

        state.delete_create_character();
        state.delete_create_character();
        assert_eq!(state.create_name(), "a東");
        state.move_create_cursor(PickerKey::Left);
        state.delete_create_character();
        assert_eq!(state.create_name(), "東");
    }

    #[test]
    fn create_escape_cancels_without_creating_a_session() {
        let tmux = Tmux::for_test_shell_script("exit 99");
        let config = test_config();
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: "work".to_owned(),
                cursor: "work".len(),
            },
            ..PickerState::default()
        };
        let mut input = InputReader::new();

        handle_key(&mut state, PickerKey::Escape, &tmux, &config, &mut input)
            .expect("create cancellation should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert_eq!(state.action_error, None);
    }

    #[test]
    fn create_submission_passes_the_corrected_name_to_session_creation() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let log = std::env::temp_dir().join(format!("stay-picker-create-log-{stamp}"));
        let script = format!(
            "printf '%s\\n' \"$2 $3 $4 $5 $6 $7 $8 $9\" >> '{}'\nexit 0",
            log.display()
        );
        let tmux = Tmux::for_test_shell_script(script);
        let config = test_config();
        let mut state = PickerState::default();
        let mut input = InputReader::new();

        handle_idle_key(&mut state, PickerKey::Char('c'), &tmux, &config, &mut input)
            .expect("create shortcut should be handled");
        for character in "work".chars() {
            handle_key(
                &mut state,
                PickerKey::Char(character),
                &tmux,
                &config,
                &mut input,
            )
            .expect("create character should be handled");
        }
        handle_key(&mut state, PickerKey::Left, &tmux, &config, &mut input)
            .expect("create cursor movement should be handled");
        handle_key(&mut state, PickerKey::Char('X'), &tmux, &config, &mut input)
            .expect("create correction should be handled");
        let outcome = handle_key(&mut state, PickerKey::Enter, &tmux, &config, &mut input)
            .expect("create submission should be handled");

        assert!(matches!(
            outcome,
            Some(PickerOutcome::Attach { session_name, .. }) if session_name == "worXk"
        ));
        let calls = fs::read_to_string(&log).expect("read fake tmux calls");
        assert!(
            calls
                .lines()
                .any(|line| line.contains("new-session -d -s worXk"))
        );
        let _ = fs::remove_file(log);
    }

    #[test]
    fn invalid_create_name_is_rejected_without_session_creation() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let log = std::env::temp_dir().join(format!("stay-picker-invalid-create-log-{stamp}"));
        let script = format!("printf '%s\\n' \"$2\" >> '{}'\nexit 0", log.display());
        let tmux = Tmux::for_test_shell_script(script);
        let config = test_config();
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: "bad.name".to_owned(),
                cursor: "bad.name".len(),
            },
            ..PickerState::default()
        };

        handle_create_key(
            &mut state,
            PickerKey::Enter,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("invalid create should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert!(
            state
                .action_error
                .as_deref()
                .is_some_and(|error| error.contains("disallowed character"))
        );
        assert!(!log.exists());
    }

    #[test]
    fn duplicate_create_name_preserves_the_actionable_error() {
        let script = "if test \"$2\" = -f; then
          test \"$4\" = new-session && test \"$7\" = duplicate && {
            printf 'duplicate name\\n' >&2; exit 1;
          }
        elif test \"$2\" = new-session && test \"$5\" = duplicate; then
          printf 'duplicate name\\n' >&2; exit 1;
        fi
        exit 0";
        let tmux = Tmux::for_test_shell_script(script);
        let config = test_config();
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: "duplicate".to_owned(),
                cursor: "duplicate".len(),
            },
            ..PickerState::default()
        };

        handle_create_key(
            &mut state,
            PickerKey::Enter,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("duplicate create should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert!(
            state
                .action_error
                .as_deref()
                .is_some_and(|error| error.contains("duplicate name"))
        );
    }

    #[test]
    fn edit_name_mode_renders_the_name_prompt_and_supports_editing() {
        let mut state = PickerState {
            mode: PickerMode::EditName {
                session_name: "build".to_owned(),
                input: "build".to_owned(),
                cursor: "build".len(),
            },
            ..PickerState::default()
        };
        state.push_edit_name_character('r');
        state.push_edit_name_character('e');
        state.push_edit_name_character('n');
        assert_eq!(
            state.prompt().as_deref(),
            Some("Edit session name: buildren")
        );
        state.delete_edit_name_character();
        assert_eq!(
            state.edit_name(),
            ("build".to_owned(), "buildre".to_owned())
        );
    }

    #[test]
    fn edit_name_home_and_end_move_to_the_prompt_boundaries() {
        let mut state = PickerState {
            mode: PickerMode::EditName {
                session_name: "build".to_owned(),
                input: "build".to_owned(),
                cursor: "build".len(),
            },
            ..PickerState::default()
        };

        state.move_edit_name_cursor(PickerKey::Home);
        assert_eq!(state.prompt().as_deref(), Some("Edit session name: build"));
        state.move_edit_name_cursor(PickerKey::End);
        assert_eq!(state.prompt().as_deref(), Some("Edit session name: build"));
    }

    #[test]
    fn name_prompts_render_one_reverse_video_cursor_cell() {
        for mode in [
            PickerMode::Create {
                input: "build".to_owned(),
                cursor: 2,
            },
            PickerMode::EditName {
                session_name: "build".to_owned(),
                input: "build".to_owned(),
                cursor: 2,
            },
        ] {
            let state = PickerState {
                mode,
                ..PickerState::default()
            };
            let line = state.prompt_line().expect("name prompt should render");
            let cursor_spans = line
                .spans
                .iter()
                .filter(|span| span.style.add_modifier.contains(Modifier::REVERSED))
                .collect::<Vec<_>>();
            assert_eq!(cursor_spans.len(), 1);
            assert_eq!(cursor_spans[0].content, "i");
            assert!(!line.spans.iter().any(|span| span.content.contains('█')));
        }

        let state = PickerState {
            mode: PickerMode::Create {
                input: "build".to_owned(),
                cursor: "build".len(),
            },
            ..PickerState::default()
        };
        let line = state.prompt_line().expect("end cursor should render");
        let cursor_span = line
            .spans
            .iter()
            .find(|span| span.style.add_modifier.contains(Modifier::REVERSED))
            .expect("end cursor span");
        assert_eq!(cursor_span.content, " ");
    }

    #[test]
    fn edit_name_cursor_moves_clamps_and_inserts_in_the_middle() {
        let mut state = PickerState {
            mode: PickerMode::EditName {
                session_name: "build".to_owned(),
                input: "build".to_owned(),
                cursor: "build".len(),
            },
            ..PickerState::default()
        };

        state.move_edit_name_cursor(PickerKey::Left);
        state.push_edit_name_character('X');
        assert_eq!(state.edit_name(), ("build".to_owned(), "builXd".to_owned()));
        for _ in 0..10 {
            state.move_edit_name_cursor(PickerKey::Left);
        }
        state.push_edit_name_character('^');
        assert_eq!(state.edit_name().1, "^builXd");
        for _ in 0..10 {
            state.move_edit_name_cursor(PickerKey::Right);
        }
        state.push_edit_name_character('$');
        assert_eq!(state.edit_name().1, "^builXd$");
    }

    #[test]
    fn edit_name_backspace_deletes_one_unicode_scalar() {
        let mut state = PickerState {
            mode: PickerMode::EditName {
                session_name: "a東京b".to_owned(),
                input: "a東京b".to_owned(),
                cursor: "a東京b".len(),
            },
            ..PickerState::default()
        };

        state.delete_edit_name_character();
        state.delete_edit_name_character();
        assert_eq!(state.edit_name().1, "a東");
        state.move_edit_name_cursor(PickerKey::Left);
        state.delete_edit_name_character();
        assert_eq!(state.edit_name().1, "東");
    }

    #[test]
    fn edit_name_escape_preserves_the_original_session() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let original = session("build", false);
        let mut state = PickerState {
            sessions: vec![original.clone()],
            selected_name: Some("build".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::new();

        handle_idle_key(&mut state, PickerKey::Char('e'), &tmux, &config, &mut input)
            .expect("edit key should be handled");
        assert_eq!(state.prompt().as_deref(), Some("Edit session name: build"));
        handle_key(&mut state, PickerKey::Char('x'), &tmux, &config, &mut input)
            .expect("edit character should be handled");
        handle_key(&mut state, PickerKey::Escape, &tmux, &config, &mut input)
            .expect("edit cancellation should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert_eq!(state.sessions, vec![original]);
        assert_eq!(state.selected_name.as_deref(), Some("build"));
    }

    #[test]
    fn edit_name_enter_renames_and_selects_the_refreshed_row() {
        let tmux = Tmux::for_test_shell_script(
            "case \"$2\" in
               rename-session) exit 0 ;;
               list-panes) printf 'renamed:0:1:0:::\u{1f}/tmp\u{1f}sh\\n' ;;
             esac",
        );
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("build", false)],
            selected_name: Some("build".to_owned()),
            mode: PickerMode::EditName {
                session_name: "build".to_owned(),
                input: "renamed".to_owned(),
                cursor: "renamed".len(),
            },
            ..PickerState::default()
        };

        handle_key(
            &mut state,
            PickerKey::Enter,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("rename should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert_eq!(state.action_error, None);
        assert_eq!(state.selected_name.as_deref(), Some("renamed"));
        assert_eq!(state.sessions[0].name, "renamed");
    }

    #[test]
    fn edit_name_validation_and_duplicate_failures_preserve_the_original() {
        let config = test_config();
        for (tmux, edited, expected_error) in [
            (
                Tmux::for_test_shell_script("exit 99"),
                "bad.name",
                "disallowed character",
            ),
            (
                Tmux::for_test_shell_script("printf 'duplicate name\\n' >&2; exit 1"),
                "other",
                "duplicate name",
            ),
        ] {
            let mut state = PickerState {
                sessions: vec![session("build", false)],
                selected_name: Some("build".to_owned()),
                mode: PickerMode::EditName {
                    session_name: "build".to_owned(),
                    input: edited.to_owned(),
                    cursor: edited.len(),
                },
                ..PickerState::default()
            };

            handle_key(
                &mut state,
                PickerKey::Enter,
                &tmux,
                &config,
                &mut InputReader::new(),
            )
            .expect("rename failure should be handled");

            assert!(matches!(state.mode, PickerMode::Idle));
            assert_eq!(state.sessions[0].name, "build");
            assert_eq!(state.selected_name.as_deref(), Some("build"));
            assert!(
                state
                    .action_error
                    .as_deref()
                    .is_some_and(|error| error.contains(expected_error))
            );
        }
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
        assert!(
            !line.spans[0]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            line.spans[2]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
    }

    #[test]
    fn terminated_recreate_enters_confirmation_with_no_focused() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![SessionRecord {
                terminated: true,
                exit_code: Some(7),
                ..session("work", false)
            }],
            selected_name: Some("work".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::new();

        handle_idle_key(&mut state, PickerKey::Char('r'), &tmux, &config, &mut input)
            .expect("recreate key should be handled");

        assert_eq!(
            state.prompt().as_deref(),
            Some("Recreate session \"work\"? Yes No")
        );
        assert!(matches!(
            &state.mode,
            PickerMode::RecreateConfirm { selector, .. }
                if selector.focused_option() == YesNoOption::No
        ));
    }

    #[test]
    fn terminated_recreate_cancellation_leaves_the_session_untouched() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        for cancel_key in [PickerKey::Char('n'), PickerKey::Escape] {
            let terminated = SessionRecord {
                terminated: true,
                exit_code: Some(7),
                ..session("work", false)
            };
            let mut state = PickerState {
                sessions: vec![terminated.clone()],
                selected_name: Some("work".to_owned()),
                ..PickerState::default()
            };
            let mut input = InputReader::new();
            handle_idle_key(&mut state, PickerKey::Char('r'), &tmux, &config, &mut input)
                .expect("recreate key should be handled");
            handle_key(&mut state, cancel_key, &tmux, &config, &mut input)
                .expect("confirmation key should be handled");

            assert!(matches!(state.mode, PickerMode::Idle));
            assert_eq!(state.sessions, vec![terminated]);
        }
    }

    #[test]
    fn confirming_terminated_recreate_runs_once_and_refreshes_inventory() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let log = std::env::temp_dir().join(format!("stay-picker-recreate-log-{stamp}"));
        let marker = std::env::temp_dir().join(format!("stay-picker-recreate-marker-{stamp}"));
        let script = format!(
            "if test \"$2\" = -f; then command=\"$4\"; else command=\"$2\"; fi\nprintf '%s\\n' \"$command\" >> '{}'\ncase \"$command\" in\n  list-panes)\n    if test -f '{}'; then printf '%s\\n' 'work:0:1:0:::\u{1f}/tmp\u{1f}sh'; else printf '%s\\n' 'work:0:1:1:7:1:\u{1f}\u{1f}sh'; fi\n    ;;\n  kill-session)\n    : > '{}'\n    ;;\n  display-message|new-session|set-option|set-window-option)\n    ;;\nesac\n",
            log.display(),
            marker.display(),
            marker.display(),
        );
        let tmux = Tmux::for_test_shell_script(script);
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![SessionRecord {
                terminated: true,
                exit_code: Some(7),
                ..session("work", false)
            }],
            selected_name: Some("work".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::new();
        handle_idle_key(&mut state, PickerKey::Char('r'), &tmux, &config, &mut input)
            .expect("recreate key should be handled");
        handle_key(&mut state, PickerKey::Left, &tmux, &config, &mut input)
            .expect("left should focus Yes");
        handle_key(&mut state, PickerKey::Enter, &tmux, &config, &mut input)
            .expect("confirmation should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert!(!state.sessions[0].terminated);
        assert_eq!(state.action_error, None);
        let calls = fs::read_to_string(&log).expect("read fake tmux calls");
        assert_eq!(
            calls.lines().filter(|line| *line == "new-session").count(),
            1
        );
        assert!(calls.lines().any(|line| line == "kill-session"));
        let row = session_row_with_suffix(
            &state.sessions[0],
            false,
            100,
            4,
            picker_status_detail(&state.sessions[0], state.recreate_notice.as_ref(), None),
        );
        let row_text = row
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(
            row_text.contains("[detached - terminated with exit code 7 before recreate]"),
            "row: {row_text:?}"
        );
        assert_eq!(state.prompt(), None);
        assert_eq!(state.status(), IDLE_STATUS);
        let _ = fs::remove_file(log);
        let _ = fs::remove_file(marker);
    }

    #[test]
    fn live_recreate_keeps_the_direct_action_path() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("work", false)],
            selected_name: Some("work".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::new();

        handle_idle_key(&mut state, PickerKey::Char('r'), &tmux, &config, &mut input)
            .expect("recreate key should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert!(
            state
                .action_error
                .as_deref()
                .is_some_and(|error| error.contains("tmux command failed"))
        );
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
    fn input_reader_parses_home_and_end_sequences() {
        let mut input =
            InputReader::with_pending(b"\x1b[H\x1b[F\x1bOH\x1bOF\x1b[1~\x1b[4~".to_vec());
        for expected in [
            PickerKey::Home,
            PickerKey::End,
            PickerKey::Home,
            PickerKey::End,
            PickerKey::Home,
            PickerKey::End,
        ] {
            assert_eq!(
                input.next(Duration::ZERO).expect("read home/end key"),
                Some(expected)
            );
        }
    }

    #[test]
    fn input_reader_parses_common_readline_control_keys() {
        let mut input = InputReader::with_pending(
            [0x01, 0x05, 0x02, 0x06, 0x04, 0x0b, 0x15, 0x17, 0x03].to_vec(),
        );
        for expected in [
            PickerKey::Home,
            PickerKey::End,
            PickerKey::Left,
            PickerKey::Right,
            PickerKey::DeleteForward,
            PickerKey::DeleteToEnd,
            PickerKey::DeleteToStart,
            PickerKey::DeletePreviousWord,
            PickerKey::Escape,
        ] {
            assert_eq!(
                input
                    .next(Duration::ZERO)
                    .expect("read readline control key"),
                Some(expected)
            );
        }
    }

    #[test]
    fn readline_delete_controls_edit_the_create_name() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            mode: PickerMode::Create {
                input: "one two".to_owned(),
                cursor: "one two".len(),
            },
            ..PickerState::default()
        };
        let mut input = InputReader::new();

        handle_create_key(
            &mut state,
            PickerKey::DeletePreviousWord,
            &tmux,
            &config,
            &mut input,
        )
        .expect("Ctrl-W should be handled");
        assert_eq!(state.create_name(), "one ");
        handle_create_key(
            &mut state,
            PickerKey::DeleteToStart,
            &tmux,
            &config,
            &mut input,
        )
        .expect("Ctrl-U should be handled");
        assert_eq!(state.create_name(), "");

        state.push_create_character('a');
        state.push_create_character('b');
        state.push_create_character('c');
        state.move_create_cursor(PickerKey::Home);
        handle_create_key(
            &mut state,
            PickerKey::DeleteForward,
            &tmux,
            &config,
            &mut input,
        )
        .expect("Ctrl-D should be handled");
        assert_eq!(state.create_name(), "bc");
        state.move_create_cursor(PickerKey::End);
        state.push_create_character('d');
        state.move_create_cursor(PickerKey::Home);
        handle_create_key(
            &mut state,
            PickerKey::DeleteToEnd,
            &tmux,
            &config,
            &mut input,
        )
        .expect("Ctrl-K should be handled");
        assert_eq!(state.create_name(), "");
    }

    #[test]
    fn attach_modifier_toggles_produce_all_four_attach_outcomes() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        for (keys, expected_read_only, expected_low_priority, expected_row_detail) in [
            (&[][..], false, false, None),
            (
                &[PickerKey::Char('v')][..],
                true,
                false,
                Some("attach with view-only"),
            ),
            (
                &[PickerKey::Char('l')][..],
                false,
                true,
                Some("attach with low-priority"),
            ),
            (
                &[PickerKey::Char('v'), PickerKey::Char('l')][..],
                true,
                true,
                Some("attach with view-only + low-priority"),
            ),
        ] {
            let mut state = PickerState {
                sessions: vec![session("work", false)],
                selected_name: Some("work".to_owned()),
                ..PickerState::default()
            };
            let mut input = InputReader::new();
            for key in keys {
                assert!(
                    handle_idle_key(&mut state, *key, &tmux, &config, &mut input)
                        .expect("modifier key should be handled")
                        .is_none()
                );
            }
            assert_eq!(state.status(), IDLE_STATUS);
            let suffix = picker_status_detail(
                &state.sessions[0],
                state.recreate_notice.as_ref(),
                state.pending_attach.row_detail(),
            );
            let suffix_text = suffix
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>();
            match expected_row_detail {
                Some(detail) => assert!(suffix_text.ends_with(&format!(" [{detail}]"))),
                None => assert_eq!(suffix_text, " [detached]"),
            }
            let outcome = handle_idle_key(&mut state, PickerKey::Enter, &tmux, &config, &mut input)
                .expect("guard key should be handled")
                .expect("expected an attach outcome");
            match outcome {
                PickerOutcome::Attach {
                    session_name,
                    read_only,
                    low_priority,
                    ..
                } => {
                    assert_eq!(session_name, "work");
                    assert_eq!(read_only, expected_read_only);
                    assert_eq!(low_priority, expected_low_priority);
                }
                PickerOutcome::Quit => panic!("expected an attach outcome"),
            }
        }
    }

    #[test]
    fn attach_modifier_toggles_turn_off_and_selection_changes_clear_them() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("alpha", false), session("beta", false)],
            selected_name: Some("alpha".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::new();

        handle_idle_key(&mut state, PickerKey::Char('v'), &tmux, &config, &mut input)
            .expect("view-only toggle should be handled");
        handle_idle_key(&mut state, PickerKey::Char('v'), &tmux, &config, &mut input)
            .expect("view-only toggle-off should be handled");
        assert_eq!(state.status(), IDLE_STATUS);

        handle_idle_key(&mut state, PickerKey::Char('l'), &tmux, &config, &mut input)
            .expect("low-priority toggle should be handled");
        state.move_down();
        assert_eq!(state.selected_name.as_deref(), Some("beta"));
        assert_eq!(state.status(), IDLE_STATUS);

        handle_idle_key(&mut state, PickerKey::Char('v'), &tmux, &config, &mut input)
            .expect("view-only toggle should be handled");
        assert_eq!(state.status(), IDLE_STATUS);
        let suffix = picker_status_detail(
            &state.sessions[1],
            state.recreate_notice.as_ref(),
            state.pending_attach.row_detail(),
        );
        let suffix_text = suffix
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(suffix_text, " [detached] [attach with view-only]");
    }

    #[test]
    fn pending_attach_detail_is_only_rendered_on_the_selected_row() {
        let state = PickerState {
            sessions: vec![session("alpha", false), session("beta", false)],
            selected_name: Some("alpha".to_owned()),
            pending_attach: PendingAttachModifiers {
                read_only: true,
                low_priority: true,
            },
            ..PickerState::default()
        };
        let selected_suffix = picker_status_detail(
            &state.sessions[0],
            state.recreate_notice.as_ref(),
            state.pending_attach.row_detail(),
        );
        let unselected_suffix =
            picker_status_detail(&state.sessions[1], state.recreate_notice.as_ref(), None);
        let selected_text = selected_suffix
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        let unselected_text = unselected_suffix
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(
            selected_text,
            " [detached] [attach with view-only + low-priority]"
        );
        assert_eq!(unselected_text, " [detached]");
        assert_eq!(state.status(), IDLE_STATUS);
    }

    #[test]
    fn pending_attach_detail_respects_narrow_row_width() {
        let session = session("alpha", false);
        let suffix =
            picker_status_detail(&session, None, Some("attach with view-only + low-priority"));
        let row = session_row_with_suffix(&session, true, 24, 5, suffix);
        let row_text = row
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(UnicodeWidthStr::width(row_text.as_str()), 24);
        assert!(row_text.contains("[detached]"));
        assert!(row_text.contains("[attach"));
    }

    #[test]
    fn attach_modifier_toggles_preserve_typed_ahead_input() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("work", false)],
            selected_name: Some("work".to_owned()),
            ..PickerState::default()
        };
        let mut input = InputReader::with_pending(b"typed-ahead".to_vec());
        handle_idle_key(&mut state, PickerKey::Char('v'), &tmux, &config, &mut input)
            .expect("view-only toggle should be handled");
        handle_idle_key(&mut state, PickerKey::Char('l'), &tmux, &config, &mut input)
            .expect("low-priority toggle should be handled");
        let outcome = handle_idle_key(&mut state, PickerKey::Enter, &tmux, &config, &mut input)
            .expect("Enter should be handled")
            .expect("expected an attach outcome");
        match outcome {
            PickerOutcome::Attach {
                read_only,
                low_priority,
                residual_input,
                ..
            } => {
                assert!(read_only);
                assert!(low_priority);
                assert_eq!(residual_input, b"typed-ahead");
            }
            PickerOutcome::Quit => panic!("expected an attach outcome"),
        }
    }

    #[test]
    fn entering_other_picker_modes_clears_attach_modifiers() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = test_config();
        for key in [
            PickerKey::Char('c'),
            PickerKey::Char('e'),
            PickerKey::Char('k'),
            PickerKey::Char('r'),
            PickerKey::Char('q'),
            PickerKey::Escape,
        ] {
            let mut state = PickerState {
                sessions: vec![session("work", false)],
                selected_name: Some("work".to_owned()),
                ..PickerState::default()
            };
            let mut input = InputReader::new();
            handle_idle_key(&mut state, PickerKey::Char('v'), &tmux, &config, &mut input)
                .expect("view-only toggle should be handled");
            let _ = handle_idle_key(&mut state, key, &tmux, &config, &mut input);
            assert_eq!(state.pending_attach, PendingAttachModifiers::default());
        }
    }

    #[test]
    fn view_only_and_low_priority_keys_are_ignored_without_a_selection() {
        let tmux = Tmux::for_test_shell_script("exit 1");
        let config = Config {
            default_command: None,
            detach_key: 0x1c,
            copy_mode_key: 0,
            history_lines: 10_000,
            log_capture_interval_seconds: 5,
        };
        for key in [PickerKey::Char('v'), PickerKey::Char('l')] {
            let mut state = PickerState::default();
            let mut input = InputReader::new();
            let outcome = handle_idle_key(&mut state, key, &tmux, &config, &mut input)
                .expect("guard key should be handled");
            assert!(outcome.is_none());
            assert_eq!(state.pending_attach, PendingAttachModifiers::default());
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
                PickerMode::Idle
                | PickerMode::Create { .. }
                | PickerMode::EditName { .. }
                | PickerMode::KillAllConfirm { .. }
                | PickerMode::RecreateConfirm { .. } => panic!("expected kill confirmation"),
            },
            YesNoOption::No
        );
        assert_eq!(state.confirm_name(), "original");
    }

    #[test]
    fn kill_all_with_no_terminated_sessions_reports_exact_feedback() {
        let tmux = Tmux::for_test_shell_script("exit 99");
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![session("live", false)],
            selected_name: Some("live".to_owned()),
            ..PickerState::default()
        };

        handle_idle_key(
            &mut state,
            PickerKey::Char('K'),
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("K should be handled");

        assert!(matches!(state.mode, PickerMode::Idle));
        assert_eq!(state.status(), "No terminated sessions to kill.");
    }

    #[test]
    fn kill_all_confirmation_snapshots_only_terminated_rows_and_defaults_to_no() {
        let tmux = Tmux::for_test_shell_script("exit 99");
        let config = test_config();
        let first = SessionRecord {
            terminated: true,
            ..session("first", false)
        };
        let live = session("live", false);
        let second = SessionRecord {
            terminated: true,
            ..session("second", false)
        };
        let sessions = vec![first, live, second];
        let mut state = PickerState {
            sessions: sessions.clone(),
            selected_name: Some("live".to_owned()),
            ..PickerState::default()
        };

        handle_idle_key(
            &mut state,
            PickerKey::Char('K'),
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("K should be handled");

        assert_eq!(
            state.prompt().as_deref(),
            Some("Kill 2 terminated sessions? Yes No")
        );
        assert!(matches!(
            &state.mode,
            PickerMode::KillAllConfirm {
                session_names,
                selector,
            } if session_names == &["first", "second"]
                && selector.focused_option() == YesNoOption::No
        ));

        handle_key(
            &mut state,
            PickerKey::Escape,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("Escape should cancel");
        assert!(matches!(state.mode, PickerMode::Idle));
        assert_eq!(state.sessions, sessions);
    }

    #[test]
    fn kill_all_yes_kills_the_snapshot_and_refreshes_the_picker() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let log = std::env::temp_dir().join(format!("stay-picker-kill-all-log-{stamp}"));
        let script = format!(
            "case \"$2\" in kill-session) printf '%s\\n' \"$4\" >> '{}';; list-panes) ;; esac",
            log.display()
        );
        let tmux = Tmux::for_test_shell_script(script);
        let config = test_config();
        let mut state = PickerState {
            sessions: vec![
                SessionRecord {
                    terminated: true,
                    ..session("first", false)
                },
                session("live", false),
                SessionRecord {
                    terminated: true,
                    ..session("second", false)
                },
            ],
            ..PickerState::default()
        };

        handle_idle_key(
            &mut state,
            PickerKey::Char('K'),
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("K should be handled");
        handle_key(
            &mut state,
            PickerKey::Left,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("Left should select Yes");
        handle_key(
            &mut state,
            PickerKey::Enter,
            &tmux,
            &config,
            &mut InputReader::new(),
        )
        .expect("Enter should confirm");

        assert!(state.sessions.is_empty());
        assert_eq!(state.action_error, None);
        let calls = fs::read_to_string(&log).expect("read kill-all log");
        assert_eq!(calls.lines().collect::<Vec<_>>(), ["first", "second"]);
        let _ = fs::remove_file(log);
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
        let selected = session_row_with_name_width(&session("build", false), true, 24, 5);
        let ordinary = session_row_with_name_width(&session("build", false), false, 24, 5);
        assert_eq!(selected.spans[0].content, "build");
        assert!(
            selected.spans[0]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert_eq!(selected.spans[1].content, " [detached]");
        assert!(
            selected.spans[1]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            !selected
                .spans
                .last()
                .expect("trailing padding")
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert_eq!(ordinary.spans[1].content, " ");
        assert_eq!(ordinary.spans[2].content, "[detached]");
        assert_eq!(ordinary.spans[2].style.fg, Some(Color::Gray));
    }

    #[test]
    fn selected_create_row_leaves_trailing_padding_unreversed() {
        let row = create_row(true, 24);
        assert_eq!(row.spans[0].content, "create new session");
        assert!(row.spans[0].style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(row.spans[1].content, "      ");
        assert!(!row.spans[1].style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn session_statuses_align_after_the_longest_name_and_are_grey() {
        let short = session_row_with_name_width(&session("a", false), false, 32, 7);
        let long = session_row_with_name_width(&session("longest", true), false, 32, 7);

        assert_eq!(UnicodeWidthStr::width(short.spans[0].content.as_ref()), 1);
        assert_eq!(UnicodeWidthStr::width(long.spans[0].content.as_ref()), 7);
        assert_eq!(short.spans[0].content, "a");
        assert_eq!(short.spans[1].content, "      ");
        assert_eq!(short.spans[2].content, " ");
        assert_eq!(short.spans[3].content, "[detached]");
        assert_eq!(long.spans[1].content, " ");
        assert_eq!(long.spans[2].content, "[attached]");
        assert_eq!(short.spans[1].style.fg, None);
        assert_eq!(short.spans[3].style.fg, Some(Color::Gray));
        assert_eq!(long.spans[2].style.fg, Some(Color::Gray));
    }

    #[test]
    fn wide_names_are_padded_by_terminal_display_width() {
        let row = session_row_with_name_width(&session("東京", false), false, 12, 2);
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
            dead_signal: None,
            dead_time: Some(0),
            current_directory: None,
            current_command: None,
        };
        let unfocused = session_row_with_name_width(&terminated, false, 80, 5);
        let selected = session_row_with_name_width(&terminated, true, 80, 5);
        assert!(
            unfocused
                .spans
                .iter()
                .any(|span| span.content == "exit=" && span.style.fg == Some(Color::Red))
        );
        assert!(
            unfocused
                .spans
                .iter()
                .any(|span| span.content == "7" && span.style.fg == Some(Color::Red))
        );
        assert!(
            selected
                .spans
                .iter()
                .any(|span| span.content.contains("exit=") && span.style.fg != Some(Color::Red))
        );
        assert!(
            selected
                .spans
                .iter()
                .any(|span| span.content == "7" && span.style.fg != Some(Color::Red))
        );
        assert!(
            selected
                .spans
                .iter()
                .any(|span| span.content.contains("@1970-01-01T")),
            "selected spans: {:?}",
            selected.spans
        );
    }

    #[test]
    fn terminated_rows_render_a_signal_number_emphasised_like_a_nonzero_exit_code() {
        let signalled = SessionRecord {
            name: "build".to_owned(),
            attached: false,
            created: 0,
            terminated: true,
            exit_code: None,
            dead_signal: Some(9),
            dead_time: Some(0),
            current_directory: None,
            current_command: None,
        };
        let unfocused = session_row_with_name_width(&signalled, false, 80, 5);
        let selected = session_row_with_name_width(&signalled, true, 80, 5);
        assert!(
            unfocused
                .spans
                .iter()
                .any(|span| span.content == "9" && span.style.fg == Some(Color::Red))
        );
        assert!(
            unfocused
                .spans
                .iter()
                .any(|span| span.content.contains("signal="))
        );
        assert!(
            selected
                .spans
                .iter()
                .any(|span| span.content == "9" && span.style.fg != Some(Color::Red))
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
            dead_signal: None,
            dead_time: Some(0),
            current_directory: None,
            current_command: None,
        };
        let with_exit = session_row_with_name_width(&terminated, false, 25, 13);
        let with_exit_text = with_exit
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(UnicodeWidthStr::width(with_exit_text.as_str()), 25);
        assert!(with_exit_text.contains("[terminated exit=7]"));
        assert!(!with_exit_text.contains('@'));

        let marker_only = session_row_with_name_width(&terminated, false, 19, 13);
        let marker_only_text = marker_only
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(UnicodeWidthStr::width(marker_only_text.as_str()), 19);
        assert!(marker_only_text.ends_with("[terminated]"));
        assert!(!marker_only_text.contains("exit="));
    }

    #[test]
    fn narrow_recreate_details_keep_exit_code_and_recreate_words() {
        let full = vec![crate::tmux::SuffixSpan {
            text: " [detached - terminated with exit code 7 before recreate]".to_owned(),
            emphasis: false,
        }];
        let compact = compact_recreate_suffix(&full, 30).expect("compact recreate detail");
        let text = compact
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert_eq!(text, " [exit code 7 - recreate]");
        assert!(text.contains("exit code 7"));
        assert!(text.contains("recreate"));
    }

    #[cfg(unix)]
    #[allow(unsafe_code)]
    #[test]
    fn panic_restores_the_picker_terminal_state() {
        use nix::pty::{ForkptyResult, Winsize, forkpty};
        use nix::sys::termios;
        use nix::sys::wait::waitpid;
        use nix::unistd::{pipe, read, write};
        use std::os::fd::AsFd;

        let _lock = crate::test_global_state_lock();
        let (release_read, release_write) = pipe().expect("create picker panic handshake");
        let result = unsafe { forkpty(None::<&Winsize>, None) }.expect("allocate picker PTY");
        match result {
            ForkptyResult::Child => {
                drop(release_write);
                let mut release = [0_u8; 1];
                read(&release_read, &mut release).expect("wait for baseline termios");
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
                drop(release_read);
                let before = termios::tcgetattr(master.as_fd()).expect("read picker PTY state");
                write(&release_write, &[0]).expect("release picker panic test");
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
        use nix::pty::{ForkptyResult, Winsize, forkpty};
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
        use nix::poll::{PollFd, PollFlags, poll};
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
        use nix::pty::{ForkptyResult, Winsize, forkpty};
        use std::ffi::CString;
        use std::os::fd::AsFd;
        use std::os::unix::ffi::OsStrExt;

        let _lock = crate::test_global_state_lock();
        let executable = CString::new(std::env::current_exe().unwrap().as_os_str().as_bytes())
            .expect("test executable path contains no NUL");
        let helper_name = match preference {
            ScreenPreference::Auto => "picker_run_auto_helper",
            ScreenPreference::ForceMainScreen => "picker_run_main_helper",
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
            log_capture_interval_seconds: 5,
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

    /// Reap the child if it has already exited; otherwise it is stuck, so
    /// SIGKILL it and reap the corpse. Never blocks indefinitely.
    #[cfg(unix)]
    fn reap_or_kill(child: nix::unistd::Pid) -> i32 {
        use nix::sys::signal::{Signal, kill};
        use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};

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
    fn picker_auto_preference_enters_the_alternate_buffer_when_probe_succeeds() {
        let spec = EmulatorSpec {
            honors_alt_screen: true,
            responds_to_dsr: true,
            inject: vec![0x1b],
            inject_after: Duration::from_millis(100),
            inject_on_first_dsr: false,
        };
        let (code, emu) = run_picker_in_pty(&spec, ScreenPreference::Auto);
        assert_eq!(code, 0, "picker should quit cleanly on Esc");
        assert!(
            emu.saw_enter_alt_screen,
            "a successful Auto probe must enter the alternate screen"
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

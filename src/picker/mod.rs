//! The interactive session picker.

use crate::config::Config;
use crate::session;
use crate::tmux::{SessionRecord, Tmux};
use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use std::collections::VecDeque;
use std::io::{self, IsTerminal, Write};
use std::panic::{self, PanicHookInfo};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const ESCAPE_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(20);
const IDLE_STATUS: &str = "↑/↓ select  Enter attach  Esc quit";
const EMPTY_STATUS: &str = "Esc quit";

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

/// Opens the picker and, when the user attaches, hands off to the relay.
///
/// # Errors
///
/// Returns an error when terminal setup, picker input/rendering, or the
/// selected session's attach operation fails.
pub fn run(tmux: &Tmux, config: &Config) -> Result<u8, String> {
    if !io::stdout().is_terminal() {
        return Err("the interactive picker requires a terminal".to_owned());
    }

    let outcome = run_picker(tmux)?;
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

fn run_picker(tmux: &Tmux) -> Result<PickerOutcome, String> {
    let _terminal_guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|error| format!("failed to initialize picker terminal: {error}"))?;
    let mut input = InputReader::new();
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
            match key {
                PickerKey::Quit => return Ok(PickerOutcome::Quit),
                PickerKey::Up => state.move_up(),
                PickerKey::Down => state.move_down(),
                PickerKey::Enter => {
                    if let Some(session_name) = state.selected_name.clone() {
                        let residual_input = input.drain_available()?;
                        return Ok(PickerOutcome::Attach {
                            session_name,
                            residual_input,
                        });
                    }
                }
                PickerKey::Other => {}
            }
        }
    }
}

#[derive(Default)]
struct PickerState {
    sessions: Vec<SessionRecord>,
    selected_name: Option<String>,
    poll_error: Option<String>,
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
        if let Some(error) = &self.poll_error {
            return error;
        }
        if self.sessions.is_empty() {
            EMPTY_STATUS
        } else {
            IDLE_STATUS
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
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
        frame.render_widget(Paragraph::new(Line::styled(text, style)), row_area);
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
    frame.render_widget(Paragraph::new(state.status()), status_area);
}

fn session_row(session: &SessionRecord, selected: bool, width: u16) -> String {
    let width = width as usize;
    let suffix = if selected {
        String::new()
    } else {
        format!(" [{}]", session.marker())
    };
    let suffix_width = UnicodeWidthStr::width(suffix.as_str());
    let available = width.saturating_sub(suffix_width);
    let mut row = truncate_to_width(&session.name, available);
    let row_width = UnicodeWidthStr::width(row.as_str());
    row.push_str(&" ".repeat(width.saturating_sub(row_width + suffix_width)));
    row.push_str(&suffix);
    row
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
    Enter,
    Quit,
    Other,
}

struct InputReader {
    pending: VecDeque<u8>,
}

impl InputReader {
    fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }

    fn next(&mut self, timeout: Duration) -> Result<Option<PickerKey>, String> {
        let Some(byte) = self.read_byte(timeout)? else {
            return Ok(None);
        };
        let key = match byte {
            b'\r' | b'\n' => PickerKey::Enter,
            b'q' | 0x1b => self.escape_or_quit(byte)?,
            _ => PickerKey::Other,
        };
        Ok(Some(key))
    }

    fn escape_or_quit(&mut self, byte: u8) -> Result<PickerKey, String> {
        if byte == b'q' {
            return Ok(PickerKey::Quit);
        }
        let Some(next) = self.read_byte(ESCAPE_SEQUENCE_TIMEOUT)? else {
            return Ok(PickerKey::Quit);
        };
        if next != b'[' && next != b'O' {
            self.pending.push_front(next);
            return Ok(PickerKey::Quit);
        }
        let Some(direction) = self.read_byte(ESCAPE_SEQUENCE_TIMEOUT)? else {
            self.pending.push_front(next);
            return Ok(PickerKey::Quit);
        };
        match direction {
            b'A' => Ok(PickerKey::Up),
            b'B' => Ok(PickerKey::Down),
            _ => Ok(PickerKey::Other),
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
                KeyCode::Char('q') => Ok(Some(b'q')),
                KeyCode::Up => Ok(Some(b'\x01')),
                KeyCode::Down => Ok(Some(b'\x02')),
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

struct TerminalGuard {
    active: Arc<Mutex<bool>>,
    previous_hook: Option<Arc<Mutex<Option<PanicHook>>>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self, String> {
        enable_raw_mode().map_err(|error| format!("failed to enter raw terminal mode: {error}"))?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(format!("failed to enter alternate screen: {error}"));
        }

        let active = Arc::new(Mutex::new(true));
        let previous = Arc::new(Mutex::new(Some(panic::take_hook())));
        let hook_active = Arc::clone(&active);
        let hook_previous = Arc::clone(&previous);
        panic::set_hook(Box::new(move |info| {
            restore_if_active(&hook_active);
            if let Ok(previous) = hook_previous.lock() {
                if let Some(previous) = previous.as_ref() {
                    previous(info);
                }
            }
        }));

        Ok(Self {
            active,
            previous_hook: Some(previous),
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_if_active(&self.active);
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

fn restore_if_active(active: &Arc<Mutex<bool>>) {
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
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str, attached: bool) -> SessionRecord {
        SessionRecord {
            name: name.to_owned(),
            attached,
            created: 0,
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
        assert_eq!(state.status(), IDLE_STATUS);
    }

    #[test]
    fn selected_rows_omit_the_marker_and_fill_the_row() {
        let selected = session_row(&session("build", false), true, 16);
        let ordinary = session_row(&session("build", false), false, 16);
        assert_eq!(selected, "build           ");
        assert_eq!(ordinary, "build        [d]");
    }

    #[test]
    fn wide_names_are_padded_by_terminal_display_width() {
        let row = session_row(&session("東京", false), false, 12);
        assert_eq!(UnicodeWidthStr::width(row.as_str()), 12);
        assert!(row.ends_with("[d]"));
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
                    let _guard = TerminalGuard::enter().expect("enter picker terminal");
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
}

//! What each key press and mouse report does to the picker: the selection,
//! the hovered row, and when the user resumes a session or dismisses the
//! picker. Nothing here draws or reads the terminal.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// The longest gap between two clicks on one row that still makes a double
/// click; the common desktop default.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// What an event asks the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The state changed: draw again.
    Redraw,
    /// Nothing changed.
    Ignore,
    /// Resume the session at this index.
    Resume(usize),
    /// Close the picker and leave a plain shell.
    Dismiss,
}

/// The picker's selection, hover and click history over `len` sessions.
#[derive(Debug)]
pub(crate) struct State {
    len: usize,
    selected: usize,
    hover: Option<usize>,
    last_click: Option<(usize, Instant)>,
}

impl State {
    /// A picker over `len` sessions, the first selected.
    pub(crate) fn new(len: usize) -> Self {
        Self {
            len,
            selected: 0,
            hover: None,
            last_click: None,
        }
    }

    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn hover(&self) -> Option<usize> {
        self.hover
    }

    fn select(&mut self, index: usize) -> Step {
        let index = index.min(self.len.saturating_sub(1));
        if index == self.selected {
            return Step::Ignore;
        }
        self.selected = index;
        Step::Redraw
    }

    /// Applies a key press; `page` is how many sessions one screen shows.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's KeyCode has dozens of keys the picker ignores"
    )]
    pub(crate) fn on_key(&mut self, key: KeyEvent, page: usize) -> Step {
        if key.kind != KeyEventKind::Press {
            return Step::Ignore;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Step::Dismiss,
            KeyCode::Esc | KeyCode::Char('q') => Step::Dismiss,
            KeyCode::Enter => Step::Resume(self.selected),
            KeyCode::Up | KeyCode::Char('k') => self.select(self.selected.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.select(self.selected.saturating_add(1)),
            KeyCode::Home | KeyCode::Char('g') => self.select(0),
            KeyCode::End | KeyCode::Char('G') => self.select(self.len),
            KeyCode::PageUp => self.select(self.selected.saturating_sub(page.max(1))),
            KeyCode::PageDown => self.select(self.selected.saturating_add(page.max(1))),
            KeyCode::Char(digit @ '1'..='9') => digit
                .to_digit(10)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .filter(|&index| index < self.len)
                .map_or(Step::Ignore, Step::Resume),
            _ => Step::Ignore,
        }
    }

    /// Applies a mouse report; `hit` is the session drawn on the reported row,
    /// if any.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's MouseEventKind has buttons and directions the picker ignores"
    )]
    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent, hit: Option<usize>, now: Instant) -> Step {
        let hovered = self.set_hover(hit);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.select(self.selected.saturating_sub(1)),
            MouseEventKind::ScrollDown => self.select(self.selected.saturating_add(1)),
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(index) = hit else {
                    self.last_click = None;
                    return hovered;
                };
                let double = self.last_click.is_some_and(|(last, at)| {
                    last == index && now.saturating_duration_since(at) <= DOUBLE_CLICK
                });
                if double {
                    self.last_click = None;
                    return Step::Resume(index);
                }
                self.last_click = Some((index, now));
                match self.select(index) {
                    Step::Ignore => hovered,
                    step => step,
                }
            }
            _ => hovered,
        }
    }

    fn set_hover(&mut self, hit: Option<usize>) -> Step {
        if self.hover == hit {
            return Step::Ignore;
        }
        self.hover = hit;
        Step::Redraw
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 5,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn click() -> MouseEvent {
        mouse(MouseEventKind::Down(MouseButton::Left))
    }

    #[test]
    fn keys_move_the_selection_within_the_list() {
        let mut state = State::new(4);
        assert_eq!(state.on_key(key(KeyCode::Up), 2), Step::Ignore);
        assert_eq!(state.on_key(key(KeyCode::Char('j')), 2), Step::Redraw);
        assert_eq!(state.on_key(key(KeyCode::PageDown), 2), Step::Redraw);
        assert_eq!(state.selected(), 3);
        assert_eq!(state.on_key(key(KeyCode::Down), 2), Step::Ignore);
        assert_eq!(state.on_key(key(KeyCode::Char('g')), 2), Step::Redraw);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.on_key(key(KeyCode::End), 2), Step::Redraw);
        assert_eq!(state.selected(), 3);
    }

    #[test]
    fn enter_digits_and_escape_end_the_picker() {
        let mut state = State::new(4);
        assert_eq!(state.on_key(key(KeyCode::Char('3')), 1), Step::Resume(2));
        assert_eq!(state.on_key(key(KeyCode::Char('9')), 1), Step::Ignore);
        assert_eq!(state.on_key(key(KeyCode::Enter), 1), Step::Resume(0));
        assert_eq!(state.on_key(key(KeyCode::Esc), 1), Step::Dismiss);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(state.on_key(ctrl_c, 1), Step::Dismiss);
    }

    #[test]
    fn a_double_click_resumes_and_a_single_click_selects() {
        let start = Instant::now();
        let mut state = State::new(3);
        assert_eq!(state.on_mouse(click(), Some(0), start), Step::Redraw);
        assert_eq!(
            state.selected(),
            0,
            "a click on the selected row keeps it open"
        );
        let late = start + Duration::from_millis(500);
        assert_eq!(state.on_mouse(click(), Some(1), late), Step::Redraw);
        assert_eq!(state.selected(), 1);
        let soon = late + Duration::from_millis(250);
        assert_eq!(state.on_mouse(click(), Some(1), soon), Step::Resume(1));
    }

    #[test]
    fn clicks_on_different_rows_are_not_a_double_click() {
        let start = Instant::now();
        let mut state = State::new(3);
        state.on_mouse(click(), Some(0), start);
        let soon = start + Duration::from_millis(100);
        assert_eq!(state.on_mouse(click(), Some(2), soon), Step::Redraw);
        assert_eq!(state.selected(), 2);
    }

    #[test]
    fn the_wheel_moves_the_selection() {
        let now = Instant::now();
        let mut state = State::new(3);
        let down = mouse(MouseEventKind::ScrollDown);
        state.on_mouse(down, None, now);
        state.on_mouse(down, None, now);
        assert_eq!(state.selected(), 2);
        assert_eq!(state.on_mouse(down, None, now), Step::Ignore);
    }

    #[test]
    fn hover_follows_the_pointer_and_redraws_only_on_change() {
        let now = Instant::now();
        let mut state = State::new(3);
        let moved = mouse(MouseEventKind::Moved);
        assert_eq!(state.on_mouse(moved, Some(1), now), Step::Redraw);
        assert_eq!(state.hover(), Some(1));
        assert_eq!(state.on_mouse(moved, Some(1), now), Step::Ignore);
        assert_eq!(state.on_mouse(moved, None, now), Step::Redraw);
        assert_eq!(state.hover(), None);
    }
}

//! What each key press and mouse report does to the picker: the selection,
//! the hovered row, the search line, and when the user resumes a session or
//! dismisses the picker. Nothing here draws or reads the terminal.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// What an event asks the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The state changed: draw again.
    Redraw,
    /// The search line opened, closed or changed: list the rows again, then
    /// call [`State::set_len`].
    Filter,
    /// Nothing changed.
    Ignore,
    /// Resume the session at this index.
    Resume(usize),
    /// Close the picker and leave a plain shell.
    Dismiss,
}

impl Step {
    /// This step, or `other` when this one changes nothing; one event can
    /// both move the hover and change the selection.
    fn or(self, other: Self) -> Self {
        match self {
            Self::Ignore => other,
            Self::Redraw | Self::Filter | Self::Resume(_) | Self::Dismiss => self,
        }
    }
}

/// The picker's selection, hovered row and search line over `len` sessions.
#[derive(Debug)]
pub(crate) struct State {
    len: usize,
    selected: usize,
    hover: Option<usize>,
    /// The search line's text, while it is open.
    query: Option<String>,
}

impl State {
    /// A picker over `len` sessions, the first selected.
    pub(crate) fn new(len: usize) -> Self {
        Self {
            len,
            selected: 0,
            hover: None,
            query: None,
        }
    }

    /// A picker opened on the search line, with `query` typed.
    pub(crate) fn searching(query: String) -> Self {
        Self {
            query: Some(query),
            ..Self::new(0)
        }
    }

    /// The search line's text, while it is open.
    pub(crate) fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// The list now holds `len` rows: select the first and forget the hover.
    pub(crate) fn set_len(&mut self, len: usize) {
        self.len = len;
        self.selected = 0;
        self.hover = None;
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

    fn resume(&self) -> Step {
        if self.len == 0 {
            Step::Ignore
        } else {
            Step::Resume(self.selected)
        }
    }

    /// Applies a key press; `page` is how many sessions one screen shows.
    /// While the search line is open, printable keys edit it.
    pub(crate) fn on_key(&mut self, key: KeyEvent, page: usize) -> Step {
        if key.kind != KeyEventKind::Press {
            return Step::Ignore;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Step::Dismiss;
        }
        if let Some(step) = self.on_move_key(key.code, page) {
            return step;
        }
        let Some(query) = self.query.as_mut() else {
            return self.on_list_key(key.code);
        };
        if let Some(step) = Self::on_search_key(query, key) {
            return step;
        }
        self.query = None;
        Step::Filter
    }

    /// The keys that move or resume on the list and the search line alike.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's KeyCode has dozens of keys the picker ignores"
    )]
    fn on_move_key(&mut self, code: KeyCode, page: usize) -> Option<Step> {
        Some(match code {
            KeyCode::Enter => self.resume(),
            KeyCode::Up => self.select(self.selected.saturating_sub(1)),
            KeyCode::Down => self.select(self.selected.saturating_add(1)),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(self.len),
            KeyCode::PageUp => self.select(self.selected.saturating_sub(page.max(1))),
            KeyCode::PageDown => self.select(self.selected.saturating_add(page.max(1))),
            _ => return None,
        })
    }

    /// A key on the list, with the search line closed.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's KeyCode has dozens of keys the picker ignores"
    )]
    fn on_list_key(&mut self, code: KeyCode) -> Step {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => Step::Dismiss,
            KeyCode::Char('/') => {
                self.query = Some(String::new());
                Step::Filter
            }
            KeyCode::Char('k') => self.select(self.selected.saturating_sub(1)),
            KeyCode::Char('j') => self.select(self.selected.saturating_add(1)),
            KeyCode::Char('g') => self.select(0),
            KeyCode::Char('G') => self.select(self.len),
            KeyCode::Char(digit @ '1'..='9') => digit
                .to_digit(10)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .filter(|&index| index < self.len)
                .map_or(Step::Ignore, Step::Resume),
            _ => Step::Ignore,
        }
    }

    /// A key on the open search line; `None` when it closes the line.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's KeyCode has dozens of keys the search line ignores"
    )]
    fn on_search_key(query: &mut String, key: KeyEvent) -> Option<Step> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        Some(match key.code {
            KeyCode::Esc => return None,
            KeyCode::Backspace => {
                if query.pop().is_some() {
                    Step::Filter
                } else {
                    Step::Ignore
                }
            }
            KeyCode::Char('u') if control => {
                if query.is_empty() {
                    Step::Ignore
                } else {
                    query.clear();
                    Step::Filter
                }
            }
            KeyCode::Char(c)
                if !control && !key.modifiers.contains(KeyModifiers::ALT) && !c.is_control() =>
            {
                query.push(c);
                Step::Filter
            }
            _ => Step::Ignore,
        })
    }

    /// Applies a mouse report; `hit` is the session drawn on the reported row,
    /// if any. A click on another row selects it, and a click on the selected
    /// row resumes it.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's MouseEventKind has buttons and directions the picker ignores"
    )]
    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent, hit: Option<usize>) -> Step {
        let hovered = self.set_hover(hit);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.select(self.selected.saturating_sub(1)).or(hovered),
            MouseEventKind::ScrollDown => self.select(self.selected.saturating_add(1)).or(hovered),
            MouseEventKind::Down(MouseButton::Left) => match hit {
                Some(index) if index == self.selected => Step::Resume(index),
                Some(index) => self.select(index).or(hovered),
                None => hovered,
            },
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
    fn a_click_selects_another_row_and_resumes_the_selected_one() {
        let mut state = State::new(3);
        assert_eq!(state.on_mouse(click(), Some(1)), Step::Redraw);
        assert_eq!(state.selected(), 1);
        assert_eq!(state.on_mouse(click(), Some(2)), Step::Redraw);
        assert_eq!(state.selected(), 2);
        assert_eq!(state.on_mouse(click(), Some(2)), Step::Resume(2));
    }

    #[test]
    fn a_click_outside_the_rows_changes_nothing() {
        let mut state = State::new(3);
        assert_eq!(state.on_mouse(click(), None), Step::Ignore);
        assert_eq!(state.selected(), 0);
    }

    fn press(state: &mut State, code: KeyCode) -> Step {
        state.on_key(key(code), 1)
    }

    #[test]
    fn slash_opens_the_search_line_and_letters_type_into_it() {
        let mut state = State::new(3);
        assert_eq!(press(&mut state, KeyCode::Char('/')), Step::Filter);
        assert_eq!(state.query(), Some(""));
        assert_eq!(press(&mut state, KeyCode::Char('q')), Step::Filter);
        assert_eq!(press(&mut state, KeyCode::Char('3')), Step::Filter);
        assert_eq!(state.query(), Some("q3"));
        assert_eq!(press(&mut state, KeyCode::Backspace), Step::Filter);
        assert_eq!(state.query(), Some("q"));
        let clear = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(state.on_key(clear, 1), Step::Filter);
        assert_eq!(state.query(), Some(""));
        assert_eq!(press(&mut state, KeyCode::Backspace), Step::Ignore);
    }

    #[test]
    fn escape_closes_the_search_line_then_the_picker() {
        let mut state = State::searching("login".to_owned());
        state.set_len(2);
        assert_eq!(press(&mut state, KeyCode::Down), Step::Redraw);
        assert_eq!(press(&mut state, KeyCode::Enter), Step::Resume(1));
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Filter);
        assert_eq!(state.query(), None);
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Dismiss);
    }

    #[test]
    fn a_new_list_selects_its_first_row_and_enter_needs_a_row() {
        let mut state = State::new(3);
        press(&mut state, KeyCode::Down);
        state.set_len(0);
        assert_eq!(state.selected(), 0);
        assert_eq!(press(&mut state, KeyCode::Enter), Step::Ignore);
    }

    #[test]
    fn the_wheel_moves_the_selection() {
        let mut state = State::new(3);
        let down = mouse(MouseEventKind::ScrollDown);
        state.on_mouse(down, None);
        state.on_mouse(down, None);
        assert_eq!(state.selected(), 2);
        assert_eq!(state.on_mouse(down, None), Step::Ignore);
    }

    #[test]
    fn a_wheel_at_the_end_still_draws_the_new_hover() {
        let mut state = State::new(1);
        let down = mouse(MouseEventKind::ScrollDown);
        assert_eq!(state.on_mouse(down, Some(0)), Step::Redraw);
        assert_eq!(state.hover(), Some(0));
    }

    #[test]
    fn hover_follows_the_pointer_and_redraws_only_on_change() {
        let mut state = State::new(3);
        let moved = mouse(MouseEventKind::Moved);
        assert_eq!(state.on_mouse(moved, Some(1)), Step::Redraw);
        assert_eq!(state.hover(), Some(1));
        assert_eq!(state.on_mouse(moved, Some(1)), Step::Ignore);
        assert_eq!(state.on_mouse(moved, None), Step::Redraw);
        assert_eq!(state.hover(), None);
    }
}

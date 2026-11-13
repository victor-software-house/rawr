//! What each key press and mouse report does to the picker: the selection,
//! the hovered row, the search box, whether the list is expanded, and when
//! the user resumes a session or dismisses the picker. Nothing here draws or
//! reads the terminal.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// What an event asks the loop to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The state changed: draw again.
    Redraw,
    /// The search text changed: list the rows again, then call
    /// [`State::set_len`].
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

/// The picker's selection, hovered row and search box over `total` sessions.
///
/// The box is always there. While it holds text the list shows every match;
/// otherwise it shows `limit` sessions, or all of them once expanded.
#[derive(Debug)]
pub(crate) struct State {
    /// How many sessions the list holds.
    total: usize,
    /// How many sessions show before `ctrl+o` expands the list.
    limit: usize,
    selected: usize,
    hover: Option<usize>,
    /// What the user typed in the search box.
    text: String,
    /// Whether the list holds every local session, as `rawr find` opens it,
    /// instead of only the pane's own while the box is empty.
    everything: bool,
    expanded: bool,
}

impl State {
    /// A picker over the pane's `total` sessions, the first selected, that
    /// shows `limit` of them until expanded.
    pub(crate) fn new(total: usize, limit: usize) -> Self {
        Self {
            total,
            limit,
            selected: 0,
            hover: None,
            text: String::new(),
            everything: false,
            expanded: false,
        }
    }

    /// A picker over every local session, with `text` typed in the box.
    pub(crate) fn searching(text: String, limit: usize) -> Self {
        Self {
            text,
            everything: true,
            ..Self::new(0, limit)
        }
    }

    /// What the user typed in the search box.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The text to search every local session for, or `None` while the list
    /// is the pane's own sessions.
    pub(crate) fn query(&self) -> Option<&str> {
        (self.everything || self.filtering()).then_some(self.text.as_str())
    }

    /// Whether the box holds text, so the list shows every match.
    pub(crate) fn filtering(&self) -> bool {
        !self.text.is_empty()
    }

    /// Whether `ctrl+o` has shown the whole list.
    pub(crate) fn expanded(&self) -> bool {
        self.expanded
    }

    /// How many sessions the list shows now.
    pub(crate) fn visible(&self) -> usize {
        if self.filtering() || self.expanded {
            self.total
        } else {
            self.total.min(self.limit)
        }
    }

    /// How many sessions `ctrl+o` would show that the list hides now.
    pub(crate) fn earlier(&self) -> usize {
        self.total.saturating_sub(self.visible())
    }

    /// Whether `ctrl+o` changes what the list shows: more sessions than
    /// rows, and no search text, which shows every match anyway.
    pub(crate) fn can_expand(&self) -> bool {
        !self.filtering() && self.total > self.limit
    }

    /// The list now holds `total` rows: select the first and forget the
    /// hover.
    pub(crate) fn set_len(&mut self, total: usize) {
        self.total = total;
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
        let index = index.min(self.visible().saturating_sub(1));
        if index == self.selected {
            return Step::Ignore;
        }
        self.selected = index;
        Step::Redraw
    }

    fn resume(&self) -> Step {
        if self.visible() == 0 {
            Step::Ignore
        } else {
            Step::Resume(self.selected)
        }
    }

    /// Shows the whole list, or the first `limit` sessions again; nothing
    /// changes when the list has no more sessions than rows, or while the
    /// box holds text, which shows every match anyway.
    fn toggle_expanded(&mut self) -> Step {
        if !self.can_expand() {
            return Step::Ignore;
        }
        self.expanded = !self.expanded;
        self.selected = self.selected.min(self.visible().saturating_sub(1));
        self.hover = None;
        Step::Redraw
    }

    /// Empties the search box.
    fn clear(&mut self) -> Step {
        if self.text.is_empty() {
            return Step::Ignore;
        }
        self.text.clear();
        Step::Filter
    }

    /// Applies a key press; `page` is how many sessions one screen shows.
    /// Printable keys type into the search box.
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
        self.on_text_key(key)
    }

    /// The keys that move or resume.
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
            KeyCode::End => self.select(self.total),
            KeyCode::PageUp => self.select(self.selected.saturating_sub(page.max(1))),
            KeyCode::PageDown => self.select(self.selected.saturating_add(page.max(1))),
            _ => return None,
        })
    }

    /// A key for the search box and the list's `ctrl+o`: Esc clears the box,
    /// and dismisses the picker when it is empty.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "crossterm's KeyCode has dozens of keys the search box ignores"
    )]
    fn on_text_key(&mut self, key: KeyEvent) -> Step {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc if self.text.is_empty() => Step::Dismiss,
            KeyCode::Esc => self.clear(),
            KeyCode::Backspace => {
                if self.text.pop().is_some() {
                    Step::Filter
                } else {
                    Step::Ignore
                }
            }
            KeyCode::Char('u') if control => self.clear(),
            KeyCode::Char('o') if control => self.toggle_expanded(),
            KeyCode::Char(c)
                if !control && !key.modifiers.contains(KeyModifiers::ALT) && !c.is_control() =>
            {
                self.text.push(c);
                Step::Filter
            }
            _ => Step::Ignore,
        }
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

    /// How many sessions show before `ctrl+o`, as the config defaults.
    const ROWS: usize = 3;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
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

    fn press(state: &mut State, code: KeyCode) -> Step {
        state.on_key(key(code), 1)
    }

    fn type_text(state: &mut State, text: &str) {
        for c in text.chars() {
            assert_eq!(press(state, KeyCode::Char(c)), Step::Filter);
        }
    }

    #[test]
    fn arrows_home_end_and_paging_move_the_selection() {
        let mut state = State::new(4, 4);
        assert_eq!(state.on_key(key(KeyCode::Up), 2), Step::Ignore);
        assert_eq!(state.on_key(key(KeyCode::Down), 2), Step::Redraw);
        assert_eq!(state.on_key(key(KeyCode::PageDown), 2), Step::Redraw);
        assert_eq!(state.selected(), 3);
        assert_eq!(state.on_key(key(KeyCode::Down), 2), Step::Ignore);
        assert_eq!(state.on_key(key(KeyCode::Home), 2), Step::Redraw);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.on_key(key(KeyCode::End), 2), Step::Redraw);
        assert_eq!(state.selected(), 3);
        assert_eq!(state.on_key(key(KeyCode::PageUp), 2), Step::Redraw);
        assert_eq!(state.selected(), 1);
    }

    #[test]
    fn the_old_list_shortcuts_are_typed_text() {
        let mut state = State::new(4, 4);
        type_text(&mut state, "qjkgG39/");
        assert_eq!(state.text(), "qjkgG39/");
        assert_eq!(state.selected(), 0);
    }

    #[test]
    fn typing_needs_no_slash_and_backspace_and_ctrl_u_edit() {
        let mut state = State::new(3, ROWS);
        assert_eq!(state.query(), None);
        type_text(&mut state, "shop");
        assert_eq!(state.text(), "shop");
        assert_eq!(state.query(), Some("shop"));
        assert_eq!(press(&mut state, KeyCode::Backspace), Step::Filter);
        assert_eq!(state.text(), "sho");
        assert_eq!(state.on_key(ctrl('u'), 1), Step::Filter);
        assert_eq!(state.text(), "");
        assert_eq!(state.query(), None);
        assert_eq!(press(&mut state, KeyCode::Backspace), Step::Ignore);
        assert_eq!(state.on_key(ctrl('u'), 1), Step::Ignore);
    }

    #[test]
    fn a_shifted_letter_types_and_other_control_keys_do_not() {
        let mut state = State::new(3, ROWS);
        let shifted = KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT);
        assert_eq!(state.on_key(shifted, 1), Step::Filter);
        assert_eq!(state.on_key(ctrl('w'), 1), Step::Ignore);
        let alt = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(state.on_key(alt, 1), Step::Ignore);
        assert_eq!(state.text(), "S");
    }

    #[test]
    fn escape_clears_the_text_then_dismisses() {
        let mut state = State::new(3, ROWS);
        type_text(&mut state, "login");
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Filter);
        assert_eq!(state.text(), "");
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Dismiss);
    }

    #[test]
    fn escape_dismisses_an_empty_box_in_a_search_over_everything() {
        let mut state = State::searching(String::new(), ROWS);
        assert_eq!(state.query(), Some(""));
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Dismiss);

        let mut state = State::searching("login".to_owned(), ROWS);
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Filter);
        assert_eq!(state.query(), Some(""));
        assert_eq!(press(&mut state, KeyCode::Esc), Step::Dismiss);
    }

    #[test]
    fn ctrl_c_dismisses_whatever_is_typed() {
        let mut state = State::new(3, ROWS);
        assert_eq!(state.on_key(ctrl('c'), 1), Step::Dismiss);
        type_text(&mut state, "login");
        assert_eq!(state.on_key(ctrl('c'), 1), Step::Dismiss);
    }

    #[test]
    fn enter_resumes_the_selected_session_and_needs_a_row() {
        let mut state = State::new(4, 4);
        press(&mut state, KeyCode::Down);
        assert_eq!(press(&mut state, KeyCode::Enter), Step::Resume(1));
        state.set_len(0);
        assert_eq!(state.selected(), 0);
        assert_eq!(press(&mut state, KeyCode::Enter), Step::Ignore);
    }

    #[test]
    fn a_click_selects_another_row_and_resumes_the_selected_one() {
        let mut state = State::new(3, ROWS);
        assert_eq!(state.on_mouse(click(), Some(1)), Step::Redraw);
        assert_eq!(state.selected(), 1);
        assert_eq!(state.on_mouse(click(), Some(2)), Step::Redraw);
        assert_eq!(state.selected(), 2);
        assert_eq!(state.on_mouse(click(), Some(2)), Step::Resume(2));
    }

    #[test]
    fn a_click_outside_the_rows_changes_nothing() {
        let mut state = State::new(3, ROWS);
        assert_eq!(state.on_mouse(click(), None), Step::Ignore);
        assert_eq!(state.selected(), 0);
    }

    #[test]
    fn the_wheel_moves_the_selection() {
        let mut state = State::new(3, ROWS);
        let down = mouse(MouseEventKind::ScrollDown);
        state.on_mouse(down, None);
        state.on_mouse(down, None);
        assert_eq!(state.selected(), 2);
        assert_eq!(state.on_mouse(down, None), Step::Ignore);
    }

    #[test]
    fn a_wheel_at_the_end_still_draws_the_new_hover() {
        let mut state = State::new(1, ROWS);
        let down = mouse(MouseEventKind::ScrollDown);
        assert_eq!(state.on_mouse(down, Some(0)), Step::Redraw);
        assert_eq!(state.hover(), Some(0));
    }

    #[test]
    fn hover_follows_the_pointer_and_redraws_only_on_change() {
        let mut state = State::new(3, ROWS);
        let moved = mouse(MouseEventKind::Moved);
        assert_eq!(state.on_mouse(moved, Some(1)), Step::Redraw);
        assert_eq!(state.hover(), Some(1));
        assert_eq!(state.on_mouse(moved, Some(1)), Step::Ignore);
        assert_eq!(state.on_mouse(moved, None), Step::Redraw);
        assert_eq!(state.hover(), None);
    }

    #[test]
    fn seven_sessions_show_three_until_ctrl_o() {
        let mut state = State::new(7, ROWS);
        assert_eq!(state.visible(), 3);
        assert_eq!(state.earlier(), 4);
        assert!(state.can_expand());

        assert_eq!(state.on_key(ctrl('o'), 1), Step::Redraw);
        assert!(state.expanded());
        assert_eq!(state.visible(), 7);
        assert_eq!(state.earlier(), 0);

        assert_eq!(state.on_key(ctrl('o'), 1), Step::Redraw);
        assert!(!state.expanded());
        assert_eq!(state.visible(), 3);
    }

    #[test]
    fn two_sessions_and_three_sessions_have_nothing_earlier() {
        for total in [2, 3] {
            let state = State::new(total, ROWS);
            assert_eq!(state.visible(), total);
            assert_eq!(state.earlier(), 0);
            assert!(!state.can_expand());
        }
    }

    #[test]
    fn ctrl_o_does_nothing_when_it_would_show_nothing_more() {
        for total in [2, 3] {
            let mut state = State::new(total, ROWS);
            assert_eq!(state.on_key(ctrl('o'), 1), Step::Ignore);
            assert!(!state.expanded());
        }

        let mut state = State::new(7, ROWS);
        type_text(&mut state, "shop");
        assert_eq!(state.on_key(ctrl('o'), 1), Step::Ignore);
        assert!(!state.expanded());
    }

    #[test]
    fn the_selection_stays_within_the_rows_shown() {
        let mut state = State::new(7, ROWS);
        assert_eq!(state.on_key(key(KeyCode::End), 1), Step::Redraw);
        assert_eq!(state.selected(), 2);
        assert_eq!(press(&mut state, KeyCode::Down), Step::Ignore);

        state.on_key(ctrl('o'), 1);
        press(&mut state, KeyCode::End);
        assert_eq!(state.selected(), 6);

        state.on_key(ctrl('o'), 1);
        assert_eq!(state.selected(), 2);
    }

    #[test]
    fn filtering_shows_every_match() {
        let mut state = State::new(7, ROWS);
        type_text(&mut state, "shop");
        state.set_len(5);
        assert!(state.filtering());
        assert_eq!(state.visible(), 5);
        assert_eq!(state.earlier(), 0);
        assert!(!state.can_expand());

        press(&mut state, KeyCode::End);
        assert_eq!(state.selected(), 4);

        state.on_key(ctrl('u'), 1);
        state.set_len(7);
        assert_eq!(state.visible(), 3);
        assert_eq!(state.earlier(), 4);
    }

    #[test]
    fn a_new_list_selects_its_first_row() {
        let mut state = State::new(4, 4);
        press(&mut state, KeyCode::Down);
        state.on_mouse(mouse(MouseEventKind::Moved), Some(1));
        state.set_len(3);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.hover(), None);
    }
}

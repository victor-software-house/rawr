//! What the app holds and how keys and the mouse change it. Nothing here
//! draws or reads the terminal, so every event is a plain function of the
//! state.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::app::feed::Pane;
use crate::catalog::Hit;
use crate::session::Agent;
use crate::status::Status;

/// Which list the keys move in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    /// The sidebar of panes.
    Panes,
    /// The selected pane's sessions.
    Sessions,
    /// The search results; the box at the bottom takes the keys.
    Search,
}

/// Whether the app has heard from Rex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Link {
    Connecting,
    Live,
    /// Rex could not be reached.
    Offline(String),
    /// The event stream ended; the panes shown are the last ones heard.
    Stale(String),
}

/// What an event asks the loop to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    None,
    Quit,
    /// Give the terminal back and become `claude --resume` for this session.
    Resume(Agent),
    /// Show this pane in the Rex app.
    GoTo {
        session_id: String,
        block_id: String,
    },
    /// The search text changed; the loop finds the matches.
    Search,
}

/// A button; each does what its key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    GoTo,
    Resume,
    Search,
    Help,
    Quit,
    /// Folds the sidebar to a rail of glyphs, or opens it again.
    Sidebar,
}

/// Something on screen that takes a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Pane(usize),
    Session(usize),
    Result(usize),
    Search,
    Button(Button),
    /// The edge of the sidebar; dragging it resizes the sidebar.
    Edge,
    /// The line that stands for a pane's untitled scratch sessions.
    Scratch,
}

/// The narrowest open sidebar, and the narrowest main area beside it.
pub(crate) const MIN_SIDEBAR: u16 = 22;
const MIN_MAIN: u16 = 30;

/// How many lines one wheel notch moves a list.
const WHEEL: i16 = 2;

/// How far a list is scrolled: the first of its lines on screen. Keys move
/// the selection and the list follows it; the wheel moves the list alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Scroll {
    pub(crate) top: u16,
    /// Whether the next frame brings the selection into view.
    pub(crate) follow: bool,
}

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) pane: usize,
    pub(crate) session: usize,
    pub(crate) focus: Focus,
    pub(crate) link: Link,
    pub(crate) query: String,
    pub(crate) results: Vec<Hit>,
    /// Whether the transcripts are still being read for the current query.
    pub(crate) deep: bool,
    /// How many transcripts the deep search has read, of how many.
    pub(crate) read: (usize, usize),
    pub(crate) result: usize,
    pub(crate) help: bool,
    /// One line for the status bar, such as the Rex app's refusal.
    pub(crate) notice: Option<String>,
    /// The block this app runs in, so its own pane is marked.
    pub(crate) here: Option<String>,
    /// What the pointer is over.
    pub(crate) hover: Option<Target>,
    /// The open sidebar's width, in columns.
    pub(crate) sidebar: u16,
    /// Whether the sidebar is folded to its rail.
    pub(crate) folded: bool,
    /// Whether the sidebar's edge is being dragged.
    pub(crate) dragging: bool,
    /// The spinner's frame, moved on by the loop's tick while a pane works.
    pub(crate) frame: usize,
    /// Whether a pane's untitled scratch sessions are listed.
    pub(crate) show_scratch: bool,
    /// The panes, sessions and results lists, in that order.
    scrolls: [Scroll; 3],
}

/// A list's place in [`App::scrolls`].
const fn slot(list: Focus) -> usize {
    match list {
        Focus::Panes => 0,
        Focus::Sessions => 1,
        Focus::Search => 2,
    }
}

/// Where panes go in the list: the ones that need the user first.
pub(crate) const fn rank(status: Status) -> u8 {
    match status {
        Status::NeedsPermission | Status::NeedsAnswer | Status::NeedsLogin | Status::NeedsInput => {
            0
        }
        Status::Failed => 1,
        Status::Done => 2,
        Status::Working => 3,
        Status::Idle => 4,
        Status::Stopped => 5,
    }
}

impl App {
    pub(crate) fn new(here: Option<String>) -> Self {
        Self {
            panes: Vec::new(),
            pane: 0,
            session: 0,
            focus: Focus::Panes,
            link: Link::Connecting,
            query: String::new(),
            results: Vec::new(),
            deep: false,
            read: (0, 0),
            result: 0,
            help: false,
            notice: None,
            here,
            hover: None,
            sidebar: 38,
            folded: false,
            dragging: false,
            frame: 0,
            show_scratch: false,
            scrolls: [Scroll::default(); 3],
        }
    }

    pub(crate) const fn scroll_of(&self, list: Focus) -> Scroll {
        self.scrolls[slot(list)]
    }

    fn scroll_mut(&mut self, list: Focus) -> &mut Scroll {
        &mut self.scrolls[slot(list)]
    }

    /// Keeps where the last frame scrolled each list to.
    pub(crate) fn settle(&mut self, tops: &[(Focus, u16)]) {
        for (list, top) in tops {
            *self.scroll_mut(*list) = Scroll {
                top: *top,
                follow: false,
            };
        }
    }

    pub(crate) fn selected(&self) -> Option<&Pane> {
        self.panes.get(self.pane)
    }

    /// Whether any pane works, so the spinner needs the tick.
    pub(crate) fn animating(&self) -> bool {
        self.panes.iter().any(|pane| pane.status == Status::Working)
    }

    /// Takes a new list of panes, sorted by attention, and keeps the
    /// selection on the same pane when it is still there.
    pub(crate) fn set_panes(&mut self, mut panes: Vec<Pane>) {
        panes.sort_by(|a, b| {
            rank(a.status)
                .cmp(&rank(b.status))
                .then_with(|| newest(a).cmp(&newest(b)))
        });
        let kept = self.selected().map(|pane| pane.block_id.clone());
        self.panes = panes;
        self.link = Link::Live;
        self.pane = kept
            .and_then(|id| self.panes.iter().position(|pane| pane.block_id == id))
            .unwrap_or(0)
            .min(self.panes.len().saturating_sub(1));
        self.session = self.session.min(self.session_count().saturating_sub(1));
    }

    fn session_count(&self) -> usize {
        self.selected().map_or(0, |pane| pane.sessions.len())
    }

    /// The session `r` resumes: the highlighted one, or the selected pane's
    /// newest that no live Claude holds.
    fn chosen(&self) -> Option<Agent> {
        match self.focus {
            Focus::Search => self
                .results
                .get(self.result)
                .filter(|hit| self.running(&hit.agent.session_id).is_none())
                .map(|hit| hit.agent.clone()),
            Focus::Sessions => self
                .selected()
                .and_then(|pane| pane.sessions.get(self.session))
                .filter(|(_, held)| !held)
                .map(|(agent, _)| agent.clone()),
            Focus::Panes => self.selected().and_then(|pane| {
                pane.sessions
                    .iter()
                    .find(|(_, held)| !held)
                    .map(|(agent, _)| agent.clone())
            }),
        }
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        self.notice = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        if ctrl && key.code == KeyCode::Char('b') {
            self.folded = !self.folded;
            return Action::None;
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        if self.focus == Focus::Search {
            return self.on_search_key(key);
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                Action::None
            }
            KeyCode::Char('/') => {
                self.focus = Focus::Search;
                Action::Search
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Panes => Focus::Sessions,
                    Focus::Sessions | Focus::Search => Focus::Panes,
                };
                Action::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.focus = Focus::Sessions;
                Action::None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.focus = Focus::Panes;
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.step(1);
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.step(-1);
                Action::None
            }
            KeyCode::Char('r') => self.resume(),
            KeyCode::Char('<' | '[') => {
                self.widen(-4, u16::MAX);
                Action::None
            }
            KeyCode::Char('>' | ']') => {
                self.widen(4, u16::MAX);
                Action::None
            }
            KeyCode::Enter => match self.focus {
                Focus::Panes => self.go_to(),
                Focus::Sessions | Focus::Search => self.resume(),
            },
            _ => Action::None,
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Panes;
                self.query.clear();
                self.results.clear();
                Action::None
            }
            KeyCode::Enter => self.open_hit(),
            KeyCode::Down => {
                self.step(1);
                Action::None
            }
            KeyCode::Up => {
                self.step(-1);
                Action::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                Action::Search
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                Action::Search
            }
            _ => Action::None,
        }
    }

    /// Handles a mouse report. `hit` is what the last frame drew under the
    /// pointer, `list` the list under it, and `width` the screen's width,
    /// which bounds the sidebar.
    pub(crate) fn on_mouse(
        &mut self,
        mouse: MouseEvent,
        hit: Option<Target>,
        list: Option<Focus>,
        size: (u16, u16),
    ) -> Action {
        let width = size.0;
        if self.help {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.help = false;
            }
            return Action::None;
        }
        match mouse.kind {
            MouseEventKind::Moved => {
                // A terminal stops reporting once the pointer leaves the pane,
                // and the window's padding has no cells, so a fast move can
                // leave with its last report a cell or two inside. A band of
                // two columns and one row at each side counts as outside.
                let (width, height) = size;
                let edge = mouse.column < 2
                    || mouse.row < 1
                    || mouse.column + 2 >= width
                    || mouse.row + 1 >= height;
                self.hover = if edge { None } else { hit };
                Action::None
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by = if mouse.kind == MouseEventKind::ScrollDown {
                    1
                } else {
                    -1
                };
                if let Some(list) = list {
                    self.scroll(list, by);
                }
                Action::None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.notice = None;
                match hit {
                    Some(Target::Edge) => {
                        self.dragging = true;
                        Action::None
                    }
                    Some(target) => self.click(target),
                    None => Action::None,
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                if self.folded {
                    self.folded = false;
                }
                self.sidebar = mouse.column.saturating_add(1);
                self.widen(0, width);
                Action::None
            }
            MouseEventKind::Up(_) => {
                self.dragging = false;
                self.hover = hit;
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Grows the sidebar by `by` columns, keeping both parts usable on a
    /// screen `width` wide.
    fn widen(&mut self, by: i16, width: u16) {
        let most = width.saturating_sub(MIN_MAIN).max(MIN_SIDEBAR);
        self.sidebar = self
            .sidebar
            .saturating_add_signed(by)
            .clamp(MIN_SIDEBAR, most);
    }

    fn click(&mut self, target: Target) -> Action {
        match target {
            Target::Pane(index) => {
                if self.focus == Focus::Panes && self.pane == index {
                    return self.go_to();
                }
                self.focus = Focus::Panes;
                self.pane = index;
                self.session = 0;
                *self.scroll_mut(Focus::Sessions) = Scroll::default();
                Action::None
            }
            Target::Session(index) => {
                if self.focus == Focus::Sessions && self.session == index {
                    return self.resume();
                }
                self.focus = Focus::Sessions;
                self.session = index;
                Action::None
            }
            Target::Result(index) => {
                if self.result == index {
                    return self.open_hit();
                }
                self.result = index;
                Action::None
            }
            Target::Search => {
                self.focus = Focus::Search;
                Action::Search
            }
            Target::Button(button) => self.press(button),
            Target::Edge => Action::None,
            Target::Scratch => {
                self.show_scratch = !self.show_scratch;
                Action::None
            }
        }
    }

    fn press(&mut self, button: Button) -> Action {
        match button {
            Button::GoTo => self.go_to(),
            Button::Resume => self.resume(),
            Button::Search => {
                self.focus = Focus::Search;
                Action::Search
            }
            Button::Help => {
                self.help = true;
                Action::None
            }
            Button::Quit => Action::Quit,
            Button::Sidebar => {
                self.folded = !self.folded;
                Action::None
            }
        }
    }

    /// Moves `list`, the list under the pointer, by `by` notches, leaving its
    /// selection where it is. The next frame clamps it to the list.
    fn scroll(&mut self, list: Focus, by: i16) {
        let scroll = self.scroll_mut(list);
        scroll.top = scroll.top.saturating_add_signed(by.saturating_mul(WHEEL));
        scroll.follow = false;
    }

    pub(crate) fn on_paste(&mut self, text: &str) -> Action {
        self.focus = Focus::Search;
        self.query.push_str(&text.replace(['\n', '\r', '\t'], " "));
        Action::Search
    }

    /// The window lost focus: nothing is hovered or dragged.
    pub(crate) fn on_blur(&mut self) {
        self.hover = None;
        self.dragging = false;
    }

    pub(crate) fn set_results(&mut self, results: Vec<Hit>) {
        self.results = results;
        self.result = 0;
        *self.scroll_mut(Focus::Search) = Scroll::default();
    }

    /// Adds the transcript matches a deep search found for the current query.
    pub(crate) fn add_results(&mut self, more: Vec<Hit>, (read, total): (usize, usize)) {
        let known: std::collections::HashSet<String> = self
            .results
            .iter()
            .map(|hit| hit.agent.session_id.clone())
            .collect();
        self.results.extend(
            more.into_iter()
                .filter(|hit| !known.contains(&hit.agent.session_id)),
        );
        self.deep = read < total;
        self.read = (read, total);
    }

    /// The pane whose Claude runs `session_id` now, when the sidebar has it.
    pub(crate) fn running(&self, session_id: &str) -> Option<usize> {
        self.panes.iter().position(|pane| {
            pane.status != Status::Stopped
                && pane
                    .sessions
                    .first()
                    .is_some_and(|(agent, held)| *held && agent.session_id == session_id)
        })
    }

    fn resume(&self) -> Action {
        self.chosen().map_or(Action::None, Action::Resume)
    }

    /// A search hit whose Claude runs goes to its pane; any other resumes.
    fn open_hit(&self) -> Action {
        let Some(hit) = self.results.get(self.result) else {
            return Action::None;
        };
        match self
            .running(&hit.agent.session_id)
            .and_then(|index| self.panes.get(index))
        {
            Some(pane) => Action::GoTo {
                session_id: pane.session_id.clone(),
                block_id: pane.block_id.clone(),
            },
            None => self.resume(),
        }
    }

    fn go_to(&self) -> Action {
        self.selected().map_or(Action::None, |pane| Action::GoTo {
            session_id: pane.session_id.clone(),
            block_id: pane.block_id.clone(),
        })
    }

    fn step(&mut self, by: isize) {
        let (at, len) = match self.focus {
            Focus::Panes => (&mut self.pane, self.panes.len()),
            Focus::Sessions => {
                let len = self.session_count();
                (&mut self.session, len)
            }
            Focus::Search => (&mut self.result, self.results.len()),
        };
        if len == 0 {
            return;
        }
        *at = at.saturating_add_signed(by).min(len - 1);
        self.scroll_mut(self.focus).follow = true;
        if self.focus == Focus::Panes {
            self.session = 0;
            *self.scroll_mut(Focus::Sessions) = Scroll::default();
        }
    }
}

/// How long ago a pane's newest session was active; a pane with no age
/// sorts last.
fn newest(pane: &Pane) -> std::time::Duration {
    pane.sessions
        .first()
        .and_then(|(agent, _)| agent.idle)
        .unwrap_or(std::time::Duration::MAX)
}

impl App {
    /// The session under the cursor: the search hit, the highlighted session,
    /// or the selected pane's newest.
    pub(crate) fn current(&self) -> Option<&Agent> {
        match self.focus {
            Focus::Search => self.results.get(self.result).map(|hit| &hit.agent),
            Focus::Sessions => self
                .selected()
                .and_then(|pane| pane.sessions.get(self.session))
                .map(|(agent, _)| agent),
            Focus::Panes => self
                .selected()
                .and_then(|pane| pane.sessions.first())
                .map(|(agent, _)| agent),
        }
    }
}

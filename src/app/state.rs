//! What the app holds and how keys change it. Nothing here draws or reads
//! the terminal, so every key is a plain function of the state.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

use crate::app::feed::Pane;
use crate::session::Agent;
use crate::status::Status;

/// How the screen is laid out. The preview build cycles them with `ctrl+l`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Panes on the left, the selected pane on the right.
    Columns,
    /// Panes on top, the selected pane's sessions below.
    Stacked,
    /// One full-width view at a time.
    Tabs,
}

impl Layout {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Columns => "two columns",
            Self::Stacked => "stacked",
            Self::Tabs => "tabs",
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Columns => Self::Stacked,
            Self::Stacked => Self::Tabs,
            Self::Tabs => Self::Columns,
        }
    }
}

/// Which list the cursor is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Panes,
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

/// What a key asks the loop to do.
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

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) pane: usize,
    pub(crate) session: usize,
    pub(crate) focus: Focus,
    pub(crate) layout: Layout,
    pub(crate) link: Link,
    pub(crate) query: String,
    pub(crate) results: Vec<Agent>,
    pub(crate) result: usize,
    pub(crate) help: bool,
    /// One line for the status bar, such as the Rex app's refusal.
    pub(crate) notice: Option<String>,
    /// The block this app runs in, so its own pane is marked.
    pub(crate) here: Option<String>,
}

/// Where panes go in the list: the ones that need the user first.
pub(crate) const fn rank(status: Status) -> u8 {
    match status {
        Status::NeedsInput => 0,
        Status::Failed => 1,
        Status::Working => 2,
        Status::Idle => 3,
        Status::Stopped => 4,
    }
}

impl App {
    pub(crate) fn new(layout: Layout, here: Option<String>) -> Self {
        Self {
            panes: Vec::new(),
            pane: 0,
            session: 0,
            focus: Focus::Panes,
            layout,
            link: Link::Connecting,
            query: String::new(),
            results: Vec::new(),
            result: 0,
            help: false,
            notice: None,
            here,
        }
    }

    pub(crate) fn selected(&self) -> Option<&Pane> {
        self.panes.get(self.pane)
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
            Focus::Search => self.results.get(self.result).cloned(),
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
        if ctrl && key.code == KeyCode::Char('l') {
            self.layout = self.layout.next();
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
            KeyCode::Enter => self.resume(),
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

    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent) -> Action {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.step(1),
            MouseEventKind::ScrollUp => self.step(-1),
            _ => {}
        }
        Action::None
    }

    pub(crate) fn set_results(&mut self, results: Vec<Agent>) {
        self.results = results;
        self.result = 0;
    }

    fn resume(&self) -> Action {
        self.chosen().map_or(Action::None, Action::Resume)
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
        if self.focus == Focus::Panes {
            self.session = 0;
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

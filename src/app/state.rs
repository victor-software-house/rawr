//! What the app holds and how keys and the mouse change it. Nothing here
//! draws or reads the terminal, so every event is a plain function of the
//! state.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::app::feed::Pane;
use crate::catalog::Hit;
use crate::session::Agent;
use crate::status::Status;

/// Where the keys go: one of the three columns, or the search over them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Focus {
    /// Every pane, by what it needs.
    Panes,
    /// The selected pane's sessions.
    Sessions,
    /// One session: its tasks, prompts, tool calls and files.
    Session,
    /// The search over every local session, drawn over the columns.
    Search,
}

impl Focus {
    /// The columns on screen, left to right: the panes, and the selected
    /// pane with its sessions and the one shown.
    pub(crate) const COLUMNS: [Self; 2] = [Self::Panes, Self::Session];

    /// The column on screen that holds this focus, from 0: the pane's
    /// sessions and the session shown share one.
    pub(crate) const fn column(self) -> usize {
        match self {
            Self::Panes | Self::Search => 0,
            Self::Sessions | Self::Session => 1,
        }
    }

    /// Where this focus keeps its filter and its place in the key order.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Panes | Self::Search => 0,
            Self::Sessions => 1,
            Self::Session => 2,
        }
    }
}

/// How the columns share the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Arrangement {
    /// The focused column takes the width; the other two fold to rails.
    Rails,
    /// One column at a time across the whole width, under a path line.
    Single,
}

/// The session column's views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Tab {
    Overview,
    Prompts,
    Activity,
    Files,
}

impl Tab {
    pub(crate) const ALL: [Self; 4] = [Self::Overview, Self::Prompts, Self::Activity, Self::Files];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Prompts => "Prompts",
            Self::Activity => "Activity",
            Self::Files => "Files",
        }
    }

    fn shift(self, by: isize) -> Self {
        let at = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        let len = Self::ALL.len();
        let next = (at + len).saturating_add_signed(by) % len;
        Self::ALL[next]
    }
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
    /// Resume this session: in a tab when the window floats, else here.
    Resume(Agent),
    /// Show this pane in the Rex app.
    GoTo {
        session_id: String,
        block_id: String,
    },
    /// The search text changed; the loop asks the daemon for matches.
    Search,
}

/// A button; each does what its key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    GoTo,
    Resume,
}

/// Something on screen that takes a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Pane(usize),
    Session(usize),
    Result(usize),
    Search,
    Button(Button),
    /// A column, folded or not; a click focuses it.
    Column(Focus),
    Tab(Tab),
    /// The line that stands for a pane's untitled scratch sessions.
    Scratch,
    /// The edge of a column, which a drag moves.
    Edge(Grip),
}

/// A column's movable edge: the column it sizes, and where the column
/// starts, from which a drag measures the new width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Grip {
    pub(crate) column: Focus,
    /// The column's fixed side: its left edge when it grows rightwards, its
    /// right edge otherwise.
    pub(crate) anchor: u16,
    pub(crate) rightward: bool,
}

/// A folded column drawn out over the others while the pointer is on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Drawer {
    pub(crate) column: Focus,
    /// When it started opening or closing.
    at: Instant,
    open: bool,
}

/// A move of focus on screen: the column it left, the widths the columns
/// had then, and when.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Move {
    pub(crate) was: Focus,
    from: [u16; 2],
    at: Instant,
}

/// What lies under the pointer, as the last frame drew it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Under {
    pub(crate) target: Option<Target>,
    /// The list the wheel scrolls there.
    pub(crate) list: Option<Focus>,
    /// The folded column there.
    pub(crate) rail: Option<Focus>,
    /// Whether the pointer is where it keeps the drawer out.
    pub(crate) keep: bool,
    /// Whether the pointer is over the rail the drawer comes out of.
    pub(crate) on_rail: bool,
    /// The screen's width and height.
    pub(crate) size: (u16, u16),
}

/// A folded column's width.
pub(crate) const RAIL: u16 = 5;

/// The narrowest a column draws in full; narrower, it draws as a rail.
pub(crate) const FULL: u16 = 28;

/// How soon after a drawer starts to shut the pointer may come back into
/// it and have it open again.
const GRACE: Duration = Duration::from_millis(200);

/// How long the pointer may stay past the window's edge before the drawer
/// shuts, so a pointer that slips out and back keeps it.
const AWAY: Duration = Duration::from_millis(600);

/// How fast a column's edge moves on screen, in columns a second, so a
/// long move and a short one go at the same speed.
const SPEED: f32 = 320.0;

/// How long a move of `columns` takes: at [`SPEED`], kept between a
/// glance and a quarter second.
fn span(columns: u16) -> Duration {
    Duration::from_secs_f32(f32::from(columns) / SPEED)
        .clamp(Duration::from_millis(70), Duration::from_millis(240))
}

/// `elapsed` of `span`, from 0 to 1, eased out.
fn eased(elapsed: Duration, span: Duration) -> (f32, f32) {
    let t = (elapsed.as_secs_f32() / span.as_secs_f32()).min(1.0);
    (t, 1.0 - (1.0 - t).powi(3))
}

/// The panes take the width they want when they lead and the pane the
/// rest, when that is enough to draw in full; otherwise the panes fold to
/// a rail.
pub(crate) fn widths(lead: Focus, total: u16, wants: [u16; 3]) -> [u16; 2] {
    if lead.column() == 0 {
        let want = wants[0].min(total.saturating_sub(RAIL));
        let rest = total.saturating_sub(want);
        if rest >= FULL {
            return [want, rest];
        }
        return [total.saturating_sub(RAIL), RAIL];
    }
    [RAIL, total.saturating_sub(RAIL)]
}

/// The narrowest a column may be dragged.
pub(crate) const NARROWEST: u16 = 28;

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
    /// The column the search returns to.
    back: Focus,
    /// The column the columns are laid out around. Keys move it with focus;
    /// a click in a column already drawn in full moves focus alone, so the
    /// columns stay where they are.
    lead: Focus,
    pub(crate) arrangement: Arrangement,
    pub(crate) tab: Tab,
    /// The column focus last left, and when, for the move on screen.
    pub(crate) moved: Option<Move>,
    pub(crate) link: Link,
    pub(crate) query: String,
    pub(crate) results: Vec<Hit>,
    /// Whether the transcripts are still being read for the current query.
    pub(crate) deep: bool,
    /// How many transcripts the deep search has read, of how many.
    pub(crate) read: (usize, usize),
    pub(crate) result: usize,
    /// Each column's filter, typed after `/`.
    pub(crate) filters: [String; 3],
    /// Whether the keys type into the focused column's filter.
    pub(crate) filtering: bool,
    pub(crate) help: bool,
    /// One line for the search box's border, such as the Rex app's refusal.
    pub(crate) notice: Option<String>,
    /// The block this app runs in, so its own pane is marked.
    pub(crate) here: Option<String>,
    /// What the pointer is over.
    pub(crate) hover: Option<Target>,
    /// The spinner's frame, moved on by the loop's tick while a pane works.
    pub(crate) frame: usize,
    /// Whether a pane's untitled scratch sessions are listed.
    pub(crate) show_scratch: bool,
    /// The pane to select once the feed lists it, by block ID.
    pub(crate) want: Option<String>,
    /// The panes, sessions, session and results, in that order.
    scrolls: [Scroll; 4],
    /// The width each column takes when it has focus or is drawn out; the
    /// session column takes the rest when it has focus.
    pub(crate) wants: [u16; 3],
    pub(crate) drawer: Option<Drawer>,
    /// When the pointer left the drawer.
    left: Option<Instant>,
    /// When the pointer went past the window's edge, while it stays there.
    outside: Option<Instant>,
    /// The screen's width as last drawn, which sets how far a move goes.
    pub(crate) cols: u16,
    /// The edge being dragged.
    pub(crate) dragging: Option<Grip>,
}

/// A list's place in [`App::scrolls`].
const fn slot(list: Focus) -> usize {
    match list {
        Focus::Panes => 0,
        Focus::Sessions => 1,
        Focus::Session => 2,
        Focus::Search => 3,
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

/// Whether `text` holds `filter`, ignoring case; an empty filter holds all.
pub(crate) fn passes(filter: &str, text: &str) -> bool {
    filter.is_empty() || text.to_lowercase().contains(&filter.to_lowercase())
}

impl App {
    pub(crate) fn new(here: Option<String>) -> Self {
        Self {
            panes: Vec::new(),
            pane: 0,
            session: 0,
            // The panes fold to a rail that draws out under the pointer, so
            // the sessions have the width.
            focus: Focus::Session,
            back: Focus::Session,
            lead: Focus::Session,
            arrangement: Arrangement::Rails,
            tab: Tab::Overview,
            moved: None,
            link: Link::Connecting,
            query: String::new(),
            results: Vec::new(),
            deep: false,
            read: (0, 0),
            result: 0,
            filters: Default::default(),
            filtering: false,
            help: false,
            notice: None,
            here,
            hover: None,
            frame: 0,
            show_scratch: false,
            want: None,
            scrolls: [Scroll::default(); 4],
            wants: [48, 60, 72],
            drawer: None,
            left: None,
            outside: None,
            cols: 0,
            dragging: None,
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

    /// The filter of `column`.
    pub(crate) fn filter(&self, column: Focus) -> &str {
        &self.filters[column.index()]
    }

    /// The width the columns share: the screen less its margins and the
    /// edge between the columns.
    pub(crate) const fn total(&self) -> u16 {
        self.cols.saturating_sub(3)
    }

    /// How long the last move of focus takes: the farthest any edge goes.
    fn move_span(&self) -> Duration {
        let Some(moved) = self.moved else {
            return Duration::ZERO;
        };
        // One view at a time slides a few columns in.
        if self.arrangement == Arrangement::Single {
            return span(10);
        }
        let to = widths(self.lead, self.total(), self.wants);
        let far = moved
            .from
            .iter()
            .zip(to)
            .map(|(a, b)| a.abs_diff(b))
            .max()
            .unwrap_or(0);
        span(far)
    }

    /// How far the last move of focus has come, from 0 to 1, eased out.
    pub(crate) fn progress(&self) -> f32 {
        self.moved
            .map_or(1.0, |moved| eased(moved.at.elapsed(), self.move_span()).1)
    }

    /// Each column's width on screen now, as the last move has brought it.
    pub(crate) fn layout(&self) -> [u16; 2] {
        let total = self.total();
        let to = widths(self.lead, total, self.wants);
        let Some(moved) = self.moved else {
            return to;
        };
        let progress = self.progress();
        let mix = |a: u16, b: u16| -> u16 {
            let at = f32::from(a) + (f32::from(b) - f32::from(a)) * progress;
            // The result lies between two u16 values, so it fits.
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let at = at.round().max(0.0) as u16;
            at
        };
        let first = mix(moved.from[0], to[0]).min(total);
        [first, total - first]
    }

    /// Marks a move from the columns as they stand, so a move that cuts
    /// into another starts where that one had got to.
    fn start_move(&mut self, was: Focus) {
        let mut from = self.layout();
        // A drawer that is out becomes its column at the width it is drawn,
        // taking the room from the widest of the others.
        if let Some((column, out)) = self.drawn() {
            let full = self.drawer_full(column);
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let wide = (f32::from(RAIL) + (f32::from(full) - f32::from(RAIL)) * out)
                .round()
                .max(0.0) as u16;
            let total = self.total();
            from = if column.column() == 0 {
                [wide.min(total), total.saturating_sub(wide)]
            } else {
                [total.saturating_sub(wide), wide.min(total)]
            };
        }
        self.moved = Some(Move {
            was,
            from,
            at: Instant::now(),
        });
    }

    /// How wide `column` is when drawn out all the way.
    pub(crate) fn drawer_full(&self, column: Focus) -> u16 {
        self.wants[column.index()].min(self.cols.saturating_sub(2 + RAIL))
    }

    /// How long the drawer of `column` takes to open or shut.
    fn drawer_span(&self, column: Focus) -> Duration {
        span(self.drawer_full(column).saturating_sub(RAIL))
    }

    /// Whether the screen changes with the clock: a pane works, so the
    /// spinner turns, or focus is still moving.
    pub(crate) fn animating(&self) -> bool {
        self.panes.iter().any(|pane| pane.status == Status::Working) || self.moving()
    }

    /// Moves focus to `column`, marking the move for the screen. The
    /// columns reflow, so a drawer has nothing left to cover.
    fn focus_on(&mut self, column: Focus) {
        if column != self.lead {
            self.start_move(self.lead);
            self.lead = column;
            self.drawer = None;
        }
        if column != self.focus {
            self.focus = column;
            self.filtering = false;
        }
    }

    /// Focuses `column` after a click in it. One drawn in full keeps the
    /// columns where they are; a folded one is laid out around.
    fn focus_here(&mut self, column: Focus) {
        let full = self.arrangement == Arrangement::Rails
            && self
                .layout()
                .get(column.column())
                .is_some_and(|wide| *wide >= FULL);
        if full {
            if column != self.focus {
                self.focus = column;
                self.filtering = false;
            }
        } else {
            self.focus_on(column);
        }
    }

    /// The column the columns are laid out around.
    pub(crate) const fn lead(&self) -> Focus {
        self.lead
    }

    /// How far the drawer is out, from 0 to 1, eased; none once it has shut.
    pub(crate) fn drawn(&self) -> Option<(Focus, f32)> {
        let drawer = self.drawer?;
        let (t, eased) = eased(drawer.at.elapsed(), self.drawer_span(drawer.column));
        if drawer.open {
            Some((drawer.column, eased))
        } else if t < 1.0 {
            Some((drawer.column, 1.0 - eased))
        } else {
            None
        }
    }

    /// Draws `column` out, or shuts the drawer when `column` is none,
    /// starting from where the drawer is now so a reversal never jumps.
    fn draw_out(&mut self, column: Option<Focus>) {
        let now = self.drawn();
        let (target, open) = match (column, now) {
            (Some(column), _) => (column, true),
            (None, Some((column, _))) => (column, false),
            (None, None) => {
                self.drawer = None;
                return;
            }
        };
        if self
            .drawer
            .is_some_and(|drawer| drawer.column == target && drawer.open == open)
        {
            return;
        }
        // Start the curve where the drawer stands: an out-ness `o` sits at
        // time `1 - cbrt(1 - o)` opening, and `1 - cbrt(o)` closing.
        let out = match now {
            Some((column, out)) if column == target => out,
            _ => 0.0,
        };
        let t = if open {
            1.0 - (1.0 - out).cbrt()
        } else {
            1.0 - out.cbrt()
        };
        let at = Instant::now()
            .checked_sub(self.drawer_span(target).mul_f32(t))
            .unwrap_or_else(Instant::now);
        self.drawer = Some(Drawer {
            column: target,
            at,
            open,
        });
    }

    /// Takes a new list of panes, sorted by attention, and keeps the
    /// selection on the same pane when it is still there.
    pub(crate) fn set_panes(&mut self, mut panes: Vec<Pane>) {
        panes.sort_by(|a, b| {
            rank(a.status)
                .cmp(&rank(b.status))
                .then_with(|| newest(a).cmp(&newest(b)))
        });
        // The pane asked for, until the feed lists it; then the one selected.
        let kept = self
            .want
            .clone()
            .or_else(|| self.selected().map(|pane| pane.block_id.clone()));
        let before = self.pane;
        self.panes = panes;
        self.link = Link::Live;
        let found = kept
            .as_ref()
            .and_then(|id| self.panes.iter().position(|pane| &pane.block_id == id));
        if found.is_some() {
            self.want = None;
        }
        self.pane = found.unwrap_or(0).min(self.panes.len().saturating_sub(1));
        // A pane that moved in the list stays in view.
        if self.pane != before {
            self.scroll_mut(Focus::Panes).follow = true;
        }
        self.session = self.session.min(self.session_count().saturating_sub(1));
    }

    fn session_count(&self) -> usize {
        self.selected().map_or(0, |pane| pane.sessions.len())
    }

    /// The panes the panes filter lets through, by index.
    pub(crate) fn visible_panes(&self) -> Vec<usize> {
        let filter = self.filter(Focus::Panes);
        self.panes
            .iter()
            .enumerate()
            .filter(|(_, pane)| {
                passes(
                    filter,
                    &format!(
                        "{} {} {} {}",
                        pane.title(),
                        pane.name,
                        pane.tab,
                        pane.msg.as_deref().unwrap_or_default()
                    ),
                )
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// The selected pane's sessions the sessions filter lets through, titled
    /// ones first, then the scratch ones when they are shown.
    pub(crate) fn visible_sessions(&self) -> Vec<usize> {
        let filter = self.filter(Focus::Sessions);
        let Some(pane) = self.selected() else {
            return Vec::new();
        };
        let (scratch, titled): (Vec<(usize, bool)>, Vec<(usize, bool)>) = pane
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, (agent, _))| passes(filter, &format!("{} {}", agent.title, agent.cwd)))
            .map(|(index, (agent, _))| (index, is_scratch(agent)))
            .partition(|(_, scratch)| *scratch);
        let shown = if self.show_scratch {
            scratch
        } else {
            Vec::new()
        };
        titled
            .into_iter()
            .chain(shown)
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether the pane view lists the pane's sessions: when it has more
    /// than one, or a filter is set on them.
    pub(crate) fn lists_sessions(&self) -> bool {
        !self.filter(Focus::Sessions).is_empty()
            || self.selected().is_some_and(|pane| pane.sessions.len() > 1)
    }

    /// The first session the selected pane lists.
    fn first_session(&self) -> usize {
        self.visible_sessions().first().copied().unwrap_or(0)
    }

    /// The column on screen: the focused one, or the one the search covers.
    pub(crate) const fn at(&self) -> Focus {
        match self.focus {
            Focus::Search => self.back,
            Focus::Panes | Focus::Sessions | Focus::Session => self.focus,
        }
    }

    /// Whether focus is still moving on screen, so frames come fast.
    pub(crate) fn moving(&self) -> bool {
        self.moved
            .is_some_and(|moved| moved.at.elapsed() < self.move_span())
            || self
                .drawer
                .is_some_and(|drawer| drawer.at.elapsed() < self.drawer_span(drawer.column))
    }

    /// The session `r` resumes: the one shown, or the selected pane's newest
    /// that no live Claude holds.
    fn chosen(&self) -> Option<Agent> {
        match self.focus {
            Focus::Search => self
                .results
                .get(self.result)
                .filter(|hit| self.running(&hit.agent.session_id).is_none())
                .map(|hit| hit.agent.clone()),
            Focus::Sessions | Focus::Session => self
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
        match (ctrl, key.code) {
            (true, KeyCode::Char('c')) => return Action::Quit,
            (true, KeyCode::Char('l')) => {
                self.arrangement = match self.arrangement {
                    Arrangement::Rails => Arrangement::Single,
                    Arrangement::Single => Arrangement::Rails,
                };
                self.start_move(self.focus);
                return Action::None;
            }
            (true, KeyCode::Char('f')) => {
                if self.focus != Focus::Search {
                    self.back = self.focus;
                    self.focus = Focus::Search;
                }
                return Action::Search;
            }
            _ => {}
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        if self.focus == Focus::Search {
            return self.on_search_key(key);
        }
        if self.filtering {
            return self.on_filter_key(key);
        }
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc => {
                let index = self.focus.index();
                if !self.filters[index].is_empty() {
                    self.filters[index].clear();
                } else if self.focus == Focus::Panes {
                    return Action::Quit;
                } else {
                    self.focus_on(Focus::Panes);
                }
                Action::None
            }
            KeyCode::Char('?') => {
                self.help = true;
                Action::None
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                Action::None
            }
            KeyCode::Char('1') => {
                self.focus_on(Focus::Panes);
                Action::None
            }
            KeyCode::Char('2' | '3') => {
                self.focus_on(Focus::Session);
                Action::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // Through the panes, the pane's sessions when it lists them,
                // and the session.
                let order: Vec<Focus> = [Focus::Panes, Focus::Sessions, Focus::Session]
                    .into_iter()
                    .filter(|focus| *focus != Focus::Sessions || self.lists_sessions())
                    .collect();
                let at = order
                    .iter()
                    .position(|focus| *focus == self.focus)
                    .unwrap_or(0);
                let by = if key.code == KeyCode::Tab {
                    1
                } else {
                    order.len() - 1
                };
                self.focus_on(order[(at + by) % order.len()]);
                Action::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.focus_on(Focus::Session);
                Action::None
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                self.focus_on(Focus::Panes);
                Action::None
            }
            KeyCode::Char(']') if self.focus == Focus::Session => {
                self.tab = self.tab.shift(1);
                *self.scroll_mut(Focus::Session) = Scroll::default();
                Action::None
            }
            KeyCode::Char('[') if self.focus == Focus::Session => {
                self.tab = self.tab.shift(-1);
                *self.scroll_mut(Focus::Session) = Scroll::default();
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
            KeyCode::Char('g') => self.go_to(),
            KeyCode::Enter => match self.focus {
                Focus::Panes => self.go_to(),
                Focus::Sessions => {
                    self.focus_on(Focus::Session);
                    Action::None
                }
                Focus::Session | Focus::Search => self.resume(),
            },
            _ => Action::None,
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) -> Action {
        let column = self.focus.index();
        match key.code {
            KeyCode::Esc => {
                self.filters[column].clear();
                self.filtering = false;
            }
            KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                self.filtering = false;
                if key.code != KeyCode::Enter {
                    self.step(if key.code == KeyCode::Down { 1 } else { -1 });
                }
            }
            KeyCode::Backspace => {
                self.filters[column].pop();
            }
            KeyCode::Char(c) => self.filters[column].push(c),
            _ => {}
        }
        self.keep_visible();
        Action::None
    }

    /// Moves the selection onto the first row the filters let through, when
    /// the selected one is filtered out.
    fn keep_visible(&mut self) {
        let panes = self.visible_panes();
        if !panes.contains(&self.pane)
            && let Some(first) = panes.first()
        {
            self.pane = *first;
            self.session = self.first_session();
        }
        let sessions = self.visible_sessions();
        if !sessions.contains(&self.session)
            && let Some(first) = sessions.first()
        {
            self.session = *first;
        }
        for list in [Focus::Panes, Focus::Sessions, Focus::Session] {
            *self.scroll_mut(list) = Scroll {
                top: 0,
                follow: true,
            };
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.focus = self.back;
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

    /// Handles a mouse report; `under` is what the last frame drew under
    /// the pointer.
    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent, under: Under) -> Action {
        if self.help {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.help = false;
            }
            return Action::None;
        }
        match mouse.kind {
            MouseEventKind::Moved => {
                // Rex goes on reporting while the pointer is outside the pane,
                // in its padding or beyond, with the position clamped to the
                // outermost cells. A report there may be the pointer outside,
                // so the outermost ring never hovers.
                let (width, height) = under.size;
                let edge = mouse.column == 0
                    || mouse.row == 0
                    || mouse.column + 1 >= width
                    || mouse.row + 1 >= height;
                self.hover = if edge { None } else { under.target };
                // A folded column draws out while the pointer is on it, and
                // goes back when the pointer leaves it.
                if self.focus != Focus::Search {
                    self.follow_pointer(edge, under);
                }
                Action::None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(grip) = self.dragging {
                    let wide = if grip.rightward {
                        mouse.column.saturating_sub(grip.anchor) + 1
                    } else {
                        grip.anchor.saturating_sub(mouse.column)
                    };
                    self.wants[grip.column.column()] = wide.max(NARROWEST);
                    // A drag follows the pointer, with nothing easing.
                    self.moved = None;
                }
                Action::None
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by = if mouse.kind == MouseEventKind::ScrollDown {
                    1
                } else {
                    -1
                };
                if let Some(list) = under.list {
                    self.scroll(list, by);
                }
                Action::None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.notice = None;
                under
                    .target
                    .map_or(Action::None, |target| self.click(target))
            }
            MouseEventKind::Up(_) => {
                self.dragging = None;
                self.hover = under.target;
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Opens or shuts the drawer as the pointer moves. It opens on a rail,
    /// and shuts once the pointer is past its edge by a little slack; a
    /// pointer back in it soon after, or on its rail, opens it again.
    fn follow_pointer(&mut self, edge: bool, under: Under) {
        // A report on the window's edge may be the pointer outside it: the
        // drawer waits a moment, in case it comes back (see `tick`).
        if edge {
            if self.outside.is_none() {
                self.outside = Some(Instant::now());
            }
            return;
        }
        self.outside = None;
        let Some(drawer) = self.drawer else {
            if let Some(column) = under.rail {
                self.draw_out(Some(column));
            }
            return;
        };
        if let Some(column) = under.rail {
            self.draw_out(Some(column));
        } else if drawer.open {
            if !under.keep {
                self.left = Some(Instant::now());
                self.draw_out(None);
            }
        } else if under.on_rail
            || (under.keep && self.left.is_some_and(|left| left.elapsed() < GRACE))
        {
            self.draw_out(Some(drawer.column));
        }
    }

    /// When the loop must wake with no input: when a pointer past the
    /// window's edge has been away long enough to shut the drawer.
    pub(crate) fn wake_at(&self) -> Option<Instant> {
        self.outside
            .filter(|_| self.drawer.is_some_and(|drawer| drawer.open))
            .map(|since| since + AWAY)
    }

    /// Shuts the drawer once the pointer has stayed past the window's edge.
    pub(crate) fn tick(&mut self) {
        if self.outside.is_some_and(|since| since.elapsed() >= AWAY) {
            self.outside = None;
            if self.drawer.is_some_and(|drawer| drawer.open) {
                self.left = Some(Instant::now());
                self.draw_out(None);
            }
        }
    }

    /// Whether `column` is drawn out over the others.
    pub(crate) fn out(&self, column: Focus) -> bool {
        self.drawer
            .is_some_and(|drawer| drawer.open && drawer.column == column)
    }

    fn click(&mut self, target: Target) -> Action {
        match target {
            Target::Pane(index) => {
                let here = self.focus == Focus::Panes || self.out(Focus::Panes);
                if here && self.pane == index {
                    return self.go_to();
                }
                // A pick in the drawer leaves focus where it was.
                if !self.out(Focus::Panes) {
                    self.focus_here(Focus::Panes);
                }
                self.pane = index;
                self.session = self.first_session();
                *self.scroll_mut(Focus::Sessions) = Scroll::default();
                Action::None
            }
            Target::Session(index) => {
                let here = self.focus == Focus::Sessions || self.out(Focus::Sessions);
                if here && self.session == index {
                    self.focus_here(Focus::Session);
                    return Action::None;
                }
                if !self.out(Focus::Sessions) {
                    self.focus_here(Focus::Sessions);
                }
                self.session = index;
                *self.scroll_mut(Focus::Session) = Scroll::default();
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
                if self.focus != Focus::Search {
                    self.back = self.focus;
                    self.focus = Focus::Search;
                }
                Action::Search
            }
            Target::Column(column) => {
                self.focus_on(column);
                Action::None
            }
            Target::Tab(tab) => {
                self.focus_here(Focus::Session);
                self.tab = tab;
                *self.scroll_mut(Focus::Session) = Scroll::default();
                Action::None
            }
            Target::Button(Button::GoTo) => self.go_to(),
            Target::Button(Button::Resume) => self.resume(),
            Target::Scratch => {
                self.show_scratch = !self.show_scratch;
                self.keep_visible();
                Action::None
            }
            Target::Edge(grip) => {
                self.dragging = Some(grip);
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
        let text = text.replace(['\n', '\r', '\t'], " ");
        if self.filtering {
            self.filters[self.focus.index()].push_str(&text);
            self.keep_visible();
            return Action::None;
        }
        if self.focus != Focus::Search {
            self.back = self.focus;
            self.focus = Focus::Search;
        }
        self.query.push_str(&text);
        Action::Search
    }

    /// The window lost focus: nothing is hovered.
    pub(crate) fn on_blur(&mut self) {
        self.hover = None;
        self.draw_out(None);
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

    /// Moves the focused list's selection by `by` rows the filters let
    /// through; in the session column, scrolls it.
    fn step(&mut self, by: isize) {
        // A pane that lists no sessions leaves the keys to the session.
        let focus = match self.focus {
            Focus::Sessions if !self.lists_sessions() => Focus::Session,
            other => other,
        };
        match focus {
            Focus::Panes => {
                let rows = self.visible_panes();
                if let Some(next) = moved_in(&rows, self.pane, by) {
                    self.pane = next;
                    self.session = self.first_session();
                    *self.scroll_mut(Focus::Sessions) = Scroll::default();
                    *self.scroll_mut(Focus::Session) = Scroll::default();
                }
            }
            Focus::Sessions => {
                let rows = self.visible_sessions();
                if let Some(next) = moved_in(&rows, self.session, by) {
                    self.session = next;
                    *self.scroll_mut(Focus::Session) = Scroll::default();
                }
            }
            Focus::Session => {
                self.scroll(Focus::Session, i16::try_from(by).unwrap_or(0));
                return;
            }
            Focus::Search => {
                let len = self.results.len();
                if len > 0 {
                    self.result = self.result.saturating_add_signed(by).min(len - 1);
                }
            }
        }
        self.scroll_mut(focus).follow = true;
    }
}

/// The row `by` places from `at` in `rows`, kept inside the list.
fn moved_in(rows: &[usize], at: usize, by: isize) -> Option<usize> {
    let place = rows.iter().position(|row| *row == at).unwrap_or(0);
    let next = place
        .saturating_add_signed(by)
        .min(rows.len().checked_sub(1)?);
    rows.get(next).copied()
}

/// A session with no title that started in a temporary folder: a scripted
/// run, such as `claude -p`.
pub(crate) fn is_scratch(agent: &Agent) -> bool {
    let temp = std::env::temp_dir();
    agent.title == "Untitled session"
        && (["/tmp/", "/private/tmp/", "/var/folders/"]
            .iter()
            .any(|prefix| agent.cwd.starts_with(prefix))
            || std::path::Path::new(&agent.cwd).starts_with(&temp))
}

/// How long ago a pane's newest session was active; a pane with no age
/// sorts last.
fn newest(pane: &Pane) -> Duration {
    pane.sessions
        .first()
        .and_then(|(agent, _)| agent.idle)
        .unwrap_or(Duration::MAX)
}

impl App {
    /// The session the session column shows: the search hit, or the
    /// selected session of the selected pane.
    pub(crate) fn current(&self) -> Option<&Agent> {
        match self.focus {
            Focus::Search => self.results.get(self.result).map(|hit| &hit.agent),
            Focus::Panes | Focus::Sessions | Focus::Session => self
                .selected()
                .and_then(|pane| pane.sessions.get(self.session))
                .map(|(agent, _)| agent),
        }
    }
}

/// What a window hands to the copy that replaces it, so the new one opens
/// as the old one was.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Carry {
    /// The pane to select once the panes arrive, by block ID.
    pub(crate) select: Option<String>,
    focus: Focus,
    arrangement: Arrangement,
    tab: Tab,
    session: usize,
    query: String,
    filters: [String; 3],
    show_scratch: bool,
    panes_top: u16,
    wants: [u16; 3],
    /// The column drawn out, which the new window opens drawn out, so a
    /// go-to picked from the drawer keeps it in sight.
    #[serde(default)]
    drawn_out: Option<Focus>,
}

impl App {
    pub(crate) fn carry(&self) -> Carry {
        Carry {
            select: None,
            focus: self.focus,
            arrangement: self.arrangement,
            tab: self.tab,
            session: self.session,
            query: self.query.clone(),
            filters: self.filters.clone(),
            show_scratch: self.show_scratch,
            panes_top: self.scroll_of(Focus::Panes).top,
            wants: self.wants,
            drawn_out: self
                .drawer
                .filter(|drawer| drawer.open)
                .map(|drawer| drawer.column),
        }
    }

    /// Opens as `carry` says. Returns whether a search must run again for
    /// the carried query.
    pub(crate) fn restore(&mut self, carry: Carry) -> bool {
        self.want = carry.select;
        self.focus = carry.focus;
        self.lead = match carry.focus {
            Focus::Search => self.back,
            other => other,
        };
        self.arrangement = carry.arrangement;
        self.tab = carry.tab;
        self.session = carry.session;
        self.query = carry.query;
        self.filters = carry.filters;
        self.show_scratch = carry.show_scratch;
        self.scroll_mut(Focus::Panes).top = carry.panes_top;
        self.wants = carry.wants;
        // Out all the way, with no animation: the window it replaces showed
        // it so.
        self.drawer = carry.drawn_out.map(|column| Drawer {
            column,
            at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            open: true,
        });
        self.focus == Focus::Search && !self.query.trim().is_empty()
    }
}

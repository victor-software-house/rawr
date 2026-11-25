//! What the app holds and how keys and the mouse change it. Nothing here
//! draws or reads the terminal, so every event is a plain function of the
//! state.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, mem};

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::app::facts::{EntryId, List};
use crate::app::feed::Pane;
use crate::app::history::Lead;
use crate::app::layouts::Layouts;
use crate::app::scrollbar::{self, Fade, Press, Shade, Track};
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
    /// The lists a wheel scrolls, in the order of [`slot`].
    pub(crate) const LISTS: [Self; 4] = [Self::Panes, Self::Sessions, Self::Session, Self::Search];

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
            Self::Prompts => "Conversation",
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
    /// Open the session's file at this place in its files, in the file's
    /// default app.
    OpenFile(usize),
    /// Narrow the files tab to the folder of the session's file at this
    /// place in its files.
    FilterFolder(usize),
    /// Show this pane in the Rex app.
    GoTo {
        session_id: String,
        block_id: String,
    },
    /// The search text changed; the loop asks the daemon for matches.
    Search,
    /// A wheel report over a list, signed in lines: the loop queues it and
    /// scrolls the list by what each frame takes.
    Wheel(Focus, i32),
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
    /// The close mark of an opened pane, by the pane's place in the list.
    Close(usize),
    Search,
    Button(Button),
    /// A column, folded or not; a click focuses it.
    Column(Focus),
    Tab(Tab),
    /// The line that stands for a pane's untitled scratch sessions.
    Scratch,
    /// The edge of a column, which a drag moves.
    Edge(Grip),
    /// A list's scrollbar column: a press on its thumb holds it, and a press
    /// on its track pages the list.
    Bar(Focus),
    /// An entry of the prompts or the activity, which a click opens or
    /// folds, by its id in the transcript.
    Entry(EntryId),
    /// The line that stands for the files in temporary folders.
    ScratchFiles,
    /// A chip of the files tab that shows all files, the changed or the
    /// read ones.
    FileKind(Shown),
    /// A chip of the files tab that shows one file type, by its key.
    FileType(u64),
    /// A file's folder in the files tab, by the file's place; a click
    /// narrows the tab to that folder.
    Folder(usize),
    /// A file of the files tab, by its place in the session's changed
    /// files and then its read ones; a click opens it in its default app.
    File(usize),
}

/// Which of the session's files the files tab lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Shown {
    #[default]
    All,
    Changed,
    Read,
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

/// What a drag moves, from the press that began it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drag {
    /// A column's edge.
    Edge(Grip),
    /// A list's scrollbar thumb.
    Thumb(Held),
}

/// A scrollbar thumb held by the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Held {
    pub(crate) list: Focus,
    /// The row of the thumb the press took hold of, from its top.
    grab: u32,
    /// The thumb's first row in the track, where the pointer keeps it.
    pub(crate) top: u32,
}

/// How long the drawn thumb takes to glide to a row it moves to by itself.
const GLIDE: Duration = Duration::from_millis(100);

/// How often the loop wakes to draw the thumb on its way.
const GLIDE_STEP: Duration = Duration::from_millis(16);

/// The row the drawn thumb of a list is gliding to, and where it started.
#[derive(Debug, Clone, Copy)]
struct Glide {
    from: u32,
    to: u32,
    since: Instant,
}

impl Glide {
    /// The row the thumb is on at `now`.
    fn row(&self, now: Instant) -> u32 {
        let passed = now.saturating_duration_since(self.since);
        if passed >= GLIDE {
            return self.to;
        }
        let (from, to) = (i64::from(self.from), i64::from(self.to));
        let ms = i64::try_from(passed.as_millis()).unwrap_or(0);
        let step = (to - from) * ms / i64::try_from(GLIDE.as_millis()).unwrap_or(1);
        u32::try_from(from + step).unwrap_or(self.to)
    }
}

/// What a frame made of the place the scrollbar's thumb asked the view to
/// stand at, in the prompts or the activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seek {
    /// A held entry covers it, and the view stands there.
    Held,
    /// No held entry covers it: it stands for the byte `at` of the
    /// transcript, and the view stays where it was.
    Past(List, u64),
}

/// The newest place in the session that the thumb moved to and no held entry
/// covers, and the request for the entries at the place before it, if one is
/// out.
#[derive(Debug, Clone)]
struct Reaching {
    list: List,
    /// The byte of the transcript the newest place stands for, or `None` once
    /// the thumb is back on held entries, which leaves only the request out.
    at: Option<u64>,
    /// Where the pointer keeps the thumb, in rows of its track, at that place.
    top: u32,
    /// The transcript the place is in: the one the frame found it in.
    path: PathBuf,
    /// The request out for the entries around a place: its number, the epoch
    /// of the held entries when it was sent, and the byte it asked for.
    ask: Option<(u64, u64, u64)>,
}

/// A request for the entries of `list` around the byte `at` of the transcript
/// at `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Jump {
    pub(crate) list: List,
    pub(crate) at: u64,
    pub(crate) path: PathBuf,
    /// Names the request, so only the answer to the newest one lands.
    pub(crate) ask: u64,
}

/// A folded column drawn out over the others while the pointer is on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Drawer {
    pub(crate) column: Focus,
    /// When it started opening or closing.
    at: Instant,
    open: bool,
}

/// Whether the session header is folded to one row, and where its slide began.
#[derive(Debug, Clone, Copy)]
struct Fold {
    folded: bool,
    at: Instant,
    /// How far it was folded when the slide began, from 0 to 1.
    from: f32,
}

/// The folded session header drawn out over the list.
#[derive(Debug, Clone, Copy)]
struct Peek {
    /// When it started opening or closing.
    at: Instant,
    open: bool,
}

/// How many rows the session header takes before a frame has drawn it.
const HEADER_ROWS: u16 = 9;

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
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag says whether the pointer is in one place the last frame drew"
)]
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
    /// Whether the pointer is on the row of the folded header.
    pub(crate) fold: bool,
    /// Whether the pointer is where it keeps the folded header out.
    pub(crate) peek: bool,
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

/// How long the session header takes to fold, unfold or slide out: about
/// as long as the panes drawer takes to open at its usual width. A row is
/// twice a column's height, so [`SPEED`] would make the fold a jump, and a
/// span taken from the drawer's width would change with the window.
const FOLD: Duration = Duration::from_millis(150);

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
    /// For the prompts and the activity: the entry the first line on screen
    /// belongs to, and how far into it. The view finds the line again by it, so
    /// entries that arrive above it move nothing on screen. `None` shows `top`,
    /// which is the newest entry first while it is 0.
    pub(crate) anchor: Option<Anchor>,
    /// For the prompts and the activity: where the scrollbar's thumb asks the
    /// view to stand, in the lines of the bar, which count the entries not
    /// held. The next frame finds the line it stands for, and the view stays
    /// where it is until then.
    pub(crate) seek: Option<usize>,
}

/// A place in a list of entries: a line within an entry, and how many lines
/// the wheel has moved it since a frame placed it. The sum may lie outside the
/// entry, before its first line or after its last, which is how a scroll
/// reaches the entries around it before the next frame places it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Anchor {
    pub(crate) id: EntryId,
    pub(crate) line: i32,
    pub(crate) moved: i32,
}

#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is its own switch of the view, and each has readers across the app"
)]
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
    /// Whether the files tab lists the files in temporary folders.
    pub(crate) show_scratch_files: bool,
    /// Which of the session's files the files tab lists.
    pub(crate) files_shown: Shown,
    /// The one file type the files tab lists, by its chip's key.
    pub(crate) file_type: Option<u64>,
    /// The entries opened by a click, by id, for each transcript.
    pub(crate) open: HashMap<PathBuf, HashSet<EntryId>>,
    /// The entries of the transcript shown, laid out.
    pub(crate) layouts: Layouts,
    /// The lines the session list moved lately, which set how far ahead of the
    /// view the entries are held.
    pub(crate) prefetch: Lead,
    /// The lines held ahead of the view, as the loop read them before the
    /// frame being drawn.
    pub(crate) ahead: usize,
    /// The pane to select once the feed lists it, by block ID.
    pub(crate) want: Option<String>,
    /// The panes the daemon last listed, before the opened panes join them.
    listed: Vec<Pane>,
    /// The sessions chosen from the search that no listed pane lists, each
    /// shown as an opened pane until a listed pane lists it.
    opened: Vec<Agent>,
    /// The block of the pane this window stands in for, when it is a
    /// stand-in: it lists that pane alone and never focuses the panes or
    /// the search.
    stand_in: Option<String>,
    /// The panes, sessions, session and results, in that order.
    scrolls: [Scroll; 4],
    /// Whether the session's header is folded, which the session list's place
    /// decides. It moves by [`App::now`], the clock the loop reads at each
    /// wake.
    fold: Fold,
    /// The folded header drawn out over the list, and when the pointer last
    /// left it.
    peek: Option<Peek>,
    peek_left: Option<Instant>,
    /// How many rows the header takes unfolded, as the last frame that drew it
    /// found.
    header_rows: u16,
    /// The lists whose scroll was set since [`App::take_cleared`].
    cleared: [bool; 4],
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
    /// What the pointer drags: a column's edge or a scrollbar's thumb.
    pub(crate) dragging: Option<Drag>,
    /// When the app last read an event, or drew; the scrollbars fade by it.
    pub(crate) now: Instant,
    /// When each list's scrollbar was last in use, in the order of
    /// [`Focus::LISTS`].
    fades: [Fade; 4],
    /// The scrollbars the last frame drew.
    tracks: Vec<Track>,
    /// The place past the held entries that the thumb is at, until the entries
    /// there arrive or the thumb goes to a place that is held.
    reaching: Option<Reaching>,
    /// How many requests for the entries around a place were sent.
    asks: u64,
    /// The row each list's thumb was drawn gliding to, in the order of
    /// [`Focus::LISTS`], once a frame has drawn its bar.
    thumbs: [Option<Glide>; 4],
    /// Whether input moved a list since the last frame, which the thumb
    /// follows at once instead of gliding.
    touched: bool,
    /// The row of the thumb, in its track, that the next frame finds the place
    /// of, once the entries a page brought change the counts.
    retarget: Option<u32>,
}

/// A list's place in [`App::scrolls`].
pub(crate) const fn slot(list: Focus) -> usize {
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
            open: HashMap::new(),
            layouts: Layouts::default(),
            prefetch: Lead::default(),
            ahead: 0,
            show_scratch_files: false,
            files_shown: Shown::All,
            file_type: None,
            want: None,
            listed: Vec::new(),
            opened: Vec::new(),
            stand_in: None,
            scrolls: [Scroll::default(); 4],
            fold: Fold {
                folded: false,
                at: Instant::now()
                    .checked_sub(Duration::from_secs(1))
                    .unwrap_or_else(Instant::now),
                from: 0.0,
            },
            peek: None,
            peek_left: None,
            header_rows: HEADER_ROWS,
            cleared: [false; 4],
            wants: [48, 60, 72],
            drawer: None,
            left: None,
            outside: None,
            cols: 0,
            dragging: None,
            now: Instant::now(),
            fades: [Fade::default(); 4],
            tracks: Vec::new(),
            reaching: None,
            asks: 0,
            thumbs: [None; 4],
            touched: false,
            retarget: None,
        }
    }

    /// A stand-in for the pane `block`: it lists that pane's sessions and
    /// nothing else.
    pub(crate) fn stand_in(block: String) -> Self {
        Self {
            here: Some(block.clone()),
            stand_in: Some(block),
            ..Self::new(None)
        }
    }

    /// Whether this window stands in for one pane instead of showing them all.
    pub(crate) const fn is_stand_in(&self) -> bool {
        self.stand_in.is_some()
    }

    /// Whether focus may go to `column`: a stand-in has no panes column and
    /// no search.
    const fn may_focus(&self, column: Focus) -> bool {
        match column {
            Focus::Panes | Focus::Search => !self.is_stand_in(),
            Focus::Sessions | Focus::Session => true,
        }
    }

    pub(crate) const fn scroll_of(&self, list: Focus) -> Scroll {
        self.scrolls[slot(list)]
    }

    fn scroll_mut(&mut self, list: Focus) -> &mut Scroll {
        &mut self.scrolls[slot(list)]
    }

    /// Keeps where the last frame scrolled each list to.
    pub(crate) fn settle(&mut self, tops: &[(Focus, u16)], anchor: Option<Anchor>, bars: &[Track]) {
        self.tracks = bars.to_vec();
        self.glide_thumbs();
        for (list, top) in tops {
            *self.scroll_mut(*list) = Scroll {
                top: *top,
                follow: false,
                anchor: None,
                seek: None,
            };
        }
        // The header folds while the session list is off its top. A folded
        // header whose list fits the room folding gave it stays folded: that
        // top is only the room clamping the list, and unfolding would take
        // the room away again.
        if let Some((_, top)) = tops.iter().find(|(list, _)| *list == Focus::Session) {
            let fits = !bars.iter().any(|track| track.list == Focus::Session);
            self.fold_to(*top > 0 || (self.fold.folded && fits));
        }
        // A header that has slid back has nothing left to draw.
        if self.peek.is_some_and(|peek| !peek.open) && self.peeked().is_none() {
            self.peek = None;
        }
        // Only a frame that drew the session list knows where it stands.
        if tops.iter().any(|(list, _)| *list == Focus::Session) {
            self.scroll_mut(Focus::Session).anchor = anchor;
        } else {
            // A frame that drew no session has no place to find, so a drag
            // report waiting for one would wait for ever.
            self.scroll_mut(Focus::Session).seek = None;
        }
        // The counts changed under the thumb the pointer holds: where it
        // stands now stands for a place the next frame finds.
        if let Some(top) = self.retarget.take()
            && let Some(track) = self.track(Focus::Session)
        {
            let top = self.held_thumb(Focus::Session).unwrap_or(top);
            self.put_offset(
                Focus::Session,
                track.geometry.offset_for_top(i64::from(top)),
            );
        }
    }

    /// Notes the row each thumb stands on in the frame just drawn. One that
    /// moved with no input glides there from where it was drawn; one that
    /// input moved, or the pointer holds, is there at once.
    fn glide_thumbs(&mut self) {
        let touched = mem::take(&mut self.touched);
        let now = self.now;
        for list in Focus::LISTS {
            let target = self.track(list).map(|track| track.geometry.thumb().0);
            let held = self.held_thumb(list).is_some();
            let before = self.thumbs[slot(list)];
            self.thumbs[slot(list)] = target.map(|to| match before {
                Some(glide) if glide.to == to => glide,
                Some(glide) if !touched && !held => Glide {
                    from: glide.row(now),
                    to,
                    since: now,
                },
                _ => Glide {
                    from: to,
                    to,
                    since: now,
                },
            });
        }
    }

    /// The row the thumb of `list`, which stands on row `target` of its
    /// track, is drawn on at `now`: under the pointer while it holds it, at
    /// once where input moved it, else gliding to it.
    pub(crate) fn thumb_row(&self, list: Focus, target: u32, now: Instant) -> u32 {
        if let Some(top) = self.held_thumb(list) {
            return top;
        }
        match self.thumbs[slot(list)] {
            // Input that left this thumb where it was does not stop its glide.
            Some(glide) if glide.to == target || !self.touched => glide.row(now),
            Some(_) | None => target,
        }
    }

    /// The row each thumb is drawn on at `now`, in the order of
    /// [`Focus::LISTS`].
    pub(crate) fn thumb_rows(&self, now: Instant) -> [u32; 4] {
        Focus::LISTS.map(|list| {
            let target = self.track(list).map_or(0, |track| track.geometry.thumb().0);
            self.thumb_row(list, target, now)
        })
    }

    /// Puts `list` back at its top, and drops the wheel reports waiting to
    /// scroll it.
    fn reset_scroll(&mut self, list: Focus) {
        // The place the thumb was going to is in the session left behind.
        if list == Focus::Session {
            self.reaching = None;
            self.fold_to(false);
        }
        self.put_scroll(list, Scroll::default());
    }

    /// Sets how far `list` is scrolled to something other than where the
    /// wheel left it, so the wheel reports waiting for it are stale.
    fn put_scroll(&mut self, list: Focus, scroll: Scroll) {
        *self.scroll_mut(list) = scroll;
        self.cleared[slot(list)] = true;
    }

    /// The lists whose scroll was set since the last call, other than by the
    /// wheel.
    pub(crate) fn take_cleared(&mut self) -> Vec<Focus> {
        Focus::LISTS
            .into_iter()
            .filter(|list| mem::take(&mut self.cleared[slot(*list)]))
            .collect()
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
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the value is rounded and floored at zero, and lies between two u16 values"
            )]
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
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the value is rounded and floored at zero, and lies between two u16 values"
            )]
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

    /// How far the session header is folded to one row, from 0 to 1, eased.
    pub(crate) fn folded(&self) -> f32 {
        let to = if self.fold.folded { 1.0 } else { 0.0 };
        let (t, eased) = eased(self.now.saturating_duration_since(self.fold.at), FOLD);
        if t >= 1.0 {
            to
        } else {
            self.fold.from + (to - self.fold.from) * eased
        }
    }

    /// Folds the header, or unfolds it, from where it stands now so a
    /// reversal never jumps.
    fn fold_to(&mut self, folded: bool) {
        if self.fold.folded == folded {
            return;
        }
        self.fold = Fold {
            folded,
            at: self.now,
            from: self.folded(),
        };
        if !folded {
            self.peek = None;
        }
    }

    /// How far the folded header is drawn out over the list, from 0 to 1,
    /// eased; none once it has slid back.
    pub(crate) fn peeked(&self) -> Option<f32> {
        let peek = self.peek?;
        let (t, eased) = eased(self.now.saturating_duration_since(peek.at), FOLD);
        if peek.open {
            Some(eased)
        } else if t < 1.0 {
            Some(1.0 - eased)
        } else {
            None
        }
    }

    /// Draws the folded header out, or slides it back, from where it stands
    /// now so a reversal never jumps.
    fn peek_out(&mut self, open: bool) {
        if self.peek.is_some_and(|peek| peek.open == open) {
            return;
        }
        let Some(out) = self.peeked() else {
            self.peek = open.then_some(Peek { at: self.now, open });
            return;
        };
        // The curve is the drawer's: see [`App::draw_out`].
        let t = if open {
            1.0 - (1.0 - out).cbrt()
        } else {
            1.0 - out.cbrt()
        };
        let at = self.now.checked_sub(FOLD.mul_f32(t)).unwrap_or(self.now);
        self.peek = Some(Peek { at, open });
    }

    /// How many rows the header takes unfolded.
    pub(crate) const fn header_rows(&self) -> u16 {
        self.header_rows
    }

    /// Notes how many rows the unfolded header took in the last frame.
    pub(crate) const fn note_header(&mut self, rows: Option<u16>) {
        if let Some(rows) = rows {
            self.header_rows = rows;
        }
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
        if !self.may_focus(column) {
            return;
        }
        if column != self.lead {
            self.start_move(self.lead);
            self.lead = column;
            self.drawer = None;
            self.peek = None;
        }
        if column != self.focus {
            self.focus = column;
            self.filtering = false;
        }
    }

    /// Focuses `column` after a click in it. One drawn in full keeps the
    /// columns where they are; a folded one is laid out around.
    fn focus_here(&mut self, column: Focus) {
        if !self.may_focus(column) {
            return;
        }
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

    /// Takes a new list of panes from the daemon, and lists them as
    /// [`App::merge`] does.
    pub(crate) fn set_panes(&mut self, mut panes: Vec<Pane>) {
        // A stand-in lists its own pane alone.
        if let Some(block) = &self.stand_in {
            panes.retain(|pane| &pane.block_id == block);
        }
        let first = self.link == Link::Connecting;
        self.listed = panes;
        self.link = Link::Live;
        self.merge();
        // A stand-in opens on the pane's sessions when it lists them, else on
        // the session.
        if first && self.is_stand_in() {
            let focus = if self.lists_sessions() {
                Focus::Sessions
            } else {
                Focus::Session
            };
            (self.focus, self.lead, self.back) = (focus, focus, focus);
        }
    }

    /// Lists the daemon's panes and an opened pane for each opened session
    /// none of them lists, sorted by attention with the opened panes first,
    /// and keeps the selection on the same pane when it is still there. An
    /// opened session that a daemon pane lists now leaves the opened ones, and
    /// the selection follows it to that pane.
    fn merge(&mut self) {
        // The pane asked for, until the feed lists it; then the one selected.
        let mut kept = self
            .want
            .clone()
            .or_else(|| self.selected().map(|pane| pane.block_id.clone()));
        let mut followed = None;
        let listed = &self.listed;
        self.opened.retain(|agent| {
            let Some(pane) = listed.iter().find(|pane| lists(pane, &agent.session_id)) else {
                return true;
            };
            if kept.as_deref() == Some(opened_block(&agent.session_id).as_str()) {
                kept = Some(pane.block_id.clone());
                followed = Some(agent.session_id.clone());
            }
            false
        });
        let mut panes = self.listed.clone();
        panes.extend(self.opened.iter().map(opened_pane));
        panes.sort_by(|a, b| {
            b.opened
                .cmp(&a.opened)
                .then_with(|| a.closed.cmp(&b.closed))
                .then_with(|| rank(a.status).cmp(&rank(b.status)))
                .then_with(|| newest(a).cmp(&newest(b)))
        });
        let before = self.pane;
        self.panes = panes;
        let found = kept
            .as_ref()
            .and_then(|id| self.panes.iter().position(|pane| &pane.block_id == id));
        if found.is_some() {
            self.want = None;
        }
        self.pane = found.unwrap_or(0).min(self.panes.len().saturating_sub(1));
        // A selection the list no longer shows moves to the first it does.
        let visible = self.visible_panes();
        if !visible.contains(&self.pane)
            && let Some(first) = visible.first()
        {
            self.pane = *first;
        }
        // A pane that moved in the list stays in view.
        if self.pane != before {
            self.scroll_mut(Focus::Panes).follow = true;
        }
        if let Some(place) =
            followed.and_then(|id| self.selected().and_then(|pane| session_in(pane, &id)))
        {
            self.session = place;
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
            // A stopped pane whose session was resumed elsewhere is listed
            // once, where it runs now.
            .filter(|(_, pane)| self.moved_to(pane).is_none())
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

    /// The pane that runs a stopped pane's last session now, when it was
    /// resumed in another one: the stopped pane is what it left behind.
    pub(crate) fn moved_to(&self, pane: &Pane) -> Option<&Pane> {
        if pane.status != Status::Stopped {
            return None;
        }
        let (agent, _) = pane.sessions.first()?;
        self.running(&agent.session_id)
            .and_then(|index| self.panes.get(index))
            .filter(|other| other.block_id != pane.block_id)
    }

    /// The selected pane's sessions the sessions filter lets through, titled
    /// ones first, then the scratch ones when they are shown.
    pub(crate) fn visible_sessions(&self) -> Vec<usize> {
        let filter = self.filter(Focus::Sessions);
        let Some(pane) = self.selected() else {
            return Vec::new();
        };
        let (scratch, titled): (Listed, Listed) = pane
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
            || self.now.saturating_duration_since(self.fold.at) < FOLD
            || self
                .peek
                .is_some_and(|peek| self.now.saturating_duration_since(peek.at) < FOLD)
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

    #[expect(
        clippy::too_many_lines,
        reason = "one match over every key the app reads"
    )]
    pub(crate) fn on_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        self.touched = true;
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
                if self.is_stand_in() {
                    return Action::None;
                }
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
                } else if self.focus == Focus::Panes || self.is_stand_in() {
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
            KeyCode::Tab | KeyCode::BackTab => {
                // Through the panes, the pane's sessions when it lists them,
                // and the session.
                let order: Vec<Focus> = [Focus::Panes, Focus::Sessions, Focus::Session]
                    .into_iter()
                    .filter(|focus| self.may_focus(*focus))
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
            // In the session, the arrows walk its tabs; past the first one,
            // they go back to the panes.
            KeyCode::Right | KeyCode::Char('l') if self.focus == Focus::Session => {
                if self.tab != Tab::Files {
                    self.tab = self.tab.shift(1);
                    self.reset_scroll(Focus::Session);
                }
                Action::None
            }
            KeyCode::Left | KeyCode::Char('h')
                if self.focus == Focus::Session && self.tab != Tab::Overview =>
            {
                self.tab = self.tab.shift(-1);
                self.reset_scroll(Focus::Session);
                Action::None
            }
            KeyCode::Char('2' | '3' | 'l') | KeyCode::Right => {
                self.focus_on(Focus::Session);
                Action::None
            }
            KeyCode::Char('1' | 'h') | KeyCode::Left | KeyCode::Backspace => {
                self.focus_on(Focus::Panes);
                Action::None
            }
            KeyCode::Char(']') if self.focus == Focus::Session => {
                self.tab = self.tab.shift(1);
                self.reset_scroll(Focus::Session);
                Action::None
            }
            KeyCode::Char('[') if self.focus == Focus::Session => {
                self.tab = self.tab.shift(-1);
                self.reset_scroll(Focus::Session);
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
            KeyCode::Char('x') => {
                self.close_opened(self.pane);
                Action::None
            }
            KeyCode::Enter => match self.focus {
                Focus::Panes => self.go_to(),
                Focus::Sessions => {
                    self.focus_on(Focus::Session);
                    Action::None
                }
                Focus::Session | Focus::Search => self.resume(),
            },
            KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Char(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => Action::None,
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
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => {}
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
            self.put_scroll(
                list,
                Scroll {
                    top: 0,
                    follow: true,
                    anchor: None,
                    seek: None,
                },
            );
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.close_search();
                Action::None
            }
            KeyCode::Enter => self.choose(),
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
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Delete
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => Action::None,
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
        if mouse.kind != MouseEventKind::Moved {
            self.touched = true;
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
                // Motion with no button held is a release the pointer's report
                // lost, which would leave the thumb held and paging paused.
                self.let_go();
                self.hover_over(if edge { None } else { under.target });
                // A folded column draws out while the pointer is on it, and
                // goes back when the pointer leaves it.
                if self.focus != Focus::Search {
                    self.follow_pointer(edge, under);
                }
                Action::None
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                match self.dragging {
                    Some(Drag::Edge(grip)) => {
                        let wide = if grip.rightward {
                            mouse.column.saturating_sub(grip.anchor) + 1
                        } else {
                            grip.anchor.saturating_sub(mouse.column)
                        };
                        self.wants[grip.column.column()] = wide.max(NARROWEST);
                        // A drag follows the pointer, with nothing easing.
                        self.moved = None;
                    }
                    // Only the row counts: the pointer may leave the column.
                    Some(Drag::Thumb(held)) => self.drag_thumb(held, mouse.row),
                    None => {}
                }
                Action::None
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by: i16 = if mouse.kind == MouseEventKind::ScrollDown {
                    1
                } else {
                    -1
                };
                under.list.map_or(Action::None, |list| {
                    Action::Wheel(list, i32::from(by.saturating_mul(WHEEL)))
                })
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.notice = None;
                match under.target {
                    // The bar needs the row pressed, which a click does not
                    // carry.
                    Some(Target::Bar(list)) => {
                        self.press_bar(list, mouse.row);
                        Action::None
                    }
                    Some(target) => self.click(target),
                    None => Action::None,
                }
            }
            MouseEventKind::Up(_) => {
                self.let_go();
                self.hover_over(under.target);
                Action::None
            }
            MouseEventKind::Down(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => Action::None,
        }
    }

    /// Ends a drag of a thumb or an edge, as a release does: the bar fades
    /// from now, and a place that waits for its entries is asked.
    fn let_go(&mut self) {
        if let Some(Drag::Thumb(held)) = self.dragging {
            self.fades[slot(held.list)].touch(self.now);
        }
        self.dragging = None;
    }

    /// The pointer is now over `target`. The bar's last moment of use is the
    /// pointer leaving it, so it starts to fade there.
    fn hover_over(&mut self, target: Option<Target>) {
        let was = mem::replace(&mut self.hover, target);
        if let Some(Target::Bar(list)) = was
            && was != target
        {
            self.fades[slot(list)].touch(self.now);
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
        self.follow_drawer(under);
        self.follow_header(under);
    }

    /// Opens or shuts the folded header as the pointer moves: it draws out
    /// on the folded row and slides back once the pointer has left it and
    /// the header drawn out, as the drawer does.
    fn follow_header(&mut self, under: Under) {
        if under.fold {
            self.peek_out(true);
            return;
        }
        let Some(peek) = self.peek else {
            return;
        };
        if peek.open {
            if !under.peek {
                self.peek_left = Some(self.now);
                self.peek_out(false);
            }
        } else if under.peek
            && self
                .peek_left
                .is_some_and(|left| self.now.saturating_duration_since(left) < GRACE)
        {
            self.peek_out(true);
        }
    }

    /// Opens or shuts the drawer as the pointer moves.
    fn follow_drawer(&mut self, under: Under) {
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
            .filter(|_| {
                self.drawer.is_some_and(|drawer| drawer.open)
                    || self.peek.is_some_and(|peek| peek.open)
            })
            .map(|since| since + AWAY)
    }

    /// Shuts the drawer and the header drawn out once the pointer has stayed
    /// past the window's edge, and returns whether it shut one. A pointer that
    /// has stayed away with both shut is forgotten, so one opened later is not
    /// shut for it.
    pub(crate) fn tick(&mut self) -> bool {
        if self.outside.is_none_or(|since| since.elapsed() < AWAY) {
            return false;
        }
        self.outside = None;
        let drawer = self.drawer.is_some_and(|drawer| drawer.open);
        let header = self.peek.is_some_and(|peek| peek.open);
        if drawer {
            self.left = Some(Instant::now());
            self.draw_out(None);
        }
        if header {
            self.peek_left = Some(self.now);
            self.peek_out(false);
        }
        drawer || header
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
                self.reset_scroll(Focus::Sessions);
                self.reset_scroll(Focus::Session);
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
                self.reset_scroll(Focus::Session);
                Action::None
            }
            Target::Result(index) => {
                self.result = index;
                self.choose()
            }
            Target::Close(index) => {
                self.close_opened(index);
                Action::None
            }
            Target::Search => {
                if self.is_stand_in() {
                    return Action::None;
                }
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
                self.reset_scroll(Focus::Session);
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
                self.dragging = Some(Drag::Edge(grip));
                Action::None
            }
            // A press on a bar is taken in [`App::on_mouse`].
            Target::Bar(_) => Action::None,
            Target::Entry(id) => {
                // The entry belongs to the session drawn when it was clicked.
                let transcript = self.current().map(|agent| agent.transcript.clone());
                self.focus_here(Focus::Session);
                if let Some(transcript) = transcript {
                    let open = self.open.entry(transcript).or_default();
                    if !open.remove(&id) {
                        open.insert(id);
                    }
                }
                Action::None
            }
            Target::File(index) => Action::OpenFile(index),
            Target::FileKind(shown) => {
                self.focus_here(Focus::Session);
                self.files_shown = shown;
                Action::None
            }
            Target::FileType(key) => {
                self.focus_here(Focus::Session);
                self.file_type = (self.file_type != Some(key)).then_some(key);
                Action::None
            }
            Target::Folder(index) => Action::FilterFolder(index),
            Target::ScratchFiles => {
                self.focus_here(Focus::Session);
                self.show_scratch_files = !self.show_scratch_files;
                Action::None
            }
        }
    }

    /// Moves `list` by `by` notches of the wheel, leaving its selection where
    /// it is. The next frame clamps it to the list.
    fn scroll(&mut self, list: Focus, by: i16) {
        self.scroll_lines(list, i64::from(by.saturating_mul(WHEEL)));
    }

    /// Moves `list` by `lines`, leaving its selection where it is. The next
    /// frame clamps it to the list.
    pub(crate) fn scroll_lines(&mut self, list: Focus, lines: i64) {
        self.touched = true;
        self.fades[slot(list)].touch(self.now);
        if list == Focus::Session {
            self.prefetch.scrolled(lines, Instant::now());
        }
        let scroll = self.scroll_mut(list);
        if let Some(anchor) = &mut scroll.anchor {
            let lines = i32::try_from(lines).unwrap_or(if lines < 0 { i32::MIN } else { i32::MAX });
            anchor.moved = anchor.moved.saturating_add(lines);
        } else {
            let lines = i16::try_from(lines).unwrap_or(if lines < 0 { i16::MIN } else { i16::MAX });
            scroll.top = scroll.top.saturating_add_signed(lines);
        }
        scroll.follow = false;
    }

    pub(crate) fn on_paste(&mut self, text: &str) -> Action {
        let text = text.replace(['\n', '\r', '\t'], " ");
        if self.filtering {
            self.filters[self.focus.index()].push_str(&text);
            self.keep_visible();
            return Action::None;
        }
        if self.is_stand_in() {
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
        self.hover_over(None);
        self.draw_out(None);
        self.peek_out(false);
        // The release of a thumb held when focus went may never come: let go
        // of it as a release does, so a place waiting for its entries is asked.
        if let Some(Drag::Thumb(held)) = self.dragging {
            self.fades[slot(held.list)].touch(self.now);
            self.dragging = None;
        }
    }

    pub(crate) fn set_results(&mut self, results: Vec<Hit>) {
        self.results = results;
        self.result = 0;
        self.reset_scroll(Focus::Search);
    }

    /// Adds the transcript matches a deep search found for the current query.
    pub(crate) fn add_results(&mut self, more: Vec<Hit>, (read, total): (usize, usize)) {
        let known: HashSet<String> = self
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

    /// Shuts the search as Esc does: focus goes back, and the query and the
    /// results are cleared.
    fn close_search(&mut self) {
        self.focus = self.back;
        self.query.clear();
        self.results.clear();
    }

    /// Chooses the result: the search closes and the result's session is
    /// shown in the session column, in the pane that lists it, else in an
    /// opened pane. Nothing resumes and Rex keeps its pane.
    fn choose(&mut self) -> Action {
        let Some(agent) = self.results.get(self.result).map(|hit| hit.agent.clone()) else {
            return Action::None;
        };
        self.close_search();
        let id = agent.session_id.clone();
        if self.place_of(&id).is_none() {
            if !self.opened.iter().any(|opened| opened.session_id == id) {
                self.opened.push(agent.clone());
            }
            self.want = Some(opened_block(&id));
            self.merge();
        }
        let Some((pane, session)) = self.place_of(&id) else {
            return Action::None;
        };
        // A pane asked for before would pull the selection away at the next
        // list.
        self.want = None;
        self.pane = pane;
        self.session = session;
        // A filter that hides the chosen session would hide what was asked for.
        if !self.visible_panes().contains(&pane) {
            self.filters[Focus::Panes.index()].clear();
        }
        if !self.visible_sessions().contains(&session) {
            self.filters[Focus::Sessions.index()].clear();
            self.show_scratch |= !self.visible_sessions().contains(&session);
        }
        self.scroll_mut(Focus::Panes).follow = true;
        self.reset_scroll(Focus::Sessions);
        self.reset_scroll(Focus::Session);
        self.focus_here(Focus::Session);
        Action::None
    }

    /// The pane that lists the session `session_id`, and its place there:
    /// one the list shows before one it hides.
    fn place_of(&self, session_id: &str) -> Option<(usize, usize)> {
        self.panes
            .iter()
            .enumerate()
            .filter_map(|(index, pane)| {
                session_in(pane, session_id).map(|place| (index, pane, place))
            })
            .min_by_key(|(_, pane, _)| self.moved_to(pane).is_some())
            .map(|(index, _, place)| (index, place))
    }

    /// Closes the opened pane at `index`, if it is one. When it was the
    /// selected pane, the first pane is selected.
    fn close_opened(&mut self, index: usize) {
        let Some(id) = self
            .panes
            .get(index)
            .filter(|pane| pane.opened)
            .and_then(|pane| pane.sessions.first())
            .map(|(agent, _)| agent.session_id.clone())
        else {
            return;
        };
        let before = self.selected().map(|pane| pane.block_id.clone());
        self.opened.retain(|agent| agent.session_id != id);
        self.merge();
        if self.selected().map(|pane| pane.block_id.clone()) != before {
            self.session = self.first_session();
            self.reset_scroll(Focus::Sessions);
            self.reset_scroll(Focus::Session);
        }
    }

    /// Shows the selected pane; a closed one has no pane to show, so its
    /// session resumes instead. A stand-in shows no other pane and goes
    /// nowhere.
    fn go_to(&self) -> Action {
        if self.is_stand_in() {
            return Action::None;
        }
        match self.selected() {
            Some(pane) if pane.closed => self.resume(),
            Some(pane) => Action::GoTo {
                session_id: pane.session_id.clone(),
                block_id: pane.block_id.clone(),
            },
            None => Action::None,
        }
    }

    /// Moves the focused list's selection by `by` rows the filters let
    /// through; in the session column, scrolls it.
    fn step(&mut self, by: isize) {
        // A pane that lists no sessions leaves the keys to the session.
        let focus = match self.focus {
            Focus::Sessions if !self.lists_sessions() => Focus::Session,
            Focus::Panes | Focus::Sessions | Focus::Session | Focus::Search => self.focus,
        };
        match focus {
            Focus::Panes => {
                let rows = self.visible_panes();
                if let Some(next) = moved_in(&rows, self.pane, by) {
                    self.pane = next;
                    self.session = self.first_session();
                    self.reset_scroll(Focus::Sessions);
                    self.reset_scroll(Focus::Session);
                }
            }
            Focus::Sessions => {
                let rows = self.visible_sessions();
                if let Some(next) = moved_in(&rows, self.session, by) {
                    self.session = next;
                    self.reset_scroll(Focus::Session);
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
        self.fades[slot(focus)].touch(self.now);
    }

    /// A press on the scrollbar of `list`, on screen row `row`: on its thumb
    /// it takes hold of it, and on its track it pages the list toward the
    /// press, a view's height.
    fn press_bar(&mut self, list: Focus, row: u16) {
        let Some(track) = self.track(list) else {
            return;
        };
        let thumb = track.geometry.thumb();
        // A track with no thumb has nothing to press.
        if thumb.1 == 0 {
            return;
        }
        self.fades[slot(list)].touch(self.now);
        match scrollbar::press(thumb, u32::from(row.saturating_sub(track.area.y))) {
            Press::Thumb { grab } => {
                self.dragging = Some(Drag::Thumb(Held {
                    list,
                    grab,
                    top: thumb.0,
                }));
            }
            Press::Track { above } => {
                let page = i64::from(track.area.height);
                self.scroll_lines(list, if above { -page } else { page });
                // The wheel reports waiting for the list move it from where it
                // was.
                self.cleared[slot(list)] = true;
            }
        }
    }

    /// The pointer, held on a thumb, is now on screen row `row`: the thumb
    /// follows it, keeping the row that was grabbed under it, and the list
    /// goes where the thumb stands.
    fn drag_thumb(&mut self, held: Held, row: u16) {
        let Some(track) = self.track(held.list) else {
            self.dragging = None;
            return;
        };
        let geometry = track.geometry;
        let pointer = i64::from(row) - i64::from(track.area.y) - i64::from(held.grab);
        let room = i64::from(geometry.visible.saturating_sub(geometry.thumb().1));
        self.dragging = Some(Drag::Thumb(Held {
            top: u32::try_from(pointer.clamp(0, room)).unwrap_or(0),
            ..held
        }));
        self.fades[slot(held.list)].touch(self.now);
        self.put_offset(held.list, geometry.offset_for_top(pointer));
    }

    /// Puts the first line in view of `list` at `offset` lines of its bar,
    /// as no wheel report did, so the reports waiting for it are stale.
    fn put_offset(&mut self, list: Focus, offset: u32) {
        let was = self.scroll_of(list);
        let scroll = if list == Focus::Session && matches!(self.tab, Tab::Prompts | Tab::Activity) {
            // The bar counts entries that are not held, so the next frame finds
            // the line among the held ones. Until then the view stays.
            Scroll {
                seek: Some(usize::try_from(offset).unwrap_or(usize::MAX)),
                follow: false,
                ..was
            }
        } else {
            Scroll {
                top: u16::try_from(offset).unwrap_or(u16::MAX),
                follow: false,
                anchor: None,
                seek: None,
            }
        };
        self.put_scroll(list, scroll);
    }

    /// What the last frame made of the thumb's place, when it had one to find.
    /// A place no held entry covers is the one to ask for next, with a request
    /// for an older one still out.
    pub(crate) fn found_seek(&mut self, seek: Option<Seek>) {
        match seek {
            Some(Seek::Held) => {
                // A request out is still answered, and the answer joins the
                // entries without moving the view.
                if let Some(reaching) = &mut self.reaching {
                    reaching.at = None;
                }
                if self
                    .reaching
                    .as_ref()
                    .is_some_and(|reaching| reaching.ask.is_none())
                {
                    self.reaching = None;
                }
            }
            Some(Seek::Past(list, at)) => {
                let top = self.held_thumb(Focus::Session);
                let path = self.current().map(|agent| agent.transcript.clone());
                match (&mut self.reaching, path) {
                    (Some(reaching), Some(path))
                        if reaching.path == path && reaching.list == list =>
                    {
                        reaching.at = Some(at);
                        reaching.top = top.unwrap_or(reaching.top);
                    }
                    (_, Some(path)) => {
                        self.reaching = Some(Reaching {
                            list,
                            at: Some(at),
                            top: top.unwrap_or(0),
                            path,
                            ask: None,
                        });
                    }
                    (_, None) => self.reaching = None,
                }
            }
            None => {}
        }
    }

    /// The request to send now: the newest place no held entry covers, when
    /// none is out, the thumb having moved on to it or let go of it. `epoch`
    /// gives the generation of the held entries of a list of a transcript,
    /// which an answer must still be in.
    pub(crate) fn take_jump(&mut self, epoch: impl FnOnce(List, &Path) -> u64) -> Option<Jump> {
        // A drag report not yet drawn may put the thumb somewhere else.
        let pending = self.scroll_of(Focus::Session).seek.is_some();
        let reaching = self.reaching.as_mut()?;
        let at = reaching.at?;
        if reaching.ask.is_some() || pending {
            return None;
        }
        self.asks += 1;
        reaching.ask = Some((self.asks, epoch(reaching.list, &reaching.path), at));
        Some(Jump {
            list: reaching.list,
            at,
            path: reaching.path.clone(),
            ask: self.asks,
        })
    }

    /// When the loop must wake for a place that waits to be asked: now, unless
    /// a request is out, or a drag report waits to be drawn, whose frame comes
    /// next and may move the thumb.
    pub(crate) fn jump_wake(&self) -> Option<Instant> {
        let reaching = self.reaching.as_ref()?;
        (reaching.at.is_some()
            && reaching.ask.is_none()
            && self.scroll_of(Focus::Session).seek.is_none())
        .then_some(self.now)
    }

    /// Whether a drag report waits for the frame that finds its place.
    pub(crate) fn seeking(&self) -> bool {
        self.scroll_of(Focus::Session).seek.is_some()
    }

    /// Whether the answer `ask` about `list` of the transcript at `path`,
    /// read in `epoch`, is to the request the window is waiting for.
    pub(crate) fn expects(&self, path: &Path, list: List, ask: u64, epoch: u64) -> bool {
        self.reaching.as_ref().is_some_and(|reaching| {
            reaching.path == path
                && reaching.list == list
                && reaching
                    .ask
                    .is_some_and(|(sent, read, _)| (sent, read) == (ask, epoch))
        })
    }

    /// Whether the answer `ask` about `list` of the transcript at `path` is to
    /// the request the window is waiting for, whatever generation of the held
    /// entries it was read in.
    pub(crate) fn awaits(&self, path: &Path, list: List, ask: u64) -> bool {
        self.reaching.as_ref().is_some_and(|reaching| {
            reaching.path == path
                && reaching.list == list
                && reaching.ask.is_some_and(|(sent, _, _)| sent == ask)
        })
    }

    /// Whether the answer `ask` is the one to land, read in `epoch`, the
    /// generation of the entries the window holds for `path`, when it holds
    /// any. An answer the window waits for and cannot land gives up the place,
    /// so no page stays held back for it.
    pub(crate) fn answered(
        &mut self,
        path: &Path,
        list: List,
        ask: u64,
        epoch: Option<u64>,
    ) -> bool {
        if epoch.is_some_and(|epoch| self.expects(path, list, ask, epoch)) {
            return true;
        }
        if self.awaits(path, list, ask) {
            self.drop_jump();
        }
        false
    }

    /// The entries around the place have arrived, `fresh` of them not held
    /// before: the view stands at `place`, the entry nearest it, with its first
    /// line on top. Entries all held already show nothing the view does not, so
    /// it stays where it is.
    pub(crate) fn arrive(&mut self, place: EntryId, fresh: usize) {
        let reaching = self.reaching.take();
        // The thumb went on to another place while the page was on its way:
        // the next frame finds where it stands now, and the place is asked for
        // if no held entry covers it.
        self.retarget = reaching
            .as_ref()
            .and_then(|reaching| {
                let (_, _, asked) = reaching.ask?;
                reaching.at.filter(|at| *at != asked).map(|_| reaching.top)
            })
            .or(self.retarget);
        // The thumb came back to held entries, which the view stands at.
        let covered = reaching.is_some_and(|reaching| reaching.at.is_none());
        if fresh == 0 || covered {
            return;
        }
        self.put_scroll(
            Focus::Session,
            Scroll {
                top: 0,
                follow: false,
                anchor: Some(Anchor {
                    id: place,
                    line: 0,
                    moved: 0,
                }),
                seek: None,
            },
        );
    }

    /// The entries around the place are not coming: the view stays.
    pub(crate) fn drop_jump(&mut self) {
        self.reaching = None;
        self.retarget = None;
    }

    /// Whether the view is held where it is for a jump, or while the thumb is
    /// held on the session, so no page is asked for in the meantime.
    pub(crate) fn holds_view(&self) -> bool {
        self.reaching.is_some()
            || self.retarget.is_some()
            || self.seeking()
            || self.held_thumb(Focus::Session).is_some()
    }

    /// The scrollbar of `list` as the last frame drew it.
    fn track(&self, list: Focus) -> Option<Track> {
        self.tracks.iter().find(|track| track.list == list).copied()
    }

    /// Where the pointer keeps the thumb of `list`, in rows of its track, while
    /// it holds it.
    pub(crate) fn held_thumb(&self, list: Focus) -> Option<u32> {
        match self.dragging {
            Some(Drag::Thumb(held)) if held.list == list => Some(held.top),
            Some(Drag::Thumb(_) | Drag::Edge(_)) | None => None,
        }
    }

    /// How the scrollbar of `list` draws at `now`.
    pub(crate) fn bar_shade(&self, list: Focus, now: Instant) -> Shade {
        self.fades[slot(list)].shade(now, self.bar_lit(list))
    }

    /// Whether the pointer is on the scrollbar of `list` or holds its thumb.
    fn bar_lit(&self, list: Focus) -> bool {
        self.hover == Some(Target::Bar(list)) || self.held_thumb(list).is_some()
    }

    /// How every scrollbar draws at `now`, in the order of [`Focus::LISTS`].
    pub(crate) fn bar_shades(&self, now: Instant) -> [Shade; 4] {
        Focus::LISTS.map(|list| self.bar_shade(list, now))
    }

    /// When a scrollbar next draws differently with no event: where its hold
    /// ends, then each step of its fade.
    pub(crate) fn bars_wake(&self) -> Option<Instant> {
        let gliding = self
            .thumbs
            .iter()
            .flatten()
            .any(|glide| glide.row(self.now) != glide.to)
            .then(|| self.now + GLIDE_STEP);
        Focus::LISTS
            .into_iter()
            .filter_map(|list| self.fades[slot(list)].next_change(self.now, self.bar_lit(list)))
            .chain(gliding)
            .min()
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

/// A pane's session positions, each with whether it is a scratch one.
type Listed = Vec<(usize, bool)>;

/// A session with no title that started in a temporary folder: a scripted
/// run, such as `claude -p`.
pub(crate) fn is_scratch(agent: &Agent) -> bool {
    agent.title == "Untitled session" && in_temp(&agent.cwd)
}

/// Whether `path` is in a temporary folder, which the system clears.
pub(crate) fn in_temp(path: &str) -> bool {
    ["/tmp/", "/private/tmp/", "/var/folders/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || Path::new(path).starts_with(env::temp_dir())
}

/// How long ago a pane's newest session was active; a pane with no age
/// sorts last.
fn newest(pane: &Pane) -> Duration {
    pane.sessions
        .first()
        .and_then(|(agent, _)| agent.idle)
        .unwrap_or(Duration::MAX)
}

/// The block ID of the opened pane for `session_id`.
fn opened_block(session_id: &str) -> String {
    format!("opened:{session_id}")
}

/// The opened pane for a session no daemon pane lists.
fn opened_pane(agent: &Agent) -> Pane {
    Pane {
        session_id: String::new(),
        block_id: opened_block(&agent.session_id),
        name: agent.title.clone(),
        status: Status::Stopped,
        tab: String::new(),
        progress: None,
        msg: None,
        sessions: vec![(agent.clone(), false)],
        closed: true,
        opened: true,
    }
}

/// Whether `pane` lists the session `session_id`.
fn lists(pane: &Pane, session_id: &str) -> bool {
    session_in(pane, session_id).is_some()
}

/// Where `pane` lists the session `session_id`.
fn session_in(pane: &Pane, session_id: &str) -> Option<usize> {
    pane.sessions
        .iter()
        .position(|(agent, _)| agent.session_id == session_id)
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
            Focus::Panes | Focus::Sessions | Focus::Session => carry.focus,
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

/// Panes and sessions the tests of the app's state and view build on.
#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::app::feed::Pane;
    use crate::session::Agent;
    use crate::status::Status;

    /// A session titled `title`, started in an invented folder.
    pub(crate) fn agent(session_id: &str, title: &str) -> Agent {
        Agent {
            session_id: session_id.to_owned(),
            cwd: "/work/shop".to_owned(),
            title: title.to_owned(),
            idle: Some(Duration::from_secs(60)),
            transcript: PathBuf::from("/work/shop/session.jsonl"),
        }
    }

    /// The pane `block`, named `name`, with a session for each of `titles`.
    pub(crate) fn pane(block: &str, name: &str, status: Status, titles: &[&str]) -> Pane {
        Pane {
            session_id: "main".to_owned(),
            block_id: block.to_owned(),
            name: name.to_owned(),
            status,
            tab: "tab 1".to_owned(),
            progress: None,
            msg: None,
            sessions: titles
                .iter()
                .enumerate()
                .map(|(place, title)| (agent(&format!("{block}-{place}"), title), false))
                .collect(),
            closed: false,
            opened: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::fixtures::{agent, pane};
    use super::*;

    const OWN: &str = "block-own";

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn three_panes(own_titles: &[&str]) -> Vec<Pane> {
        vec![
            pane("block-a", "alpha/one", Status::Working, &["Fix login"]),
            pane(OWN, "work/shell", Status::Stopped, own_titles),
            pane("block-b", "beta/two", Status::Idle, &["Tune search"]),
        ]
    }

    fn stand_in(own_titles: &[&str]) -> App {
        let mut app = App::stand_in(OWN.to_owned());
        app.set_panes(three_panes(own_titles));
        app
    }

    /// A search for which the sessions `ids` are the results, as the daemon
    /// answers it.
    fn search_finds(app: &mut App, ids: &[&str]) {
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        app.set_results(
            ids.iter()
                .map(|id| Hit {
                    agent: agent(id, "Found session"),
                    snippet: None,
                    prompts: 1,
                })
                .collect(),
        );
    }

    /// A press of the left button on `target`.
    fn press_on(app: &mut App, target: Target) -> Action {
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 3,
                modifiers: KeyModifiers::NONE,
            },
            Under {
                target: Some(target),
                ..Under::default()
            },
        )
    }

    /// An app with the three panes, the last of which lists two sessions.
    fn listed_app() -> App {
        let mut app = App::new(None);
        app.set_panes(vec![
            pane("block-a", "alpha/one", Status::Working, &["Fix login"]),
            pane(OWN, "work/shell", Status::Stopped, &["Fix checkout"]),
            pane(
                "block-b",
                "beta/two",
                Status::Idle,
                &["Tune search", "Add coupons"],
            ),
        ]);
        app
    }

    fn selected_block(app: &App) -> &str {
        app.selected().map_or("", |pane| pane.block_id.as_str())
    }

    #[test]
    fn enter_on_a_result_a_pane_lists_selects_its_pane_and_session() {
        let mut app = listed_app();
        search_finds(&mut app, &["block-b-1"]);
        let action = app.on_key(key(KeyCode::Enter));
        assert!(matches!(action, Action::None), "{action:?}");
        assert_eq!(selected_block(&app), "block-b");
        assert_eq!(app.session, 1);
        assert_eq!(app.focus, Focus::Session);
        assert!(app.query.is_empty() && app.results.is_empty());
    }

    #[test]
    fn a_pane_asked_for_earlier_does_not_pull_the_choice_away() {
        let mut app = listed_app();
        app.want = Some("block-a".to_owned());
        search_finds(&mut app, &["block-b-1"]);
        app.on_key(key(KeyCode::Enter));
        app.set_panes(vec![
            pane("block-a", "alpha/one", Status::Working, &["Fix login"]),
            pane(
                "block-b",
                "beta/two",
                Status::Idle,
                &["Tune search", "Add coupons"],
            ),
        ]);
        assert_eq!(selected_block(&app), "block-b");
        assert_eq!(app.session, 1);
    }

    #[test]
    fn a_click_on_a_result_that_is_not_selected_chooses_it() {
        let mut app = listed_app();
        search_finds(&mut app, &["block-a-0", "block-b-0"]);
        let action = press_on(&mut app, Target::Result(1));
        assert!(matches!(action, Action::None), "{action:?}");
        assert_eq!(selected_block(&app), "block-b");
        assert_eq!(app.session, 0);
        assert_ne!(app.focus, Focus::Search);
    }

    #[test]
    fn a_result_no_pane_lists_opens_first_in_the_list_and_selected() {
        let mut app = listed_app();
        search_finds(&mut app, &["old-0"]);
        let action = press_on(&mut app, Target::Result(0));
        assert!(matches!(action, Action::None), "{action:?}");
        assert_eq!(app.panes.len(), 4);
        assert_eq!(app.pane, 0);
        let opened = &app.panes[0];
        assert!(opened.opened && opened.closed);
        assert_eq!(opened.block_id, "opened:old-0");
        assert_eq!(opened.sessions.len(), 1);
        assert_eq!(app.session, 0);
        assert_eq!(app.focus, Focus::Session);
        assert_eq!(
            app.current().map(|agent| agent.session_id.as_str()),
            Some("old-0")
        );
    }

    #[test]
    fn choosing_an_opened_session_twice_leaves_one_opened_pane() {
        let mut app = listed_app();
        for _ in 0..2 {
            search_finds(&mut app, &["old-0"]);
            app.on_key(key(KeyCode::Enter));
        }
        assert_eq!(app.panes.iter().filter(|pane| pane.opened).count(), 1);
        assert_eq!(app.panes.len(), 4);
        assert_eq!(selected_block(&app), "opened:old-0");
    }

    #[test]
    fn x_closes_the_selected_opened_pane_and_selects_the_first_pane() {
        let mut app = listed_app();
        search_finds(&mut app, &["old-0"]);
        app.on_key(key(KeyCode::Enter));
        app.on_key(key(KeyCode::Char('x')));
        assert!(app.panes.iter().all(|pane| !pane.opened));
        assert_eq!(app.panes.len(), 3);
        assert_eq!(app.pane, 0);
        assert_eq!(selected_block(&app), "block-a");
        // A pane Rex lists has no close.
        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(app.panes.len(), 3);
    }

    #[test]
    fn a_click_on_the_close_mark_closes_the_opened_pane() {
        let mut app = listed_app();
        search_finds(&mut app, &["old-0"]);
        app.on_key(key(KeyCode::Enter));
        let action = press_on(&mut app, Target::Close(0));
        assert!(matches!(action, Action::None), "{action:?}");
        assert!(app.panes.iter().all(|pane| !pane.opened));
        assert_eq!(selected_block(&app), "block-a");
    }

    #[test]
    fn a_daemon_pane_that_comes_to_list_an_opened_session_replaces_it() {
        let mut app = listed_app();
        search_finds(&mut app, &["old-0"]);
        app.on_key(key(KeyCode::Enter));
        let mut panes = three_panes(&["Fix checkout"]);
        panes.push(pane("old", "gamma/three", Status::Working, &["Old work"]));
        app.set_panes(panes);
        assert!(app.panes.iter().all(|pane| !pane.opened));
        assert_eq!(selected_block(&app), "old");
        assert_eq!(app.session, 0);
    }

    #[test]
    fn a_frame_without_the_session_list_leaves_its_anchor() {
        let mut app = App::new(None);
        let anchor = Anchor {
            id: EntryId {
                line: 100,
                index: 0,
            },
            line: 3,
            moved: 0,
        };
        app.scroll_mut(Focus::Session).anchor = Some(anchor);
        app.settle(&[(Focus::Panes, 0)], None, &[]);
        assert_eq!(app.scroll_of(Focus::Session).anchor, Some(anchor));
        app.settle(&[(Focus::Session, 5)], None, &[]);
        assert_eq!(app.scroll_of(Focus::Session).anchor, None);
    }

    #[test]
    fn a_wheel_step_is_kept_beside_the_anchor_it_moves() {
        let mut app = App::new(None);
        app.scroll_lines(Focus::Session, 4);
        assert_eq!(app.scroll_of(Focus::Session).top, 4);
        let anchor = Anchor {
            id: EntryId {
                line: 100,
                index: 0,
            },
            line: 3,
            moved: 0,
        };
        app.scroll_mut(Focus::Session).anchor = Some(anchor);
        app.scroll_lines(Focus::Session, 2);
        app.scroll_lines(Focus::Session, -1);
        let scroll = app.scroll_of(Focus::Session);
        assert_eq!(scroll.anchor, Some(Anchor { moved: 1, ..anchor }));
        assert_eq!(scroll.top, 4);
    }

    /// A pointer report inside the window, over what `under` says.
    fn pointer_at(app: &mut App, under: Under) {
        let under = Under {
            size: (100, 40),
            ..under
        };
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Moved,
                column: 10,
                row: 10,
                modifiers: KeyModifiers::NONE,
            },
            under,
        );
    }

    #[test]
    fn the_header_folds_over_a_fixed_span_whatever_its_height_or_the_width() {
        for (cols, rows) in [(120, 12), (300, 4)] {
            let mut app = App::new(None);
            app.cols = cols;
            app.note_header(Some(rows));
            app.fold_to(true);
            app.now += FOLD / 2;
            let mid = app.folded();
            assert!(mid > 0.0 && mid < 1.0, "{mid}");
            app.now += FOLD;
            assert!((app.folded() - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn the_pointer_on_the_folded_header_draws_it_out_and_leaving_shuts_it() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        assert_eq!(app.peeked(), None);
        pointer_at(
            &mut app,
            Under {
                fold: true,
                ..Under::default()
            },
        );
        assert_eq!(app.peeked(), Some(0.0));
        app.now += Duration::from_secs(1);
        assert_eq!(app.peeked(), Some(1.0));
        // On the header drawn out, or back on the folded row, it stays.
        for under in [
            Under {
                peek: true,
                ..Under::default()
            },
            Under {
                fold: true,
                peek: true,
                ..Under::default()
            },
        ] {
            pointer_at(&mut app, under);
            app.now += Duration::from_secs(1);
            assert_eq!(app.peeked(), Some(1.0));
        }
        // Off both, it slides back.
        pointer_at(&mut app, Under::default());
        app.now += Duration::from_millis(20);
        assert!(app.peeked().is_some_and(|out| out > 0.0 && out < 1.0));
        app.now += Duration::from_secs(1);
        assert_eq!(app.peeked(), None);
    }

    #[test]
    fn the_header_comes_back_for_a_pointer_that_returns_within_the_grace() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        let on_header = Under {
            peek: true,
            ..Under::default()
        };
        pointer_at(
            &mut app,
            Under {
                fold: true,
                ..Under::default()
            },
        );
        app.now += Duration::from_secs(1);
        pointer_at(&mut app, Under::default());
        app.now += GRACE / 2;
        pointer_at(&mut app, on_header);
        assert!(app.peek.is_some_and(|peek| peek.open));
        // Past the grace, the header drawn out is no longer the pointer's to
        // call back.
        pointer_at(&mut app, Under::default());
        app.now += GRACE * 2;
        pointer_at(&mut app, on_header);
        assert!(app.peek.is_some_and(|peek| !peek.open));
    }

    #[test]
    fn a_pointer_past_the_window_edge_shuts_the_header_after_the_away_time() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        pointer_at(
            &mut app,
            Under {
                fold: true,
                ..Under::default()
            },
        );
        app.now += Duration::from_secs(1);
        app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Moved,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            Under {
                size: (100, 40),
                ..Under::default()
            },
        );
        assert!(app.wake_at().is_some());
        app.outside = app.outside.and_then(|at| at.checked_sub(AWAY * 2));
        assert!(app.tick());
        app.now += Duration::from_secs(1);
        assert_eq!(app.peeked(), None);
        assert_eq!(app.wake_at(), None);
    }

    #[test]
    fn a_drawer_opened_after_the_pointer_left_stays_open() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        // The pointer reports from the window's edge while no drawer is out,
        // and stays there past the time that shuts a drawer.
        let edge = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        let under = Under {
            size: (100, 40),
            ..Under::default()
        };
        app.on_mouse(edge, under);
        app.outside = app.outside.and_then(|at| at.checked_sub(AWAY * 2));
        assert!(app.outside.is_some());
        // The loop ticks on every wake, so the pointer is forgotten at once.
        assert!(!app.tick());
        assert_eq!(app.outside, None);
        // A window that replaces another opens with the drawer out.
        let mut carry = app.carry();
        carry.drawn_out = Some(Focus::Panes);
        app.restore(carry);
        assert_eq!(app.wake_at(), None);
        assert!(!app.tick());
        assert!(app.out(Focus::Panes));
    }

    #[test]
    fn a_click_on_a_pane_puts_the_session_back_at_its_top() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        app.scroll_mut(Focus::Session).anchor = Some(Anchor {
            id: EntryId {
                line: 100,
                index: 0,
            },
            line: 3,
            moved: 0,
        });
        app.take_cleared();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let under = Under {
            target: Some(Target::Pane(2)),
            ..Under::default()
        };
        app.on_mouse(click, under);
        assert_eq!(app.pane, 2);
        assert_eq!(app.scroll_of(Focus::Session), Scroll::default());
        // The loop drops the wheel reports waiting for the lists it reset.
        assert_eq!(app.take_cleared(), [Focus::Sessions, Focus::Session]);
        assert_eq!(app.take_cleared(), []);
    }

    #[test]
    fn a_stand_in_lists_only_its_own_pane() {
        let app = stand_in(&["Fix checkout"]);
        let blocks: Vec<&str> = app
            .panes
            .iter()
            .map(|pane| pane.block_id.as_str())
            .collect();
        assert_eq!(blocks, [OWN]);
    }

    #[test]
    fn a_full_app_lists_every_pane() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        assert_eq!(app.panes.len(), 3);
    }

    #[test]
    fn a_stand_in_opens_on_the_sessions_only_when_the_pane_lists_them() {
        assert_eq!(stand_in(&["Fix checkout"]).focus, Focus::Session);
        assert_eq!(
            stand_in(&["Fix checkout", "Add coupons"]).focus,
            Focus::Sessions
        );
    }

    #[test]
    fn no_key_takes_a_stand_in_to_the_panes_or_the_search() {
        let keys = [
            key(KeyCode::Char('1')),
            key(KeyCode::Char('h')),
            key(KeyCode::Left),
            key(KeyCode::Backspace),
            key(KeyCode::Tab),
            key(KeyCode::BackTab),
            key(KeyCode::Char('2')),
            key(KeyCode::Char('g')),
            key(KeyCode::Enter),
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        ];
        for titles in [&["Fix checkout"][..], &["Fix checkout", "Add coupons"][..]] {
            let mut app = stand_in(titles);
            for press in keys {
                let action = app.on_key(press);
                assert!(
                    !matches!(action, Action::GoTo { .. } | Action::Search),
                    "{press:?} asked for {action:?}"
                );
                assert!(
                    matches!(app.focus, Focus::Sessions | Focus::Session),
                    "{press:?} moved focus to {:?}",
                    app.focus
                );
            }
        }
    }

    #[test]
    fn the_same_keys_reach_the_panes_in_the_full_app() {
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        app.on_key(key(KeyCode::Char('1')));
        assert_eq!(app.focus, Focus::Panes);
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert_eq!(app.focus, Focus::Search);
    }

    #[test]
    fn a_click_on_the_search_or_a_paste_leaves_a_stand_in_as_it_was() {
        let mut app = stand_in(&["Fix checkout"]);
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        let under = Under {
            target: Some(Target::Search),
            ..Under::default()
        };
        assert_eq!(app.on_mouse(click, under), Action::None);
        assert_eq!(app.on_paste("coupons"), Action::None);
        assert_eq!(app.focus, Focus::Session);
        assert_eq!(app.query, "");
    }

    #[test]
    fn a_paste_goes_into_a_stand_ins_active_filter() {
        let mut app = stand_in(&["Fix checkout", "Add coupons"]);
        app.on_key(key(KeyCode::Char('/')));
        app.on_paste("coupons");
        assert_eq!(app.filter(Focus::Sessions), "coupons");
        assert_eq!(app.query, "");
    }

    #[test]
    fn esc_clears_a_stand_ins_filter_and_then_quits() {
        let mut app = stand_in(&["Fix checkout", "Add coupons"]);
        for press in [KeyCode::Char('/'), KeyCode::Char('a'), KeyCode::Enter] {
            app.on_key(key(press));
        }
        assert_eq!(app.filter(Focus::Sessions), "a");
        assert_eq!(app.on_key(key(KeyCode::Esc)), Action::None);
        assert_eq!(app.filter(Focus::Sessions), "");
        assert_eq!(app.on_key(key(KeyCode::Esc)), Action::Quit);
    }

    /// The scrollbar of `list`, `rows` tall at column 30 from row 4, drawn for
    /// a view at `offset` of `total` lines.
    fn track(list: Focus, rows: u16, (offset, total): (usize, usize)) -> Track {
        Track {
            list,
            area: Rect::new(30, 4, 1, rows),
            geometry: scrollbar::Geometry::new(offset, rows, total),
        }
    }

    fn pointer(kind: MouseEventKind, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: 30,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn on_bar(list: Focus) -> Under {
        Under {
            target: Some(Target::Bar(list)),
            size: (100, 40),
            ..Under::default()
        }
    }

    const LEFT: MouseButton = MouseButton::Left;

    #[test]
    fn a_drag_keeps_the_grabbed_row_of_the_thumb_under_the_pointer() {
        let mut app = App::new(None);
        let bar = track(Focus::Panes, 20, (0, 100));
        app.settle(&[(Focus::Panes, 0)], None, &[bar]);
        // The thumb is 4 rows from the top; the press takes its third row.
        assert_eq!(bar.geometry.thumb(), (0, 4));
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 6), on_bar(Focus::Panes));
        assert_eq!(app.held_thumb(Focus::Panes), Some(0));
        app.on_mouse(pointer(MouseEventKind::Drag(LEFT), 11), Under::default());
        assert_eq!(app.held_thumb(Focus::Panes), Some(5));
        let offset = bar.geometry.offset_for_top(5);
        assert_eq!(
            u32::from(app.scroll_of(Focus::Panes).top),
            offset,
            "the list stands where a thumb 5 rows down stands for"
        );
        // Past the ends of the track the thumb stops at them.
        app.on_mouse(pointer(MouseEventKind::Drag(LEFT), 90), Under::default());
        assert_eq!(app.held_thumb(Focus::Panes), Some(16));
        assert_eq!(app.scroll_of(Focus::Panes).top, 80);
        app.on_mouse(pointer(MouseEventKind::Drag(LEFT), 0), Under::default());
        assert_eq!(app.held_thumb(Focus::Panes), Some(0));
        assert_eq!(app.scroll_of(Focus::Panes).top, 0);
        app.on_mouse(pointer(MouseEventKind::Up(LEFT), 0), Under::default());
        assert_eq!(app.dragging, None);
    }

    #[test]
    fn a_drag_or_a_page_drops_the_wheel_reports_waiting_for_the_list() {
        let mut app = App::new(None);
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (0, 100))],
        );
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 4), on_bar(Focus::Panes));
        assert_eq!(app.take_cleared(), []);
        app.on_mouse(pointer(MouseEventKind::Drag(LEFT), 8), Under::default());
        assert_eq!(app.take_cleared(), [Focus::Panes]);
        app.on_mouse(pointer(MouseEventKind::Up(LEFT), 8), Under::default());
        app.on_mouse(
            pointer(MouseEventKind::Down(LEFT), 20),
            on_bar(Focus::Panes),
        );
        assert_eq!(app.take_cleared(), [Focus::Panes]);
    }

    #[test]
    fn a_press_on_the_track_pages_the_list_by_its_height_toward_the_press() {
        let mut app = App::new(None);
        // A list 20 rows tall, the view at its top.
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (0, 100))],
        );
        app.on_mouse(
            pointer(MouseEventKind::Down(LEFT), 14),
            on_bar(Focus::Panes),
        );
        assert_eq!(app.dragging, None);
        assert_eq!(app.scroll_of(Focus::Panes).top, 20);
        // The thumb is now 4 rows down, so a press above it pages back.
        app.settle(
            &[(Focus::Panes, 20)],
            None,
            &[track(Focus::Panes, 20, (20, 100))],
        );
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 5), on_bar(Focus::Panes));
        assert_eq!(app.scroll_of(Focus::Panes).top, 0);
    }

    #[test]
    fn a_press_on_a_bar_the_last_frame_did_not_draw_does_nothing() {
        let mut app = App::new(None);
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 6), on_bar(Focus::Panes));
        assert_eq!(app.dragging, None);
        assert_eq!(app.scroll_of(Focus::Panes), Scroll::default());
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (0, 100))],
        );
        // The next frame draws no bar, as when the list shrinks to fit.
        app.settle(&[(Focus::Panes, 0)], None, &[]);
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 6), on_bar(Focus::Panes));
        assert_eq!(app.dragging, None);
    }

    #[test]
    fn a_bar_shows_while_its_list_scrolls_and_fades_out_after() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut app = App::new(None);
        assert_eq!(app.bar_shade(Focus::Panes, t0), Shade::Hidden);
        app.now = t0;
        app.scroll_lines(Focus::Panes, 2);
        let shades: Vec<Shade> = [0, 799, 800, 850, 999, 1000]
            .into_iter()
            .map(|at| app.bar_shade(Focus::Panes, t0 + ms(at)))
            .collect();
        assert_eq!(
            shades,
            [
                Shade::Rest,
                Shade::Rest,
                Shade::Fading(1),
                Shade::Fading(2),
                Shade::Fading(4),
                Shade::Hidden,
            ]
        );
        // Another list's bar is unaffected.
        assert_eq!(app.bar_shade(Focus::Sessions, t0), Shade::Hidden);
    }

    #[test]
    fn a_key_that_moves_the_selection_shows_the_bar() {
        let t0 = Instant::now();
        let mut app = App::new(None);
        app.set_panes(three_panes(&["Fix checkout"]));
        app.focus_on(Focus::Panes);
        app.now = t0;
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.bar_shade(Focus::Panes, t0), Shade::Rest);
    }

    #[test]
    fn the_pointer_on_a_bar_lights_it_and_leaving_starts_the_fade() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut app = App::new(None);
        app.now = t0;
        app.on_mouse(
            pointer(MouseEventKind::Moved, 8),
            Under {
                target: Some(Target::Bar(Focus::Panes)),
                size: (100, 40),
                ..Under::default()
            },
        );
        // However long it stays, the bar stays lit.
        assert_eq!(app.bar_shade(Focus::Panes, t0 + ms(60_000)), Shade::Lit);
        let left = t0 + ms(60_000);
        app.now = left;
        app.on_mouse(
            MouseEvent {
                column: 10,
                ..pointer(MouseEventKind::Moved, 8)
            },
            Under {
                size: (100, 40),
                ..Under::default()
            },
        );
        assert_eq!(app.bar_shade(Focus::Panes, left + ms(799)), Shade::Rest);
        assert_eq!(app.bar_shade(Focus::Panes, left + ms(1000)), Shade::Hidden);
    }

    #[test]
    fn a_held_thumb_keeps_the_bar_lit_and_letting_go_starts_the_fade() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut app = App::new(None);
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (0, 100))],
        );
        app.now = t0;
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 4), on_bar(Focus::Panes));
        assert_eq!(app.bar_shade(Focus::Panes, t0 + ms(60_000)), Shade::Lit);
        let released = t0 + ms(60_000);
        app.now = released;
        app.on_mouse(pointer(MouseEventKind::Up(LEFT), 4), Under::default());
        assert_eq!(app.bar_shade(Focus::Panes, released + ms(799)), Shade::Rest);
        assert_eq!(
            app.bar_shade(Focus::Panes, released + ms(1000)),
            Shade::Hidden
        );
    }

    #[test]
    fn the_loop_wakes_where_a_bar_changes_and_not_at_rest() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut app = App::new(None);
        app.now = t0;
        assert_eq!(app.bars_wake(), None);
        app.scroll_lines(Focus::Panes, 2);
        assert_eq!(app.bars_wake(), Some(t0 + ms(800)));
        app.now = t0 + ms(800);
        assert_eq!(app.bars_wake(), Some(t0 + ms(850)));
        app.now = t0 + ms(1000);
        assert_eq!(app.bars_wake(), None);
    }

    #[test]
    fn losing_focus_lets_go_of_a_held_thumb_whose_place_is_asked_already() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut app = shown_app();
        app.settle(
            &[(Focus::Session, 0)],
            None,
            &[track(Focus::Session, 20, (0, 100))],
        );
        app.now = t0;
        app.on_mouse(
            pointer(MouseEventKind::Down(LEFT), 4),
            on_bar(Focus::Session),
        );
        app.found_seek(Some(Seek::Past(List::Activity, 0)));
        // Held, and asked for at once.
        assert_eq!(
            app.take_jump(|_, _| 7),
            Some(Jump {
                list: List::Activity,
                at: 0,
                path: PathBuf::from(TRANSCRIPT),
                ask: 1,
            })
        );
        app.on_blur();
        assert_eq!(app.dragging, None);
        assert_eq!(app.held_thumb(Focus::Session), None);
        assert_eq!(
            app.bar_shade(Focus::Session, t0 + ms(799)),
            Shade::Rest,
            "the bar fades from the moment it was let go of"
        );
        assert_eq!(app.take_jump(|_, _| 7), None);
    }

    #[test]
    fn motion_with_no_button_lets_go_of_a_held_thumb_and_paging_resumes() {
        let mut app = shown_app();
        app.settle(
            &[(Focus::Session, 0)],
            None,
            &[track(Focus::Session, 20, (0, 100))],
        );
        app.on_mouse(
            pointer(MouseEventKind::Down(LEFT), 4),
            on_bar(Focus::Session),
        );
        assert!(app.holds_view());
        // The release was lost; the next report says no button is held.
        app.on_mouse(pointer(MouseEventKind::Moved, 9), on_bar(Focus::Session));
        assert_eq!(app.dragging, None);
        assert!(!app.holds_view());
    }

    /// The transcript of every session the fixtures make.
    const TRANSCRIPT: &str = "/work/shop/session.jsonl";

    /// An app that shows a session, with the Session list's bar drawn.
    fn shown_app() -> App {
        let mut app = App::new(None);
        app.set_panes(vec![pane(
            "block-a",
            "alpha/one",
            Status::Idle,
            &["Fix login"],
        )]);
        app
    }

    /// An app whose thumb was let go of on a place no held entry covers, and
    /// the request for it sent, numbered 1 and read in epoch 7.
    fn asked_app() -> (App, Jump) {
        let mut app = shown_app();
        app.settle(
            &[(Focus::Session, 0)],
            None,
            &[track(Focus::Session, 20, (0, 100))],
        );
        app.now = Instant::now();
        app.on_mouse(
            pointer(MouseEventKind::Down(LEFT), 4),
            on_bar(Focus::Session),
        );
        app.found_seek(Some(Seek::Past(List::Activity, 0)));
        app.on_mouse(pointer(MouseEventKind::Up(LEFT), 4), Under::default());
        let jump = app.take_jump(|_, _| 7).expect("a jump on letting go");
        (app, jump)
    }

    #[test]
    fn a_place_to_ask_for_wakes_the_loop_at_once_until_it_asks() {
        let t0 = Instant::now();
        let mut app = shown_app();
        app.settle(
            &[(Focus::Session, 0)],
            None,
            &[track(Focus::Session, 20, (0, 100))],
        );
        app.now = t0;
        app.on_mouse(
            pointer(MouseEventKind::Down(LEFT), 4),
            on_bar(Focus::Session),
        );
        app.found_seek(Some(Seek::Past(List::Activity, 0)));
        // Held, with no request sent and no drag report to draw.
        assert_eq!(app.jump_wake(), Some(t0));
        app.on_mouse(pointer(MouseEventKind::Up(LEFT), 4), Under::default());
        let later = t0 + Duration::from_millis(5);
        app.now = later;
        assert_eq!(app.jump_wake(), Some(later));
        // A drag report waits for its frame, which comes first.
        app.scroll_mut(Focus::Session).seek = Some(3);
        assert_eq!(app.jump_wake(), None);
        app.scroll_mut(Focus::Session).seek = None;
        // The request sent waits for its answer.
        assert!(app.take_jump(|_, _| 0).is_some());
        assert_eq!(app.jump_wake(), None);
    }

    #[test]
    fn an_answer_for_another_transcript_never_lands() {
        let (mut app, jump) = asked_app();
        let other = Path::new("/work/shop/other.jsonl");
        assert!(app.expects(&jump.path, List::Activity, jump.ask, 7));
        assert!(!app.expects(other, List::Activity, jump.ask, 7));
        assert!(!app.awaits(other, List::Activity, jump.ask));
        // It is no answer to wait for, and gives nothing up.
        assert!(!app.answered(other, List::Activity, jump.ask, Some(7)));
        assert!(app.holds_view());
        assert!(app.answered(&jump.path, List::Activity, jump.ask, Some(7)));
    }

    #[test]
    fn an_answer_awaited_but_not_landable_gives_up_the_place() {
        // Read in another generation, as after a rewrite of the transcript.
        let (mut app, jump) = asked_app();
        assert!(!app.answered(&jump.path, List::Activity, jump.ask, Some(8)));
        assert!(!app.holds_view());
        // No entries held for the transcript at all.
        let (mut app, jump) = asked_app();
        assert!(!app.answered(&jump.path, List::Activity, jump.ask, None));
        assert!(!app.holds_view());
    }

    #[test]
    fn the_answer_to_a_request_stays_awaited_after_ctrl_f_and_gives_the_place_up() {
        let (mut app, jump) = asked_app();
        app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert_eq!(app.focus, Focus::Search);
        assert!(app.holds_view());
        // The answer comes in a generation the window left: the paging that
        // the place held back resumes.
        assert!(!app.answered(&jump.path, List::Activity, jump.ask, Some(8)));
        assert!(!app.holds_view());
    }

    #[test]
    fn a_place_belongs_to_the_transcript_the_frame_found_it_in() {
        let (_, jump) = asked_app();
        assert_eq!(jump.path, Path::new(TRANSCRIPT));
    }

    #[test]
    fn entries_all_held_already_leave_the_view_where_it_is() {
        let (mut app, _) = asked_app();
        let place = EntryId {
            line: 100,
            index: 0,
        };
        app.arrive(place, 0);
        assert!(!app.holds_view());
        assert_eq!(app.scroll_of(Focus::Session).anchor, None);
        let (mut app, _) = asked_app();
        app.arrive(place, 3);
        assert!(!app.holds_view());
        assert_eq!(
            app.scroll_of(Focus::Session).anchor.map(|anchor| anchor.id),
            Some(place)
        );
    }

    #[test]
    fn a_thumb_that_moves_with_no_input_glides_to_its_row_and_input_moves_it_at_once() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        let mut app = App::new(None);
        app.now = t0;
        // The thumb stands on row 0, then its list's counts change under it.
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (0, 100))],
        );
        assert_eq!(app.thumb_row(Focus::Panes, 0, t0), 0);
        // The frame that draws the new row keeps the thumb where it was.
        assert_eq!(app.thumb_row(Focus::Panes, 8, t0), 0);
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (40, 100))],
        );
        let at = |app: &App, after: u64| app.thumb_row(Focus::Panes, 8, t0 + ms(after));
        // A key that leaves the thumb where it was does not stop the glide.
        app.on_key(key(KeyCode::Char('x')));
        assert_eq!(at(&app, 50), 4);
        app.touched = false;
        assert_eq!(
            [
                at(&app, 0),
                at(&app, 25),
                at(&app, 50),
                at(&app, 99),
                at(&app, 100)
            ],
            [0, 2, 4, 7, 8]
        );
        assert_eq!(app.bars_wake(), Some(t0 + ms(16)));
        app.now = t0 + ms(100);
        assert_eq!(app.bars_wake(), None);
        // Input that moves the list moves the thumb with it, at once.
        app.now = t0 + ms(200);
        app.scroll_lines(Focus::Panes, 20);
        assert_eq!(app.thumb_row(Focus::Panes, 12, app.now), 12);
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (60, 100))],
        );
        assert_eq!(app.thumb_row(Focus::Panes, 12, app.now), 12);
    }

    #[test]
    fn a_frame_that_draws_no_session_gives_up_a_drag_report_waiting_for_one() {
        let mut app = shown_app();
        app.scroll_mut(Focus::Session).seek = Some(3);
        assert!(app.seeking());
        app.settle(&[(Focus::Panes, 0)], None, &[]);
        assert!(!app.seeking());
        app.scroll_mut(Focus::Session).seek = Some(3);
        app.settle(&[(Focus::Session, 0)], None, &[]);
        assert!(!app.seeking());
    }

    #[test]
    fn a_held_thumb_stays_under_the_pointer_while_the_counts_change() {
        let t0 = Instant::now();
        let mut app = App::new(None);
        app.now = t0;
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (0, 100))],
        );
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 4), on_bar(Focus::Panes));
        app.on_mouse(pointer(MouseEventKind::Drag(LEFT), 9), Under::default());
        assert_eq!(app.thumb_row(Focus::Panes, 0, t0), 5);
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 20, (40, 100))],
        );
        assert_eq!(app.thumb_row(Focus::Panes, 8, t0), 5);
    }

    #[test]
    fn a_press_on_a_track_with_no_thumb_does_nothing() {
        let mut app = App::new(None);
        // A track of one row has no thumb.
        app.settle(
            &[(Focus::Panes, 0)],
            None,
            &[track(Focus::Panes, 1, (0, 100))],
        );
        app.on_mouse(pointer(MouseEventKind::Down(LEFT), 4), on_bar(Focus::Panes));
        assert_eq!(app.held_thumb(Focus::Panes), None);
        assert_eq!(app.scroll_of(Focus::Panes).top, 0);
        assert_eq!(app.take_cleared(), []);
        assert_eq!(app.bar_shade(Focus::Panes, Instant::now()), Shade::Hidden);
    }

    #[test]
    fn a_bar_the_pointer_leaves_fades_whichever_way_it_goes() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        let onto = Under {
            target: Some(Target::Bar(Focus::Panes)),
            size: (100, 40),
            ..Under::default()
        };
        let off = Under {
            size: (100, 40),
            ..Under::default()
        };
        let bar = |app: &mut App| {
            app.settle(
                &[(Focus::Panes, 0)],
                None,
                &[track(Focus::Panes, 20, (0, 100))],
            );
            app.now = t0;
            app.on_mouse(pointer(MouseEventKind::Moved, 8), onto);
            assert_eq!(app.bar_shade(Focus::Panes, t0), Shade::Lit);
        };
        // The window loses focus.
        let mut app = App::new(None);
        bar(&mut app);
        app.on_blur();
        assert_eq!(app.bar_shade(Focus::Panes, t0 + ms(799)), Shade::Rest);
        assert_eq!(app.bar_shade(Focus::Panes, t0 + ms(1000)), Shade::Hidden);
        // A release over another target.
        let mut app = App::new(None);
        bar(&mut app);
        app.on_mouse(pointer(MouseEventKind::Up(LEFT), 8), off);
        assert_eq!(app.bar_shade(Focus::Panes, t0 + ms(799)), Shade::Rest);
        assert_eq!(app.bar_shade(Focus::Panes, t0 + ms(1000)), Shade::Hidden);
    }
}

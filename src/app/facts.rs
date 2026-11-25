//! What a session's transcript says about it beyond its title: the model,
//! the branch, the pull requests it opened, how long it ran, how many turns
//! and tokens it took, and the last thing the user asked.
//!
//! The daemon reads a transcript from its newest line back, a part at a
//! time, so the newest facts show at once and the older lines fill in the
//! counts behind them. It keeps each reading by path, so moving the
//! selection back goes on where the reading stopped.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::{env, fs, mem};

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::session;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Facts {
    pub(crate) model: Option<String>,
    pub(crate) branch: Option<String>,
    /// The pull requests the session opened, in that order.
    pub(crate) prs: Vec<Pr>,
    /// The first and last timestamps, as the transcript writes them.
    pub(crate) started: Option<String>,
    pub(crate) last: Option<String>,
    pub(crate) prompts: usize,
    pub(crate) replies: usize,
    pub(crate) output_tokens: u64,
    /// The last prompt the user typed, on one line.
    pub(crate) last_prompt: Option<String>,
    /// How many sub-agents the session started.
    pub(crate) agents: usize,
    /// The session's task list as it stands, from its task and to-do calls.
    pub(crate) tasks: Vec<Task>,
    /// The files it changed, through its edit tools or a shell command
    /// that plainly writes them, the most recent last, with how often.
    pub(crate) files: Vec<(String, usize)>,
    /// The files it read, the most recent last, with how often.
    #[serde(default)]
    pub(crate) read: Vec<(String, usize)>,
    /// The conversation, oldest first, the last [`KEEP`] of it: what the
    /// user typed, what the session was sent, and Claude's replies.
    pub(crate) log: Vec<Said>,
    /// The tools Claude called, oldest first, the last [`KEEP`] of them.
    pub(crate) activity: Vec<Said>,
    /// How many entries the conversation has in the lines read so far, past
    /// the [`KEEP`] it keeps.
    pub(crate) log_total: usize,
    /// How many tool calls the lines read so far hold.
    pub(crate) activity_total: usize,
    /// Whether every line of the transcript is read. The daemon reads from
    /// the newest line back, so until then the counts, the start and the
    /// task list cover only the newest part.
    #[serde(default)]
    pub(crate) whole: bool,
}

/// How many prompts and tool calls the facts keep.
pub(crate) const KEEP: usize = 300;

/// One of the two lists of entries a transcript gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum List {
    /// The conversation: what the user typed and Claude's replies.
    Log,
    /// The tools Claude called.
    Activity,
}

impl List {
    /// Both lists, in the order a window keeps them.
    pub(crate) const BOTH: [Self; 2] = [Self::Log, Self::Activity];

    /// This list's place in an array of both.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Log => 0,
            Self::Activity => 1,
        }
    }
}

impl Facts {
    /// The entries of `list` the facts keep, to change.
    pub(crate) const fn entries_mut(&mut self, list: List) -> &mut Vec<Said> {
        match list {
            List::Log => &mut self.log,
            List::Activity => &mut self.activity,
        }
    }

    /// The entries of `list` the facts keep.
    pub(crate) fn entries(&self, list: List) -> &[Said] {
        match list {
            List::Log => &self.log,
            List::Activity => &self.activity,
        }
    }

    /// How many entries `list` has in the lines read so far.
    pub(crate) const fn total(&self, list: List) -> usize {
        match list {
            List::Log => self.log_total,
            List::Activity => self.activity_total,
        }
    }

    /// Whether `list` may have entries older than the ones kept: the lines
    /// are not all read, or they gave more than were kept.
    pub(crate) fn older(&self, list: List) -> bool {
        !self.whole || self.total(list) > self.entries(list).len()
    }
}

/// One task of a session's list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Task {
    pub(crate) id: String,
    pub(crate) subject: String,
    /// `pending`, `in_progress` or `completed`.
    pub(crate) status: String,
}

/// An entry of the conversation with when it was written: something the
/// user typed or the session was sent, or a tool call. `text` is what
/// Claude Code shows after the entry's mark, on one line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Said {
    /// Where the entry sits in its transcript; the same entry has the same id
    /// however its line is read.
    pub(crate) id: EntryId,
    pub(crate) at: Option<String>,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) kind: Kind,
    /// The longer text the entry opens to, when it has one: a message's
    /// body, a sub-agent's prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) more: Option<String>,
    /// A tool call's ID, which its result names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) call: Option<String>,
    /// What a tool call gave back, once its result is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<Outcome>,
}

/// Where an entry sits in its transcript: the byte offset of its line, and its
/// place among the entries that line gives. It is unique within one
/// transcript, so a view keeps its place by it as the entries around it change.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, Deserialize,
)]
pub(crate) struct EntryId {
    pub(crate) line: u64,
    pub(crate) index: u16,
}

/// The ids of the entries one transcript line gives, in order.
struct Place {
    line: u64,
    next: u16,
}

impl Place {
    fn take(&mut self) -> EntryId {
        let id = EntryId {
            line: self.line,
            index: self.next,
        };
        self.next = self.next.saturating_add(1);
        id
    }
}

/// What a tool call gave back: its text, cut to [`RESULT_CHARS`], and
/// whether the tool reported an error.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Outcome {
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) error: bool,
}

/// How many characters of a tool's result the facts keep.
const RESULT_CHARS: usize = 2000;

/// What an entry is, which sets the mark Claude Code draws before it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Kind {
    /// A prompt: `❯ text`.
    #[default]
    Prompt,
    /// A slash command as typed: `❯ /name args`, or `❯ Skill(name)`.
    Command,
    /// A shell command the user ran with `!`.
    Shell,
    /// A note for memory, typed after `#`.
    Memory,
    /// A background task's end: `⏺ summary`, coloured by its status.
    Ended { status: String },
    /// A message from a teammate or another session: `Message from @name`.
    Message { from: String },
    /// A teammate gone idle: `⏺ @name finished`, coloured by how.
    Idle { from: String, how: String },
    /// A message relayed from a channel: `← source: text`.
    Relayed { from: String },
    /// The user stopped Claude mid-turn.
    Interrupted,
    /// What Claude wrote back: `⏺ text`.
    Reply,
    /// A tool call: `Name(text)`, the name as Claude Code shows it.
    Tool { name: String },
}

impl Kind {
    /// Whether the user typed the entry, so it counts as a prompt.
    pub(crate) fn typed(&self) -> bool {
        matches!(
            self,
            Self::Prompt | Self::Command | Self::Shell | Self::Memory
        )
    }
}

impl Said {
    /// What a filter matches the entry on.
    pub(crate) fn searched(&self) -> Cow<'_, str> {
        match &self.kind {
            Kind::Tool { name }
            | Kind::Message { from: name }
            | Kind::Idle { from: name, .. }
            | Kind::Relayed { from: name } => Cow::Owned(format!("{name} {}", self.text)),
            Kind::Prompt
            | Kind::Command
            | Kind::Shell
            | Kind::Memory
            | Kind::Ended { .. }
            | Kind::Interrupted
            | Kind::Reply => Cow::Borrowed(&self.text),
        }
    }
}

/// A pull request a session opened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Pr {
    /// `owner/repo`.
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) url: String,
}

/// The fields of a transcript line the facts use; every other is skipped
/// without being built, and a message's content stays unparsed until the
/// line's kind needs it.
#[derive(Debug, Deserialize)]
struct Record<'line> {
    #[serde(rename = "type", borrow, default)]
    kind: Cow<'line, str>,
    #[serde(borrow, default)]
    timestamp: Option<Cow<'line, str>>,
    #[serde(rename = "gitBranch", borrow, default)]
    git_branch: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    cwd: Option<Cow<'line, str>>,
    #[serde(rename = "isMeta", default)]
    is_meta: bool,
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
    #[serde(rename = "isCompactSummary", default)]
    summary: bool,
    #[serde(borrow, default)]
    message: Option<Message<'line>>,
    #[serde(rename = "prRepository", default)]
    pr_repository: Option<String>,
    #[serde(rename = "prNumber", default)]
    pr_number: Option<u64>,
    #[serde(rename = "prUrl", default)]
    pr_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Message<'line> {
    #[serde(borrow, default)]
    model: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    content: Option<&'line RawValue>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    output_tokens: u64,
}

/// One part of a message's content, as far as the facts look.
#[derive(Debug, Deserialize)]
struct Part<'line> {
    #[serde(rename = "type", borrow, default)]
    kind: Cow<'line, str>,
    #[serde(borrow, default)]
    text: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    name: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    input: Option<&'line RawValue>,
    /// A tool call's ID, which its result names.
    #[serde(borrow, default)]
    id: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    tool_use_id: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    content: Option<&'line RawValue>,
    #[serde(default)]
    is_error: bool,
}

/// The fields of a tool call's input that the facts read; every tool
/// fills a few of them.
#[derive(Debug, Default, Deserialize)]
struct Input<'line> {
    #[serde(borrow, default)]
    subject: Option<Cow<'line, str>>,
    #[serde(rename = "taskId", default)]
    task_id: Option<serde_json::Value>,
    #[serde(borrow, default)]
    status: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    file_path: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    command: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    description: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    pattern: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    skill: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    todos: Option<Vec<Todo<'line>>>,
    #[serde(borrow, default)]
    old_string: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    notebook_path: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    path: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    pages: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    url: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    query: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    subagent_type: Option<Cow<'line, str>>,
    #[serde(borrow, default)]
    to: Option<Cow<'line, str>>,
    #[serde(default)]
    message: Option<serde_json::Value>,
    #[serde(borrow, default)]
    prompt: Option<Cow<'line, str>>,
}

#[derive(Debug, Deserialize)]
struct Todo<'line> {
    #[serde(borrow, default)]
    content: Cow<'line, str>,
    #[serde(borrow, default)]
    status: Cow<'line, str>,
}

/// How much of a transcript one step back reads: a few milliseconds of
/// parsing, so a newer request waits no longer than that.
const PART: u64 = 1 << 18;

/// A transcript being read from its newest line back to its first, and
/// forward as it grows. Every step leaves facts to show: the newest lines
/// come first, and the older ones fill in the counts behind them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Reading {
    /// The file's length when it was last looked at.
    len: u64,
    /// Where the unread older part ends: every whole line from here on is
    /// read.
    back: u64,
    /// The offset after the last whole line read.
    end: u64,
    /// Whether the reading has found its first end.
    started: bool,
    stretch: Stretch,
}

impl Reading {
    /// Catches up with lines written since the last look. Returns `None`
    /// when the file cannot be read, and whether anything changed.
    pub(crate) fn catch_up(&mut self, path: &Path) -> Option<bool> {
        let len = fs::metadata(path).ok()?.len();
        // A transcript only grows at its end; one that shrank was written
        // again, and is read again.
        if len < self.len {
            *self = Self::default();
        }
        if self.started && len == self.len {
            return Some(false);
        }
        let mut file = fs::File::open(path).ok()?;
        if self.started {
            let (newer, end) = read_forward(&mut file, self.end)?;
            self.stretch.then(newer);
            self.end = end;
        } else {
            self.end = last_line_end(&mut file, len)?;
            self.back = self.end;
            self.started = true;
        }
        self.len = len;
        Some(true)
    }

    /// Reads the part of the transcript just before what is read. Returns
    /// `None` when the file cannot be read.
    pub(crate) fn step_back(&mut self, path: &Path) -> Option<()> {
        let mut file = fs::File::open(path).ok()?;
        let mut size = PART;
        loop {
            let from = self.back.saturating_sub(size);
            let bytes = read_at(&mut file, from, self.back)?;
            // The part's first bytes end a line that starts before it; a
            // line longer than the part takes a larger one.
            let start = if from == 0 {
                0
            } else if let Some(at) = bytes
                .get(..bytes.len().saturating_sub(1))
                .and_then(|head| head.iter().position(|byte| *byte == b'\n'))
            {
                at + 1
            } else {
                size = size.saturating_mul(2);
                continue;
            };
            let mut older = read_lines(
                bytes.get(start..).unwrap_or_default(),
                from + u64::try_from(start).ok()?,
            );
            older.then(mem::take(&mut self.stretch));
            self.stretch = older;
            self.back = from + u64::try_from(start).ok()?;
            return Some(());
        }
    }

    /// Whether any line is read.
    pub(crate) fn begun(&self) -> bool {
        self.back < self.end
    }

    /// Whether every line is read.
    pub(crate) fn whole(&self) -> bool {
        self.started && self.back == 0
    }

    /// The facts the lines read so far give.
    pub(crate) fn facts(&self) -> Facts {
        let mut facts = self.stretch.facts.clone();
        facts.tasks = tasks(&self.stretch.ops);
        facts.whole = self.whole();
        facts
    }
}

/// How far past a page's last line the daemon reads for the results of the
/// page's calls.
const AHEAD: u64 = 256 << 10;

/// How many bytes a page reads before it stops, once it holds an entry: a list
/// whose entries lie far apart gives a short page and `more`, and the next page
/// goes on from it. A page with no entry reads on until it finds one.
const PAGE_BYTES: u64 = 4 << 20;

/// The longest line read for the results of a page's calls.
const LINE_MOST: u64 = 4 << 20;

/// Which way a page reads from the entry it starts at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Way {
    /// The entries just before it.
    Older,
    /// The entries just after it.
    Newer,
}

/// Some entries of one list, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) entries: Vec<Said>,
    /// Whether the transcript has more entries of the list beyond the page, in
    /// the way it was read. It is `false` for an older page only once the first
    /// line is read.
    pub(crate) more: bool,
}

/// Up to `count` (at most [`KEEP`]) entries of `list` just before or after the
/// entry `at`, read from the transcript at `path` and kept nowhere. The ids are
/// those the whole transcript's reading gives the same lines. A tool call's
/// result written after the page shows, when it is within [`AHEAD`] of it.
/// An error when the transcript cannot be opened or read.
pub(crate) fn page(
    path: &Path,
    way: Way,
    list: List,
    at: EntryId,
    count: usize,
) -> io::Result<Page> {
    let count = count.clamp(1, KEEP);
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    match way {
        Way::Older => older(&mut file, path, len, list, at, count),
        Way::Newer => newer(&mut file, len, list, at, count),
    }
    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a line could not be read"))
}

/// The entries of `list` in `stretch`.
fn list_of(stretch: &mut Stretch, list: List) -> &mut Vec<Said> {
    match list {
        List::Log => &mut stretch.facts.log,
        List::Activity => &mut stretch.facts.activity,
    }
}

fn older(
    file: &mut fs::File,
    path: &Path,
    len: u64,
    list: List,
    before: EntryId,
    count: usize,
) -> Option<Page> {
    // The line `before` is in gives the entries before it on that line too.
    let mut here = read_lines(&line_at(file, before.line)?, before.line);
    here.facts.log.retain(|said| said.id < before);
    here.facts.activity.retain(|said| said.id < before);
    here.facts.log_total = here.facts.log.len();
    here.facts.activity_total = here.facts.activity.len();
    let mut reading = Reading {
        len,
        back: before.line,
        end: before.line,
        started: true,
        stretch: here,
    };
    loop {
        let found = list_of(&mut reading.stretch, list).len();
        let read = before.line - reading.back;
        if reading.back == 0 || found >= count || (found > 0 && read >= PAGE_BYTES) {
            break;
        }
        reading.step_back(path)?;
    }
    let total = reading.stretch.facts.total(list);
    let mut entries = mem::take(list_of(&mut reading.stretch, list));
    entries.drain(..entries.len().saturating_sub(count));
    if list == List::Activity {
        reading.stretch.facts.activity = entries;
        results_after(file, before.line, len, &mut reading.stretch)?;
        entries = mem::take(&mut reading.stretch.facts.activity);
    }
    Some(Page {
        entries,
        more: reading.back > 0 || total > count,
    })
}

fn newer(file: &mut fs::File, len: u64, list: List, after: EntryId, count: usize) -> Option<Page> {
    file.seek(SeekFrom::Start(after.line)).ok()?;
    let mut lines = BufReader::with_capacity(1 << 18, &mut *file);
    let mut line = Vec::new();
    let mut stretch = Stretch::default();
    let mut at = after.line;
    // Where the lines read so far end, and how many entries of the list are
    // after `after`.
    let mut end = at;
    let mut found = 0;
    while found < count && (found == 0 || end - after.line < PAGE_BYTES) {
        line.clear();
        let read = lines.read_until(b'\n', &mut line).ok()?;
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        if let Ok(record) = serde_json::from_slice::<Record<'_>>(&line) {
            add(&mut stretch, record, at);
        }
        at += u64::try_from(read).ok()?;
        end = at;
        list_of(&mut stretch, list).retain(|said| said.id > after);
        found = list_of(&mut stretch, list).len();
    }
    drop(lines);
    let entries = list_of(&mut stretch, list);
    let more = found > count || end < len;
    entries.truncate(count);
    if list == List::Activity {
        results_after(file, end, len, &mut stretch)?;
    }
    Some(Page {
        entries: mem::take(list_of(&mut stretch, list)),
        more,
    })
}

/// The bytes of the line that starts at `at`, with its line break.
fn line_at(file: &mut fs::File, at: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(at)).ok()?;
    let mut line = Vec::new();
    BufReader::new(file).read_until(b'\n', &mut line).ok()?;
    (line.last() == Some(&b'\n')).then_some(line)
}

/// Gives the calls of `stretch`'s activity the results written from byte `from`
/// on, up to [`AHEAD`] of it and up to `len`, until every call has one.
fn results_after(file: &mut fs::File, from: u64, len: u64, stretch: &mut Stretch) -> Option<()> {
    let stop = len.min(from.saturating_add(AHEAD));
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut lines = BufReader::with_capacity(1 << 18, file);
    let mut line = Vec::new();
    let mut at = from;
    while at < stop
        && stretch
            .facts
            .activity
            .iter()
            .any(|said| said.result.is_none())
    {
        line.clear();
        // A line longer than the most is not read whole.
        let read = lines
            .by_ref()
            .take(LINE_MOST)
            .read_until(b'\n', &mut line)
            .ok()?;
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        at += u64::try_from(read).ok()?;
        if let Ok(record) = serde_json::from_slice::<Record<'_>>(&line)
            && record.kind == "user"
            && !record.is_meta
            && !record.summary
            && !record.sidechain
            && let Some(content) = record.message.and_then(|message| message.content)
        {
            results(stretch, content);
        }
    }
    Some(())
}

/// The task list Claude Code keeps for the session whose transcript is
/// at `transcript`, one file per task under `~/.claude/tasks`: in the
/// folder of the session's team, `session-` and the first 8 characters
/// of its ID, else in the folder named by the ID, following the note a
/// cleared session leaves of the list it moved to. `None` when the
/// session keeps no list there, and its transcript's calls tell instead.
pub(crate) fn stored_tasks(transcript: &Path) -> Option<Vec<Task>> {
    let id = transcript.file_stem()?.to_str()?;
    let root = session::claude_dir().ok()?.join("tasks");
    let mut folders = vec![
        root.join(format!("session-{}", id.get(..8)?)),
        root.join(id),
    ];
    // A cleared session's folder names the list it followed after.
    if let Some(note) = folders
        .iter()
        .find_map(|folder| fs::read_to_string(folder.join(".session-list")).ok())
    {
        let note: String = note.trim().chars().take(64).collect();
        let name: String = note
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if !name.is_empty() {
            folders.insert(0, root.join(name));
        }
    }
    let folder = folders.into_iter().find(|folder| folder.is_dir())?;
    let mut tasks: Vec<Task> = fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| serde_json::from_slice::<Task>(&fs::read(entry.path()).ok()?).ok())
        .collect();
    tasks.sort_by_key(|task| (task.id.parse::<u64>().unwrap_or(u64::MAX), task.id.clone()));
    Some(tasks)
}

/// What a run of consecutive lines gives. The task calls stay as they
/// came: an update may name a task an older run created.
#[derive(Debug, Clone, Default)]
struct Stretch {
    facts: Facts,
    ops: Vec<TaskOp>,
    /// Results whose calls are in an older stretch, by call ID.
    results: Vec<(String, Outcome)>,
}

#[derive(Debug, Clone)]
enum TaskOp {
    Create(String),
    Update {
        id: String,
        status: Option<String>,
        subject: Option<String>,
    },
    /// The older to-do tool sends the whole list each time.
    List(Vec<Task>),
}

impl Stretch {
    /// Adds the run of lines that follows this one.
    fn then(&mut self, later: Self) {
        let (facts, newer) = (&mut self.facts, later.facts);
        facts.model = newer.model.or(facts.model.take());
        facts.branch = newer.branch.or(facts.branch.take());
        for pr in newer.prs {
            if !facts.prs.contains(&pr) {
                facts.prs.push(pr);
            }
        }
        facts.started = facts.started.take().or(newer.started);
        facts.last = newer.last.or(facts.last.take());
        facts.prompts += newer.prompts;
        facts.replies += newer.replies;
        facts.output_tokens += newer.output_tokens;
        facts.last_prompt = newer.last_prompt.or(facts.last_prompt.take());
        facts.agents += newer.agents;
        for (path, count) in newer.files {
            touch(&mut facts.files, &path, count);
        }
        for (path, count) in newer.read {
            touch(&mut facts.read, &path, count);
        }
        facts.log_total += newer.log_total;
        facts.activity_total += newer.activity_total;
        facts.log.extend(newer.log);
        trim(&mut facts.log);
        // Results that reached this stretch's calls from the newer one.
        for (call, outcome) in later.results {
            match facts
                .activity
                .iter_mut()
                .rev()
                .find(|said| said.call.as_deref() == Some(call.as_str()))
            {
                Some(said) => said.result = Some(outcome),
                None => self.results.push((call, outcome)),
            }
        }
        facts.activity.extend(newer.activity);
        trim(&mut facts.activity);
        self.ops.extend(later.ops);
    }
}

/// How many files the facts keep in each of their lists: more than the
/// entries, since a session's scratch files would push its own out.
const FILES: usize = 1000;

/// Counts `count` more uses of `path` in `list`, which moves it to the
/// end, as the most recent, keeping the last [`FILES`].
fn touch(list: &mut Vec<(String, usize)>, path: &str, count: usize) {
    let before = list
        .iter()
        .position(|(file, _)| file == path)
        .map_or(0, |index| list.remove(index).1);
    list.push((path.to_owned(), before + count));
    if list.len() > FILES {
        list.drain(..list.len() - FILES);
    }
}

/// Keeps the last [`KEEP`] of `list`.
fn trim<T>(list: &mut Vec<T>) {
    if list.len() > KEEP {
        list.drain(..list.len() - KEEP);
    }
}

/// The task list the task calls leave, oldest call first.
fn tasks(ops: &[TaskOp]) -> Vec<Task> {
    let mut tasks: Vec<Task> = Vec::new();
    for op in ops {
        match op {
            TaskOp::Create(subject) => {
                let id = tasks
                    .iter()
                    .filter_map(|task| task.id.parse::<u64>().ok())
                    .max()
                    .map_or(1, |most| most + 1)
                    .to_string();
                tasks.push(Task {
                    id,
                    subject: subject.clone(),
                    status: "pending".to_owned(),
                });
            }
            TaskOp::Update {
                id,
                status,
                subject,
            } => {
                if status.as_deref() == Some("deleted") {
                    tasks.retain(|task| task.id != *id);
                } else if let Some(task) = tasks.iter_mut().find(|task| task.id == *id) {
                    if let Some(status) = status {
                        status.clone_into(&mut task.status);
                    }
                    if let Some(subject) = subject {
                        subject.clone_into(&mut task.subject);
                    }
                }
            }
            TaskOp::List(list) => list.clone_into(&mut tasks),
        }
    }
    tasks
}

/// The bytes of `file` from `from` to `to`.
fn read_at(file: &mut fs::File, from: u64, to: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = vec![0; usize::try_from(to - from).ok()?];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// The offset after the last whole line of `file`, which is `len` long; a
/// line still being written waits for the next look.
fn last_line_end(file: &mut fs::File, len: u64) -> Option<u64> {
    let mut size = PART;
    loop {
        let from = len.saturating_sub(size);
        let bytes = read_at(file, from, len)?;
        if let Some(at) = bytes.iter().rposition(|byte| *byte == b'\n') {
            return Some(from + u64::try_from(at).ok()? + 1);
        }
        if from == 0 {
            return Some(0);
        }
        size = size.saturating_mul(2);
    }
}

/// What the whole lines of `file` from `offset` on give, with the offset
/// after the last of them.
fn read_forward(file: &mut fs::File, offset: u64) -> Option<(Stretch, u64)> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut lines = BufReader::with_capacity(1 << 18, file);
    let mut line = Vec::new();
    let mut stretch = Stretch::default();
    let mut at = offset;
    loop {
        line.clear();
        let read = lines.read_until(b'\n', &mut line).ok()?;
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        let line_at = at;
        at += u64::try_from(read).ok()?;
        if let Ok(record) = serde_json::from_slice::<Record<'_>>(&line) {
            add(&mut stretch, record, line_at);
        }
    }
    Some((stretch, at))
}

/// What a run of whole lines gives, the first of them starting at byte `base`
/// of the transcript.
fn read_lines(bytes: &[u8], base: u64) -> Stretch {
    let mut stretch = Stretch::default();
    let mut at = base;
    for line in bytes.split(|byte| *byte == b'\n') {
        if let Ok(record) = serde_json::from_slice::<Record<'_>>(line) {
            add(&mut stretch, record, at);
        }
        at += u64::try_from(line.len()).unwrap_or(0) + 1;
    }
    stretch
}

/// Adds what `record`, the transcript line that starts at byte `line`, gives.
fn add(stretch: &mut Stretch, record: Record<'_>, line: u64) {
    let mut place = Place { line, next: 0 };
    if record.sidechain {
        return;
    }
    let facts = &mut stretch.facts;
    if let Some(at) = record.timestamp.as_deref() {
        if facts.started.is_none() {
            facts.started = Some(at.to_owned());
        }
        facts.last = Some(at.to_owned());
    }
    if let Some(branch) = record.git_branch.filter(|branch| !branch.is_empty()) {
        facts.branch = Some(branch.into_owned());
    }
    match record.kind.as_ref() {
        "pr-link" => {
            if let (Some(repo), Some(number), Some(url)) =
                (record.pr_repository, record.pr_number, record.pr_url)
            {
                let pr = Pr { repo, number, url };
                if !facts.prs.contains(&pr) {
                    facts.prs.push(pr);
                }
            }
        }
        "assistant" => {
            let Some(message) = record.message else {
                return;
            };
            if let Some(model) = message.model.filter(|model| !model.starts_with('<')) {
                facts.model = Some(model.into_owned());
            }
            facts.output_tokens += message.usage.map_or(0, |usage| usage.output_tokens);
            facts.replies += 1;
            let at = record.timestamp.as_deref();
            if let Some(parts) = message
                .content
                .and_then(|content| serde_json::from_str::<Vec<Part<'_>>>(content.get()).ok())
            {
                let cwd = record.cwd.as_deref();
                for part in &parts {
                    match part.kind.as_ref() {
                        "tool_use" => call(stretch, at, cwd, part, &mut place),
                        // Claude's own words, as its reply rows show them.
                        "text" => {
                            let text = clean(part.text.as_deref().unwrap_or_default());
                            if !text.is_empty() && text != "(no content)" {
                                keep(
                                    &mut stretch.facts.log,
                                    &mut stretch.facts.log_total,
                                    Said {
                                        id: place.take(),
                                        at: at.map(str::to_owned),
                                        text,
                                        kind: Kind::Reply,
                                        more: None,
                                        call: None,
                                        result: None,
                                    },
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        "user" if !record.is_meta && !record.summary => {
            if let Some(content) = record.message.as_ref().and_then(|message| message.content) {
                results(stretch, content);
            }
            let facts = &mut stretch.facts;
            if let Some((kind, text, more)) = record
                .message
                .and_then(|message| message.content)
                .and_then(entry)
            {
                if kind.typed() {
                    facts.prompts += 1;
                    facts.last_prompt = Some(text.clone());
                }
                keep(
                    &mut facts.log,
                    &mut facts.log_total,
                    Said {
                        id: place.take(),
                        at: record.timestamp.as_deref().map(str::to_owned),
                        text,
                        kind,
                        more,
                        call: None,
                        result: None,
                    },
                );
            }
        }
        _ => {}
    }
}

/// What a user line shows, as Claude Code draws it: a prompt, a command,
/// a message the session was sent. Tool results, injected context and
/// the entries Claude Code draws nothing for give `None`.
fn entry(content: &RawValue) -> Option<Shown> {
    let text = if let Ok(text) = serde_json::from_str::<Cow<'_, str>>(content.get()) {
        text.into_owned()
    } else {
        let parts = serde_json::from_str::<Vec<Part<'_>>>(content.get()).ok()?;
        if parts.iter().any(|part| part.kind == "tool_result") {
            return None;
        }
        parts
            .iter()
            .filter_map(|part| part.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (kind, shown, more) = said_as(text.trim())?;
    let shown = clean(&shown);
    (!shown.is_empty()).then(|| {
        (
            kind,
            shown,
            more.map(|more| clean(&more))
                .filter(|more| !more.is_empty()),
        )
    })
}

/// An entry's kind, the text it shows, and the longer text it opens to.
type Shown = (Kind, String, Option<String>);

/// How many characters of an entry's text the facts keep.
const TEXT_CHARS: usize = 4000;

/// `text` with its line breaks, without control characters, cut to
/// [`TEXT_CHARS`].
fn clean(text: &str) -> String {
    let mut out: String = text
        .trim()
        .chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            '\n' => Some('\n'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .take(TEXT_CHARS)
        .collect();
    if text.chars().count() > TEXT_CHARS {
        out.push('…');
    }
    out
}

/// Puts each tool result in `content` with its call, or keeps it for a
/// call an older stretch holds.
fn results(stretch: &mut Stretch, content: &RawValue) {
    let Ok(parts) = serde_json::from_str::<Vec<Part<'_>>>(content.get()) else {
        return;
    };
    for part in parts.iter().filter(|part| part.kind == "tool_result") {
        let Some(call) = part.tool_use_id.as_deref() else {
            continue;
        };
        let text = part.content.map_or_default(result_text);
        let mut text: String = clean(&text).chars().take(RESULT_CHARS).collect();
        if text.is_empty() {
            "(No output)".clone_into(&mut text);
        }
        let outcome = Outcome {
            text,
            error: part.is_error,
        };
        match stretch
            .facts
            .activity
            .iter_mut()
            .rev()
            .find(|said| said.call.as_deref() == Some(call))
        {
            Some(said) => said.result = Some(outcome),
            None => stretch.results.push((call.to_owned(), outcome)),
        }
    }
}

/// A tool result's text: a string, or its text parts one after another,
/// a picture as `[Image]`.
fn result_text(content: &RawValue) -> String {
    if let Ok(text) = serde_json::from_str::<Cow<'_, str>>(content.get()) {
        return text.into_owned();
    }
    serde_json::from_str::<Vec<Part<'_>>>(content.get()).map_or_default(|parts| {
        parts
            .iter()
            .map(|part| match part.kind.as_ref() {
                "image" => "[Image]".to_owned(),
                _ => part.text.as_deref().unwrap_or_default().to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// The lead Claude Code writes before a message from another session.
const PEER_LEAD: &str = "Another Claude session sent a message";

/// The lead before a message relayed from a channel.
const RELAY_LEAD: &str = "A message arrived from ";

/// How Claude Code draws a user line's text, in its order of checks.
fn said_as(text: &str) -> Option<Shown> {
    if text.is_empty() || text == "(no content)" {
        return None;
    }
    if text == "[Request interrupted by user]"
        || text == "[Request interrupted by user for tool use]"
    {
        return Some((Kind::Interrupted, "Interrupted".to_owned(), None));
    }
    // A message's own lead line comes before its tag.
    let body = if text.starts_with(PEER_LEAD) || text.starts_with(RELAY_LEAD) {
        text.split_once('\n')
            .map_or("", |(_, rest)| rest.trim_start())
    } else {
        text
    };
    if body.starts_with("<teammate-message ") {
        let from = attribute(body, "teammate_id")
            .unwrap_or("teammate")
            .to_owned();
        let inner = element(body, "teammate-message").unwrap_or_default();
        if inner.starts_with('{') {
            return teammate_note(from, inner);
        }
        return Some(match attribute(body, "summary") {
            Some(summary) => (
                Kind::Message { from },
                summary.to_owned(),
                Some(inner.to_owned()),
            ),
            None => (Kind::Message { from }, inner.to_owned(), None),
        });
    }
    if body.starts_with("<cross-session-message") {
        let from = attribute(body, "from-name")
            .or_else(|| {
                attribute(body, "from").map(|from| {
                    let name = from.rsplit('/').next().unwrap_or(from);
                    name.strip_suffix(".sock").unwrap_or(name)
                })
            })
            .unwrap_or("peer");
        let what = element(body, "cross-session-message").unwrap_or_default();
        return Some((
            Kind::Message {
                from: from.to_owned(),
            },
            what.to_owned(),
            None,
        ));
    }
    if body.starts_with("<channel source=\"") {
        let source = attribute(body, "source").unwrap_or_default();
        let server = source.rsplit(':').next().unwrap_or(source);
        let from = match attribute(body, "user") {
            Some(user) => format!("{server} · {user}"),
            None => server.to_owned(),
        };
        let what = element(body, "channel").unwrap_or_default();
        return Some((Kind::Relayed { from }, what.to_owned(), None));
    }
    if text.starts_with("<command-name>") || text.starts_with("<command-message>") {
        let name = element(text, "command-name")
            .or_else(|| element(text, "command-message"))?
            .trim_start_matches('/');
        if element(text, "skill-format").is_some() {
            return Some((Kind::Command, format!("Skill({name})"), None));
        }
        let args = element(text, "command-args").unwrap_or_default();
        return Some((Kind::Command, format!("/{name} {args}"), None));
    }
    if text.starts_with("<bash-input>") {
        return Some((Kind::Shell, element(text, "bash-input")?.to_owned(), None));
    }
    if text.contains("<user-memory-input>") {
        return Some((
            Kind::Memory,
            element(text, "user-memory-input")?.to_owned(),
            None,
        ));
    }
    if text.contains("<task-notification") {
        // A tool call's late result shows with the call, not here.
        if element(text, "task-type") == Some("tool_call") {
            return None;
        }
        let summary = element(text, "summary")?;
        let status = element(text, "status").unwrap_or_default().to_owned();
        return Some((Kind::Ended { status }, summary.to_owned(), None));
    }
    // Shell output, command output, ticks, caveats, wakes and the rest of
    // what Claude Code sends as a user line draw nothing here.
    if text.starts_with('<') {
        return None;
    }
    Some((Kind::Prompt, text.to_owned(), None))
}

/// One of the notes Claude Code's teammates send as JSON, as Claude Code
/// draws it; `None` for the ones it draws nothing for.
fn teammate_note(from: String, json: &str) -> Option<Shown> {
    let note: serde_json::Value = serde_json::from_str(json).ok()?;
    let field = |name: &str| {
        note.get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let kind = field("type").unwrap_or_default();
    Some(match kind.as_str() {
        "idle_notification" => {
            let how = match field("idleReason").as_deref() {
                Some("failed") => "failed",
                Some("interrupted") => "was interrupted",
                _ => "finished",
            };
            let result = field("result");
            let text = field("failureReason")
                .or_else(|| field("summary"))
                .or_else(|| {
                    result
                        .as_deref()
                        .and_then(|result| result.lines().next())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            (
                Kind::Idle {
                    from,
                    how: how.to_owned(),
                },
                text,
                result,
            )
        }
        "task_completed" => {
            let id = note
                .get("taskId")
                .map_or_default(|id| id.as_str().map_or_else(|| id.to_string(), str::to_owned));
            let subject = field("taskSubject").map_or_default(|subject| format!(" ({subject})"));
            (
                Kind::Message { from },
                format!("Completed task #{id}{subject}"),
                None,
            )
        }
        "shutdown_approved" => return None,
        "shutdown_request" => (
            Kind::Message { from },
            format!(
                "Shutdown request{}",
                field("reason").map_or_default(|reason| format!(": {reason}"))
            ),
            None,
        ),
        "shutdown_rejected" => (
            Kind::Message { from },
            format!(
                "Shutdown rejected{}",
                field("reason").map_or_default(|reason| format!(": {reason}"))
            ),
            None,
        ),
        "plan_approval_request" => (
            Kind::Message { from },
            "Plan approval request".to_owned(),
            field("planContent"),
        ),
        "plan_approval_response" => {
            let approved = note
                .get("approved")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            (
                Kind::Message { from },
                if approved {
                    "✓ Plan approved"
                } else {
                    "✗ Plan rejected"
                }
                .to_owned(),
                field("feedback"),
            )
        }
        "task_assignment" => (
            Kind::Message { from },
            format!(
                "Task #{} assigned: {}",
                field("taskId").unwrap_or_default(),
                field("subject").unwrap_or_default()
            ),
            field("description"),
        ),
        _ => (
            Kind::Message { from },
            field("content")
                .or_else(|| field("summary"))
                .unwrap_or_else(|| json.to_owned()),
            None,
        ),
    })
}

/// The text inside the first `<name …>…</name>` of `text`.
fn element<'text>(text: &'text str, name: &str) -> Option<&'text str> {
    let open = format!("<{name}");
    let mut from = 0;
    let start = loop {
        let at = from + text.get(from..)?.find(&open)?;
        let after = at + open.len();
        match text.get(after..)?.chars().next() {
            Some('>' | ' ' | '\n' | '\t') => break after,
            _ => from = after,
        }
    };
    let inner = start + text.get(start..)?.find('>')? + 1;
    let end = inner + text.get(inner..)?.find(&format!("</{name}>"))?;
    Some(text.get(inner..end)?.trim())
}

/// The value of the first `name="…"` attribute in `text`.
fn attribute<'text>(text: &'text str, name: &str) -> Option<&'text str> {
    let key = format!(" {name}=\"");
    let start = text.find(&key)? + key.len();
    let end = start + text.get(start..)?.find('"')?;
    text.get(start..end)
}

/// Adds `item` to the end of `list`, keeping the last [`KEEP`], and counts it
/// in `total`.
fn keep(list: &mut Vec<Said>, total: &mut usize, item: Said) {
    *total += 1;
    list.push(item);
    if list.len() > KEEP {
        list.drain(..list.len() - KEEP);
    }
}

/// What one tool call adds: its line in the activity, a task change, a sub-
/// agent, or a file it changed.
fn call(
    stretch: &mut Stretch,
    at: Option<&str>,
    cwd: Option<&str>,
    part: &Part<'_>,
    place: &mut Place,
) {
    let name = part.name.as_deref().unwrap_or("tool");
    let input = part
        .input
        .and_then(|input| serde_json::from_str::<Input<'_>>(input.get()).ok())
        .unwrap_or_default();
    let facts = &mut stretch.facts;
    match name {
        "Agent" | "Task" => facts.agents += 1,
        "TaskCreate" => stretch.ops.push(TaskOp::Create(
            input.subject.as_deref().unwrap_or_default().to_owned(),
        )),
        "TaskUpdate" => stretch.ops.push(TaskOp::Update {
            id: match &input.task_id {
                Some(serde_json::Value::String(id)) => id.clone(),
                Some(serde_json::Value::Number(id)) => id.to_string(),
                _ => String::new(),
            },
            status: input.status.as_deref().map(str::to_owned),
            subject: input.subject.as_deref().map(str::to_owned),
        }),
        "TodoWrite" => {
            if let Some(todos) = &input.todos {
                stretch.ops.push(TaskOp::List(
                    todos
                        .iter()
                        .enumerate()
                        .map(|(index, todo)| Task {
                            id: (index + 1).to_string(),
                            subject: todo.content.as_ref().to_owned(),
                            status: todo.status.as_ref().to_owned(),
                        })
                        .collect(),
                ));
            }
        }
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            if let Some(path) = input
                .file_path
                .as_deref()
                .or(input.notebook_path.as_deref())
            {
                touch(&mut facts.files, path, 1);
            }
        }
        "Read" => {
            if let Some(path) = input.file_path.as_deref() {
                touch(&mut facts.read, path, 1);
            }
        }
        // What a shell command plainly writes counts as changed.
        "Bash" => {
            for path in super::shell::writes(input.command.as_deref().unwrap_or_default(), cwd) {
                touch(&mut facts.files, &path, 1);
            }
        }
        _ => {}
    }
    if let Some((shown, what, more)) = tool_line(name, &input, part.input, cwd) {
        keep(
            &mut facts.activity,
            &mut facts.activity_total,
            Said {
                id: place.take(),
                at: at.map(str::to_owned),
                text: clean(&what),
                kind: Kind::Tool { name: shown },
                more: more
                    .map(|more| clean(&more))
                    .filter(|more| !more.is_empty()),
                call: part.id.as_deref().map(str::to_owned),
                result: None,
            },
        );
    }
}

/// A tool's shown name, the text in its brackets, and what it opens to.
type Shown3 = (String, String, Option<String>);

/// The name Claude Code shows for a tool call and what it writes in the
/// brackets after it; `None` for the calls it draws no row for, such as
/// the task list's.
fn tool_line(
    name: &str,
    input: &Input<'_>,
    raw: Option<&RawValue>,
    cwd: Option<&str>,
) -> Option<Shown3> {
    let path = |path: Option<&Cow<'_, str>>| path.map_or_default(|path| shown_path(path, cwd));
    let (shown, what) = match name {
        "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" => return None,
        "Bash" | "PowerShell" => (
            name.to_owned(),
            input.command.as_deref().unwrap_or_default().to_owned(),
        ),
        "Read" => {
            let mut what = path(input.file_path.as_ref());
            if let Some(pages) = input.pages.as_deref() {
                let _ = write!(what, " · pages {pages}");
            }
            ("Read".to_owned(), what)
        }
        "Edit" | "MultiEdit" => {
            let created = input.old_string.as_deref() == Some("");
            let shown = if created { "Create" } else { "Update" };
            (shown.to_owned(), path(input.file_path.as_ref()))
        }
        "Write" => ("Write".to_owned(), path(input.file_path.as_ref())),
        "NotebookEdit" => (
            "Edit Notebook".to_owned(),
            path(input.notebook_path.as_ref()),
        ),
        "Grep" | "Glob" => {
            let mut what = format!(
                "pattern: \"{}\"",
                input.pattern.as_deref().unwrap_or_default()
            );
            if let Some(dir) = input.path.as_ref() {
                let _ = write!(what, ", path: \"{}\"", shown_path(dir, cwd));
            }
            ("Search".to_owned(), what)
        }
        "Agent" | "Task" => {
            let shown = match input.subagent_type.as_deref() {
                None | Some("" | "general-purpose") => "Agent",
                Some(kind) => kind,
            };
            return Some((
                shown.to_owned(),
                input.description.as_deref().unwrap_or_default().to_owned(),
                input.prompt.as_deref().map(str::to_owned),
            ));
        }
        "WebFetch" => (
            "Fetch".to_owned(),
            input.url.as_deref().unwrap_or_default().to_owned(),
        ),
        "WebSearch" => (
            "Web Search".to_owned(),
            format!("\"{}\"", input.query.as_deref().unwrap_or_default()),
        ),
        "Skill" => (
            "Skill".to_owned(),
            input
                .skill
                .as_deref()
                .unwrap_or_default()
                .trim_start_matches('/')
                .to_owned(),
        ),
        "SendMessage" => {
            let message = match &input.message {
                Some(serde_json::Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            let to = input.to.as_deref().unwrap_or_default();
            return Some(("SendMessage".to_owned(), to.to_owned(), Some(message)));
        }
        _ => match name
            .strip_prefix("mcp__")
            .and_then(|rest| rest.split_once("__"))
        {
            Some((server, tool)) => (format!("{server} - {tool} (MCP)"), fields(raw)),
            None => (
                name.to_owned(),
                input
                    .description
                    .as_deref()
                    .map_or_else(|| fields(raw), str::to_owned),
            ),
        },
    };
    Some((shown, what, None))
}

/// A tool's input as Claude Code writes it for a tool with no row of its
/// own: `key: value` pairs, the values as JSON.
fn fields(raw: Option<&RawValue>) -> String {
    raw.and_then(|raw| {
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(raw.get()).ok()
    })
    .map_or_default(|map| {
        map.iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// `path` as Claude Code shows it: inside the session's folder relative to
/// it, under the home folder from `~`, otherwise whole.
fn shown_path(path: &str, cwd: Option<&str>) -> String {
    if let Some(inside) = cwd
        .and_then(|cwd| path.strip_prefix(cwd))
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !rest.is_empty())
    {
        return inside.to_owned();
    }
    if let Some(home) = env::var_os("HOME").and_then(|home| home.into_string().ok())
        && let Some(rest) = path.strip_prefix(&home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::fs::{self, File, OpenOptions};
    use std::io::{BufRead, BufReader, ErrorKind, Write as _};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use std::{env, process, thread};

    use serde_json::{Value, json};

    use super::{
        AHEAD, EntryId, Facts, KEEP, LINE_MOST, List, Reading, Said, Way, page, read_forward, tasks,
    };
    use crate::daemon::{FromDaemon, ToDaemon};

    /// A transcript of `turns` turns: a prompt, then a reply that creates
    /// a task, edits a file and, every fifth turn, carries a long text, so
    /// some lines are longer than one part.
    fn transcript(turns: usize) -> String {
        let mut text = String::new();
        for turn in 0..turns {
            let at = format!("2026-01-01T00:{:02}:{:02}Z", turn / 60 % 60, turn % 60);
            // The last turn's edit gets its result at the start of this one.
            if turn > 0 {
                let _ = write!(
                    text,
                    r#"{{"type":"user","timestamp":"{at}","message":{{"content":[{{"type":"tool_result","tool_use_id":"e{}","content":"edited {}"}}]}}}}"#,
                    turn - 1,
                    turn - 1
                );
                text.push('\n');
            }
            let _ = write!(
                text,
                r#"{{"type":"user","timestamp":"{at}","gitBranch":"b{}","message":{{"content":"prompt {turn}"}}}}"#,
                turn / 100
            );
            text.push('\n');
            let long = if turn % 5 == 0 {
                "x".repeat(300_000)
            } else {
                String::new()
            };
            let _ = write!(
                text,
                r#"{{"type":"assistant","timestamp":"{at}","message":{{"model":"m{}","usage":{{"output_tokens":{turn}}},"content":[{{"type":"text","text":"{long}"}},{{"type":"tool_use","name":"TaskCreate","input":{{"subject":"task {turn}"}}}},{{"type":"tool_use","name":"TaskUpdate","input":{{"taskId":"{}","status":"completed"}}}},{{"type":"tool_use","id":"e{turn}","name":"Edit","input":{{"file_path":"f{}"}}}}]}}}}"#,
                turn / 50,
                turn / 2 + 1,
                turn % 7
            );
            text.push('\n');
        }
        text
    }

    fn file(name: &str, text: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("rawr-facts-{name}-{}.jsonl", process::id()));
        fs::write(&path, text).expect("write the transcript");
        path
    }

    /// The facts of the whole file, read from its first line on.
    fn forward(path: &PathBuf) -> Facts {
        let mut opened = File::open(path).expect("open the transcript");
        let (stretch, _) = read_forward(&mut opened, 0).expect("read the transcript");
        let mut facts = stretch.facts;
        facts.tasks = tasks(&stretch.ops);
        facts.whole = true;
        facts
    }

    #[test]
    fn reading_back_gives_what_reading_forward_does() {
        let path = file("back", &transcript(400));
        let mut reading = Reading::default();
        assert_eq!(reading.catch_up(&path), Some(true));
        let mut steps = 0;
        while !reading.whole() {
            reading.step_back(&path).expect("step back");
            steps += 1;
        }
        assert!(steps > 1, "the transcript took {steps} parts");
        assert_eq!(reading.facts(), forward(&path));
        // Each edit but the last has its result, though parts split them.
        let results = reading
            .facts()
            .activity
            .iter()
            .filter(|said| said.result.is_some())
            .count();
        assert_eq!(results, KEEP - 1);
        let _ = fs::remove_file(path);
    }

    /// Every entry of the log and the activity, by id.
    fn ids(facts: &Facts) -> Vec<EntryId> {
        facts
            .log
            .iter()
            .chain(&facts.activity)
            .map(|said| said.id)
            .collect()
    }

    #[test]
    fn a_line_read_twice_gives_the_same_ids() {
        let path = file("ids", &transcript(40));
        let first = forward(&path);
        // Read again, from the newest line back in parts, and once more
        // from the first line.
        let mut reading = Reading::default();
        reading.catch_up(&path).expect("catch up");
        while !reading.whole() {
            reading.step_back(&path).expect("step back");
        }
        let again = reading.facts();
        assert_eq!(ids(&first), ids(&again));
        assert_eq!(ids(&first), ids(&forward(&path)));
        let all = ids(&first);
        assert!(all.len() > 1);
        // No two entries share an id, and an assistant line that gives several
        // entries numbers them in order.
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
        assert!(
            sorted.iter().any(|id| id.index > 0),
            "a line gave more than one entry"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn the_newest_lines_come_first() {
        let path = file("first", &transcript(400));
        let mut reading = Reading::default();
        reading.catch_up(&path).expect("catch up");
        reading.step_back(&path).expect("step back");
        let facts = reading.facts();
        assert!(!facts.whole);
        assert_eq!(facts.last_prompt.as_deref(), Some("prompt 399"));
        assert_eq!(facts.model.as_deref(), Some("m7"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn every_entry_is_counted_past_the_ones_kept() {
        let path = file("totals", &transcript(400));
        let mut reading = Reading::default();
        reading.catch_up(&path).expect("catch up");
        reading.step_back(&path).expect("step back");
        // Part of the transcript is read: the count is of the lines so far,
        // and older entries may exist.
        let partial = reading.facts();
        assert!(!partial.whole);
        assert!(partial.activity_total < 400, "{}", partial.activity_total);
        assert!(partial.older(List::Activity));
        while !reading.whole() {
            reading.step_back(&path).expect("step back");
        }
        let facts = reading.facts();
        // Each turn has one prompt and one edit, and every fifth turn a reply.
        assert_eq!((facts.log_total, facts.activity_total), (480, 400));
        assert_eq!((facts.log.len(), facts.activity.len()), (KEEP, KEEP));
        assert!(facts.older(List::Log) && facts.older(List::Activity));
        assert_eq!(facts, forward(&path));
        let _ = fs::remove_file(path);

        // A transcript that fits in the facts has nothing older.
        let path = file("small", &transcript(40));
        let mut reading = Reading::default();
        reading.catch_up(&path).expect("catch up");
        while !reading.whole() {
            reading.step_back(&path).expect("step back");
        }
        let facts = reading.facts();
        assert_eq!(facts.activity_total, 40);
        assert!(!facts.older(List::Log) && !facts.older(List::Activity));
        let _ = fs::remove_file(path);
    }

    /// One line of a transcript.
    fn line(value: &Value) -> String {
        format!("{value}\n")
    }

    fn prompt(text: &str) -> String {
        line(&json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:00Z",
            "message": {"content": text},
        }))
    }

    /// A reply with `parts`, such as text and tool calls.
    fn reply(parts: &[Value]) -> String {
        line(&json!({
            "type": "assistant",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": {"model": "m", "content": parts},
        }))
    }

    fn text(text: &str) -> Value {
        json!({"type": "text", "text": text})
    }

    fn bash(id: &str, command: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "Bash", "input": {"command": command}})
    }

    /// What the call `id` gave back.
    fn output(id: &str, out: &str) -> String {
        line(&json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:02Z",
            "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": out}]},
        }))
    }

    /// `count` turns, each of three lines: a prompt, a reply with a call, and
    /// the call's result.
    fn turns(count: usize) -> Vec<String> {
        (0..count)
            .map(|turn| {
                [
                    prompt(&format!("prompt {turn}")),
                    reply(&[
                        text(&format!("reply {turn}")),
                        bash(&format!("c{turn}"), &format!("echo {turn}")),
                    ]),
                    output(&format!("c{turn}"), &format!("out {turn}")),
                ]
                .concat()
            })
            .collect()
    }

    /// What `list` has of a transcript's facts.
    fn of(facts: &Facts, list: List) -> Vec<Said> {
        facts.entries(list).to_vec()
    }

    #[test]
    fn a_page_holds_the_entries_before_the_one_asked() {
        let all = turns(1000);
        let path = file("older", &all.concat());
        // The same lines, ending where the page ends, read as the daemon reads
        // them for the facts.
        let ends = |turns: usize| forward(&file("prefix", &all[..turns].concat()));
        let window = forward(&path);
        for (list, ended) in [(List::Activity, 700), (List::Log, 850)] {
            let first = of(&window, list)[0].id;
            let got = page(&path, Way::Older, list, first, 100).expect("a page");
            assert_eq!(got.entries, of(&ends(ended), list)[200..], "{list:?}");
            assert!(got.more, "{list:?}");
        }
        let got = page(
            &path,
            Way::Older,
            List::Activity,
            of(&window, List::Activity)[0].id,
            100,
        )
        .expect("a page");
        // Counted from the newest of 1,000 calls, the newest 300 are held, and
        // the page is calls 301 to 400.
        let texts: Vec<_> = got.entries.iter().map(|said| said.text.as_str()).collect();
        assert_eq!(texts.first(), Some(&"echo 600"));
        assert_eq!(texts.last(), Some(&"echo 699"));
        assert!(got.entries.iter().all(|said| said.result.is_some()));
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(
            env::temp_dir().join(format!("rawr-facts-prefix-{}.jsonl", process::id())),
        );
    }

    #[test]
    fn the_first_entries_of_a_session() {
        let path = file("first-entries", &turns(3).concat());
        let all = of(&forward(&path), List::Activity);
        let got = page(&path, Way::Older, List::Activity, all[1].id, 100).expect("a page");
        assert_eq!(got.entries, all[..1]);
        assert!(!got.more);
        let got = page(&path, Way::Older, List::Activity, all[0].id, 100).expect("a page");
        assert_eq!((got.entries.len(), got.more), (0, false));
        let _ = fs::remove_file(path);
    }

    /// A reply with the calls `one` and `two`, then the result of `one`, that
    /// is `one_out`, after `between`, then the result of `two`.
    fn two_calls(between: &str, one_out: &str) -> String {
        [
            prompt("run both"),
            reply(&[
                text("both"),
                bash("one", "echo one"),
                bash("two", "echo two"),
            ]),
            between.to_owned(),
            output("one", one_out),
            output("two", "two out"),
        ]
        .concat()
    }

    #[test]
    fn a_result_just_past_the_page() {
        let path = file("past", &two_calls("", "one out"));
        let both = of(&forward(&path), List::Activity);
        // The page ends inside the reply that makes both calls, so the result
        // of the first is on a line after the page's last.
        let got = page(&path, Way::Older, List::Activity, both[1].id, 100).expect("a page");
        assert_eq!(got.entries.len(), 1);
        assert_eq!(got.entries[0].id, both[0].id);
        assert_eq!(
            got.entries[0]
                .result
                .as_ref()
                .map(|outcome| outcome.text.as_str()),
            Some("one out")
        );
        assert!(!got.more);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_result_far_past_the_page_is_left_out() {
        let filler = line(&json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:01Z",
            "message": {"content": [{"type": "text", "text": "x".repeat(usize::try_from(AHEAD).expect("a size"))}]},
        }));
        let path = file("far", &two_calls(&filler, "one out"));
        let both = of(&forward(&path), List::Activity);
        let got = page(&path, Way::Older, List::Activity, both[1].id, 100).expect("a page");
        assert_eq!(got.entries.len(), 1);
        assert_eq!(got.entries[0].result, None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_result_in_a_line_longer_than_the_cap_is_left_out() {
        let long = "x".repeat(usize::try_from(LINE_MOST).expect("a size") + (1 << 20));
        let path = file("long", &two_calls("", &long));
        let both = of(&forward(&path), List::Activity);
        let got = page(&path, Way::Older, List::Activity, both[1].id, 100).expect("a page");
        assert_eq!(got.entries.len(), 1);
        assert_eq!(got.entries[0].result, None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_page_of_a_sparse_list_stops_at_the_cap_and_the_next_goes_on() {
        // Ten prompts, each followed by a call that gives no entry of the
        // conversation and a result of 1.5 MiB.
        let turns: Vec<String> = (0..10)
            .map(|turn| {
                [
                    prompt(&format!("prompt {turn}")),
                    reply(&[bash(&format!("c{turn}"), "echo")]),
                    output(&format!("c{turn}"), &"x".repeat(3 << 19)),
                ]
                .concat()
            })
            .collect();
        let path = file("sparse", &turns.concat());
        let last = of(&forward(&path), List::Log)[9].id;
        let mut at = last;
        let mut found = Vec::new();
        let mut pages = 0;
        loop {
            let got = page(&path, Way::Older, List::Log, at, 100).expect("a page");
            pages += 1;
            // A page reads about 4 MiB past its first entry, and holds the
            // entries it found, so a long list takes several.
            assert!(got.entries.len() < 5, "{} entries", got.entries.len());
            found.splice(0..0, got.entries.iter().map(|said| said.text.clone()));
            let Some(first) = got.entries.first() else {
                break;
            };
            at = first.id;
            if !got.more {
                break;
            }
            assert!(pages < 20);
        }
        assert!(pages > 2);
        let expected: Vec<String> = (0..9).map(|turn| format!("prompt {turn}")).collect();
        assert_eq!(found, expected);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn pages_chain_back_to_the_first_line() {
        let path = file("chain", &turns(1000).concat());
        let window = of(&forward(&path), List::Activity);
        let mut at = window[0].id;
        let mut texts = Vec::new();
        let mut pages = 0;
        loop {
            let got = page(&path, Way::Older, List::Activity, at, 100).expect("a page");
            pages += 1;
            texts.splice(0..0, got.entries.iter().map(|said| said.text.clone()));
            let Some(first) = got.entries.first() else {
                break;
            };
            at = first.id;
            if !got.more {
                break;
            }
        }
        assert_eq!(pages, 7);
        let expected: Vec<String> = (0..700).map(|turn| format!("echo {turn}")).collect();
        assert_eq!(texts, expected);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_newer_page_reloads_what_follows_an_entry() {
        let path = file("newer", &turns(1000).concat());
        let window = of(&forward(&path), List::Activity);
        let got = page(&path, Way::Newer, List::Activity, window[10].id, 50).expect("a page");
        assert_eq!(got.entries, window[11..61]);
        assert!(got.more);
        let end = page(&path, Way::Newer, List::Activity, window[290].id, 50).expect("a page");
        assert_eq!(end.entries, window[291..]);
        assert!(!end.more);
        let log = of(&forward(&path), List::Log);
        let got = page(&path, Way::Newer, List::Log, log[10].id, 50).expect("a page");
        assert_eq!(got.entries, log[11..61]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_page_of_a_transcript_that_is_gone_is_an_error() {
        let path = env::temp_dir().join("rawr-facts-gone.jsonl");
        let id = EntryId { line: 0, index: 0 };
        let error = page(&path, Way::Older, List::Log, id, 100).expect_err("no page");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    /// The size of the result of tool call `turn` of the timing transcript: a
    /// few calls give long results, as a build or a file read does.
    const fn result_size(turn: usize) -> usize {
        match turn % 50 {
            0 => 120_000,
            1..=5 => 12_000,
            _ => 1_500,
        }
    }

    /// The time at the 95th percentile of `times`.
    fn p95(times: &mut [Duration]) -> Duration {
        times.sort();
        times[times.len() * 95 / 100]
    }

    /// Round trips of a page of 100 entries of each list, over a socket pair as
    /// the daemon and a window talk, on an invented transcript of 50 MB, in a
    /// release build:
    /// `cargo test --release -- --ignored page_round_trip --nocapture`.
    #[test]
    #[ignore = "a timing run, in a release build"]
    #[expect(
        clippy::too_many_lines,
        reason = "one run that writes the transcript, serves the pages and times them"
    )]
    fn page_round_trip_on_a_transcript_of_fifty_megabytes() {
        // Turns of a prompt, a reply with a call, and its result, until the
        // transcript is 50 MB; the offsets of the lines the pages start at.
        let mut body = String::new();
        let mut starts = Vec::new();
        while body.len() < 50 << 20 {
            let turn = starts.len();
            let prompt_at = body.len();
            body.push_str(&prompt(&format!("prompt {turn}: change the tax code")));
            let reply_at = body.len();
            body.push_str(&reply(&[
                text(&format!("reply {turn}: reading the file first")),
                bash(&format!("c{turn}"), &format!("grep -rn tax src/part{turn}")),
            ]));
            let out = "a line of the output of the command\n".repeat(result_size(turn) / 36);
            body.push_str(&output(&format!("c{turn}"), &out));
            starts.push((prompt_at, reply_at));
        }
        let path = file("timing", &body);
        let turns = starts.len();
        drop(body);
        let (client, server) = UnixStream::pair().expect("a socket pair");
        let daemon = {
            let path = path.clone();
            thread::spawn(move || {
                let mut requests = BufReader::new(server.try_clone().expect("a clone"));
                let mut server = server;
                let mut line = String::new();
                while requests.read_line(&mut line).is_ok_and(|read| read > 0) {
                    let ToDaemon::Older {
                        list,
                        before,
                        count,
                        ..
                    } = serde_json::from_str(&line).expect("a request")
                    else {
                        panic!("the request is an older");
                    };
                    let got = page(&path, Way::Older, list, before, count).expect("a page");
                    let mut answer = serde_json::to_vec(&FromDaemon::Page {
                        path: path.clone(),
                        list,
                        way: Way::Older,
                        at: before,
                        entries: got.entries,
                        more: got.more,
                        failed: None,
                    })
                    .expect("an answer");
                    answer.push(b'\n');
                    server.write_all(&answer).expect("write the answer");
                    line.clear();
                }
            })
        };
        let mut answers = BufReader::new(client.try_clone().expect("a clone"));
        let mut client = client;
        let mut times = Vec::new();
        let mut line = String::new();
        // A fixed run of pseudo-random places in the transcript, past the first
        // pages, for both lists.
        let mut seed: u64 = 7;
        for round in 0..400 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let turn = 150 + usize::try_from(seed >> 33).expect("a number") % (turns - 150);
            let (list, line_at, index) = if round % 2 == 0 {
                (List::Activity, starts[turn].1, 1)
            } else {
                (List::Log, starts[turn].0, 0)
            };
            let before = EntryId {
                line: u64::try_from(line_at).expect("an offset"),
                index,
            };
            let began = Instant::now();
            let mut request = serde_json::to_vec(&ToDaemon::Older {
                path: path.clone(),
                list,
                before,
                count: 100,
            })
            .expect("a request");
            request.push(b'\n');
            client.write_all(&request).expect("send the request");
            line.clear();
            answers.read_line(&mut line).expect("read the answer");
            let FromDaemon::Page { entries, .. } = serde_json::from_str(&line).expect("an answer")
            else {
                panic!("the answer is a page");
            };
            times.push(began.elapsed());
            assert_eq!(entries.len(), 100);
        }
        drop(client);
        drop(answers);
        daemon.join().expect("the daemon thread ends");
        let size = fs::metadata(&path).expect("the file").len();
        times.sort();
        let (median, slowest) = (times[times.len() / 2], times[times.len() - 1]);
        let p95 = p95(&mut times);
        eprintln!(
            "{turns} turns, {} MiB; 400 pages of 100 entries: median {median:?}, p95 {p95:?}, max {slowest:?}",
            size >> 20,
        );
        let _ = fs::remove_file(path);
        assert!(p95 < Duration::from_millis(20), "p95 {p95:?}");
    }

    #[test]
    fn lines_written_later_add_to_a_partial_read() {
        let all = transcript(400);
        let cut = all.len() / 2 + 3;
        // The cut falls inside a line, which waits until it is whole.
        let path = file("grow", &all[..cut]);
        let mut reading = Reading::default();
        reading.catch_up(&path).expect("catch up");
        reading.step_back(&path).expect("step back");
        let mut more = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open to append");
        more.write_all(&all.as_bytes()[cut..]).expect("append");
        assert_eq!(reading.catch_up(&path), Some(true));
        while !reading.whole() {
            reading.step_back(&path).expect("step back");
        }
        assert_eq!(reading.facts(), forward(&path));
        assert_eq!(reading.catch_up(&path), Some(false));
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod entries {
    use super::{Kind, said_as};

    fn shown(text: &str) -> Option<(Kind, String, Option<String>)> {
        said_as(text)
    }

    #[test]
    fn user_lines_show_as_claude_code_draws_them() {
        assert_eq!(
            shown("fix the build"),
            Some((Kind::Prompt, "fix the build".to_owned(), None))
        );
        assert_eq!(
            shown(
                "<command-message>review</command-message>\n<command-name>/review</command-name>\n<command-args>42</command-args>"
            ),
            Some((Kind::Command, "/review 42".to_owned(), None))
        );
        assert_eq!(
            shown("<bash-input>ls -la</bash-input>"),
            Some((Kind::Shell, "ls -la".to_owned(), None))
        );
        assert_eq!(
            shown(
                "<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>Build finished</summary>\n</task-notification>"
            ),
            Some((
                Kind::Ended {
                    status: "completed".to_owned()
                },
                "Build finished".to_owned(),
                None
            ))
        );
        assert_eq!(
            shown(
                "Another Claude session sent a message:\n<cross-session-message from-name=\"notes\">Done with the draft</cross-session-message>"
            ),
            Some((
                Kind::Message {
                    from: "notes".to_owned()
                },
                "Done with the draft".to_owned(),
                None
            ))
        );
        assert_eq!(
            shown(
                "<teammate-message teammate_id=\"writer\" summary=\"Draft ready\">\nThe draft is in place.\n</teammate-message>"
            ),
            Some((
                Kind::Message {
                    from: "writer".to_owned()
                },
                "Draft ready".to_owned(),
                Some("The draft is in place.".to_owned())
            ))
        );
        assert_eq!(
            shown("[Request interrupted by user]"),
            Some((Kind::Interrupted, "Interrupted".to_owned(), None))
        );
        assert_eq!(
            shown("<bash-stdout>out</bash-stdout><bash-stderr></bash-stderr>"),
            None
        );
        assert_eq!(
            shown("<local-command-caveat>Caveat</local-command-caveat>"),
            None
        );
        assert_eq!(
            shown(
                "<task-notification>\n<tool-use-id>t1</tool-use-id>\n<task-type>tool_call</task-type>\n<summary>x</summary></task-notification>"
            ),
            None
        );
    }
}

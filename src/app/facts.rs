//! What a session's transcript says about it beyond its title: the model,
//! the branch, the pull requests it opened, how long it ran, how many turns
//! and tokens it took, and the last thing the user asked.
//!
//! The daemon reads a transcript from its newest line back, a part at a
//! time, so the newest facts show at once and the older lines fill in the
//! counts behind them. It keeps each reading by path, so moving the
//! selection back goes on where the reading stopped.

use std::borrow::Cow;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;
use serde_json::value::RawValue;

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
    /// The files it edited or wrote, the most recent last, with how often.
    pub(crate) files: Vec<(String, usize)>,
    /// What the user typed, oldest first, the last [`KEEP`] of it.
    pub(crate) log: Vec<Said>,
    /// The tools Claude called, oldest first, the last [`KEEP`] of them.
    pub(crate) activity: Vec<Said>,
    /// Whether every line of the transcript is read. The daemon reads from
    /// the newest line back, so until then the counts, the start and the
    /// task list cover only the newest part.
    #[serde(default)]
    pub(crate) whole: bool,
}

/// How many prompts and tool calls the facts keep.
pub(crate) const KEEP: usize = 300;

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
    pub(crate) at: Option<String>,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) kind: Kind,
    /// The longer text the entry opens to, when it has one: a message's
    /// body, a sub-agent's prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) more: Option<String>,
}

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
            | Kind::Interrupted => Cow::Borrowed(&self.text),
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
            let mut older = read_lines(bytes.get(start..).unwrap_or_default());
            older.then(std::mem::take(&mut self.stretch));
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

/// What a run of consecutive lines gives. The task calls stay as they
/// came: an update may name a task an older run created.
#[derive(Debug, Clone, Default)]
struct Stretch {
    facts: Facts,
    ops: Vec<TaskOp>,
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
            let before = facts
                .files
                .iter()
                .position(|(file, _)| *file == path)
                .map_or(0, |index| facts.files.remove(index).1);
            facts.files.push((path, before + count));
        }
        trim(&mut facts.files);
        facts.log.extend(newer.log);
        trim(&mut facts.log);
        facts.activity.extend(newer.activity);
        trim(&mut facts.activity);
        self.ops.extend(later.ops);
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
        at += u64::try_from(read).ok()?;
        if let Ok(record) = serde_json::from_slice::<Record<'_>>(&line) {
            add(&mut stretch, record);
        }
    }
    Some((stretch, at))
}

/// What a run of whole lines gives.
fn read_lines(bytes: &[u8]) -> Stretch {
    let mut stretch = Stretch::default();
    for line in bytes.split(|byte| *byte == b'\n') {
        if let Ok(record) = serde_json::from_slice::<Record<'_>>(line) {
            add(&mut stretch, record);
        }
    }
    stretch
}

fn add(stretch: &mut Stretch, record: Record<'_>) {
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
                for part in parts.iter().filter(|part| part.kind == "tool_use") {
                    call(stretch, at, cwd, part);
                }
            }
        }
        "user" if !record.is_meta && !record.summary => {
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
                    Said {
                        at: record.timestamp.as_deref().map(str::to_owned),
                        text,
                        kind,
                        more,
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
                .map(|id| id.as_str().map_or_else(|| id.to_string(), str::to_owned))
                .unwrap_or_default();
            let subject = field("taskSubject")
                .map(|subject| format!(" ({subject})"))
                .unwrap_or_default();
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
                field("reason")
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ),
            None,
        ),
        "shutdown_rejected" => (
            Kind::Message { from },
            format!(
                "Shutdown rejected{}",
                field("reason")
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
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

/// Adds `item` to the end of `list`, keeping the last [`KEEP`].
fn keep(list: &mut Vec<Said>, item: Said) {
    list.push(item);
    if list.len() > KEEP {
        list.drain(..list.len() - KEEP);
    }
}

/// What one tool call adds: its line in the activity, a task change, a sub-
/// agent, or a file it changed.
fn call(stretch: &mut Stretch, at: Option<&str>, cwd: Option<&str>, part: &Part<'_>) {
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
            if let Some(path) = input.file_path.as_deref() {
                match facts.files.iter().position(|(file, _)| file == path) {
                    Some(index) => {
                        let (file, count) = facts.files.remove(index);
                        facts.files.push((file, count + 1));
                    }
                    None => facts.files.push((path.to_owned(), 1)),
                }
                if facts.files.len() > KEEP {
                    facts.files.remove(0);
                }
            }
        }
        _ => {}
    }
    if let Some((shown, what, more)) = tool_line(name, &input, part.input, cwd) {
        keep(
            &mut facts.activity,
            Said {
                at: at.map(str::to_owned),
                text: clean(&what),
                kind: Kind::Tool { name: shown },
                more: more
                    .map(|more| clean(&more))
                    .filter(|more| !more.is_empty()),
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
    let path =
        |path: Option<&Cow<'_, str>>| path.map(|path| shown_path(path, cwd)).unwrap_or_default();
    let (shown, what) = match name {
        "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" => return None,
        "Bash" | "PowerShell" => (
            name.to_owned(),
            input.command.as_deref().unwrap_or_default().to_owned(),
        ),
        "Read" => {
            let mut what = path(input.file_path.as_ref());
            if let Some(pages) = input.pages.as_deref() {
                what.push_str(&format!(" · pages {pages}"));
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
                what.push_str(&format!(", path: \"{}\"", shown_path(dir, cwd)));
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
    .map(|map| {
        map.iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_default()
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
    if let Some(home) = std::env::var_os("HOME").and_then(|home| home.into_string().ok())
        && let Some(rest) = path.strip_prefix(&home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::path::PathBuf;

    use super::{Facts, Reading, read_forward, tasks};

    /// A transcript of `turns` turns: a prompt, then a reply that creates
    /// a task, edits a file and, every fifth turn, carries a long text, so
    /// some lines are longer than one part.
    fn transcript(turns: usize) -> String {
        let mut text = String::new();
        for turn in 0..turns {
            let at = format!("2026-01-01T00:{:02}:{:02}Z", turn / 60 % 60, turn % 60);
            text.push_str(&format!(
                r#"{{"type":"user","timestamp":"{at}","gitBranch":"b{}","message":{{"content":"prompt {turn}"}}}}"#,
                turn / 100
            ));
            text.push('\n');
            let long = if turn % 5 == 0 {
                "x".repeat(300_000)
            } else {
                String::new()
            };
            text.push_str(&format!(
                r#"{{"type":"assistant","timestamp":"{at}","message":{{"model":"m{}","usage":{{"output_tokens":{turn}}},"content":[{{"type":"text","text":"{long}"}},{{"type":"tool_use","name":"TaskCreate","input":{{"subject":"task {turn}"}}}},{{"type":"tool_use","name":"TaskUpdate","input":{{"taskId":"{}","status":"completed"}}}},{{"type":"tool_use","name":"Edit","input":{{"file_path":"f{}"}}}}]}}}}"#,
                turn / 50,
                turn / 2 + 1,
                turn % 7
            ));
            text.push('\n');
        }
        text
    }

    fn file(name: &str, text: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("rawr-facts-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, text).expect("write the transcript");
        path
    }

    /// The facts of the whole file, read from its first line on.
    fn forward(path: &PathBuf) -> Facts {
        let mut opened = std::fs::File::open(path).expect("open the transcript");
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
        let _ = std::fs::remove_file(path);
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
        let _ = std::fs::remove_file(path);
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
        let mut more = std::fs::OpenOptions::new()
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
        let _ = std::fs::remove_file(path);
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

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

use crate::session::sanitize;

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

/// A line with when it was written: a prompt, or a tool call and what it
/// acted on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Said {
    pub(crate) at: Option<String>,
    pub(crate) text: String,
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
                for part in parts.iter().filter(|part| part.kind == "tool_use") {
                    call(stretch, at, part);
                }
            }
        }
        "user" if !record.is_meta && !record.summary => {
            if let Some(text) = record
                .message
                .and_then(|message| message.content)
                .and_then(typed)
            {
                facts.prompts += 1;
                keep(
                    &mut facts.log,
                    Said {
                        at: record.timestamp.as_deref().map(str::to_owned),
                        text: text.clone(),
                    },
                );
                facts.last_prompt = Some(text);
            }
        }
        _ => {}
    }
}

/// The text the user typed, when `content` is a prompt rather than a tool
/// result or an injected block.
fn typed(content: &RawValue) -> Option<String> {
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
            .join(" ")
    };
    let text = text.trim();
    if text.is_empty() || text.starts_with('<') {
        return None;
    }
    Some(sanitize(
        &text.split_whitespace().collect::<Vec<_>>().join(" "),
    ))
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
fn call(stretch: &mut Stretch, at: Option<&str>, part: &Part<'_>) {
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
    let what = input
        .description
        .as_deref()
        .or(input.command.as_deref())
        .or(input.file_path.as_deref())
        .or(input.pattern.as_deref())
        .or(input.skill.as_deref())
        .or(input.subject.as_deref())
        .unwrap_or_default();
    let what = what.lines().next().unwrap_or_default();
    keep(
        &mut facts.activity,
        Said {
            at: at.map(str::to_owned),
            text: sanitize(&format!("{name}  {what}")),
        },
    );
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

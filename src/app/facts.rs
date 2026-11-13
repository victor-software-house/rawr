//! What a session's transcript says about it beyond its title: the model,
//! the branch, the pull requests it opened, how long it ran, how many turns
//! and tokens it took, and the last thing the user asked.
//!
//! The daemon reads them and keeps them by path, with the size and time the
//! file had, so moving the selection back costs nothing.

use std::borrow::Cow;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
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

/// Goes on reading the transcript at `path` from byte `offset`, where an
/// earlier read that found `facts` stopped, and returns the facts with the
/// offset after the last whole line. A transcript only grows at its end,
/// so a session that is still running costs only its new lines.
pub(crate) fn read_on(path: &Path, mut facts: Facts, offset: u64) -> Option<(Facts, u64)> {
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut lines = BufReader::with_capacity(1 << 18, file);
    let mut line = Vec::new();
    let mut at = offset;
    loop {
        line.clear();
        let read = lines.read_until(b'\n', &mut line).ok()?;
        // A line still being written waits for the next read.
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        at += u64::try_from(read).ok()?;
        let Ok(record) = serde_json::from_slice::<Record<'_>>(&line) else {
            continue;
        };
        add(&mut facts, record);
    }
    Some((facts, at))
}

fn add(facts: &mut Facts, record: Record<'_>) {
    if record.sidechain {
        return;
    }
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
                    call(facts, at, part);
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
fn call(facts: &mut Facts, at: Option<&str>, part: &Part<'_>) {
    let name = part.name.as_deref().unwrap_or("tool");
    let input = part
        .input
        .and_then(|input| serde_json::from_str::<Input<'_>>(input.get()).ok())
        .unwrap_or_default();
    match name {
        "Agent" | "Task" => facts.agents += 1,
        "TaskCreate" => {
            let id = (facts
                .tasks
                .iter()
                .filter_map(|task| task.id.parse::<u64>().ok())
                .max())
            .map_or(1, |most| most + 1)
            .to_string();
            facts.tasks.push(Task {
                id,
                subject: input.subject.as_deref().unwrap_or_default().to_owned(),
                status: "pending".to_owned(),
            });
        }
        "TaskUpdate" => {
            let id = match &input.task_id {
                Some(serde_json::Value::String(id)) => id.clone(),
                Some(serde_json::Value::Number(id)) => id.to_string(),
                _ => String::new(),
            };
            if let Some(status) = input.status.as_deref() {
                if status == "deleted" {
                    facts.tasks.retain(|task| task.id != id);
                } else if let Some(task) = facts.tasks.iter_mut().find(|task| task.id == id) {
                    status.clone_into(&mut task.status);
                }
            }
            if let (Some(subject), Some(task)) = (
                input.subject.as_deref(),
                facts.tasks.iter_mut().find(|task| task.id == id),
            ) {
                subject.clone_into(&mut task.subject);
            }
        }
        // The older to-do tool sends the whole list each time.
        "TodoWrite" => {
            if let Some(todos) = &input.todos {
                facts.tasks = todos
                    .iter()
                    .enumerate()
                    .map(|(index, todo)| Task {
                        id: (index + 1).to_string(),
                        subject: todo.content.as_ref().to_owned(),
                        status: todo.status.as_ref().to_owned(),
                    })
                    .collect();
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

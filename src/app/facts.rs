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
    if let Some(at) = record.timestamp {
        if facts.started.is_none() {
            facts.started = Some(at.clone().into_owned());
        }
        facts.last = Some(at.into_owned());
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
            facts.agents += message.content.map_or(0, |content| uses(content, "Agent"));
        }
        "user" if !record.is_meta && !record.summary => {
            if let Some(text) = record
                .message
                .and_then(|message| message.content)
                .and_then(typed)
            {
                facts.prompts += 1;
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

/// How many `tool_use` parts of `content` call `tool`.
fn uses(content: &RawValue, tool: &str) -> usize {
    serde_json::from_str::<Vec<Part<'_>>>(content.get()).map_or(0, |parts| {
        parts
            .iter()
            .filter(|part| part.kind == "tool_use" && part.name.as_deref() == Some(tool))
            .count()
    })
}

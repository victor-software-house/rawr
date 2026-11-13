//! What a session's transcript says about it beyond its title: the model,
//! the branch, the pull requests it opened, how long it ran, how many turns
//! and tokens it took, and the last thing the user asked.
//!
//! A transcript is read once per change: the result is kept by path, with
//! the size and time it had, so moving the selection back costs nothing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

use crate::session::sanitize;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) model: Option<String>,
    pub(crate) branch: Option<String>,
    /// `owner/repo#n`, in the order the session opened them.
    pub(crate) prs: Vec<String>,
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

/// The transcripts read so far, by path.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    read: HashMap<PathBuf, (u64, Option<SystemTime>, Facts)>,
}

impl Cache {
    /// The facts of `path`, read again only when the file changed.
    pub(crate) fn get(&mut self, path: &Path) -> Option<&Facts> {
        let meta = fs::metadata(path).ok()?;
        let stamp = (meta.len(), meta.modified().ok());
        let fresh = self
            .read
            .get(path)
            .is_some_and(|(len, at, _)| (*len, *at) == stamp);
        if !fresh {
            let facts = read(path)?;
            self.read.insert(path.to_owned(), (stamp.0, stamp.1, facts));
        }
        self.read.get(path).map(|(_, _, facts)| facts)
    }
}

/// The fields of a transcript line the facts use; every other is skipped.
#[derive(Debug, Deserialize)]
struct Record {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "gitBranch", default)]
    git_branch: Option<String>,
    #[serde(rename = "isMeta", default)]
    is_meta: bool,
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
    #[serde(default)]
    message: Option<Message>,
    #[serde(rename = "prRepository", default)]
    pr_repository: Option<String>,
    #[serde(rename = "prNumber", default)]
    pr_number: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    output_tokens: u64,
}

fn read(path: &Path) -> Option<Facts> {
    let bytes = fs::read(path).ok()?;
    let mut facts = Facts::default();
    for line in bytes.split(|byte| *byte == b'\n') {
        let Ok(record) = serde_json::from_slice::<Record>(line) else {
            continue;
        };
        if record.sidechain {
            continue;
        }
        if let Some(at) = record.timestamp {
            if facts.started.is_none() {
                facts.started = Some(at.clone());
            }
            facts.last = Some(at);
        }
        if let Some(branch) = record.git_branch.filter(|branch| !branch.is_empty()) {
            facts.branch = Some(branch);
        }
        match record.kind.as_str() {
            "pr-link" => {
                if let (Some(repo), Some(number)) = (record.pr_repository, record.pr_number) {
                    let pr = format!("{repo}#{number}");
                    if !facts.prs.contains(&pr) {
                        facts.prs.push(pr);
                    }
                }
            }
            "assistant" => {
                let Some(message) = record.message else {
                    continue;
                };
                if let Some(model) = message.model.filter(|model| !model.starts_with('<')) {
                    facts.model = Some(model);
                }
                facts.output_tokens += message.usage.map_or(0, |usage| usage.output_tokens);
                facts.replies += 1;
                facts.agents += uses(&message.content, "Agent");
            }
            "user" if !record.is_meta => {
                if let Some(text) = record
                    .message
                    .as_ref()
                    .and_then(|message| typed(&message.content))
                {
                    facts.prompts += 1;
                    facts.last_prompt = Some(text);
                }
            }
            _ => {}
        }
    }
    Some(facts)
}

/// The text the user typed, when `content` is a prompt rather than a tool
/// result or an injected block.
fn typed(content: &serde_json::Value) -> Option<String> {
    let text = match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => {
            if parts
                .iter()
                .any(|part| part.get("type").and_then(|kind| kind.as_str()) == Some("tool_result"))
            {
                return None;
            }
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
        }
        _ => return None,
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
fn uses(content: &serde_json::Value, tool: &str) -> usize {
    content.as_array().map_or(0, |parts| {
        parts
            .iter()
            .filter(|part| {
                part.get("type").and_then(|kind| kind.as_str()) == Some("tool_use")
                    && part.get("name").and_then(|name| name.as_str()) == Some(tool)
            })
            .count()
    })
}

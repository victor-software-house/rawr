//! What a session's transcript says about it beyond its title: the model,
//! the branch, the pull requests it opened, how long it ran, how many turns
//! and tokens it took, and the last thing the user asked.
//!
//! The daemon reads them and keeps them by path, with the size and time the
//! file had, so moving the selection back costs nothing.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

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
    #[serde(rename = "isCompactSummary", default)]
    summary: bool,
    #[serde(default)]
    message: Option<Message>,
    #[serde(rename = "prRepository", default)]
    pr_repository: Option<String>,
    #[serde(rename = "prNumber", default)]
    pr_number: Option<u64>,
    #[serde(rename = "prUrl", default)]
    pr_url: Option<String>,
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

/// Reads the facts of the transcript at `path`, line by line.
pub(crate) fn read(path: &Path) -> Option<Facts> {
    let mut lines = BufReader::new(fs::File::open(path).ok()?);
    let mut line = Vec::new();
    let mut facts = Facts::default();
    loop {
        line.clear();
        if lines.read_until(b'\n', &mut line).ok()? == 0 {
            break;
        }
        let Ok(record) = serde_json::from_slice::<Record>(&line) else {
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
                    continue;
                };
                if let Some(model) = message.model.filter(|model| !model.starts_with('<')) {
                    facts.model = Some(model);
                }
                facts.output_tokens += message.usage.map_or(0, |usage| usage.output_tokens);
                facts.replies += 1;
                facts.agents += uses(&message.content, "Agent");
            }
            "user" if !record.is_meta && !record.summary => {
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

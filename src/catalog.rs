//! Every resumable local Claude Code session, for search.
//!
//! Claude Code appends one line per typed prompt to `history.jsonl`, naming
//! the session and the folder it started in. That one file is the search
//! index: it holds the words a user remembers a session by. One listing of
//! `projects/*/` says which sessions still have a transcript, and each found
//! session's title comes from its transcript's end, read once per load.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::session::{Agent, sanitize, transcript_title};

/// The longest title drawn from a first prompt, in characters.
const TITLE_CHARS: usize = 120;

/// A session search can find.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) agent: Agent,
    /// Its prompts, folder, ID and title, lowercase, one per line.
    text: String,
    /// When its latest prompt was typed, in milliseconds since the epoch.
    latest: u64,
    /// Whether the title came from a typed prompt rather than a slash command.
    titled: bool,
    /// Its prompts as typed, oldest first, for the line a search shows.
    prompts: Vec<String>,
    /// Its transcript, which a deep search reads.
    pub(crate) transcript: PathBuf,
}

/// A session's transcript, as one listing of `projects/*/` finds it.
#[derive(Debug)]
struct Transcript {
    path: PathBuf,
    /// When it was last written.
    modified: Option<SystemTime>,
}

/// One line of `history.jsonl`.
#[derive(Debug, Deserialize)]
struct Prompt<'line> {
    #[serde(borrow)]
    display: Cow<'line, str>,
    #[serde(borrow)]
    project: Cow<'line, str>,
    #[serde(rename = "sessionId", borrow)]
    session_id: Option<Cow<'line, str>>,
    timestamp: u64,
}

/// The words of `query`, lowercase; a session matches when it holds them all.
pub(crate) fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

fn holds_all(text: &str, terms: &[String]) -> bool {
    terms.iter().all(|term| text.contains(term.as_str()))
}

/// The sessions under `claude` that still have a transcript and that no
/// process in `live` holds, newest prompt first.
///
/// # Errors
///
/// Fails when `history.jsonl` or the projects folder exists but cannot be
/// read. A missing one means no sessions.
pub(crate) fn load(claude: &Path, live: &HashSet<String>) -> Result<Vec<Found>> {
    let transcripts = transcripts(&claude.join("projects"))?;
    let path = claude.join("history.jsonl");
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
    };
    let now = SystemTime::now();
    let mut sessions: HashMap<String, Found> = HashMap::new();
    for row in BufReader::new(file).split(b'\n') {
        let row = row.with_context(|| format!("read {}", path.display()))?;
        let Ok(prompt) = serde_json::from_slice::<Prompt<'_>>(&row) else {
            continue;
        };
        let Some(id) = prompt.session_id.as_deref() else {
            continue;
        };
        if live.contains(id) {
            continue;
        }
        let Some(transcript) = transcripts.get(id) else {
            continue;
        };
        let found = sessions.entry(id.to_owned()).or_insert_with(|| Found {
            agent: Agent {
                session_id: id.to_owned(),
                cwd: prompt.project.as_ref().to_owned(),
                title: title(&prompt.display),
                idle: transcript
                    .modified
                    .and_then(|at| now.duration_since(at).ok()),
                transcript: transcript.path.clone(),
            },
            text: format!("{}\n{}\n", prompt.project.to_lowercase(), id.to_lowercase()),
            latest: 0,
            titled: false,
            prompts: Vec::new(),
            transcript: transcript.path.clone(),
        });
        if !found.titled && !is_command(&prompt.display) {
            found.agent.title = title(&prompt.display);
            found.titled = true;
        }
        found.prompts.push(prompt.display.as_ref().to_owned());
        found.text.push_str(&prompt.display.to_lowercase());
        found.text.push('\n');
        found.latest = found.latest.max(prompt.timestamp);
    }
    for found in sessions.values_mut() {
        if let Some(title) = transcripts
            .get(&found.agent.session_id)
            .and_then(|transcript| transcript_title(&transcript.path))
        {
            found.agent.title = sanitize(&title);
            found.text.push_str(&title.to_lowercase());
            found.text.push('\n');
        }
    }
    let mut found: Vec<Found> = sessions.into_values().collect();
    found.sort_by_key(|found| std::cmp::Reverse(found.latest));
    Ok(found)
}

/// Every `projects/*/<session id>.jsonl`, by session ID.
fn transcripts(projects: &Path) -> Result<HashMap<String, Transcript>> {
    let mut found = HashMap::new();
    let folders = match fs::read_dir(projects) {
        Ok(folders) => folders,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(error).with_context(|| format!("list {}", projects.display())),
    };
    for folder in folders.flatten() {
        let Ok(files) = fs::read_dir(folder.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let id = id.to_owned();
            let modified = file.metadata().and_then(|meta| meta.modified()).ok();
            found.insert(id, Transcript { path, modified });
        }
    }
    Ok(found)
}

/// Whether a prompt is a slash command, such as `/config`, which says
/// nothing about what the session was for.
fn is_command(prompt: &str) -> bool {
    prompt.trim_start().starts_with('/')
}

/// The first non-blank line of a prompt, safe to draw, cut at
/// [`TITLE_CHARS`] with `…`.
fn title(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(sanitize)
        .unwrap_or_default();
    if line.is_empty() {
        return "Untitled session".to_owned();
    }
    if line.chars().count() <= TITLE_CHARS {
        return line;
    }
    let cut: String = line.chars().take(TITLE_CHARS).collect();
    format!("{}…", cut.trim_end())
}

/// The sessions matching `terms`: the pane's own first, in the pane's order,
/// then every other match from `catalog`, newest first, each session once.
pub(crate) fn search(pane: &[Agent], catalog: &[Found], terms: &[String]) -> Vec<Agent> {
    let by_id: HashMap<&str, &Found> = catalog
        .iter()
        .map(|found| (found.agent.session_id.as_str(), found))
        .collect();
    let mut shown: HashSet<&str> = HashSet::new();
    let mut results = Vec::new();
    for agent in pane {
        let mut text = format!(
            "{}\n{}\n{}\n",
            agent.title.to_lowercase(),
            agent.cwd.to_lowercase(),
            agent.session_id.to_lowercase()
        );
        if let Some(found) = by_id.get(agent.session_id.as_str()) {
            text.push_str(&found.text);
        }
        if holds_all(&text, terms) && shown.insert(agent.session_id.as_str()) {
            results.push(agent.clone());
        }
    }
    for found in catalog {
        if holds_all(&found.text, terms) && shown.insert(found.agent.session_id.as_str()) {
            results.push(found.agent.clone());
        }
    }
    results
}

/// A session a search found, with the line that matched.
#[derive(Debug, Clone)]
pub(crate) struct Hit {
    pub(crate) agent: Agent,
    /// The typed prompt, or the stretch of transcript, that holds the first
    /// search word, cut around it.
    pub(crate) snippet: Option<String>,
    /// How many prompts the user typed in the session.
    pub(crate) prompts: usize,
}

/// The longest snippet, in characters.
const SNIPPET_CHARS: usize = 160;

/// `text` cut to about [`SNIPPET_CHARS`] around the first place `term`
/// occurs, on one line and safe to draw.
pub(crate) fn around(text: &str, term: &str) -> String {
    let flat = sanitize(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    let lower = flat.to_lowercase();
    let chars: Vec<char> = flat.chars().collect();
    let at = lower
        .find(term)
        .map_or(0, |byte| lower[..byte].chars().count());
    let start = at.saturating_sub(SNIPPET_CHARS / 3);
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut cut: String = chars[start..end].iter().collect();
    if start > 0 {
        cut.insert(0, '…');
    }
    if end < chars.len() {
        cut.push('…');
    }
    cut
}

/// Like [`search`], with the line that matched and the session's prompt
/// count.
pub(crate) fn hits(pane: &[Agent], catalog: &[Found], terms: &[String]) -> Vec<Hit> {
    let by_id: HashMap<&str, &Found> = catalog
        .iter()
        .map(|found| (found.agent.session_id.as_str(), found))
        .collect();
    let first = terms.first().map(String::as_str).unwrap_or_default();
    search(pane, catalog, terms)
        .into_iter()
        .map(|agent| {
            let found = by_id.get(agent.session_id.as_str());
            let snippet = found.and_then(|found| {
                found
                    .prompts
                    .iter()
                    .rev()
                    .find(|prompt| prompt.to_lowercase().contains(first))
                    .map(|prompt| around(prompt, first))
            });
            Hit {
                prompts: found.map_or(0, |found| found.prompts.len()),
                agent,
                snippet,
            }
        })
        .collect()
}

/// The sessions in `catalog` whose transcript holds every term, though no
/// prompt or title does: what Claude wrote, tool output, file names. Each
/// transcript is read whole, once per search; `skip` holds the sessions
/// already found.
pub(crate) fn deep(
    catalog: &[Found],
    terms: &[String],
    skip: &HashSet<String>,
    stale: impl Fn() -> bool,
) -> Vec<Hit> {
    let Some(first) = terms.first() else {
        return Vec::new();
    };
    catalog
        .iter()
        .filter(|found| !skip.contains(&found.agent.session_id))
        .take_while(|_| !stale())
        .filter_map(|found| {
            let bytes = fs::read(&found.transcript).ok()?;
            // Only what the user and Claude wrote counts: the instructions,
            // reminders and attachments Claude Code adds to every session
            // would match everything.
            let lines: Vec<(&[u8], Vec<u8>)> = bytes
                .split(|byte| *byte == b'\n')
                .map(|line| (line, line.to_ascii_lowercase()))
                .filter(|(_, lower)| spoken(lower))
                .collect();
            let holds = |term: &String| {
                lines
                    .iter()
                    .any(|(_, lower)| memchr::memmem::find(lower, term.as_bytes()).is_some())
            };
            if !terms.iter().all(holds) {
                return None;
            }
            let (line, at) = lines.iter().find_map(|(line, lower)| {
                memchr::memmem::find(lower, first.as_bytes()).map(|at| (*line, at))
            })?;
            let start = at.saturating_sub(SNIPPET_CHARS);
            let end = (at + SNIPPET_CHARS * 2).min(line.len());
            let raw = String::from_utf8_lossy(&line[start..end])
                .replace("\\n", " ")
                .replace("\\\"", "\"")
                .replace("\\t", " ");
            Some(Hit {
                agent: found.agent.clone(),
                snippet: Some(around(&raw, first)),
                prompts: found.prompts.len(),
            })
        })
        .collect()
}

/// Whether a transcript line, lowercase, holds words the user or Claude
/// wrote: a user or assistant message, and not one Claude Code marks as its
/// own, such as the loaded instructions or a reminder.
fn spoken(lower: &[u8]) -> bool {
    let has = |needle: &[u8]| memchr::memmem::find(lower, needle).is_some();
    (has(b"\"type\":\"user\"") || has(b"\"type\":\"assistant\""))
        && !has(b"\"ismeta\":true")
        && !has(b"<system-reminder>")
        && !has(b"\"tool_use_id\"")
}

#[cfg(test)]
mod tests {
    use indoc::formatdoc;

    use super::*;

    const SHOP: &str = "b8e4d7c2-1a9f-4e63-8d07-2c5f9a3e6b14";
    const GONE: &str = "6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33";
    const NOTES: &str = "0c3e9a41-77d2-4b8e-a1f6-5d2b8c9e0f17";

    /// A Claude Code folder with a history naming three sessions, two of
    /// which still have a transcript.
    fn fixture(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rawr-catalog-{name}-{}", std::process::id()));
        let shop = dir.join("projects/-home-dev-code-shop");
        let notes = dir.join("projects/-home-dev-notes");
        fs::create_dir_all(&shop).unwrap();
        fs::create_dir_all(&notes).unwrap();
        fs::write(shop.join(format!("{SHOP}.jsonl")), "{}\n").unwrap();
        fs::write(
            notes.join(format!("{NOTES}.jsonl")),
            "{\"type\":\"ai-title\",\"aiTitle\":\"Weekly notes draft\"}\n",
        )
        .unwrap();
        fs::write(
            dir.join("history.jsonl"),
            formatdoc!(
                r#"
                    {{"display":"/config","pastedContents":{{}},"timestamp":90,"project":"/home/dev/code/shop","sessionId":"{shop}"}}
                    {{"display":"Fix the login page\nand more","pastedContents":{{}},"timestamp":100,"project":"/home/dev/code/shop","sessionId":"{shop}"}}
                    {{"display":"old prompt","pastedContents":{{}},"timestamp":150,"project":"/home/dev/code/shop","sessionId":"{gone}"}}
                    {{"display":"Draft the weekly notes","pastedContents":{{}},"timestamp":200,"project":"/home/dev/notes","sessionId":"{notes}"}}
                    not json
                    {{"display":"the redirect loops","pastedContents":{{}},"timestamp":300,"project":"/home/dev/code/shop","sessionId":"{shop}"}}
                "#,
                shop = SHOP,
                gone = GONE,
                notes = NOTES,
            ),
        )
        .unwrap();
        dir
    }

    fn ids(agents: &[Agent]) -> Vec<&str> {
        agents
            .iter()
            .map(|agent| agent.session_id.as_str())
            .collect()
    }

    #[test]
    fn only_sessions_with_a_transcript_are_found_newest_first() {
        let dir = fixture("load");
        let found = load(&dir, &HashSet::new()).unwrap();
        let agents: Vec<Agent> = found.iter().map(|found| found.agent.clone()).collect();
        assert_eq!(ids(&agents), [SHOP, NOTES]);
        assert_eq!(agents[0].title, "Fix the login page");
        assert_eq!(agents[1].title, "Weekly notes draft");
        assert_eq!(agents[0].cwd, "/home/dev/code/shop");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_live_session_is_not_found() {
        let dir = fixture("live");
        let live = HashSet::from([SHOP.to_owned()]);
        let found = load(&dir, &live).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent.session_id, NOTES);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_word_must_match_in_any_prompt_ignoring_case() {
        let dir = fixture("match");
        let catalog = load(&dir, &HashSet::new()).unwrap();
        assert_eq!(
            ids(&search(&[], &catalog, &terms("LOGIN redirect"))),
            [SHOP]
        );
        assert_eq!(search(&[], &catalog, &terms("login checkout")), Vec::new());
        assert_eq!(ids(&search(&[], &catalog, &terms("notes"))), [NOTES]);
        assert_eq!(ids(&search(&[], &catalog, &terms("draft"))), [NOTES]);
        assert_eq!(ids(&search(&[], &catalog, &terms(""))), [SHOP, NOTES]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_pane_s_sessions_come_first_and_once() {
        let dir = fixture("pane");
        let catalog = load(&dir, &HashSet::new()).unwrap();
        let mut pane = catalog[1].agent.clone();
        pane.title = "Weekly notes".to_owned();
        let results = search(std::slice::from_ref(&pane), &catalog, &terms("dev"));
        assert_eq!(ids(&results), [NOTES, SHOP]);
        assert_eq!(results[0].title, "Weekly notes");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_long_first_prompt_is_cut_with_an_ellipsis() {
        let long = "word ".repeat(40);
        let cut = title(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= TITLE_CHARS + 1);
        assert_eq!(title("  \n\n"), "Untitled session");
        assert_eq!(title("\u{1b}[2Jhi"), "[2Jhi");
    }
}

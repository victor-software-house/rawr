//! Every resumable local Claude Code session, for search.
//!
//! Claude Code appends one line per typed prompt to `history.jsonl`, naming
//! the session and the folder it started in. That one file is the search
//! index: it holds the words a user remembers a session by. One listing of
//! `projects/*/` says which sessions still have a transcript. No transcript
//! is opened.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind};
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::session::{Agent, sanitize};

/// The longest title drawn from a first prompt, in characters.
const TITLE_CHARS: usize = 120;

/// A session search can find.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) agent: Agent,
    /// Its prompts, folder and ID, lowercase, one per line.
    text: String,
    /// When its latest prompt was typed, in milliseconds since the epoch.
    latest: u64,
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
        let Some(modified) = transcripts.get(id) else {
            continue;
        };
        let found = sessions.entry(id.to_owned()).or_insert_with(|| Found {
            agent: Agent {
                session_id: id.to_owned(),
                cwd: prompt.project.as_ref().to_owned(),
                title: title(&prompt.display),
                idle: modified.and_then(|at| now.duration_since(at).ok()),
            },
            text: format!("{}\n{}\n", prompt.project.to_lowercase(), id.to_lowercase()),
            latest: 0,
        });
        found.text.push_str(&prompt.display.to_lowercase());
        found.text.push('\n');
        found.latest = found.latest.max(prompt.timestamp);
    }
    let mut found: Vec<Found> = sessions.into_values().collect();
    found.sort_by_key(|found| std::cmp::Reverse(found.latest));
    Ok(found)
}

/// Every `projects/*/<session id>.jsonl`, by session ID, with the time it was
/// last written.
fn transcripts(projects: &Path) -> Result<HashMap<String, Option<SystemTime>>> {
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
            if let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) {
                let modified = file.metadata().and_then(|meta| meta.modified()).ok();
                found.insert(id.to_owned(), modified);
            }
        }
    }
    Ok(found)
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
        fs::write(notes.join(format!("{NOTES}.jsonl")), "{}\n").unwrap();
        fs::write(
            dir.join("history.jsonl"),
            formatdoc!(
                r#"
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

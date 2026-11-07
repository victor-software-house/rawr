//! What the picker knows about a Claude session: whether a live process holds
//! it, its title, and how long ago it was active.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use memchr::memmem;
use rustix::process::{Pid, test_kill_process};
use serde::Deserialize;

use crate::index::Entry;

/// A session the picker can offer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Agent {
    pub(crate) session_id: String,
    pub(crate) cwd: String,
    pub(crate) title: String,
    /// Time since the transcript was last written.
    pub(crate) idle: Option<Duration>,
}

impl Agent {
    /// The shell line that resumes this session from its start folder.
    pub(crate) fn resume_command(&self) -> String {
        format!(
            "cd -- {} && claude --resume {}",
            shell_quote(&self.cwd),
            shell_quote(&self.session_id)
        )
    }

    /// The start folder with the home directory written as `~`.
    pub(crate) fn short_cwd(&self) -> String {
        let home = std::env::home_dir();
        match home.as_deref().and_then(Path::to_str) {
            Some(home) if self.cwd == home => "~".to_owned(),
            Some(home) => match self.cwd.strip_prefix(home) {
                Some(rest) if rest.starts_with('/') => format!("~{rest}"),
                _ => self.cwd.clone(),
            },
            None => self.cwd.clone(),
        }
    }
}

/// Quotes a word for POSIX shells and zsh: single quotes, with each `'`
/// written as `'\''`.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The record Claude Code keeps per running process.
#[derive(Debug, Deserialize)]
struct Running {
    pid: i32,
    #[serde(rename = "sessionId")]
    session_id: String,
}

/// Claude Code's configuration folder: `$CLAUDE_CONFIG_DIR`, or `~/.claude`.
fn claude_dir() -> Result<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir)),
        _ => Ok(std::env::home_dir()
            .context("no home directory")?
            .join(".claude")),
    }
}

fn is_alive(pid: i32) -> bool {
    Pid::from_raw(pid).is_some_and(|pid| match test_kill_process(pid) {
        Ok(()) => true,
        Err(error) => error == rustix::io::Errno::PERM,
    })
}

/// Session IDs that a live Claude process holds now.
pub(crate) fn live_sessions() -> Result<HashSet<String>> {
    let dir = claude_dir()?.join("sessions");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(error).with_context(|| format!("list {}", dir.display())),
    };
    let mut live = HashSet::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if let Ok(running) = serde_json::from_slice::<Running>(&bytes)
            && is_alive(running.pid)
        {
            live.insert(running.session_id);
        }
    }
    Ok(live)
}

/// The pane's sessions that still have a transcript and no live process,
/// newest first.
pub(crate) fn resumable(entries: &[Entry], live: &HashSet<String>) -> Vec<Agent> {
    let now = SystemTime::now();
    entries
        .iter()
        .filter(|entry| !live.contains(&entry.session_id))
        .filter_map(|entry| {
            let modified = fs::metadata(&entry.transcript)
                .and_then(|meta| meta.modified())
                .ok()?;
            let cwd = entry.cwd.to_str()?.to_owned();
            Some(Agent {
                session_id: entry.session_id.clone(),
                cwd,
                title: transcript_title(&entry.transcript)
                    .map_or_else(|| "Untitled session".to_owned(), |title| sanitize(&title)),
                idle: now.duration_since(modified).ok(),
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct TitleRow {
    #[serde(rename = "customTitle")]
    custom: Option<String>,
    #[serde(rename = "aiTitle")]
    ai: Option<String>,
}

/// The session's title: the last title the user set, else the last one
/// Claude generated.
fn transcript_title(path: &Path) -> Option<String> {
    let custom_marker = memmem::Finder::new(br#""type":"custom-title""#);
    let ai_marker = memmem::Finder::new(br#""type":"ai-title""#);
    let mut reader = BufReader::new(File::open(path).ok()?);
    let mut line = Vec::new();
    let (mut custom, mut ai) = (None, None);
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let is_custom = custom_marker.find(&line).is_some();
        if !is_custom && ai_marker.find(&line).is_none() {
            continue;
        }
        let Ok(row) = serde_json::from_slice::<TitleRow>(&line) else {
            continue;
        };
        if is_custom {
            custom = row.custom.or(custom);
        } else {
            ai = row.ai.or(ai);
        }
    }
    custom.or(ai).filter(|title| !title.trim().is_empty())
}

/// `text` safe to draw: control characters dropped, tabs and line breaks
/// turned into spaces. Titles come from transcripts, so an escape sequence in
/// one must not reach the terminal.
fn sanitize(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' | '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// How long ago, in the largest whole unit.
pub(crate) fn ago(idle: Option<Duration>) -> String {
    let Some(idle) = idle else {
        return "time unknown".to_owned();
    };
    let minutes = idle.as_secs() / 60;
    match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => match minutes / 1440 {
            1 => "1 day ago".to_owned(),
            days => format!("{days} days ago"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(cwd: &str) -> Agent {
        Agent {
            session_id: "6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33".to_owned(),
            cwd: cwd.to_owned(),
            title: "t".to_owned(),
            idle: None,
        }
    }

    #[test]
    fn resume_command_quotes_the_folder() {
        assert_eq!(
            agent("/home/dev/code/it's shop").resume_command(),
            r"cd -- '/home/dev/code/it'\''s shop' && claude --resume '6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33'"
        );
    }

    #[test]
    fn title_prefers_the_user_title() {
        let dir = std::env::temp_dir().join(format!("rex-resume-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        fs::write(
            &path,
            indoc::indoc! {r#"
                {"type":"ai-title","aiTitle":"First guess"}
                {"type":"user","message":"hi"}
                {"type":"custom-title","customTitle":"Named by me"}
                {"type":"ai-title","aiTitle":"Later guess"}
            "#},
        )
        .unwrap();
        assert_eq!(transcript_title(&path).as_deref(), Some("Named by me"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sanitize_drops_escape_sequences() {
        assert_eq!(sanitize("fix\u{1b}[2Jbuild"), "fix[2Jbuild");
        assert_eq!(sanitize("two\nlines\tand a tab"), "two lines and a tab");
    }

    #[test]
    fn ago_uses_the_largest_unit() {
        assert_eq!(ago(Some(Duration::from_secs(30))), "just now");
        assert_eq!(ago(Some(Duration::from_mins(5))), "5 min ago");
        assert_eq!(ago(Some(Duration::from_hours(3))), "3 h ago");
        assert_eq!(ago(Some(Duration::from_hours(24))), "1 day ago");
        assert_eq!(ago(Some(Duration::from_hours(72))), "3 days ago");
        assert_eq!(ago(None), "time unknown");
    }
}

//! What the picker knows about a Claude session: whether a live process holds
//! it, its title, and how long ago it was active.

use std::collections::HashSet;
use std::convert::Infallible;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use memchr::memmem;
use rustix::process::{Pid, test_kill_process};
use serde::Deserialize;

use crate::index::Entry;

/// A session the picker can offer.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub(crate) struct Agent {
    pub(crate) session_id: String,
    pub(crate) cwd: String,
    pub(crate) title: String,
    /// Time since the transcript was last written.
    pub(crate) idle: Option<Duration>,
    /// The transcript, which the app reads for the session's facts.
    pub(crate) transcript: PathBuf,
}

impl Agent {
    /// Resumes this session in its start folder: the process becomes
    /// `claude --resume <session id>` and returns only when that fails.
    ///
    /// # Errors
    ///
    /// Fails when the start folder is gone or `claude` cannot start.
    pub(crate) fn resume(&self) -> Result<Infallible> {
        if !Path::new(&self.cwd).is_dir() {
            bail!("{} no longer exists", self.cwd);
        }
        let error = std::process::Command::new("claude")
            .arg("--resume")
            .arg(&self.session_id)
            .current_dir(&self.cwd)
            .exec();
        Err(error).context("start claude")
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

/// The record Claude Code keeps per running process.
#[derive(Debug, Deserialize)]
struct Running {
    pid: i32,
    #[serde(rename = "sessionId")]
    session_id: String,
}

/// Claude Code's configuration folder: `$CLAUDE_CONFIG_DIR`, or `~/.claude`.
pub(crate) fn claude_dir() -> Result<PathBuf> {
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
                transcript: entry.transcript.clone(),
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

/// How much [`transcript_title`] reads from each end of a transcript: Claude
/// Code's own session list reads the same 64 KiB at each end
/// (`LITE_READ_BUF_SIZE`). Its writer appends the session's title rows again
/// once 32 KiB have been written since the last time, and again at exit, so
/// the last 64 KiB hold the current titles even after a long line. A title
/// row anywhere else in the middle is not read.
const TITLE_WINDOW: u64 = 64 << 10;

/// Which title row [`last_row`] looks for.
#[derive(Debug, Clone, Copy)]
enum Title {
    /// A title the user set: `{"type":"custom-title","customTitle":...}`.
    User,
    /// A title Claude Code generated: `{"type":"ai-title","aiTitle":...}`.
    Generated,
}

impl Title {
    const fn marker(self) -> &'static [u8] {
        match self {
            Self::User => br#""type":"custom-title""#,
            Self::Generated => br#""type":"ai-title""#,
        }
    }
}

/// The `custom-title.json` file Claude Code keeps beside a session's
/// transcript, at `<project dir>/<session id>/custom-title.json`.
#[derive(Debug, Deserialize)]
struct TitleFile {
    #[serde(rename = "customTitle")]
    custom: String,
}

/// The session's title, found as Claude Code's session list finds it. The
/// title the user set comes from the transcript's last 64 KiB, else from its
/// `custom-title.json` file, else from the first 64 KiB. Failing that, the
/// generated title comes from the last 64 KiB, else the first. Each window
/// skips the partial line at its cut.
/// The folder a transcript's session ran in, from the first of its first
/// lines that names one.
pub(crate) fn transcript_cwd(path: &Path) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Row {
        cwd: Option<String>,
    }
    let file = File::open(path).ok()?;
    std::io::BufRead::lines(std::io::BufReader::new(file))
        .take(50)
        .map_while(Result::ok)
        .find_map(|line| serde_json::from_str::<Row>(&line).ok()?.cwd)
}

pub(crate) fn transcript_title(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let tail = read_at(&mut file, len.saturating_sub(TITLE_WINDOW))?;
    if len <= TITLE_WINDOW {
        return last_row(&tail, Title::User)
            .or_else(|| title_file(path))
            .or_else(|| last_row(&tail, Title::Generated));
    }
    let tail_lines = after_first_break(&tail);
    if let Some(title) = last_row(tail_lines, Title::User).or_else(|| title_file(path)) {
        return Some(title);
    }
    let head = read_at(&mut file, 0)?;
    let head_lines = before_last_break(&head);
    last_row(head_lines, Title::User)
        .or_else(|| last_row(tail_lines, Title::Generated))
        .or_else(|| last_row(head_lines, Title::Generated))
}

/// Up to [`TITLE_WINDOW`] bytes of `file` from `offset`.
fn read_at(file: &mut File, offset: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(TITLE_WINDOW)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// `tail` after the partial line it starts in; empty when it holds no other
/// line break than the one ending the file.
fn after_first_break(tail: &[u8]) -> &[u8] {
    let body = tail.strip_suffix(b"\n").unwrap_or(tail);
    memchr::memchr(b'\n', body).map_or(&[], |cut| &body[cut + 1..])
}

/// `head` before the partial line it ends in.
fn before_last_break(head: &[u8]) -> &[u8] {
    memchr::memrchr(b'\n', head).map_or(&[], |cut| &head[..cut])
}

/// The title in the last `kind` row of `lines` that parses, unless blank.
fn last_row(lines: &[u8], kind: Title) -> Option<String> {
    let marker = memmem::Finder::new(kind.marker());
    lines
        .rsplit(|&byte| byte == b'\n')
        .filter(|line| marker.find(line).is_some())
        .find_map(|line| {
            let row = serde_json::from_slice::<TitleRow>(line).ok()?;
            match kind {
                Title::User => row.custom,
                Title::Generated => row.ai,
            }
        })
        .filter(|title| !title.trim().is_empty())
}

/// The title in the session's `custom-title.json`, unless missing or blank.
fn title_file(transcript: &Path) -> Option<String> {
    let bytes = fs::read(transcript.with_extension("").join("custom-title.json")).ok()?;
    let file = serde_json::from_slice::<TitleFile>(&bytes).ok()?;
    Some(file.custom).filter(|title| !title.trim().is_empty())
}

/// `text` safe to draw: control characters dropped, tabs and line breaks
/// turned into spaces. Titles come from transcripts, so an escape sequence in
/// one must not reach the terminal.
pub(crate) fn sanitize(text: &str) -> String {
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
            transcript: PathBuf::new(),
        }
    }

    #[test]
    fn a_gone_folder_is_named_instead_of_resumed() {
        let error = agent("/home/dev/code/gone").resume().unwrap_err();
        assert_eq!(error.to_string(), "/home/dev/code/gone no longer exists");
    }

    #[test]
    fn title_prefers_the_user_title() {
        let dir = std::env::temp_dir().join(format!("rawr-test-{}", std::process::id()));
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
    fn a_user_title_in_the_head_beats_a_generated_one_in_the_tail() {
        let dir = std::env::temp_dir().join(format!("rawr-tail-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!(
            "{{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let mut text =
            String::from("{\"type\":\"custom-title\",\"customTitle\":\"Named first\"}\n");
        for _ in 0..2000 {
            text.push_str(&filler);
        }
        text.push_str("{\"type\":\"ai-title\",\"aiTitle\":\"Near the end\"}\n");
        fs::write(&path, text).unwrap();
        assert_eq!(transcript_title(&path).as_deref(), Some("Named first"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_user_title_beats_a_later_generated_one_across_the_window() {
        let dir = std::env::temp_dir().join(format!("rawr-order-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!(
            "{{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let mut text = filler.repeat(100);
        text.push_str("{\"type\":\"custom-title\",\"customTitle\":\"Named by me\"}\n");
        text.push_str(&filler.repeat(40));
        text.push_str("{\"type\":\"ai-title\",\"aiTitle\":\"Later guess\"}\n");
        text.push_str(&filler.repeat(10));
        fs::write(&path, text).unwrap();
        assert_eq!(transcript_title(&path).as_deref(), Some("Named by me"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_title_between_the_windows_is_not_read() {
        let dir = std::env::temp_dir().join(format!("rawr-middle-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!(
            "{{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let mut text = filler.repeat(100);
        text.push_str("{\"type\":\"ai-title\",\"aiTitle\":\"In the middle\"}\n");
        text.push_str(&filler.repeat(100));
        fs::write(&path, text).unwrap();
        assert_eq!(transcript_title(&path), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_title_file_beats_a_generated_title_in_the_tail() {
        let dir = std::env::temp_dir().join(format!("rawr-file-{}", std::process::id()));
        fs::create_dir_all(dir.join("7c2e5a10-4b8d-4f61-9a3e-2d5b8c1f6e04")).unwrap();
        let path = dir.join("7c2e5a10-4b8d-4f61-9a3e-2d5b8c1f6e04.jsonl");
        fs::write(&path, "{\"type\":\"ai-title\",\"aiTitle\":\"A guess\"}\n").unwrap();
        fs::write(
            dir.join("7c2e5a10-4b8d-4f61-9a3e-2d5b8c1f6e04/custom-title.json"),
            r#"{"customTitle":"Named in the file"}"#,
        )
        .unwrap();
        assert_eq!(
            transcript_title(&path).as_deref(),
            Some("Named in the file")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_user_title_in_the_tail_beats_the_title_file() {
        let dir = std::env::temp_dir().join(format!("rawr-both-{}", std::process::id()));
        fs::create_dir_all(dir.join("0b9d4e2a-6c71-4f3e-8a52-9e1c7d3b5f20")).unwrap();
        let path = dir.join("0b9d4e2a-6c71-4f3e-8a52-9e1c7d3b5f20.jsonl");
        fs::write(
            &path,
            "{\"type\":\"custom-title\",\"customTitle\":\"Named later\"}\n",
        )
        .unwrap();
        fs::write(
            dir.join("0b9d4e2a-6c71-4f3e-8a52-9e1c7d3b5f20/custom-title.json"),
            r#"{"customTitle":"Named earlier"}"#,
        )
        .unwrap();
        assert_eq!(transcript_title(&path).as_deref(), Some("Named later"));
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

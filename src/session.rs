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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Agent {
    pub(crate) session_id: String,
    pub(crate) cwd: String,
    pub(crate) title: String,
    /// Time since the transcript was last written.
    pub(crate) idle: Option<Duration>,
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

/// How much of a transcript's end [`transcript_title`] reads. Transcripts
/// reach hundreds of megabytes, and Claude Code writes the title rows again
/// every few dozen kilobytes, the user's title next to each generated one, so
/// the end holds the current title. Of 167 local transcripts, 144 had a title
/// row: the last user title always sat within 31.6 KB of the end, and the
/// last generated title within 33.5 KB, but for one 2.6 MB transcript whose
/// only title row was its first line. Of 272 later transcripts, 99 had no
/// title row in their last 64 KiB; reading their last 1 MiB as well found no
/// title and read 59.3 MB more on every load.
const TITLE_WINDOW: u64 = 64 << 10;

/// A final line longer than this leaves too little of [`TITLE_WINDOW`] for
/// the lines before it, so [`transcript_title`] reads [`LONG_LINE_WINDOW`]
/// when the window holds no title. Such a line is a large tool result or
/// message, and the title rows sit before it.
const LONG_LINE: usize = 32 << 10;

/// The read [`transcript_title`] falls back to after a long final line. Of 272
/// local transcripts, none would have needed it, and the longest line in any
/// transcript's last 1 MiB was 674,647 bytes.
const LONG_LINE_WINDOW: u64 = 1 << 20;

/// The session's title: the last title the user set, else the last one
/// Claude generated, both read from the transcript's last 64 KiB, skipping
/// the partial line the window starts in. When those 64 KiB hold no title and
/// the final line is longer than [`LONG_LINE`], the last 1 MiB is read
/// instead. A title row further back is not read.
pub(crate) fn transcript_title(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let tail = read_tail(&mut file, len, TITLE_WINDOW)?;
    if len <= TITLE_WINDOW {
        return last_title(&tail);
    }
    let body = tail.strip_suffix(b"\n").unwrap_or(&tail);
    let final_line = body.len() - memchr::memrchr(b'\n', body).map_or(0, |at| at + 1);
    let title = whole_lines(body).and_then(last_title);
    if title.is_some() || final_line <= LONG_LINE {
        return title;
    }
    let wide = read_tail(&mut file, len, LONG_LINE_WINDOW)?;
    if len <= LONG_LINE_WINDOW {
        return last_title(&wide);
    }
    whole_lines(wide.strip_suffix(b"\n").unwrap_or(&wide)).and_then(last_title)
}

/// The last `window` bytes of `file`, whose length is `len`.
fn read_tail(file: &mut File, len: u64, window: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(len.saturating_sub(window)))
        .ok()?;
    let mut tail = Vec::new();
    file.by_ref().take(window).read_to_end(&mut tail).ok()?;
    Some(tail)
}

/// `body` after the partial line it starts in, or `None` when it holds no
/// line break.
fn whole_lines(body: &[u8]) -> Option<&[u8]> {
    memchr::memchr(b'\n', body).map(|cut| &body[cut + 1..])
}

/// The last user title in `lines`, else the last generated one.
fn last_title(lines: &[u8]) -> Option<String> {
    let custom_marker = memmem::Finder::new(br#""type":"custom-title""#);
    let ai_marker = memmem::Finder::new(br#""type":"ai-title""#);
    let (mut custom, mut ai) = (None, None);
    for line in lines.rsplit(|&byte| byte == b'\n') {
        let is_custom = custom_marker.find(line).is_some();
        if !is_custom && (ai.is_some() || ai_marker.find(line).is_none()) {
            continue;
        }
        let Ok(row) = serde_json::from_slice::<TitleRow>(line) else {
            continue;
        };
        if is_custom {
            if let Some(title) = row.custom {
                custom = Some(title);
                break;
            }
        } else {
            ai = row.ai;
        }
    }
    custom.or(ai).filter(|title| !title.trim().is_empty())
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
    fn the_title_comes_from_the_transcript_s_end() {
        let dir = std::env::temp_dir().join(format!("rawr-tail-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!(
            "{{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let mut text =
            String::from("{\"type\":\"custom-title\",\"customTitle\":\"Too far back\"}\n");
        for _ in 0..2000 {
            text.push_str(&filler);
        }
        text.push_str("{\"type\":\"ai-title\",\"aiTitle\":\"Near the end\"}\n");
        fs::write(&path, text).unwrap();
        assert_eq!(transcript_title(&path).as_deref(), Some("Near the end"));
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
    fn a_title_beyond_the_window_is_not_read() {
        let dir = std::env::temp_dir().join(format!("rawr-window-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!(
            "{{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let mut text = String::from("{\"type\":\"ai-title\",\"aiTitle\":\"Further back\"}\n");
        text.push_str(&filler.repeat(200));
        fs::write(&path, text).unwrap();
        assert_eq!(transcript_title(&path), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_title_before_a_line_longer_than_the_window_is_found() {
        let dir = std::env::temp_dir().join(format!("rawr-long-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let text = format!(
            "{{\"type\":\"custom-title\",\"customTitle\":\"Named by me\"}}\n\
             {{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(100_000)
        );
        fs::write(&path, text).unwrap();
        assert_eq!(transcript_title(&path).as_deref(), Some("Named by me"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_title_before_a_long_line_under_the_window_is_found() {
        let dir = std::env::temp_dir().join(format!("rawr-under-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!(
            "{{\"type\":\"user\",\"message\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let text = format!(
            "{{\"type\":\"custom-title\",\"customTitle\":\"Named by me\"}}\n{}\
             {{\"type\":\"user\",\"message\":\"{}\"}}\n",
            filler.repeat(20),
            "x".repeat(55_000)
        );
        fs::write(&path, text).unwrap();
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

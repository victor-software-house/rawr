//! The pane index: one JSON file per Rex block, holding the Claude sessions
//! that last ran in that pane, newest first.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::rex::{Client, Endpoint};

/// How long a session may go without a transcript file before the index drops
/// it, so one that has not written its first message yet stays.
const NO_TRANSCRIPT: Duration = Duration::from_hours(24);

/// How far before its listing `prune` starts keeping new files. Linux stamps
/// a file's mtime from a coarse clock that can lag the clock
/// [`SystemTime::now`] reads by a few milliseconds, so a file saved just after
/// the listing could otherwise look older than it. A stale file written in
/// this second goes at the next prune.
const MTIME_SLACK: Duration = Duration::from_secs(1);

/// A Rex block ID, such as `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BlockId(String);

impl BlockId {
    /// Parses a block ID, with or without its `block:` prefix, refusing
    /// anything that could escape the index directory as a file name.
    pub(crate) fn try_new(raw: &str) -> Result<Self> {
        let id = raw.strip_prefix("block:").unwrap_or(raw);
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            bail!("not a Rex block id: {raw}");
        }
        Ok(Self(id.to_owned()))
    }

    fn file_name(&self) -> String {
        format!("{}.json", self.0)
    }
}

/// The part of Claude Code's `SessionStart` hook input that `record` keeps.
#[derive(Debug, Deserialize)]
pub(crate) struct HookInput {
    session_id: String,
    transcript_path: PathBuf,
    cwd: PathBuf,
}

impl HookInput {
    /// The entry for this session, recorded at `recorded_at`.
    pub(crate) fn into_entry(self, recorded_at: u64) -> Entry {
        Entry {
            session_id: self.session_id,
            transcript: self.transcript_path,
            cwd: self.cwd,
            recorded_at,
            transcript_seen: false,
        }
    }
}

/// One Claude session that ran in a pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) session_id: String,
    /// The folder the session started in; `claude --resume` finds the
    /// session through it.
    pub(crate) cwd: PathBuf,
    pub(crate) transcript: PathBuf,
    /// Seconds since the Unix epoch when `record` stored the entry; 0 in a
    /// file written before the field existed.
    #[serde(default)]
    pub(crate) recorded_at: u64,
    /// Whether rawr has found the transcript file; a transcript that was
    /// seen and is gone was swept by Claude Code.
    #[serde(default)]
    pub(crate) transcript_seen: bool,
}

/// The sessions of one pane, newest first.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct PaneIndex {
    entries: Vec<Entry>,
}

impl PaneIndex {
    /// Reads a pane's index; a pane with no file has an empty one.
    pub(crate) fn load(dir: &Path, block: &BlockId) -> Result<Self> {
        let path = dir.join(block.file_name());
        match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
        }
    }

    /// Writes the index through a temporary file, so a reader never sees a
    /// partial one.
    pub(crate) fn save(&self, dir: &Path, block: &BlockId) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        let path = dir.join(block.file_name());
        let temporary = dir.join(format!(".{}.tmp", block.file_name()));
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&temporary, bytes).with_context(|| format!("write {}", temporary.display()))?;
        fs::rename(&temporary, &path).with_context(|| format!("replace {}", path.display()))
    }

    /// Puts a session first, dropping its older entry and anything past
    /// `per_pane` sessions.
    pub(crate) fn add(&mut self, mut entry: Entry, per_pane: usize) {
        if let Some(old) = self
            .entries
            .iter()
            .find(|e| e.session_id == entry.session_id)
        {
            entry.transcript_seen |= old.transcript_seen && old.transcript == entry.transcript;
        }
        self.entries.retain(|e| e.session_id != entry.session_id);
        self.entries.insert(0, entry);
        self.entries.truncate(per_pane);
    }

    /// Marks each entry whose transcript file exists as seen, and drops an
    /// entry whose transcript was seen and is gone, or that has had no
    /// transcript for more than 24 hours since it was recorded. `now` is
    /// seconds since the Unix epoch.
    ///
    /// A transcript that cannot be checked, such as behind an unreadable
    /// folder, counts as present: nothing is dropped on a guess.
    pub(crate) fn prune_missing(&mut self, now: u64) {
        self.entries.retain_mut(|entry| {
            let gone = matches!(
                fs::metadata(&entry.transcript),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            );
            if gone {
                return !entry.transcript_seen
                    && now.saturating_sub(entry.recorded_at) <= NO_TRANSCRIPT.as_secs();
            }
            entry.transcript_seen = true;
            true
        });
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

/// Seconds since the Unix epoch.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// `$XDG_STATE_HOME/rawr`, or `~/.local/state/rawr`.
pub(crate) fn state_dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::home_dir()
            .context("no home directory")?
            .join(".local/state"),
    };
    Ok(base.join("rawr"))
}

/// Every block the Rex server has now. Fails if any listing fails, so a
/// partial answer never counts as the whole server.
fn live_blocks() -> Result<HashSet<BlockId>> {
    let mut client = Client::connect(&Endpoint::from_environment()?)?;
    let mut blocks = HashSet::new();
    for session in client.sessions()? {
        for block in client.blocks(&session)? {
            blocks.insert(BlockId::try_new(&block.id)?);
        }
    }
    Ok(blocks)
}

/// Deletes the index files of blocks the Rex server no longer has.
///
/// A file written after the listing began, less [`MTIME_SLACK`], is kept: it
/// belongs to a pane the listing may have missed, such as one created while
/// another pane's `record` ran.
pub(crate) fn prune(dir: &Path) -> Result<()> {
    let listed = SystemTime::now()
        .checked_sub(MTIME_SLACK)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let live = live_blocks()?;
    let keep: HashSet<String> = live.iter().map(BlockId::file_name).collect();
    for entry in fs::read_dir(dir).with_context(|| format!("list {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let is_index = name
            .strip_suffix(".json")
            .is_some_and(|stem| BlockId::try_new(&format!("block:{stem}")).is_ok());
        let written_since = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| modified >= listed);
        if is_index && !keep.contains(name) && !written_since {
            fs::remove_file(entry.path())
                .with_context(|| format!("remove {}", entry.path().display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> Entry {
        Entry {
            session_id: id.to_owned(),
            cwd: PathBuf::from("/tmp"),
            transcript: PathBuf::from(format!("/tmp/{id}.jsonl")),
            recorded_at: 0,
            transcript_seen: false,
        }
    }

    /// An entry recorded at `recorded_at` whose transcript is `transcript`.
    fn recorded(id: &str, transcript: &Path, recorded_at: u64) -> Entry {
        Entry {
            transcript: transcript.to_owned(),
            recorded_at,
            ..entry(id)
        }
    }

    /// An empty folder of its own for a test to put transcripts in.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rawr-index-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ids(pane: &PaneIndex) -> Vec<&str> {
        pane.entries()
            .iter()
            .map(|e| e.session_id.as_str())
            .collect()
    }

    const HOUR: u64 = 60 * 60;
    const NOW: u64 = 1_800_000_000;

    #[test]
    fn block_id_accepts_rex_ids() {
        let block = BlockId::try_new("block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08").unwrap();
        assert_eq!(
            block.file_name(),
            "0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08.json"
        );
    }

    #[test]
    fn block_id_takes_the_prefix_as_optional() {
        assert_eq!(
            BlockId::try_new("0190f3a2-7c41").unwrap(),
            BlockId::try_new("block:0190f3a2-7c41").unwrap()
        );
    }

    #[test]
    fn block_id_refuses_paths() {
        assert!(BlockId::try_new("block:../../etc").is_err());
        assert!(BlockId::try_new("../../etc").is_err());
        assert!(BlockId::try_new("block:").is_err());
        assert!(BlockId::try_new("").is_err());
    }

    #[test]
    fn add_moves_a_session_first_and_keeps_five() {
        let mut pane = PaneIndex::default();
        for id in ["a", "b", "c", "d", "e", "f"] {
            pane.add(entry(id), 5);
        }
        pane.add(entry("c"), 5);
        assert_eq!(ids(&pane), ["c", "f", "e", "d", "b"]);
    }

    #[test]
    fn re_adding_a_seen_session_keeps_it_seen() {
        let path = PathBuf::from("/tmp/s1.jsonl");
        let mut pane = PaneIndex::default();
        let mut seen = recorded("s1", &path, NOW - 60);
        seen.transcript_seen = true;
        pane.add(seen, 5);

        pane.add(recorded("s1", &path, NOW), 5);
        assert_eq!(ids(&pane), ["s1"]);
        assert!(pane.entries()[0].transcript_seen);
        assert_eq!(pane.entries()[0].recorded_at, NOW);

        pane.add(recorded("s1", Path::new("/tmp/moved.jsonl"), NOW), 5);
        assert!(!pane.entries()[0].transcript_seen);
    }

    #[test]
    fn a_smaller_bound_drops_the_oldest() {
        let mut pane = PaneIndex::default();
        for id in ["a", "b", "c", "d", "e"] {
            pane.add(entry(id), 4);
        }
        assert_eq!(ids(&pane), ["e", "d", "c", "b"]);
    }

    #[test]
    fn a_file_written_before_the_new_fields_still_loads() {
        let dir = folder("old-file");
        let block = BlockId::try_new("block:0190f3a2-7c41").unwrap();
        fs::write(
            dir.join(block.file_name()),
            r#"{"entries":[{"session_id":"s1","cwd":"/work/shop","transcript":"/work/s1.jsonl"}]}"#,
        )
        .unwrap();
        let pane = PaneIndex::load(&dir, &block).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(ids(&pane), ["s1"]);
        assert_eq!(pane.entries()[0].recorded_at, 0);
        assert!(!pane.entries()[0].transcript_seen);
    }

    #[test]
    fn a_swept_transcript_is_dropped() {
        let dir = folder("swept");
        let path = dir.join("6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33.jsonl");
        fs::write(&path, "{}\n").unwrap();
        let mut pane = PaneIndex::default();
        pane.add(
            recorded("6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33", &path, NOW),
            5,
        );
        pane.prune_missing(NOW);
        assert!(pane.entries()[0].transcript_seen);

        fs::remove_file(&path).unwrap();
        pane.prune_missing(NOW);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(pane.entries(), []);
    }

    #[test]
    fn a_session_just_started_stays() {
        let dir = folder("just-started");
        let path = dir.join("s1.jsonl");
        let mut pane = PaneIndex::default();
        pane.add(recorded("s1", &path, NOW - 60), 5);
        pane.prune_missing(NOW);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(ids(&pane), ["s1"]);
        assert!(!pane.entries()[0].transcript_seen);
    }

    #[test]
    fn a_session_never_used_goes_after_a_day() {
        let dir = folder("never-used");
        let path = dir.join("s1.jsonl");
        let mut pane = PaneIndex::default();
        pane.add(recorded("s1", &path, NOW - 24 * HOUR), 5);
        pane.prune_missing(NOW);
        assert_eq!(ids(&pane), ["s1"], "24 hours exactly is not more than 24");

        pane.prune_missing(NOW + 1);
        assert_eq!(pane.entries(), []);

        pane.add(recorded("s2", &path, NOW - 25 * HOUR), 5);
        pane.prune_missing(NOW);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(pane.entries(), []);
    }

    #[test]
    fn an_entry_without_a_record_time_and_without_a_transcript_is_old() {
        let dir = folder("no-time");
        let path = dir.join("s1.jsonl");
        let mut pane = PaneIndex::default();
        pane.add(recorded("s1", &path, 0), 5);
        pane.prune_missing(NOW);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(pane.entries(), []);
    }

    #[test]
    fn a_transcript_found_later_marks_the_entry_seen() {
        let dir = folder("found-later");
        let path = dir.join("s1.jsonl");
        let mut pane = PaneIndex::default();
        pane.add(recorded("s1", &path, NOW - 60), 5);
        pane.prune_missing(NOW);
        assert!(!pane.entries()[0].transcript_seen);

        fs::write(&path, "{}\n").unwrap();
        pane.prune_missing(NOW);
        fs::remove_dir_all(&dir).unwrap();
        assert!(pane.entries()[0].transcript_seen);
    }
}

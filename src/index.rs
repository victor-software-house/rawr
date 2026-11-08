//! The pane index: one JSON file per Rex block, holding the Claude sessions
//! that last ran in that pane, newest first.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// How many sessions a pane remembers.
const KEEP: usize = 5;

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
    pub(crate) fn into_entry(self) -> Entry {
        Entry {
            session_id: self.session_id,
            transcript: self.transcript_path,
            cwd: self.cwd,
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
    /// [`KEEP`].
    pub(crate) fn add(&mut self, entry: Entry) {
        self.entries.retain(|e| e.session_id != entry.session_id);
        self.entries.insert(0, entry);
        self.entries.truncate(KEEP);
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }
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

#[derive(Debug, Deserialize)]
struct SessionList {
    sessions: Vec<SessionRow>,
}

#[derive(Debug, Deserialize)]
struct SessionRow {
    session_id: String,
}

#[derive(Debug, Deserialize)]
struct BlockList {
    blocks: Vec<BlockRow>,
}

#[derive(Debug, Deserialize)]
struct BlockRow {
    block_id: String,
}

/// Runs `rex` and parses its JSON output.
fn rex_json<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<T> {
    let output = Command::new("rex")
        .args(args)
        .output()
        .context("start rex")?;
    if !output.status.success() {
        bail!(
            "rex {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).with_context(|| format!("parse rex {}", args.join(" ")))
}

/// Every block the Rex server has now. Fails if any listing fails, so a
/// partial answer never counts as the whole server.
fn live_blocks() -> Result<HashSet<BlockId>> {
    let sessions: SessionList = rex_json(&["ls", "--json"])?;
    let mut blocks = HashSet::new();
    for session in sessions.sessions {
        let list: BlockList = rex_json(&["block", "ls", "-s", &session.session_id, "--json"])?;
        for block in list.blocks {
            blocks.insert(BlockId::try_new(&block.block_id)?);
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
        }
    }

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
            pane.add(entry(id));
        }
        pane.add(entry("c"));
        let ids: Vec<&str> = pane
            .entries()
            .iter()
            .map(|e| e.session_id.as_str())
            .collect();
        assert_eq!(ids, ["c", "f", "e", "d", "b"]);
    }
}

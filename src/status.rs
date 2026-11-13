//! `rawr status`: one row per Rex pane whose index lists a Claude session,
//! with the status word taken from the pane's program status records.

use std::path::Path;

use anyhow::Result;
use ctl_core::prelude::*;
use serde::Serialize;

use crate::index::{BlockId, Entry, PaneIndex};
use crate::rex::{Block, Client, Session, State, StatusRecord};
use crate::session;

/// The program name Claude Code reports its status under.
const CLAUDE: &str = "claude-code";

/// What a pane's Claude is doing, as Claude Code's agents view words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Working,
    NeedsInput,
    Idle,
    Failed,
    /// No Claude reports a status in the pane, though its index lists a
    /// session.
    Stopped,
}

impl Status {
    /// The word `rawr status` shows.
    const fn word(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::NeedsInput => "Needs input",
            Self::Idle => "Idle",
            Self::Failed => "Failed",
            Self::Stopped => "Stopped",
        }
    }

    /// The word of a block, from its program status records.
    ///
    /// Only records of Claude Code count: a program that took the pane over
    /// after Claude exited reports its own status, and the pane's Claude has
    /// stopped. A root record that waits for the user reads Needs input even
    /// while a child runs, because the user has to act. Otherwise a root that
    /// is working, or any working child record (a background task), reads
    /// Working; a child that has finished counts for nothing.
    pub(crate) fn from_records(records: &[StatusRecord]) -> Self {
        let claude: Vec<&StatusRecord> = records
            .iter()
            .filter(|record| {
                record.effective_app.as_deref().or(record.app.as_deref()) == Some(CLAUDE)
            })
            .collect();
        if claude.is_empty() {
            return Self::Stopped;
        }
        let root = claude
            .iter()
            .find(|record| record.id.is_empty())
            .map(|record| record.state);
        let working_child = claude
            .iter()
            .any(|record| !record.id.is_empty() && record.state == State::Working);
        match (root, working_child) {
            (Some(State::Blocked), _) => Self::NeedsInput,
            (Some(State::Working), _) | (_, true) => Self::Working,
            (Some(State::Error), false) => Self::Failed,
            (Some(State::Idle | State::Done) | None, false) => Self::Idle,
        }
    }

    /// The word, in the colour that says whether the pane needs someone.
    fn cell(self) -> Text {
        match self {
            Self::Working => Text::plain(self.word()),
            Self::NeedsInput => Text::new().warning(self.word()),
            Self::Failed => Text::new().error(self.word()),
            Self::Idle | Self::Stopped => Text::new().muted(self.word()),
        }
    }
}

/// One pane, as `rawr status` lists it.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Row {
    /// The session's label and the block's label, each its short ID when it
    /// has no label.
    pub(crate) pane: String,
    /// The title of the pane's newest session.
    pub(crate) title: String,
    /// The folder the pane's newest session started in.
    pub(crate) folder: String,
    pub(crate) status: Status,
}

impl Row {
    fn new(session: &Session, block: &Block, newest: &Entry, status: Status) -> Self {
        Self {
            pane: format!(
                "{}/{}",
                name(&session.label, &session.id),
                name(&block.label, &block.id)
            ),
            title: session::transcript_title(&newest.transcript).map_or_else(
                || "Untitled session".to_owned(),
                |title| session::sanitize(&title),
            ),
            folder: newest.cwd.display().to_string(),
            status,
        }
    }
}

/// `label`, or the first 8 characters of `id` after its `kind:` prefix.
fn name(label: &str, id: &str) -> String {
    if !label.is_empty() {
        return label.to_owned();
    }
    let bare = id.split_once(':').map_or(id, |(_, bare)| bare);
    bare.chars().take(8).collect()
}

/// The table of rows, which `rawr status` prints.
pub(crate) fn table(rows: &[Row]) -> Table {
    rows.iter().fold(
        Table::new(["Pane", "Title", "Folder", "Status"]).ellipsis_column(1),
        |table, row| {
            table.row([
                Text::plain(row.pane.as_str()),
                Text::plain(row.title.as_str()),
                Text::plain(row.folder.as_str()),
                row.status.cell(),
            ])
        },
    )
}

/// One row for each terminal block of the Rex server whose pane index under
/// `dir` lists a session, in the server's order.
///
/// # Errors
///
/// Fails when any call to the Rex server is refused or an index file cannot be
/// read.
pub(crate) fn rows(client: &mut Client, dir: &Path) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for session in client.sessions()? {
        for block in client.blocks(&session)? {
            if !block.is_terminal() {
                continue;
            }
            let pane = PaneIndex::load(dir, &BlockId::try_new(&block.id)?)?;
            let Some(newest) = pane.entries().first() else {
                continue;
            };
            // A block that closed since the listing has no row.
            let Some(records) = client.program_status(&session, &block)? else {
                continue;
            };
            rows.push(Row::new(
                &session,
                &block,
                newest,
                Status::from_records(&records),
            ));
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::rex::fake::FakeServer;

    const HANDSHAKE: &str = include_str!("../testdata/rex/handshake.jsonl");
    const STATUS: &str = include_str!("../testdata/rex/status.jsonl");

    fn record(id: &str, state: State, app: &str) -> StatusRecord {
        StatusRecord {
            id: id.to_owned(),
            state,
            app: Some(app.to_owned()),
            effective_app: Some(app.to_owned()),
        }
    }

    #[test]
    fn a_session_waits_on_a_permission_prompt() {
        let status = Status::from_records(&[record("", State::Blocked, "claude-code")]);
        assert_eq!(status, Status::NeedsInput);
        assert_eq!(status.word(), "Needs input");
    }

    #[test]
    fn a_finished_turn_with_a_background_task_still_running() {
        let status = Status::from_records(&[
            record("", State::Done, "claude-code"),
            record("agent-1", State::Working, "claude-code"),
        ]);
        assert_eq!(status, Status::Working);
    }

    #[test]
    fn a_pane_whose_claude_has_exited() {
        assert_eq!(Status::from_records(&[]), Status::Stopped);
    }

    #[test]
    fn the_other_root_states_have_their_words() {
        let root = |state| Status::from_records(&[record("", state, "claude-code")]);
        assert_eq!(root(State::Working), Status::Working);
        assert_eq!(root(State::Done), Status::Idle);
        assert_eq!(root(State::Idle), Status::Idle);
        assert_eq!(root(State::Error), Status::Failed);
    }

    #[test]
    fn a_working_child_makes_an_idle_or_failed_pane_working() {
        let child = record("agent-1", State::Working, "claude-code");
        for state in [State::Error, State::Idle, State::Done] {
            let status = Status::from_records(&[record("", state, "claude-code"), child.clone()]);
            assert_eq!(status, Status::Working);
        }
    }

    #[test]
    fn a_pane_waiting_for_the_user_needs_input_while_a_child_runs() {
        let status = Status::from_records(&[
            record("", State::Blocked, "claude-code"),
            record("agent-1", State::Working, "claude-code"),
        ]);
        assert_eq!(status, Status::NeedsInput);
    }

    #[test]
    fn a_finished_child_does_not_make_the_pane_working() {
        for child in [State::Done, State::Idle, State::Error] {
            let finished = record("agent-1", child, "claude-code");
            let root = |state| record("", state, "claude-code");
            let word = |state| Status::from_records(&[root(state), finished.clone()]);
            assert_eq!(word(State::Blocked), Status::NeedsInput);
            assert_eq!(word(State::Done), Status::Idle);
            assert_eq!(word(State::Error), Status::Failed);
        }
    }

    #[test]
    fn a_program_that_is_not_claude_is_not_a_running_claude() {
        let status = Status::from_records(&[record("", State::Working, "cargo")]);
        assert_eq!(status, Status::Stopped);
    }

    #[test]
    fn a_pane_without_a_label_shows_its_short_id() {
        assert_eq!(name("work", "session:0190a000-0000"), "work");
        assert_eq!(name("", "block:0190f3a2-7c41-7e10"), "0190f3a2");
    }

    #[test]
    fn a_row_for_each_terminal_whose_index_lists_a_session() {
        let dir = std::env::temp_dir().join(format!("rawr-status-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33.jsonl");
        fs::write(
            &transcript,
            "{\"type\":\"custom-title\",\"customTitle\":\"Fix checkout\"}\n",
        )
        .unwrap();
        let missing = dir.join("no-transcript.jsonl");
        for (block, transcript) in [
            ("0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08", &transcript),
            ("0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f09", &missing),
            ("0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f0c", &missing),
        ] {
            let index = serde_json::json!({
                "entries": [{
                    "session_id": "6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33",
                    "cwd": "/work/shop",
                    "transcript": transcript,
                }]
            });
            fs::write(dir.join(format!("{block}.json")), index.to_string()).unwrap();
        }

        let server = FakeServer::start("status", &[HANDSHAKE, STATUS]);
        let mut client = Client::connect(&server.endpoint).unwrap();
        let found = rows(&mut client, &dir).unwrap();
        drop(client);
        server.finish();
        fs::remove_dir_all(&dir).unwrap();

        let row = |pane: &str, title: &str, status| Row {
            pane: pane.to_owned(),
            title: title.to_owned(),
            folder: "/work/shop".to_owned(),
            status,
        };
        assert_eq!(
            found,
            [
                row("work/shell", "Fix checkout", Status::NeedsInput),
                row("work/0190f3a2", "Untitled session", Status::Stopped),
            ]
        );
    }
}

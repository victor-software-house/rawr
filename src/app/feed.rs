//! The feed: one thread that reads Rex and the pane index, and sends the
//! whole list of panes each time something changes.
//!
//! It attaches to every session's events first, then lists each session's
//! blocks and their program status, so no change between the two is lost.
//! An event can only replace a record or remove a pane, so one that the
//! listing already holds changes nothing.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::Result;

use crate::index::{self, BlockId, PaneIndex};
use crate::rex::events::Event;
use crate::rex::{Block, Client, Endpoint, Session, StatusRecord, session_is_gone};
use crate::session::{self, Agent};
use crate::status::{self, Status};

/// One Rex pane whose index lists a Claude session.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Pane {
    pub(crate) session_id: String,
    pub(crate) block_id: String,
    /// The session's label and the block's label, as `rawr status` names them.
    pub(crate) name: String,
    pub(crate) status: Status,
    /// The tab that holds the pane, such as `tab 2`.
    pub(crate) tab: String,
    /// How far the deciding record is, 0 to 100, when its program says.
    pub(crate) progress: Option<u8>,
    /// The deciding record's line: what Claude is doing or waits for.
    pub(crate) msg: Option<String>,
    /// The pane's sessions, newest first, each marked when a live Claude
    /// holds it.
    pub(crate) sessions: Vec<(Agent, bool)>,
}

impl Pane {
    /// The newest session's title.
    pub(crate) fn title(&self) -> &str {
        self.sessions
            .first()
            .map_or("Untitled session", |(agent, _)| agent.title.as_str())
    }
}

/// What the feed sends.
#[derive(Debug)]
pub(crate) enum Update {
    /// Every pane, as it is now.
    Panes(Vec<Pane>),
    /// Rex could not be reached; the text says why.
    Offline(String),
    /// The event stream ended after the panes were listed.
    Stale(String),
}

/// A pane the feed tracks: its records and where its index lives.
struct Tracked {
    session: Session,
    block: Block,
    records: Vec<StatusRecord>,
    /// The tab that holds the block: its label, else its place from 1.
    tab: String,
}

/// Runs the feed until the event stream ends or the receiver is gone.
pub(crate) fn run(updates: &Sender<Update>) {
    if let Err(error) = follow(updates) {
        let _ = updates.send(Update::Stale(format!("{error:#}")));
    }
}

fn follow(updates: &Sender<Update>) -> Result<()> {
    let endpoint = match Endpoint::from_environment() {
        Ok(endpoint) => endpoint,
        Err(error) => {
            let _ = updates.send(Update::Offline(format!("{error:#}")));
            return Ok(());
        }
    };
    let (watch, mut lister) = match Client::connect(&endpoint).and_then(|watcher| {
        let lister = Client::connect(&endpoint)?;
        Ok((watcher.watch()?, lister))
    }) {
        Ok(pair) => pair,
        Err(error) => {
            let _ = updates.send(Update::Offline(format!("{error:#}")));
            return Ok(());
        }
    };
    let dir = index::state_dir()?;
    let mut panes = BTreeMap::new();
    for session in &watch.sessions {
        list_session(&mut lister, session, &mut panes)?;
    }
    if updates.send(Update::Panes(view(&panes, &dir))).is_err() {
        return Ok(());
    }
    for event in watch.events {
        let changed = match event? {
            Event::ProgramStatusChanged {
                session_id,
                block_id,
                record,
            } => {
                if let Some(pane) = panes.get_mut(&block_id) {
                    replace(&mut pane.records, record);
                } else {
                    relist(&mut lister, &watch.sessions, &session_id, &mut panes)?;
                }
                true
            }
            Event::ProgramStatusRemoved { block_id, ids, .. } => match panes.get_mut(&block_id) {
                Some(pane) => {
                    pane.records.retain(|record| !ids.contains(&record.id));
                    true
                }
                None => false,
            },
            Event::BlockClosed { block_id, .. } => panes.remove(&block_id).is_some(),
            Event::BlockCreated { session_id, .. } => {
                relist(&mut lister, &watch.sessions, &session_id, &mut panes)?;
                true
            }
        };
        if changed && updates.send(Update::Panes(view(&panes, &dir))).is_err() {
            return Ok(());
        }
    }
    Ok(())
}

/// Puts `record` in place of the record with its ID, or adds it.
fn replace(records: &mut Vec<StatusRecord>, record: StatusRecord) {
    match records.iter_mut().find(|old| old.id == record.id) {
        Some(old) => *old = record,
        None => records.push(record),
    }
}

/// Lists the terminal blocks of `session` and their status records.
fn list_session(
    lister: &mut Client,
    session: &Session,
    panes: &mut BTreeMap<String, Tracked>,
) -> Result<()> {
    let blocks = match lister.blocks(session) {
        Ok(blocks) => blocks,
        Err(error) if session_is_gone(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    let view = lister.view(&session.id).ok();
    for block in blocks.into_iter().filter(Block::is_terminal) {
        let Some(records) = lister.program_status(session, &block)? else {
            continue;
        };
        let tab = view
            .as_ref()
            .and_then(|view| view.find(&block.id))
            .map(|(index, window, _)| match window.label.as_deref() {
                Some(label) if !label.is_empty() && !label.starts_with("Window ") => {
                    label.to_owned()
                }
                _ => format!("tab {index}"),
            })
            .unwrap_or_default();
        panes.insert(
            block.id.clone(),
            Tracked {
                session: session.clone(),
                block,
                records,
                tab,
            },
        );
    }
    Ok(())
}

/// Lists `session_id` again, for a block the feed has not seen.
fn relist(
    lister: &mut Client,
    sessions: &[Session],
    session_id: &str,
    panes: &mut BTreeMap<String, Tracked>,
) -> Result<()> {
    let session = sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned()
        .unwrap_or_else(|| Session {
            id: session_id.to_owned(),
            label: String::new(),
        });
    list_session(lister, &session, panes)
}

/// The panes whose index lists a session, with each pane's sessions read
/// again, so a session recorded since the last update shows.
fn view(panes: &BTreeMap<String, Tracked>, dir: &Path) -> Vec<Pane> {
    let live = session::live_sessions().unwrap_or_default();
    panes
        .values()
        .filter_map(|tracked| pane(tracked, dir, &live))
        .collect()
}

fn pane(tracked: &Tracked, dir: &Path, live: &HashSet<String>) -> Option<Pane> {
    let block = BlockId::try_new(&tracked.block.id).ok()?;
    let mut index = PaneIndex::load(dir, &block).ok()?;
    index.prune_missing(index::unix_now());
    let sessions: Vec<(Agent, bool)> = session::resumable(index.entries(), &HashSet::new())
        .into_iter()
        .map(|agent| {
            let held = live.contains(&agent.session_id);
            (agent, held)
        })
        .collect();
    if sessions.is_empty() {
        return None;
    }
    let (progress, msg) = Status::detail(&tracked.records);
    Some(Pane {
        session_id: tracked.session.id.clone(),
        block_id: tracked.block.id.clone(),
        name: pane_name(&tracked.session, &tracked.block),
        status: Status::from_records(&tracked.records),
        tab: tracked.tab.clone(),
        progress,
        msg,
        sessions,
    })
}

/// The session's label, then the block's label unless it is the default
/// `shell`, which every terminal pane starts with.
fn pane_name(session: &Session, block: &Block) -> String {
    let session_name = status::name(&session.label, &session.id);
    match block.label.as_str() {
        "" | "shell" => session_name,
        label => format!("{session_name}/{label}"),
    }
}

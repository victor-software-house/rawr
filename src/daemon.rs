//! `rawr daemon`: one process outside every pane that holds what every
//! `rawr` window shares, the Rex feed, each pane's status, the transcript
//! facts and the search catalog, and serves them over a Unix socket.
//!
//! Each message is one JSON object on one line. The daemon pushes the panes
//! and the link to Rex; a client asks for facts and searches. View state
//! stays in the client.

mod actions;
pub(crate) mod link;
mod server;

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
pub(crate) use server::run as serve;

use crate::app::facts::Facts;
use crate::app::feed::Pane;
use crate::app::state::Carry;
use crate::catalog::Hit;
use crate::session::Agent;

/// What a client sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ToDaemon {
    /// `view` names the view a hand-off started this window for; `here` is
    /// the Rex pane the window runs in.
    Hello {
        build: Build,
        view: Option<u64>,
        here: Option<Here>,
    },
    /// The window has drawn the panes, so the window it replaces may go.
    Drawn,
    /// Show a pane in the Rex app; `carry` is the window's view, for a copy
    /// the daemon may start there.
    GoTo {
        session: String,
        block: String,
        carry: Carry,
    },
    /// Resume a session: in a tab of `session` when the window floats, or
    /// in the window itself, which the daemon answers with `exec`.
    Resume {
        agent: Agent,
        session: Option<String>,
    },
    /// The facts of one transcript.
    Facts {
        path: PathBuf,
    },
    /// Every session that matches `query`, those of the pane `pane` first.
    /// A newer search from the same client ends this one.
    Search {
        id: u64,
        query: String,
        pane: Option<String>,
    },
    Shutdown,
}

/// What the daemon sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum FromDaemon {
    /// `view` is the window's view ID; `carry`, when a hand-off started the
    /// window, is the view of the window it replaces.
    Welcome {
        build: Build,
        view: u64,
        carry: Option<Carry>,
    },
    /// The window should become `claude --resume` for `agent`.
    Exec {
        agent: Agent,
    },
    /// One line for the window's status bar, such as why an action failed.
    Notice {
        text: String,
    },
    /// The client runs another binary; it should replace the daemon.
    Refused {
        build: Build,
    },
    /// Every pane, as it is now.
    Panes {
        panes: Vec<Pane>,
    },
    /// Whether the daemon reaches Rex.
    Link {
        state: Reach,
        reason: Option<String>,
    },
    Facts {
        path: PathBuf,
        facts: Facts,
    },
    /// A batch of matches for search `id`, after `read` of `total`
    /// transcripts. The first batch, with `read` 0, replaces the results.
    Hits {
        id: u64,
        hits: Vec<Hit>,
        read: usize,
        total: usize,
    },
}

/// A Rex pane, by its session and block IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Here {
    pub(crate) session: String,
    pub(crate) block: String,
}

impl Here {
    /// The pane this process runs in, from Rex's environment.
    pub(crate) fn current() -> Option<Self> {
        Some(Self {
            session: std::env::var("REX_SESSION").ok()?,
            block: std::env::var("REX_BLOCK").ok()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reach {
    Live,
    Offline,
    Stale,
}

/// Which binary a process runs: its path, size and modification time when
/// it started. A rebuilt or upgraded binary differs in one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Build {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

impl Build {
    /// # Errors
    ///
    /// Fails when the running binary cannot be found or read.
    pub(crate) fn current() -> Result<Self> {
        let path = std::env::current_exe().context("find the rawr binary")?;
        let meta = std::fs::metadata(&path).context("read the rawr binary")?;
        Ok(Self {
            path,
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// The folder that holds the socket and the locks.
fn folder() -> Result<PathBuf> {
    crate::index::state_dir()
}

fn socket() -> Result<PathBuf> {
    Ok(folder()?.join("daemon.sock"))
}

//! `rawr daemon`: one process outside every pane that holds what every
//! `rawr` window shares, the Rex feed, each pane's status, the transcript
//! facts and the search catalog, and serves them over a Unix socket.
//!
//! Each message is one JSON object on one line. The daemon pushes the panes
//! and the link to Rex; a client asks for facts and searches. View state
//! stays in the client.

pub(crate) mod link;
mod server;

use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
pub(crate) use server::run as serve;

use crate::app::facts::Facts;
use crate::app::feed::Pane;
use crate::catalog::Hit;

/// What a client sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ToDaemon {
    Hello {
        build: Build,
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
    Welcome {
        build: Build,
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

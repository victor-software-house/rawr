//! Bring Claude Code agents back into their Rex panes after a Rex server
//! restart.
//!
//! Rex keeps a pane's block ID across a server restart, so the block ID is the
//! pane's identity. `record` remembers which Claude sessions ran in a pane, and
//! `pick` offers them again in that pane after a restart.

mod index;
mod picker;
mod session;

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::{Context, Result};

use crate::index::{BlockId, HookInput, PaneIndex};

const USAGE: &str = "usage: rawr <record|pick>

  record  add the Claude session described on stdin to this pane's index
  pick    choose one of this pane's agents and print the command that resumes it";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let outcome = match command.as_deref() {
        Some("record") => record().map(|()| ExitCode::SUCCESS),
        Some("pick") => pick(),
        Some("--version" | "-V") => {
            println!("rawr {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some(_) | None => {
            eprintln!("{USAGE}");
            Ok(ExitCode::from(2))
        }
    };
    outcome.unwrap_or_else(|error| {
        eprintln!("rawr: {error:#}");
        ExitCode::FAILURE
    })
}

/// The pane this process runs in, from Rex's `REX_BLOCK`.
fn current_block() -> Result<Option<BlockId>> {
    match std::env::var("REX_BLOCK") {
        Ok(raw) => BlockId::try_new(&raw).map(Some),
        Err(_) => Ok(None),
    }
}

/// Adds the hook's session to this pane's index, then drops the entries of
/// panes the Rex server no longer has. Outside Rex it does nothing.
fn record() -> Result<()> {
    let Some(block) = current_block()? else {
        return Ok(());
    };
    let input: HookInput =
        serde_json::from_reader(std::io::stdin().lock()).context("read the hook input on stdin")?;
    let dir = index::state_dir()?;
    let mut pane = PaneIndex::load(&dir, &block)?;
    pane.add(input.into_entry());
    pane.save(&dir, &block)?;
    index::prune(&dir)
}

/// Shows this pane's agents and prints the chosen resume command. Exits with
/// failure, printing nothing, when there is nothing to offer or the user
/// dismisses the picker.
fn pick() -> Result<ExitCode> {
    if !std::io::stderr().is_terminal() {
        return Ok(ExitCode::FAILURE);
    }
    let Some(block) = current_block()? else {
        return Ok(ExitCode::FAILURE);
    };
    let pane = PaneIndex::load(&index::state_dir()?, &block)?;
    let live = session::live_sessions()?;
    let agents = session::resumable(pane.entries(), &live);
    if agents.is_empty() {
        return Ok(ExitCode::FAILURE);
    }
    match picker::choose(&agents)?.and_then(|n| agents.get(n)) {
        Some(agent) => {
            println!("{}", agent.resume_command());
            Ok(ExitCode::SUCCESS)
        }
        None => Ok(ExitCode::FAILURE),
    }
}

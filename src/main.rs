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

use anyhow::{Context, Result};
use ctl_core::prelude::*;
use serde::Serialize;

use crate::index::{BlockId, HookInput, PaneIndex};

/// Rex Agents: Wake and Resume.
#[derive(Debug, Parser)]
#[command(name = "rawr", version)]
#[command(about = "Bring Claude Code agents back into their Rex panes after a restart")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add the Claude Code session described on stdin to this pane's index.
    ///
    /// A Claude Code `SessionStart` hook runs it. Outside a Rex pane it does
    /// nothing.
    Record,
    /// Choose one of this pane's sessions and print the command that resumes
    /// it.
    ///
    /// The picker draws on stderr. stdout carries only the command, and the
    /// exit status is non-zero with nothing printed when there is nothing to
    /// offer or the picker is dismissed.
    Pick,
}

/// What a command leaves for the shell.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum Outcome {
    /// `record` stored the session, or had no pane to store it for.
    Recorded,
    /// `pick` chose a session; the shell evaluates `command`.
    Resume { command: String },
    /// `pick` had nothing to offer, or the user dismissed it.
    Nothing,
}

impl Present for Outcome {
    fn present(&self) -> Document {
        match self {
            Self::Recorded | Self::Nothing => Document::new(),
            Self::Resume { command } => Document::new().verbatim(command.as_str()),
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Recorded | Self::Resume { .. } => 0,
            Self::Nothing => 1,
        }
    }
}

fn main() -> ExitCode {
    App::<Cli>::new("rawr").run(|cli| match cli.command {
        Command::Record => record().map(|()| Outcome::Recorded),
        Command::Pick => pick(),
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

/// Shows this pane's agents and returns the chosen resume command, or
/// [`Outcome::Nothing`] when there is nothing to offer or the user dismisses
/// the picker.
fn pick() -> Result<Outcome> {
    if !std::io::stderr().is_terminal() {
        return Ok(Outcome::Nothing);
    }
    let Some(block) = current_block()? else {
        return Ok(Outcome::Nothing);
    };
    let pane = PaneIndex::load(&index::state_dir()?, &block)?;
    let live = session::live_sessions()?;
    let agents = session::resumable(pane.entries(), &live);
    if agents.is_empty() {
        return Ok(Outcome::Nothing);
    }
    Ok(picker::choose(&agents)?
        .and_then(|n| agents.get(n))
        .map_or(Outcome::Nothing, |agent| Outcome::Resume {
            command: agent.resume_command(),
        }))
}

#[cfg(test)]
mod tests {
    use ctl_core::prelude::*;

    use super::Outcome;

    fn shown(outcome: &Outcome) -> (String, Stream) {
        let captured = View::new(OutputFormat::Pretty, ColorMode::Never)
            .capture(outcome)
            .expect("an outcome serializes");
        (captured.text().to_owned(), captured.stream())
    }

    #[test]
    fn a_resume_prints_only_the_command_on_stdout() {
        let command = "cd -- '/home/dev/code/it'\\''s shop' && claude --resume '6f1c2a90'";
        let outcome = Outcome::Resume {
            command: command.to_owned(),
        };
        assert_eq!(shown(&outcome), (format!("{command}\n"), Stream::Stdout));
        assert_eq!(outcome.exit_code(), 0);
    }

    #[test]
    fn nothing_to_offer_prints_nothing_and_fails() {
        assert_eq!(shown(&Outcome::Nothing).0, "");
        assert_eq!(Outcome::Nothing.exit_code(), 1);
        assert_eq!(shown(&Outcome::Recorded).0, "");
    }
}

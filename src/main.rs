//! Bring Claude Code agents back into their Rex panes after a Rex server
//! restart.
//!
//! Rex keeps a pane's block ID across a server restart, so the block ID is the
//! pane's identity. `record` remembers which Claude sessions ran in a pane, and
//! `pick` offers them again in that pane after a restart.

mod catalog;
mod index;
mod picker;
mod session;
mod setup;

use std::collections::HashSet;
use std::io::IsTerminal;

use anyhow::{Context, Result};
use ctl_core::completion::CompletionCommand;
use ctl_core::prelude::*;
use ctl_core::shell::{self, Script};
use serde::Serialize;

use crate::config::Config;
use crate::index::{BlockId, HookInput, PaneIndex};
use crate::picker::Start;
use crate::session::Agent;
use crate::setup::{Change, Mode};

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
    /// Choose one of this pane's sessions and resume it.
    ///
    /// The picker draws on stderr. On a choice, rawr becomes `claude --resume`
    /// in the session's folder. The exit status is non-zero with nothing
    /// printed when there is nothing to offer or the picker is dismissed.
    Pick,
    /// Search every resumable local session and resume the chosen one.
    ///
    /// Works in or out of a Rex pane; in one, the pane's own sessions come
    /// first. Resumes and exits as `pick` does.
    Find {
        /// Words to search for; each must occur in a session's prompts,
        /// folder or ID.
        words: Vec<String>,
    },
    /// Add rawr's hook to another tool's settings.
    Setup {
        #[command(subcommand)]
        target: Target,
    },
    #[command(flatten)]
    Completion(CompletionCommand),
}

#[derive(Debug, Subcommand)]
enum Target {
    /// Add a `SessionStart` hook that runs `rawr record` to Claude Code's user
    /// settings, unless one is there.
    ///
    /// Reads `$CLAUDE_CONFIG_DIR/settings.json`, or `~/.claude/settings.json`,
    /// and keeps every other setting as it was.
    Claude {
        /// Report what would change, and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// What a command reports.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum Outcome {
    /// `record` stored the session, or had no pane to store it for.
    Recorded,
    /// There was nothing to offer, or the user dismissed the picker.
    Nothing,
    /// `setup` checked or changed a settings file.
    Setup { change: Change, path: String },
    /// `completion` printed a shell's registration script.
    Completion(Script),
}

impl Present for Outcome {
    fn present(&self) -> Document {
        match self {
            Self::Recorded | Self::Nothing => Document::new(),
            Self::Setup { change, path } => Document::new().verbatim(match change {
                Change::Added => format!("Added the `{}` hook to {path}", setup::RECORD),
                Change::WouldAdd => format!("Would add the `{}` hook to {path}", setup::RECORD),
                Change::Unchanged => format!("{path} already runs `{}`", setup::RECORD),
            }),
            Self::Completion(script) => script.present(),
        }
    }

    fn exit_code(&self) -> u8 {
        match self {
            Self::Recorded | Self::Setup { .. } | Self::Completion(_) => 0,
            Self::Nothing => 1,
        }
    }
}

fn main() -> ExitCode {
    App::<Cli>::new("rawr")
        .completion()
        // The script is the whole result, so it prints plain whatever the
        // terminal, as `source <(rawr completion zsh)` needs.
        .view(|cli| match cli.command {
            Command::Completion(_) => shell::view(OutputFormat::Pretty),
            Command::Record | Command::Pick | Command::Find { .. } | Command::Setup { .. } => {
                View::new(OutputFormat::Pretty, ColorMode::Auto)
            }
        })
        .run(|cli| match cli.command {
            Command::Record => record().map(|()| Outcome::Recorded),
            Command::Pick => pick(),
            Command::Find { words } => find(&words),
            Command::Setup {
                target: Target::Claude { dry_run },
            } => setup_claude(if dry_run { Mode::DryRun } else { Mode::Write }),
            Command::Completion(command) => Ok(Outcome::Completion(command.script("rawr")?)),
        })
}

/// The pane this process runs in, from Rex's `REX_BLOCK`.
fn current_block() -> Result<Option<BlockId>> {
    match std::env::var("REX_BLOCK") {
        Ok(raw) => BlockId::try_new(&raw).map(Some),
        Err(_) => Ok(None),
    }
}

/// Adds the hook's session to this pane's index, drops the entries whose
/// transcript is gone, then drops the entries of panes the Rex server no
/// longer has. Outside Rex it does nothing, and reads no config.
fn record() -> Result<()> {
    let Some(block) = current_block()? else {
        return Ok(());
    };
    let config = Config::load()?;
    let input: HookInput =
        serde_json::from_reader(std::io::stdin().lock()).context("read the hook input on stdin")?;
    let dir = index::state_dir()?;
    let now = index::unix_now();
    let mut pane = PaneIndex::load(&dir, &block)?;
    pane.prune_missing(now);
    pane.add(input.into_entry(now), config.history.per_pane);
    pane.save(&dir, &block)?;
    index::prune(&dir)
}

/// This pane's resumable sessions, newest first; none outside Rex.
fn pane_agents(live: &HashSet<String>) -> Result<Vec<Agent>> {
    let Some(block) = current_block()? else {
        return Ok(Vec::new());
    };
    let mut pane = PaneIndex::load(&index::state_dir()?, &block)?;
    pane.prune_missing(index::unix_now());
    Ok(session::resumable(pane.entries(), live))
}

/// Opens the picker over `pane` and resumes the chosen session, or returns
/// [`Outcome::Nothing`] when the user dismisses the picker.
fn choose(pane: &[Agent], start: Start, live: HashSet<String>) -> Result<Outcome> {
    let load = move || catalog::load(&session::claude_dir()?, &live);
    match picker::choose(pane, start, load)? {
        Some(agent) => match agent.resume()? {},
        None => Ok(Outcome::Nothing),
    }
}

/// Shows this pane's agents and resumes the chosen one, or returns
/// [`Outcome::Nothing`] when there is nothing to offer or the user dismisses
/// the picker.
fn pick() -> Result<Outcome> {
    if !std::io::stderr().is_terminal() {
        return Ok(Outcome::Nothing);
    }
    let live = session::live_sessions()?;
    let agents = pane_agents(&live)?;
    if agents.is_empty() {
        return Ok(Outcome::Nothing);
    }
    choose(&agents, Start::Pane, live)
}

/// Opens the picker on the search line with `words` typed.
fn find(words: &[String]) -> Result<Outcome> {
    if !std::io::stderr().is_terminal() {
        return Ok(Outcome::Nothing);
    }
    let live = session::live_sessions()?;
    let agents = pane_agents(&live)?;
    choose(&agents, Start::Search(words.join(" ")), live)
}

/// Adds the `rawr record` hook to Claude Code's user settings, or reports
/// that it is there.
fn setup_claude(mode: Mode) -> Result<Outcome> {
    let path = setup::settings_path(&session::claude_dir()?);
    let change = setup::claude(&path, mode)?;
    Ok(Outcome::Setup {
        change,
        path: path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use ctl_core::prelude::*;

    use super::{Cli, Outcome};

    fn shown(outcome: &Outcome) -> (String, Destination) {
        let captured = View::new(OutputFormat::Pretty, ColorMode::Never)
            .capture(outcome)
            .expect("an outcome serializes");
        (captured.text().to_owned(), captured.stream())
    }

    #[test]
    fn the_command_line_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn nothing_to_offer_prints_nothing_and_fails() {
        assert_eq!(shown(&Outcome::Nothing).0, "");
        assert_eq!(Outcome::Nothing.exit_code(), 1);
        assert_eq!(shown(&Outcome::Recorded).0, "");
    }

    #[test]
    fn completion_prints_a_registration_script_for_rawr() {
        use clap::Parser;
        let cli =
            Cli::try_parse_from(["rawr", "completion", "zsh"]).expect("completion zsh parses");
        let super::Command::Completion(command) = cli.command else {
            panic!("completion zsh is the completion verb");
        };
        let outcome = Outcome::Completion(command.script("rawr").expect("the script renders"));
        let (text, _) = shown(&outcome);
        assert!(text.contains("rawr"), "{text}");
        assert_eq!(outcome.exit_code(), 0);
    }
}

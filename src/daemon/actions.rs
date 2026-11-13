//! What the daemon does in Rex for a window: going to a pane, handing a
//! floating window over to a copy in another tab, and resuming a session in
//! a tab of its own. The window only asks; nothing here runs in it.

use anyhow::{Context, Result};

use super::Here;
use crate::app::state::Carry;
use crate::rex::{Client, Endpoint};
use crate::session::Agent;

/// What going to a pane needs from the daemon afterwards.
pub(super) enum Went {
    /// The pane shows; nothing more to do.
    Shown,
    /// A copy starts over the pane; once it has drawn, it takes over from
    /// `old` (see [`take_over`]).
    Handed { old: Here, target: String },
}

fn connect() -> Result<Client> {
    Client::connect(&Endpoint::from_environment()?)
}

/// Shows `block_id` in the Rex app. When the window `here` floats in another
/// tab, it starts a copy of the app over the pane instead, out of sight and
/// at the window's own size, as view `view`.
pub(super) fn go_to(
    here: Option<&Here>,
    session_id: &str,
    block_id: &str,
    view: u64,
) -> Result<Went> {
    let mut client = connect()?;
    let window_id = client
        .view(session_id)?
        .find(block_id)
        .map(|(_, window, _)| window.window_id.clone())
        .context("the pane is no longer in its session")?;
    let floating = match here {
        Some(here) => client
            .view(&here.session)?
            .find(&here.block)
            .filter(|(_, own, layer)| layer.floats() && own.window_id != window_id)
            .map(|(_, _, layer)| layer.bounds),
        None => None,
    };
    let (Some(bounds), Some(here)) = (floating, here) else {
        reveal(&mut client, session_id, block_id)?;
        client.show(
            here.map_or(session_id, |here| here.session.as_str()),
            session_id,
            &window_id,
        )?;
        shown(&mut client, session_id, block_id, &window_id)?;
        return Ok(Went::Shown);
    };
    let size = client.terminal_size(&here.session, &here.block)?;
    let exe = std::env::current_exe().context("find the rawr binary")?;
    client.float_command(
        session_id,
        &window_id,
        bounds,
        &[
            exe.display().to_string(),
            "--view".to_owned(),
            view.to_string(),
        ],
        &size,
    )?;
    Ok(Went::Handed {
        old: here.clone(),
        target: block_id.to_owned(),
    })
}

/// Once the copy `new` has drawn: brings `target` into view, focuses the
/// copy over it, asks the Rex app to show its tab, and closes `old`, by its
/// block ID.
pub(super) fn take_over(new: &Here, target: &str, old: &Here) -> Result<()> {
    let mut client = connect()?;
    reveal(&mut client, &new.session, target)?;
    client.focus_block(&new.session, &new.block)?;
    let window_id = client
        .view(&new.session)?
        .find(&new.block)
        .map(|(_, window, _)| window.window_id.clone())
        .context("the new window is no longer in its session")?;
    client.show(&old.session, &new.session, &window_id)?;
    shown(&mut client, &new.session, &new.block, &window_id)?;
    client.close_block(&old.session, &old.block)
}

/// Whether the window `here` floats over its tab.
pub(super) fn floats(here: &Here) -> Result<bool> {
    Ok(connect()?
        .view(&here.session)?
        .find(&here.block)
        .is_some_and(|(_, _, layer)| layer.floats()))
}

/// Resumes `agent` in a new tab of `session_id`, or of the window's session,
/// named by its title, and shows that tab.
pub(super) fn resume_in_tab(here: &Here, agent: &Agent, session_id: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        std::path::Path::new(&agent.cwd).is_dir(),
        "{} no longer exists",
        agent.cwd
    );
    let session = session_id.unwrap_or(&here.session);
    let mut client = connect()?;
    let label: String = agent.title.chars().take(128).collect();
    let window = client.open_tab(
        session,
        &label,
        &agent.cwd,
        &[
            "claude".to_owned(),
            "--resume".to_owned(),
            agent.session_id.clone(),
        ],
    )?;
    // The app learns of the new tab a moment after the server makes it, and
    // a request to show a tab it has not heard of yet does nothing; asking
    // once more after that moment lands either way.
    client.show(&here.session, session, &window)?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    client.show(&here.session, session, &window)
}

/// Brings `block_id` into view in its session: its window active, no other
/// block zoomed over it, and its floating layer drawn.
fn reveal(client: &mut Client, session_id: &str, block_id: &str) -> Result<()> {
    let view = client.view(session_id)?;
    let (_, window, layer) = view
        .find(block_id)
        .context("the pane is no longer in its session")?;
    if window
        .zoomed_block_id
        .as_deref()
        .is_some_and(|zoomed| zoomed != block_id)
    {
        client.unzoom(session_id, block_id)?;
    }
    if layer.minimized() {
        client.restore_layer(session_id, &layer.layer_id)?;
    }
    client.focus_block(session_id, block_id)
}

/// Checks that Rex now draws `block_id`: its window is the active one, no
/// other block is zoomed over it, and its layer is not minimized.
fn shown(client: &mut Client, session_id: &str, block_id: &str, window_id: &str) -> Result<()> {
    let view = client.view(session_id)?;
    let (_, window, layer) = view
        .find(block_id)
        .context("the pane is no longer in its session")?;
    let active = view.active_window_id.as_deref() == Some(window_id);
    let covered = window
        .zoomed_block_id
        .as_deref()
        .is_some_and(|zoomed| zoomed != block_id);
    anyhow::ensure!(
        active && !covered && !layer.minimized(),
        "the pane did not come into view, so rawr stayed where it is"
    );
    Ok(())
}

/// The carry a hand-off gives the copy: the old window's view, with the
/// pane it went to selected.
pub(super) fn handed(mut carry: Carry, target: &str) -> Carry {
    carry.select = Some(target.to_owned());
    carry
}

//! The rawr app: every Rex pane that has a Claude session, what each Claude
//! is doing, and the way back to a session.
//!
//! Two threads feed one channel: the terminal's input and the Rex feed. The
//! loop drains the channel and draws once after each batch, with no timer.

mod facts;
mod feed;
mod report;
mod state;
mod theme;
mod view;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use report::{Report, Reporter};
use state::{Action, App, Link};

use crate::catalog::{self, Found};
use crate::picker::terminal::{Pointer, Screen};
use crate::rex::{Client, Endpoint};
use crate::session::{self, Agent};

/// How often the spinner turns while a pane works; the loop waits on
/// input with no timer otherwise.
const SPIN: Duration = Duration::from_millis(100);

enum Msg {
    Input(Event),
    Feed(feed::Update),
    /// What a deep search found for the query of this generation.
    Deep(u64, Vec<catalog::Hit>, (usize, usize)),
    /// A transcript's facts arrived; drawing picks them up.
    Wake,
    /// Search again for the query in the box.
    Search,
}

/// Opens the app and returns the session the user chose to resume, or
/// `None` when the user quit.
///
/// # Errors
///
/// Fails when the terminal cannot be taken, drawn on or read.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "focus and paste events change nothing"
)]
pub(crate) fn run(
    select: Option<String>,
    mut replace: Option<(String, String)>,
    carry: Option<String>,
) -> Result<Option<Agent>> {
    let (send, receive) = mpsc::channel();
    let input = send.clone();
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if input.send(Msg::Input(event)).is_err() {
                break;
            }
        }
    });
    let deep_results = send.clone();
    let again = send.clone();
    let wake = send.clone();
    let (feed_send, feed_receive) = mpsc::channel();
    thread::spawn(move || feed::run(&feed_send));
    thread::spawn(move || {
        while let Ok(update) = feed_receive.recv() {
            if send.send(Msg::Feed(update)).is_err() {
                break;
            }
        }
    });

    let mut app = App::new(std::env::var("REX_BLOCK").ok());
    app.want.clone_from(&select);
    // A carried query searches again once the loop runs.
    if let Some(carry) = carry.and_then(|json| serde_json::from_str(&json).ok())
        && app.restore(carry)
    {
        let _ = again.send(Msg::Search);
    }
    let mut found: Option<Arc<Vec<Found>>> = None;
    // One worker reads transcripts for the newest query only: each new
    // query bumps the generation, and the worker drops a stale one between
    // files.
    let generation = Arc::new(AtomicU64::new(0));
    let (deep_send, deep_receive) =
        mpsc::channel::<(u64, Arc<Vec<Found>>, Vec<String>, HashSet<String>)>();
    {
        let generation = Arc::clone(&generation);
        let results = deep_results;
        thread::spawn(move || {
            while let Ok(mut job) = deep_receive.recv() {
                while let Ok(newer) = deep_receive.try_recv() {
                    job = newer;
                }
                let (id, found, terms, skip) = job;
                if generation.load(Ordering::Acquire) != id {
                    continue;
                }
                // A few transcripts at a time, newest first, so matches show
                // as they are found.
                let stale = || generation.load(Ordering::Acquire) != id;
                let total = found.len();
                let mut read = 0;
                let mut closed = false;
                for chunk in found.chunks(4) {
                    if stale() {
                        break;
                    }
                    let hits = catalog::deep(chunk, &terms, &skip, stale);
                    read += chunk.len();
                    if results.send(Msg::Deep(id, hits, (read, total))).is_err() {
                        closed = true;
                        break;
                    }
                }
                if closed {
                    break;
                }
            }
        });
    }
    let mut screen = Screen::enter_with(false)?;
    execute!(std::io::stderr(), SetCursorStyle::BlinkingBar).context("set the cursor shape")?;
    let mut reporter = Reporter::default();
    let mut cache = facts::Cache::new(move || {
        let _ = wake.send(Msg::Wake);
    });
    let started = std::time::Instant::now();
    loop {
        reporter.send(own_status(&app))?;
        app.frame = usize::try_from(started.elapsed().as_millis() / SPIN.as_millis()).unwrap_or(0);
        let mut hits = view::Hits::default();
        let agent = app.current().cloned();
        let facts = agent
            .as_ref()
            .and_then(|agent| cache.get(&agent.transcript).cloned());
        let shown = agent.as_ref().map(|agent| (agent, facts.as_ref()));
        screen
            .terminal()
            .draw(|frame| hits = view::draw(frame, &app, shown))
            .context("draw the app")?;
        app.settle(&hits.tops);
        // The copy this one replaces stays on screen until this one has drawn
        // the panes; then this one takes its place.
        if matches!(app.link, Link::Live)
            && let Some((session, block)) = replace.take()
            && let Err(error) = take_over(select.as_deref(), (&session, &block))
        {
            app.notice = Some(format!("{error:#}"));
        }
        screen.set_pointer(if app.dragging || app.hover.is_some() {
            Pointer::Hand
        } else {
            Pointer::Default
        })?;
        // The spinner follows the clock, so input arriving between ticks never
        // holds it still.
        let elapsed = started.elapsed();
        let tick = elapsed.as_millis() / SPIN.as_millis();
        app.frame = usize::try_from(tick).unwrap_or(0);
        let first = if app.animating() {
            let next = SPIN.saturating_mul(u32::try_from(tick + 1).unwrap_or(u32::MAX));
            match receive.recv_timeout(next.saturating_sub(elapsed)) {
                Ok(msg) => Some(msg),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("the app's inputs closed")
                }
            }
        } else {
            Some(receive.recv().context("the app's inputs closed")?)
        };
        for msg in first.into_iter().chain(receive.try_iter()) {
            let action = match msg {
                Msg::Input(Event::Key(key)) => app.on_key(key),
                Msg::Input(Event::Mouse(mouse)) => app.on_mouse(
                    mouse,
                    hits.target(mouse.column, mouse.row),
                    hits.list(mouse.column, mouse.row),
                    (hits.width, hits.height),
                ),
                Msg::Input(Event::Paste(text)) => app.on_paste(&text),
                Msg::Input(Event::FocusLost) => {
                    app.on_blur();
                    Action::None
                }
                Msg::Input(_) | Msg::Wake => Action::None,
                Msg::Search => Action::Search,
                Msg::Deep(id, hits, progress) => {
                    if id == generation.load(Ordering::Acquire) {
                        app.add_results(hits, progress);
                    }
                    Action::None
                }
                Msg::Feed(feed::Update::Panes(panes)) => {
                    app.set_panes(panes);
                    Action::None
                }
                Msg::Feed(feed::Update::Offline(why)) => {
                    app.link = Link::Offline(why);
                    Action::None
                }
                Msg::Feed(feed::Update::Stale(why)) => {
                    app.link = Link::Stale(why);
                    Action::None
                }
            };
            match action {
                Action::None => {}
                Action::Quit => return Ok(None),
                Action::Resume(agent) => {
                    // A floating app resumes in a tab of its own; a tiled one
                    // becomes the session.
                    let session = match app.focus {
                        state::Focus::Search => None,
                        state::Focus::Panes | state::Focus::Sessions => {
                            app.selected().map(|pane| pane.session_id.clone())
                        }
                    };
                    match floats() {
                        Ok(true) => {
                            if let Err(error) = resume_in_tab(&agent, session.as_deref()) {
                                app.notice = Some(format!("{error:#}"));
                            }
                        }
                        Ok(false) | Err(_) => return Ok(Some(agent)),
                    }
                }
                Action::GoTo {
                    session_id,
                    block_id,
                } => {
                    if let Err(error) = go_to(&session_id, &block_id, &app.carry()) {
                        app.notice = Some(format!("{error:#}"));
                    }
                }
                Action::Search => {
                    if found.is_none() {
                        // Running sessions too: search finds them, and enter
                        // goes to their pane.
                        found = Some(Arc::new(catalog::load(
                            &session::claude_dir()?,
                            &HashSet::new(),
                        )?));
                    }
                    let catalog = found.clone().unwrap_or_default();
                    let pane: Vec<Agent> = app
                        .selected()
                        .map(|pane| {
                            pane.sessions
                                .iter()
                                .map(|(agent, _)| agent.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    let terms = catalog::terms(&app.query);
                    let id = generation.fetch_add(1, Ordering::AcqRel) + 1;
                    if terms.is_empty() {
                        app.set_results(Vec::new());
                        app.deep = false;
                    } else {
                        let hits = catalog::hits(&pane, &catalog, &terms);
                        let skip = hits
                            .iter()
                            .map(|hit| hit.agent.session_id.clone())
                            .collect();
                        app.set_results(hits);
                        app.deep = true;
                        let _ = deep_send.send((id, catalog, terms, skip));
                    }
                }
            }
        }
    }
}

/// What the app's own status record says now.
fn own_status(app: &App) -> Report {
    match &app.link {
        Link::Connecting => Report::Working("Reading Rex".to_owned()),
        Link::Offline(_) => Report::Idle("Rex is not running here".to_owned()),
        Link::Live | Link::Stale(_) => {
            let count = |status| {
                app.panes
                    .iter()
                    .filter(|pane| pane.status == status)
                    .count()
            };
            let waiting = app
                .panes
                .iter()
                .filter(|pane| pane.status.needs_user())
                .count();
            let working = count(crate::status::Status::Working);
            let mut parts = vec![format!("{} panes", app.panes.len())];
            if waiting > 0 {
                parts.push(format!("{waiting} need input"));
            }
            if working > 0 {
                parts.push(format!("{working} working"));
            }
            Report::Idle(parts.join(" · "))
        }
    }
}

/// Shows `block_id` in the Rex app. When this app floats in another window,
/// it starts a copy of itself there instead, out of sight and at its own
/// size, with the pane selected; the copy draws, then switches the view
/// and closes this one (see [`take_over`]), so nothing blank or resizing is
/// ever on screen.
fn go_to(session_id: &str, block_id: &str, carry: &state::Carry) -> Result<()> {
    let mut client = Client::connect(&Endpoint::from_environment()?)?;
    let window_id = client
        .view(session_id)?
        .find(block_id)
        .map(|(_, window, _)| window.window_id.clone())
        .context("the pane is no longer in its session")?;
    let here = std::env::var("REX_SESSION").ok();
    let me = std::env::var("REX_BLOCK").ok();
    let floating = match (&here, &me) {
        (Some(here), Some(me)) => client
            .view(here)?
            .find(me)
            .filter(|(_, own, layer)| layer.floats() && own.window_id != window_id)
            .map(|(_, _, layer)| layer.bounds),
        _ => None,
    };
    let (Some(bounds), Some(here), Some(me)) = (floating, &here, &me) else {
        reveal(&mut client, session_id, block_id)?;
        client.show(
            here.as_deref().unwrap_or(session_id),
            session_id,
            &window_id,
        )?;
        return shown(&mut client, session_id, block_id, &window_id);
    };
    let size = client.terminal_size(here, me)?;
    let exe = std::env::current_exe().context("find the rawr binary")?;
    client.float_command(
        session_id,
        &window_id,
        bounds,
        &[
            exe.display().to_string(),
            "--select".to_owned(),
            block_id.to_owned(),
            "--replace-session".to_owned(),
            here.clone(),
            "--replace-block".to_owned(),
            me.clone(),
            "--carry".to_owned(),
            serde_json::to_string(carry).context("write the app's state")?,
        ],
        &size,
    )?;
    Ok(())
}

/// What a copy started by [`go_to`] does once it has drawn the panes: brings
/// the pane it went to into view, focuses itself over it, asks the Rex app
/// to show its window, and closes the copy it replaces, by that block's ID.
fn take_over(select: Option<&str>, (old_session, old_block): (&str, &str)) -> Result<()> {
    let mut client = Client::connect(&Endpoint::from_environment()?)?;
    let session = std::env::var("REX_SESSION").context("rawr runs outside Rex")?;
    let me = std::env::var("REX_BLOCK").context("rawr runs outside Rex")?;
    if let Some(block) = select {
        reveal(&mut client, &session, block)?;
    }
    client.focus_block(&session, &me)?;
    let window_id = client
        .view(&session)?
        .find(&me)
        .map(|(_, window, _)| window.window_id.clone())
        .context("this copy is no longer in its session")?;
    client.show(old_session, &session, &window_id)?;
    shown(&mut client, &session, &me, &window_id)?;
    client.close_block(old_session, old_block)
}

/// Brings `block_id` into view in its session: its window active, no other
/// block zoomed over it, and its floating layer drawn. Returns its window.
fn reveal(client: &mut Client, session_id: &str, block_id: &str) -> Result<String> {
    let view = client.view(session_id)?;
    let (_, window, layer) = view
        .find(block_id)
        .context("the pane is no longer in its session")?;
    let window_id = window.window_id.clone();
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
    client.focus_block(session_id, block_id)?;
    Ok(window_id)
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

/// Whether this app floats over its window.
fn floats() -> Result<bool> {
    let (Ok(here), Ok(me)) = (std::env::var("REX_SESSION"), std::env::var("REX_BLOCK")) else {
        return Ok(false);
    };
    let mut client = Client::connect(&Endpoint::from_environment()?)?;
    Ok(client
        .view(&here)?
        .find(&me)
        .is_some_and(|(_, _, layer)| layer.floats()))
}

/// Resumes `agent` in a new tab of `session_id`, or of this app's session,
/// named by its title, and shows that tab.
fn resume_in_tab(agent: &Agent, session_id: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        std::path::Path::new(&agent.cwd).is_dir(),
        "{} no longer exists",
        agent.cwd
    );
    let here = std::env::var("REX_SESSION").context("rawr runs outside Rex")?;
    let session = session_id.unwrap_or(&here);
    let mut client = Client::connect(&Endpoint::from_environment()?)?;
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
    client.show(&here, session, &window)?;
    thread::sleep(Duration::from_millis(200));
    client.show(&here, session, &window)
}

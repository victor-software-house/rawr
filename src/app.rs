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
pub(crate) fn run() -> Result<Option<Agent>> {
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
    let mut cache = facts::Cache::default();
    let started = std::time::Instant::now();
    loop {
        reporter.send(own_status(&app))?;
        app.frame = usize::try_from(started.elapsed().as_millis() / SPIN.as_millis()).unwrap_or(0);
        let mut hits = view::Hits::default();
        let agent = app.current().cloned();
        let facts = agent
            .as_ref()
            .and_then(|agent| cache.get(&agent.transcript).cloned());
        let shown = agent.as_ref().zip(facts.as_ref());
        screen
            .terminal()
            .draw(|frame| hits = view::draw(frame, &app, shown))
            .context("draw the app")?;
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
                Msg::Input(_) => Action::None,
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
                Action::Resume(agent) => return Ok(Some(agent)),
                Action::GoTo {
                    session_id,
                    block_id,
                } => match go_to(&session_id, &block_id) {
                    Ok(Went::Stayed) => {}
                    Ok(Went::Handed) => return Ok(None),
                    Err(error) => app.notice = Some(format!("{error:#}")),
                },
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

/// What going to a pane did with the app itself.
enum Went {
    /// The app is still on screen: tiled where it was, or floating over the
    /// pane it went to.
    Stayed,
    /// A new app floats over the pane in its session; this one ends.
    Handed,
}

/// Shows `block_id` in the Rex app. When this app floats, it floats over the
/// pane it went to: moved there within its own session, or started there
/// afresh in another session, since a block stays in its session.
fn go_to(session_id: &str, block_id: &str) -> Result<Went> {
    let mut client = Client::connect(&Endpoint::from_environment()?)?;
    let target = client.view(session_id)?;
    let (_, window, _) = target
        .find(block_id)
        .context("the pane is no longer in its session")?;
    let window_id = window.window_id.clone();
    let here = std::env::var("REX_SESSION").ok();
    let me = std::env::var("REX_BLOCK").ok();
    let floating = match (&here, &me) {
        (Some(here), Some(me)) => client
            .view(here)?
            .find(me)
            .filter(|(_, own, layer)| layer.floats() && own.window_id != window_id)
            .map(|(_, _, layer)| (layer.bounds, layer.layer_id.clone())),
        _ => None,
    };
    client.focus_block(session_id, block_id)?;
    client.show(
        here.as_deref().unwrap_or(session_id),
        session_id,
        &window_id,
    )?;
    let (Some((bounds, layer)), Some(here), Some(me)) = (floating, here, me) else {
        return Ok(Went::Stayed);
    };
    if here == session_id {
        client.move_beside(&here, &me, block_id)?;
        client.float(&here, &me, bounds)?;
        client.focus_block(&here, &me)?;
        return Ok(Went::Stayed);
    }
    let exe = std::env::current_exe().context("find the rawr binary")?;
    client.float_command(session_id, &window_id, bounds, &[exe.display().to_string()])?;
    Ok(Went::Handed)
}

//! The rawr app: every Rex pane that has a Claude session, what each Claude
//! is doing, and the way back to a session.
//!
//! Two threads feed one channel: the terminal's input and the Rex feed. The
//! loop drains the channel and draws once after each batch, with no timer.

pub(crate) mod facts;
pub(crate) mod feed;
mod report;
pub(crate) mod state;
mod theme;
mod view;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event};
use ratatui::crossterm::execute;
use report::{Report, Reporter};
use state::{Action, App, Link};

use crate::app::facts::Facts;
use crate::catalog;
use crate::daemon::link::{self, Incoming};
use crate::daemon::{FromDaemon, Reach, ToDaemon};
use crate::picker::terminal::{Pointer, Screen};
use crate::session::Agent;

/// How often the spinner turns while a pane works; the loop waits on
/// input with no timer otherwise.
const SPIN: Duration = Duration::from_millis(100);

/// How often a moving focus redraws: about 60 frames a second.
const FRAME: Duration = Duration::from_millis(16);

enum Msg {
    Input(Event),
    Daemon(Incoming),
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
pub(crate) fn run(view: Option<u64>) -> Result<Option<Agent>> {
    let (send, receive) = mpsc::channel();
    let input = send.clone();
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if input.send(Msg::Input(event)).is_err() {
                break;
            }
        }
    });
    // The daemon holds the panes, the facts and the search, and acts in Rex;
    // this window holds only its view, and draws.
    let daemon = link::start(view, move |incoming| {
        send.send(Msg::Daemon(incoming)).is_ok()
    })?;

    let mut app = App::new(std::env::var("REX_BLOCK").ok());
    // Whether this window replaces another once it has drawn the panes.
    let mut replacing = false;
    // Whether the daemon's panes have arrived since this window connected.
    let mut panes_in = false;
    // The newest search this window sent; hits for an older one are dropped.
    let mut generation: u64 = 0;
    let mut facts: HashMap<PathBuf, Facts> = HashMap::new();
    let mut asked: Option<PathBuf> = None;
    let mut screen = Screen::enter_with(false)?;
    execute!(std::io::stderr(), SetCursorStyle::BlinkingBar).context("set the cursor shape")?;
    let mut reporter = Reporter::default();
    let started = std::time::Instant::now();
    loop {
        reporter.send(own_status(&app))?;
        app.frame = usize::try_from(started.elapsed().as_millis() / SPIN.as_millis()).unwrap_or(0);
        let mut hits = view::Hits::default();
        let agent = app.current().cloned();
        if let Some(agent) = &agent
            && asked.as_ref() != Some(&agent.transcript)
        {
            asked = Some(agent.transcript.clone());
            daemon.send(ToDaemon::Facts {
                path: agent.transcript.clone(),
            });
        }
        let known = agent
            .as_ref()
            .and_then(|agent| facts.get(&agent.transcript));
        let shown = agent.as_ref().map(|agent| (agent, known));
        screen
            .terminal()
            .draw(|frame| hits = view::draw(frame, &app, shown))
            .context("draw the app")?;
        app.settle(&hits.tops);
        // The window this one replaces stays on screen until this one has
        // drawn the panes; then the daemon swaps them.
        if replacing && panes_in && matches!(app.link, Link::Live) {
            replacing = false;
            daemon.send(ToDaemon::Drawn);
        }
        screen.set_pointer(if app.hover.is_some() || app.dragging.is_some() {
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
            // A moving focus draws at the screen's rate; a spinner at its own.
            let wait = if app.moving() {
                FRAME
            } else {
                SPIN.saturating_mul(u32::try_from(tick + 1).unwrap_or(u32::MAX))
                    .saturating_sub(elapsed)
            };
            match receive.recv_timeout(wait) {
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
                Msg::Input(Event::Mouse(mouse)) => {
                    app.on_mouse(mouse, hits.under(mouse.column, mouse.row))
                }
                Msg::Input(Event::Paste(text)) => app.on_paste(&text),
                Msg::Input(Event::FocusLost) => {
                    app.on_blur();
                    Action::None
                }
                Msg::Input(_) => Action::None,
                Msg::Daemon(Incoming::Connected { carry }) => {
                    // What this window asked before the link went.
                    asked = None;
                    panes_in = false;
                    if let Some(carry) = carry {
                        replacing = true;
                        app.restore(carry);
                    }
                    if app.focus == state::Focus::Search && !app.query.trim().is_empty() {
                        Action::Search
                    } else {
                        Action::None
                    }
                }
                Msg::Daemon(Incoming::Lost(why)) => {
                    app.link = Link::Stale(why);
                    Action::None
                }
                Msg::Daemon(Incoming::Message(message)) => {
                    match message {
                        FromDaemon::Panes { panes } => {
                            panes_in = true;
                            app.set_panes(panes);
                            // A pane's activity may have moved its transcript.
                            asked = None;
                        }
                        FromDaemon::Link { state, reason } => {
                            let why = reason.unwrap_or_default();
                            app.link = match state {
                                Reach::Live => Link::Live,
                                Reach::Offline => Link::Offline(why),
                                Reach::Stale => Link::Stale(why),
                            };
                        }
                        FromDaemon::Facts { path, facts: read } => {
                            facts.insert(path, read);
                        }
                        FromDaemon::Hits {
                            id,
                            hits,
                            read,
                            total,
                        } if id == generation => {
                            if read == 0 {
                                app.set_results(hits);
                                app.deep = total > 0;
                                app.read = (0, total);
                            } else {
                                app.add_results(hits, (read, total));
                            }
                        }
                        FromDaemon::Exec { agent } => return Ok(Some(agent)),
                        FromDaemon::Notice { text } => app.notice = Some(text),
                        FromDaemon::Hits { .. }
                        | FromDaemon::Welcome { .. }
                        | FromDaemon::Refused { .. } => {}
                    }
                    Action::None
                }
            };
            match action {
                Action::None => {}
                Action::Quit => return Ok(None),
                Action::Resume(agent) => {
                    // A floating window resumes in a tab of the pane's session;
                    // the daemon decides, and answers `exec` for a tiled one.
                    let session = match app.focus {
                        state::Focus::Search => None,
                        state::Focus::Panes | state::Focus::Sessions | state::Focus::Session => {
                            app.selected().map(|pane| pane.session_id.clone())
                        }
                    };
                    daemon.send(ToDaemon::Resume { agent, session });
                }
                Action::GoTo {
                    session_id,
                    block_id,
                } => daemon.send(ToDaemon::GoTo {
                    session: session_id,
                    block: block_id,
                    carry: app.carry(),
                }),
                Action::Search => {
                    generation += 1;
                    if catalog::terms(&app.query).is_empty() {
                        app.set_results(Vec::new());
                        app.deep = false;
                    }
                    daemon.send(ToDaemon::Search {
                        id: generation,
                        query: app.query.clone(),
                        pane: app.selected().map(|pane| pane.block_id.clone()),
                    });
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

//! The rawr app: every Rex pane that has a Claude session, what each Claude
//! is doing, and the way back to a session.
//!
//! Two threads feed one channel: the terminal's input and the Rex feed. The
//! loop drains the channel and draws once after each batch, with no timer.

mod feed;
mod report;
mod state;
mod theme;
mod view;

use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event};
use report::{Report, Reporter};
pub(crate) use state::Layout;
use state::{Action, App, Link};

use crate::catalog::{self, Found};
use crate::picker::terminal::{Pointer, Screen};
use crate::session::{self, Agent};

enum Msg {
    Input(Event),
    Feed(feed::Update),
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
pub(crate) fn run(layout: Layout) -> Result<Option<Agent>> {
    let (send, receive) = mpsc::channel();
    let input = send.clone();
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if input.send(Msg::Input(event)).is_err() {
                break;
            }
        }
    });
    let (feed_send, feed_receive) = mpsc::channel();
    thread::spawn(move || feed::run(&feed_send));
    thread::spawn(move || {
        while let Ok(update) = feed_receive.recv() {
            if send.send(Msg::Feed(update)).is_err() {
                break;
            }
        }
    });

    let mut app = App::new(layout, std::env::var("REX_BLOCK").ok());
    let mut found: Option<Vec<Found>> = None;
    let mut screen = Screen::enter_with(false)?;
    let mut reporter = Reporter::default();
    loop {
        reporter.send(own_status(&app))?;
        let mut hits = view::Hits::default();
        screen
            .terminal()
            .draw(|frame| hits = view::draw(frame, &app))
            .context("draw the app")?;
        screen.set_pointer(if app.dragging || app.hover.is_some() {
            Pointer::Hand
        } else {
            Pointer::Default
        })?;
        let first = receive.recv().context("the app's inputs closed")?;
        for msg in std::iter::once(first).chain(receive.try_iter()) {
            let action = match msg {
                Msg::Input(Event::Key(key)) => app.on_key(key),
                Msg::Input(Event::Mouse(mouse)) => app.on_mouse(
                    mouse,
                    hits.target(mouse.column, mouse.row),
                    hits.list(mouse.column, mouse.row),
                    hits.body,
                ),
                Msg::Input(Event::Paste(text)) => app.on_paste(&text),
                Msg::Input(Event::FocusLost) => {
                    app.on_blur();
                    Action::None
                }
                Msg::Input(_) => Action::None,
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
                } => {
                    app.notice = Some(format!(
                        "go to pane is not built yet: {session_id} {block_id}"
                    ));
                }
                Action::Search => {
                    if found.is_none() {
                        let live = session::live_sessions().unwrap_or_default();
                        found = Some(catalog::load(&session::claude_dir()?, &live)?);
                    }
                    let pane: Vec<Agent> = app
                        .selected()
                        .map(|pane| {
                            pane.sessions
                                .iter()
                                .map(|(agent, _)| agent.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    let results = if app.query.trim().is_empty() {
                        Vec::new()
                    } else {
                        catalog::search(
                            &pane,
                            found.as_deref().unwrap_or(&[]),
                            &catalog::terms(&app.query),
                        )
                    };
                    app.set_results(results);
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
            let waiting = count(crate::status::Status::NeedsInput);
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

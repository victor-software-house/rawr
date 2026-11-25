//! The rawr app: every Rex pane that has a Claude session, what each Claude
//! is doing, and the way back to a session.
//!
//! Two threads feed one channel: the terminal's input and the Rex feed. The
//! loop drains the channel and draws once after each batch of anything but
//! wheel reports. Those wait for a clock of frames, which runs while a
//! report waits or something moves, and at rest the loop blocks on input.

pub(crate) mod facts;
pub(crate) mod feed;
pub(crate) mod history;
mod layouts;
mod markup;
mod pacing;
mod report;
mod scrollbar;
mod shell;
pub(crate) mod state;
mod terminal;
mod theme;
mod view;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{env, hint, io, thread};

use anyhow::{Context, Result};
use pacing::{MIN_SLEEP, Moves, Pacer, Report as Wheel, SPIN_LEAD, Wait, Wake};
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{self, Event, MouseEventKind};
use ratatui::crossterm::execute;
use report::{Report, Reporter, State};
use state::{Action, App, Drag, Focus, Link};
use terminal::{Pointer, Screen};

use crate::app::facts::Way;
use crate::app::history::{Ask, History, PAGE};
use crate::catalog;
use crate::daemon::link::{self, Incoming};
use crate::daemon::{FromDaemon, Reach, ToDaemon};
use crate::session::Agent;

/// How often the spinner turns while a pane works.
const SPIN: Duration = Duration::from_millis(100);

enum Msg {
    /// A terminal event, with when the input thread read it.
    Input(Event, Instant),
    Daemon(Box<Incoming>),
}

/// How a window opens.
#[derive(Debug)]
pub(crate) enum Mode {
    /// Every pane, its sessions and the search over every session.
    Full,
    /// A pane's own sessions only, for the pane `block` that a restart
    /// brought back as a fresh shell. `title` names the pane after its
    /// newest resumable session.
    StandIn {
        /// The pane, by Rex block ID.
        block: String,
        /// The pane's terminal title, and the title of its status record.
        title: String,
    },
}

/// Opens the app and returns the session the user chose to resume, or
/// `None` when the user quit. A stand-in lists only its own pane.
///
/// # Errors
///
/// Fails when the terminal cannot be taken, drawn on or read.
#[expect(
    clippy::too_many_lines,
    reason = "one loop that draws, waits and handles every message the window gets"
)]
pub(crate) fn run(view: Option<u64>, mode: &Mode) -> Result<Option<Agent>> {
    let (send, receive) = mpsc::channel();
    let input = send.clone();
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if input.send(Msg::Input(event, Instant::now())).is_err() {
                break;
            }
        }
    });
    // The daemon holds the panes, the facts and the search, and acts in Rex;
    // this window holds only its view, and draws.
    let daemon = link::start(view, move |incoming| {
        send.send(Msg::Daemon(Box::new(incoming))).is_ok()
    })?;

    markup::load();
    let mut app = match mode {
        Mode::Full => App::new(env::var("REX_BLOCK").ok()),
        Mode::StandIn { block, .. } => App::stand_in(block.clone()),
    };
    // Whether this window replaces another once it has drawn the panes.
    let mut replacing = false;
    // Whether the daemon's panes have arrived since this window connected.
    let mut panes_in = false;
    // The newest search this window sent; hits for an older one are dropped.
    let mut generation: u64 = 0;
    let mut histories: HashMap<PathBuf, History> = HashMap::new();
    let mut asked: Option<PathBuf> = None;
    // The transcript shown last frame, whose held entries are the only ones
    // kept.
    let mut shown_path: Option<PathBuf> = None;
    let title = match mode {
        Mode::Full => None,
        Mode::StandIn { title, .. } => Some(title.as_str()),
    };
    let mut screen = Screen::enter(title)?;
    execute!(io::stderr(), SetCursorStyle::BlinkingBar).context("set the cursor shape")?;
    let mut reporter = Reporter::default();
    // A stand-in reports once, before its first frame, and never again; the
    // full app reports whenever its own state flips.
    if let Some(title) = title {
        reporter.send(stand_in_status(title))?;
    }
    let started = Instant::now();
    let mut pacer = Pacer::new(started);
    // Where the last frame put what takes a click, which input read before
    // the next frame is matched against.
    let mut hits = view::Hits::default();
    // The spinner's frame in the last frame drawn.
    let mut drawn_spin = usize::MAX;
    // How each scrollbar drew in the last frame, which fades by the clock.
    let mut drawn_bars = app.bar_shades(started);
    // The lines each list moves in the frame to draw; none once it is drawn.
    let mut due = Some(Moves::default());
    loop {
        if matches!(mode, Mode::Full) {
            reporter.send(own_status(&app))?;
        }
        let agent = app.current().cloned();
        if let Some(agent) = &agent
            && shown_path.as_ref() != Some(&agent.transcript)
        {
            history::keep_only(&mut histories, &agent.transcript);
            shown_path = Some(agent.transcript.clone());
        }
        if let Some(agent) = &agent
            && asked.as_ref() != Some(&agent.transcript)
        {
            asked = Some(agent.transcript.clone());
            daemon.send(ToDaemon::Facts {
                path: agent.transcript.clone(),
            });
        }
        if let Some(moves) = due.take() {
            app.now = Instant::now();
            for (list, lines) in Focus::LISTS.into_iter().zip(moves) {
                if lines != 0 {
                    app.scroll_lines(list, lines);
                }
            }
            // The spinner follows its own cadence, so input arriving between
            // its turns never holds it still.
            app.frame = spin_at(started, Instant::now());
            drawn_spin = app.frame;
            app.ahead = app.prefetch.lines(Instant::now());
            let known = agent
                .as_ref()
                .and_then(|agent| histories.get(&agent.transcript));
            let shown = agent.as_ref().map(|agent| (agent, known));
            screen
                .draw(|frame| hits = view::draw(frame, &mut app, shown))
                .context("draw the app")?;
            app.settle(&hits.tops, hits.anchor, &hits.bars);
            app.found_seek(hits.seek);
            drawn_bars = app.bar_shades(app.now);
            // Entries held past the view toward what is not held, no more than
            // the lead, are asked for before the view reaches them.
            // None while the thumb is held or on its way to a place, which
            // would otherwise ask for every page the view passes.
            if let (Some(agent), Some(reach)) = (&agent, hits.reach)
                && !app.holds_view()
                && let Some(history) = histories.get_mut(&agent.transcript)
                && let Some(ask) = history.settle(reach, app.ahead)
            {
                let path = agent.transcript.clone();
                daemon.send(match ask.way {
                    Way::Older => ToDaemon::Older {
                        path,
                        list: ask.list,
                        before: ask.at,
                        count: ask.count,
                    },
                    Way::Newer => ToDaemon::Newer {
                        path,
                        list: ask.list,
                        after: ask.at,
                        count: ask.count,
                    },
                });
            }
            app.cols = hits.width;
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
        }
        // The clock runs while a report waits. Focus on the move wakes the
        // loop every frame of the move. Otherwise the loop sleeps until
        // input, until the spinner turns, or until a pointer past the
        // window's edge has been away long enough to shut the drawer.
        let spinner = app.animating().then(|| {
            started + SPIN * u32::try_from(spin_at(started, Instant::now()) + 1).unwrap_or(u32::MAX)
        });
        app.now = Instant::now();
        let first = match pacer.wait(
            app.moving(),
            &[spinner, app.wake_at(), app.bars_wake(), app.jump_wake()],
        ) {
            Some(wait) => wait_until(&receive, wait)?,
            None => Some(receive.recv().context("the app's inputs closed")?),
        };
        let woke = Instant::now();
        app.now = woke;
        // What reads between frames: the wheel waits for the clock, a key or
        // a click draws at once, and anything else that changes what is on
        // screen draws at once while the clock rests, and on its next tick
        // while it runs.
        let mut reports = Vec::new();
        let mut urgent = false;
        let mut dirty = false;
        for msg in first.into_iter().chain(receive.try_iter()) {
            let mut wheel = false;
            let mut calm = matches!(msg, Msg::Daemon(_));
            let action = match msg {
                Msg::Input(Event::Key(key), _) => app.on_key(key),
                Msg::Input(Event::Mouse(mouse), at) => {
                    let action = app.on_mouse(mouse, hits.under(mouse.column, mouse.row));
                    // A held thumb draws on the clock, as the pointer's moves
                    // do.
                    calm = mouse.kind == MouseEventKind::Moved
                        || (matches!(mouse.kind, MouseEventKind::Drag(_))
                            && matches!(app.dragging, Some(Drag::Thumb(_))));
                    if let Action::Wheel(list, rows) = action {
                        wheel = true;
                        reports.push(Wheel { list, rows, at });
                        Action::None
                    } else {
                        action
                    }
                }
                Msg::Input(Event::Paste(text), _) => app.on_paste(&text),
                Msg::Input(Event::FocusLost, _) => {
                    app.on_blur();
                    Action::None
                }
                Msg::Input(_, _) => Action::None,
                Msg::Daemon(incoming) => match *incoming {
                    Incoming::Connected { carry } => {
                        // What this window asked before the link went.
                        asked = None;
                        panes_in = false;
                        for history in histories.values_mut() {
                            history.lost();
                        }
                        app.drop_jump();
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
                    Incoming::Lost(why) => {
                        app.link = Link::Stale(why);
                        Action::None
                    }
                    Incoming::Message(message) => {
                        match message {
                            FromDaemon::Panes { panes } => {
                                panes_in = true;
                                app.set_panes(panes);
                                // A pane's activity may have moved its
                                // transcript.
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
                                let stale = histories.entry(path.clone()).or_default().merge(read);
                                app.layouts.forget(&path, &stale);
                            }
                            FromDaemon::Page {
                                path,
                                list,
                                way,
                                at,
                                entries,
                                more,
                                failed,
                            } => {
                                if let Some(history) = histories.get_mut(&path) {
                                    let ask = Ask {
                                        way,
                                        list,
                                        at,
                                        count: PAGE,
                                    };
                                    if let Some(reason) = failed {
                                        history.fail(ask);
                                        app.notice = Some(reason);
                                    } else {
                                        let stale = history.land(ask, entries, more);
                                        app.layouts.forget(&path, &stale);
                                    }
                                }
                            }
                            FromDaemon::Around {
                                path,
                                list,
                                ask,
                                entries,
                                older,
                                place,
                                failed,
                            } => {
                                let epoch = histories.get(&path).map(|history| history.epoch(list));
                                let current =
                                    app.current().is_some_and(|agent| agent.transcript == path);
                                if current
                                    && epoch.is_some_and(|epoch| app.expects(list, ask, epoch))
                                {
                                    if let Some(reason) = failed {
                                        app.notice = Some(reason);
                                        app.drop_jump();
                                    } else if let (Some(history), Some(place)) =
                                        (histories.get_mut(&path), place)
                                    {
                                        let stale = history.land_around(list, entries, older);
                                        app.layouts.forget(&path, &stale);
                                        app.arrive(place);
                                    } else {
                                        app.drop_jump();
                                    }
                                } else if current && app.awaits(list, ask) {
                                    // The numbering changed under the request,
                                    // as a rewrite of the transcript does: the
                                    // place is gone.
                                    app.drop_jump();
                                }
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
                },
            };
            if calm {
                dirty = true;
            } else {
                urgent |= !wheel;
            }
            match action {
                Action::None | Action::Wheel(..) => {}
                Action::Quit => return Ok(None),
                Action::OpenFile(index) | Action::FilterFolder(index) => {
                    let file = app
                        .current()
                        .and_then(|agent| histories.get(&agent.transcript))
                        .and_then(|known| {
                            // Past the changed files come the ones only read.
                            let known = &known.facts;
                            known
                                .files
                                .get(index)
                                .or_else(|| known.read.get(index.checked_sub(known.files.len())?))
                        })
                        .map(|(path, _)| PathBuf::from(path));
                    match (action, file) {
                        (Action::OpenFile(_), Some(file)) => open_file(&file, &mut app),
                        (_, Some(file)) => {
                            let folder = file.parent().map(|folder| folder.display().to_string());
                            app.filters[state::Focus::Session.index()] =
                                folder.map_or_default(|folder| format!("{folder}/"));
                        }
                        (_, None) => {}
                    }
                }
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
            // A list whose scroll this message set another way has no use for
            // the wheel reports read before it, and keeps those read after.
            pacer.reset(&app.take_cleared(), &mut reports);
        }
        // A thumb let go of, or at rest, on a place no held entry covers asks
        // for the entries there.
        if let Some(agent) = app.current().cloned()
            && let Some(jump) = app.take_jump(|list| {
                histories
                    .get(&agent.transcript)
                    .map_or(0, |history| history.epoch(list))
            })
        {
            daemon.send(ToDaemon::Around {
                path: agent.transcript,
                list: jump.list,
                at: jump.share,
                count: PAGE,
                ask: jump.ask,
            });
        }
        // A pointer past the window's edge shuts the drawer once it has
        // stayed away a moment.
        if app.tick() {
            urgent = true;
        }
        due = pacer.wake(&Wake {
            now: woke,
            reports: &reports,
            urgent,
            dirty: dirty
                || (app.animating() && spin_at(started, woke) != drawn_spin)
                || app.bar_shades(woke) != drawn_bars,
            moving: app.moving(),
        });
    }
}

/// Waits for the next message until `wait.at`. A wait that spins sleeps until
/// shortly before then and polls for the rest, unless the sleep would be too
/// short to pay; any other sleeps the whole way.
fn wait_until(receive: &mpsc::Receiver<Msg>, wait: Wait) -> Result<Option<Msg>> {
    let lead = if wait.spin { SPIN_LEAD } else { Duration::ZERO };
    let sleep = wait
        .at
        .saturating_duration_since(Instant::now())
        .saturating_sub(lead);
    if !wait.spin || sleep >= MIN_SLEEP {
        match receive.recv_timeout(sleep) {
            Ok(msg) => return Ok(Some(msg)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("the app's inputs closed")
            }
        }
    }
    while wait.spin && Instant::now() < wait.at {
        match receive.try_recv() {
            Ok(msg) => return Ok(Some(msg)),
            Err(mpsc::TryRecvError::Empty) => hint::spin_loop(),
            Err(mpsc::TryRecvError::Disconnected) => anyhow::bail!("the app's inputs closed"),
        }
    }
    Ok(None)
}

/// The spinner's frame at `now`: it turns every [`SPIN`], whatever the
/// frames between.
fn spin_at(started: Instant, now: Instant) -> usize {
    usize::try_from(now.saturating_duration_since(started).as_millis() / SPIN.as_millis())
        .unwrap_or(0)
}

/// Opens `file` in its default app, as a click on a `file://` link would;
/// a file gone since the session touched it says so in the search box.
fn open_file(file: &Path, app: &mut App) {
    if !file.exists() {
        app.notice = Some(format!("{} is gone", file.display()));
        return;
    }
    match Command::new("/usr/bin/open").arg(file).spawn() {
        Ok(mut child) => {
            // The opener ends at once; its exit is collected off the loop.
            thread::spawn(move || child.wait());
        }
        Err(error) => app.notice = Some(format!("could not open {}: {error}", file.display())),
    }
}

/// What a stand-in's status record says: idle, under its session's title,
/// since only the session's origin says rawr.
fn stand_in_status(title: &str) -> Report {
    Report {
        state: State::Idle,
        title: title.to_owned(),
        msg: "r resumes it here".to_owned(),
    }
}

/// What the full app's own status record says now: blocked while any pane
/// needs the user, idle otherwise, so a second waiting pane changes nothing.
fn own_status(app: &App) -> Report {
    let (state, msg) = if app.panes.iter().any(|pane| pane.status.needs_user()) {
        (State::Blocked, "An agent needs you")
    } else {
        (State::Idle, "Every pane and session")
    };
    Report {
        state,
        title: "rawr".to_owned(),
        msg: msg.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::app::state::fixtures::pane;
    use crate::status::Status;

    /// A report, then a key that switches the tab, then a report, in one batch:
    /// the key sets the scroll another way, so the report read before it is
    /// dropped and the one read after it moves the list.
    #[test]
    fn a_reset_drops_the_wheel_reports_read_before_it_only() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut app = App::new(None);
        app.set_panes(vec![pane(
            "block-a",
            "alpha/one",
            Status::Idle,
            &["Fix login"],
        )]);
        let mut pacer = Pacer::new(t0);
        let wheel = |at| Wheel {
            list: Focus::Session,
            rows: 2,
            at,
        };
        let mut reports = vec![wheel(t0)];
        app.on_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        pacer.reset(&app.take_cleared(), &mut reports);
        reports.push(wheel(t0 + ms(1)));
        let moves = pacer
            .wake(&Wake {
                now: t0 + ms(1),
                reports: &reports,
                urgent: false,
                dirty: false,
                moving: false,
            })
            .expect("a frame");
        assert_eq!(moves[state::slot(Focus::Session)], 2);
    }

    /// Whether the full app sends a report for each list of panes, shown in
    /// turn with these statuses.
    fn sends(lists: &[&[Status]]) -> Vec<bool> {
        let mut app = App::new(None);
        let mut reporter = Reporter::default();
        lists
            .iter()
            .map(|statuses| {
                let panes = statuses
                    .iter()
                    .enumerate()
                    .map(|(place, status)| {
                        pane(
                            &format!("block-{place}"),
                            &format!("work/{place}"),
                            *status,
                            &["Fix checkout"],
                        )
                    })
                    .collect();
                app.set_panes(panes);
                reporter.next(own_status(&app)).is_some()
            })
            .collect()
    }

    #[test]
    fn the_full_app_sends_nothing_while_no_pane_needs_the_user() {
        let lists: [&[Status]; 3] = [
            &[Status::Idle],
            &[Status::Idle, Status::Working],
            &[Status::Working, Status::Working],
        ];
        assert_eq!(sends(&lists), [true, false, false]);
    }

    #[test]
    fn the_full_app_sends_only_when_it_flips_between_idle_and_blocked() {
        let lists: [&[Status]; 5] = [
            &[Status::NeedsInput],
            &[Status::NeedsInput, Status::NeedsPermission],
            &[Status::NeedsInput, Status::NeedsPermission, Status::Working],
            &[Status::Working, Status::Idle],
            &[Status::Working, Status::NeedsLogin],
        ];
        assert_eq!(sends(&lists), [true, false, false, true, true]);
    }

    #[test]
    fn a_stand_ins_one_report_carries_its_session_title() {
        let mut reporter = Reporter::default();
        let bytes = reporter
            .next(stand_in_status("Fix checkout"))
            .expect("the first report goes out");
        let text = String::from_utf8(bytes).expect("a report is text");
        // base64 of "Fix checkout"; "rawr" is cmF3cg==.
        assert!(text.contains(":app=rawr:"), "{text:?}");
        assert!(text.contains(":title=Rml4IGNoZWNrb3V0:"), "{text:?}");
        assert!(!text.contains("title=cmF3cg=="), "{text:?}");
        assert!(text.starts_with("\x1b]7501;state=idle:"), "{text:?}");
    }
}

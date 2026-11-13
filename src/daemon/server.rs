//! The daemon's side: the lock, the socket, the Rex feed shared by every
//! client, and one reader, one writer and two workers per client.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::{Build, FromDaemon, Here, Reach, ToDaemon, actions, folder, socket};
use crate::app::facts::Reading;
use crate::app::feed::{self, Pane, Update};
use crate::app::state::Carry;
use crate::catalog::{self, Found};
use crate::session;

/// How many messages wait for one client before the daemon drops it.
const QUEUE: usize = 64;

/// How long the daemon waits before following Rex again after the feed ends.
const RETRY: Duration = Duration::from_secs(2);

/// How often a transcript being read sends what it has found so far.
const SEND_EVERY: Duration = Duration::from_millis(50);

/// What every client shares.
struct Hub {
    build: Build,
    state: Mutex<State>,
    /// Each transcript a window showed, as far as it is read.
    facts: Mutex<HashMap<PathBuf, Reading>>,
    catalog: Mutex<Option<Arc<Vec<Found>>>>,
}

struct State {
    panes: Vec<Pane>,
    link: (Reach, Option<String>),
    clients: HashMap<u64, SyncSender<FromDaemon>>,
    next: u64,
    /// Hand-offs waiting for their new window to draw, by view.
    handoffs: HashMap<u64, Handoff>,
    /// The clients with a go-to under way, which take no other until it
    /// ends, so clicks in a row start one copy, not one each.
    going: HashSet<u64>,
}

/// A floating window going to a pane in another tab: the copy the daemon
/// started there opens with `carry`, then takes over from `old`.
struct Handoff {
    carry: Carry,
    old: Here,
    target: String,
    /// The client of the window it replaces.
    from: u64,
}

fn notice(send: &SyncSender<FromDaemon>, error: &anyhow::Error) {
    let _ = send.try_send(FromDaemon::Notice {
        text: format!("{error:#}"),
    });
}

fn hold<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs the daemon until a client asks it to shut down.
///
/// # Errors
///
/// Fails when another daemon holds the lock, or the socket cannot be bound.
pub(crate) fn run() -> Result<()> {
    // Out of the terminal's session, so closing the pane that started it
    // does not end it.
    let _ = rustix::process::setsid();
    let dir = folder()?;
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restrict {}", dir.display()))?;
    let lock = File::create(dir.join("daemon.lock")).context("open daemon.lock")?;
    if lock.try_lock().is_err() {
        bail!("a rawr daemon already runs");
    }
    let path = socket()?;
    // The lock is held, so a socket file left behind belongs to no daemon.
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path).with_context(|| format!("bind {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict {}", path.display()))?;
    let hub = Arc::new(Hub {
        build: Build::current()?,
        state: Mutex::new(State {
            panes: Vec::new(),
            link: (Reach::Stale, Some("reading Rex".to_owned())),
            clients: HashMap::new(),
            next: 0,
            handoffs: HashMap::new(),
            going: HashSet::new(),
        }),
        facts: Mutex::new(HashMap::new()),
        catalog: Mutex::new(None),
    });
    {
        let hub = Arc::clone(&hub);
        thread::spawn(move || follow(&hub));
    }
    let mut out = std::io::stdout();
    writeln!(out, "ready").context("say ready")?;
    out.flush().context("say ready")?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let hub = Arc::clone(&hub);
        let path = path.clone();
        thread::spawn(move || serve(&hub, stream, &path));
    }
    drop(lock);
    Ok(())
}

/// Follows Rex for as long as the daemon runs, again after each end.
fn follow(hub: &Hub) {
    loop {
        let (send, receive) = mpsc::channel();
        let feed = thread::spawn(move || feed::run(&send));
        for update in receive {
            match update {
                Update::Panes(panes) => {
                    let mut state = hold(&hub.state);
                    state.panes.clone_from(&panes);
                    if state.link.0 != Reach::Live {
                        state.link = (Reach::Live, None);
                        broadcast(
                            &mut state,
                            &FromDaemon::Link {
                                state: Reach::Live,
                                reason: None,
                            },
                        );
                    }
                    broadcast(&mut state, &FromDaemon::Panes { panes });
                }
                Update::Offline(why) => set_link(hub, Reach::Offline, why),
                Update::Stale(why) => set_link(hub, Reach::Stale, why),
            }
        }
        let _ = feed.join();
        thread::sleep(RETRY);
    }
}

fn set_link(hub: &Hub, reach: Reach, why: String) {
    let mut state = hold(&hub.state);
    state.link = (reach, Some(why.clone()));
    broadcast(
        &mut state,
        &FromDaemon::Link {
            state: reach,
            reason: Some(why),
        },
    );
}

/// Queues `message` for every client, and drops a client whose queue is full.
fn broadcast(state: &mut State, message: &FromDaemon) {
    state
        .clients
        .retain(|_, client| match client.try_send(message.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        });
}

/// One client: the handshake, then its requests until it goes.
fn serve(hub: &Arc<Hub>, stream: UnixStream, socket: &Path) {
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let mut lines = BufReader::new(stream);
    let (send, queue) = mpsc::sync_channel::<FromDaemon>(QUEUE);
    thread::spawn(move || write_all(writer, &queue));
    let mut line = String::new();
    let Ok(ToDaemon::Hello { build, view, here }) = read(&mut lines, &mut line) else {
        return;
    };
    if build != hub.build {
        let _ = send.try_send(FromDaemon::Refused {
            build: hub.build.clone(),
        });
        // A refused client may still ask the daemon to stop.
        if let Ok(ToDaemon::Shutdown) = read(&mut lines, &mut line) {
            shut_down(socket);
        }
        return;
    }
    let (id, view) = {
        let mut state = hold(&hub.state);
        let id = state.next;
        state.next += 1;
        let view = view.unwrap_or(id);
        let carry = state
            .handoffs
            .get(&view)
            .map(|handoff| handoff.carry.clone());
        let (reach, reason) = state.link.clone();
        let _ = send.try_send(FromDaemon::Welcome {
            build: hub.build.clone(),
            view,
            carry,
        });
        // The panes before the link, so a window never draws a live link with
        // no panes.
        if reach == Reach::Live {
            let _ = send.try_send(FromDaemon::Panes {
                panes: state.panes.clone(),
            });
        }
        let _ = send.try_send(FromDaemon::Link {
            state: reach,
            reason,
        });
        state.clients.insert(id, send.clone());
        (id, view)
    };
    // The transcript this window asked for last; a read of another stops.
    let asked = Arc::new(AtomicU64::new(0));
    let facts = {
        let asked = Arc::clone(&asked);
        worker(hub, send.clone(), move |hub, send, job| {
            read_facts(hub, send, &asked, job);
        })
    };
    let generation = Arc::new(AtomicU64::new(0));
    let searches = {
        let generation = Arc::clone(&generation);
        worker(
            hub,
            send.clone(),
            move |hub, send, job: (u64, String, Option<String>)| {
                search(hub, send, &generation, job);
            },
        )
    };
    while let Ok(request) = read(&mut lines, &mut line) {
        match request {
            ToDaemon::Hello { .. } => {}
            ToDaemon::Facts { path } => {
                let id = asked.fetch_add(1, Ordering::AcqRel) + 1;
                let _ = facts.send((id, path));
            }
            ToDaemon::Search { id, query, pane } => {
                generation.store(id, Ordering::Release);
                let _ = searches.send((id, query, pane));
            }
            ToDaemon::Drawn => {
                let handoff = hold(&hub.state).handoffs.remove(&view);
                if let (Some(handoff), Some(new)) = (handoff, here.clone()) {
                    let (hub, send) = (Arc::clone(hub), send.clone());
                    thread::spawn(move || {
                        if let Err(error) = actions::take_over(&new, &handoff.target, &handoff.old)
                        {
                            notice(&send, &error);
                        }
                        hold(&hub.state).going.remove(&handoff.from);
                    });
                }
            }
            ToDaemon::GoTo {
                session,
                block,
                carry,
            } => {
                {
                    let mut state = hold(&hub.state);
                    if !state.going.insert(id) {
                        continue;
                    }
                    // The copy may connect before the go-to returns, so the
                    // hand-off it opens with is known before it starts.
                    if let Some(old) = here.clone() {
                        state.handoffs.insert(
                            view,
                            Handoff {
                                carry: actions::handed(carry, &block),
                                old,
                                target: block.clone(),
                                from: id,
                            },
                        );
                    }
                }
                let (hub, send, here) = (Arc::clone(hub), send.clone(), here.clone());
                thread::spawn(move || {
                    let went = actions::go_to(here.as_ref(), &session, &block, view);
                    // A copy that started keeps the hand-off and the go-to
                    // until it takes over; anything else ends both here.
                    if !matches!(went, Ok(actions::Went::Handed { .. })) {
                        let mut state = hold(&hub.state);
                        state.handoffs.remove(&view);
                        state.going.remove(&id);
                    }
                    if let Err(error) = went {
                        notice(&send, &error);
                    }
                });
            }
            ToDaemon::Resume { agent, session } => {
                let (send, here) = (send.clone(), here.clone());
                thread::spawn(move || {
                    let floating = here
                        .as_ref()
                        .is_some_and(|here| actions::floats(here).unwrap_or(false));
                    match here {
                        Some(here) if floating => {
                            if let Err(error) =
                                actions::resume_in_tab(&here, &agent, session.as_deref())
                            {
                                notice(&send, &error);
                            }
                        }
                        _ => {
                            let _ = send.try_send(FromDaemon::Exec { agent });
                        }
                    }
                });
            }
            ToDaemon::Shutdown => shut_down(socket),
        }
    }
    let mut state = hold(&hub.state);
    state.clients.remove(&id);
    state.going.remove(&id);
    if state.clients.is_empty() {
        // The caches serve open windows only.
        hold(&hub.facts).clear();
        *hold(&hub.catalog) = None;
    }
}

fn read(lines: &mut BufReader<UnixStream>, line: &mut String) -> Result<ToDaemon> {
    line.clear();
    if lines.read_line(line).context("read a request")? == 0 {
        bail!("the client left");
    }
    serde_json::from_str(line).context("parse a request")
}

fn write_all(mut writer: UnixStream, queue: &Receiver<FromDaemon>) {
    for message in queue {
        let Ok(mut line) = serde_json::to_vec(&message) else {
            continue;
        };
        line.push(b'\n');
        if writer.write_all(&line).is_err() {
            break;
        }
    }
    // The queue closed: the client was dropped or left. Closing the socket
    // ends its reader too.
    let _ = writer.shutdown(std::net::Shutdown::Both);
}

fn shut_down(socket: &Path) -> ! {
    let _ = fs::remove_file(socket);
    std::process::exit(0);
}

/// A thread that runs `work` on the newest job only: jobs that a newer one
/// replaced before it started are dropped.
fn worker<J: Send + 'static>(
    hub: &Arc<Hub>,
    send: SyncSender<FromDaemon>,
    work: impl Fn(&Hub, &SyncSender<FromDaemon>, J) + Send + 'static,
) -> mpsc::Sender<J> {
    let (jobs, inbox) = mpsc::channel::<J>();
    let hub = Arc::clone(hub);
    thread::spawn(move || {
        while let Ok(mut job) = inbox.recv() {
            while let Ok(newer) = inbox.try_recv() {
                job = newer;
            }
            work(&hub, &send, job);
        }
    });
    jobs
}

/// Reads the transcript at `path` from its newest line back, sending the
/// facts after the first part and then every [`SEND_EVERY`], and stops as
/// soon as the window asks for another transcript. What it read stays, so
/// coming back to the session goes on where it stopped.
fn read_facts(
    hub: &Hub,
    send: &SyncSender<FromDaemon>,
    asked: &AtomicU64,
    (id, path): (u64, PathBuf),
) {
    let stale = || asked.load(Ordering::Acquire) != id;
    let mut reading = hold(&hub.facts).remove(&path).unwrap_or_default();
    if reading.catch_up(&path).is_none() {
        return;
    }
    let mut sent: Option<Instant> = None;
    loop {
        let whole = reading.whole();
        let facts = || FromDaemon::Facts {
            path: path.clone(),
            facts: reading.facts(),
        };
        if whole {
            let _ = send.send(facts());
        } else if reading.begun() && sent.is_none_or(|at| at.elapsed() >= SEND_EVERY) {
            let _ = send.try_send(facts());
            sent = Some(Instant::now());
        }
        if whole || stale() || reading.step_back(&path).is_none() {
            break;
        }
    }
    hold(&hub.facts).insert(path, reading);
}

fn search(
    hub: &Hub,
    send: &SyncSender<FromDaemon>,
    generation: &AtomicU64,
    (id, query, pane): (u64, String, Option<String>),
) {
    let stale = || generation.load(Ordering::Acquire) != id;
    let terms = catalog::terms(&query);
    let catalog = {
        let mut held = hold(&hub.catalog);
        if let Some(catalog) = held.as_ref() {
            Arc::clone(catalog)
        } else {
            let Ok(found) =
                session::claude_dir().and_then(|dir| catalog::load(&dir, &HashSet::new()))
            else {
                return;
            };
            let found = Arc::new(found);
            *held = Some(Arc::clone(&found));
            found
        }
    };
    if terms.is_empty() || stale() {
        let _ = send.try_send(FromDaemon::Hits {
            id,
            hits: Vec::new(),
            read: 0,
            total: 0,
        });
        return;
    }
    let first: Vec<_> = pane
        .and_then(|block| {
            hold(&hub.state)
                .panes
                .iter()
                .find(|known| known.block_id == block)
                .map(|known| {
                    known
                        .sessions
                        .iter()
                        .map(|(agent, _)| agent.clone())
                        .collect()
                })
        })
        .unwrap_or_default();
    let hits = catalog::hits(&first, &catalog, &terms);
    let skip: HashSet<String> = hits
        .iter()
        .map(|hit| hit.agent.session_id.clone())
        .collect();
    let total = catalog.len();
    if send
        .send(FromDaemon::Hits {
            id,
            hits,
            read: 0,
            total,
        })
        .is_err()
    {
        return;
    }
    // A few transcripts at a time, newest first, so matches show as they
    // are found.
    let mut read = 0;
    for chunk in catalog.chunks(4) {
        if stale() {
            return;
        }
        let hits = catalog::deep(chunk, &terms, &skip, stale);
        read += chunk.len();
        if send
            .send(FromDaemon::Hits {
                id,
                hits,
                read,
                total,
            })
            .is_err()
        {
            return;
        }
    }
}

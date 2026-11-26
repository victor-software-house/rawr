//! The daemon's side: the lock, the socket, the Rex feed shared by every
//! client, and one reader, one writer and two workers per client.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{process, thread};

use anyhow::{Context, Result, anyhow, bail};
use rustix::process::setsid;

use super::{Build, FromDaemon, Here, Reach, ToDaemon, actions, folder, socket};
use crate::app::facts::{self, EntryId, List, Reading, Way};
use crate::app::feed::{self, Pane, Update};
use crate::app::state::Carry;
use crate::catalog::{self, Found};
use crate::session::{self, Agent};

/// How many messages wait for one client before the daemon drops it.
const QUEUE: usize = 64;

/// How long the daemon waits before following Rex again after the feed ends.
const RETRY: Duration = Duration::from_secs(2);

/// How long a go-to waits for the copy it started to take over before the
/// daemon gives up on the hand-off.
const HANDOFF_WAIT: Duration = Duration::from_secs(10);

/// How often a transcript being read sends what it has found so far.
const SEND_EVERY: Duration = Duration::from_millis(50);

/// What every client shares.
struct Hub {
    build: Build,
    state: Mutex<State>,
    /// Each transcript a window showed, as far as it is read.
    facts: Mutex<HashMap<PathBuf, Reading>>,
    /// The search catalog, with the modified time of `history.jsonl` it was
    /// loaded at, so a prompt typed since loads it again.
    catalog: Mutex<Option<Catalog>>,
}

/// A search catalog and the modified time of `history.jsonl` it was loaded
/// at, if the file was there.
type Catalog = (Option<SystemTime>, Arc<[Found]>);

struct State {
    /// The open panes and then the closed ones, as windows get them.
    panes: Vec<Pane>,
    /// The open panes, as the feed last sent them.
    open: Vec<Pane>,
    /// The sessions written lately whose pane was closed, newest first.
    closed: Vec<Agent>,
    link: (Reach, Option<String>),
    clients: HashMap<u64, Client>,
    next: u64,
    /// Hand-offs waiting for their new window to draw, by view.
    handoffs: HashMap<u64, Handoff>,
    /// The clients with a go-to under way, which take no other until it
    /// ends, so clicks in a row start one copy, not one each.
    going: HashSet<u64>,
}

/// One connected window: the queue its writer drains, and its socket, which
/// the daemon shuts to make the window reconnect.
struct Client {
    send: SyncSender<FromDaemon>,
    socket: UnixStream,
}

impl State {
    /// A daemon that has seen no feed and no client yet. IDs start at the
    /// wall clock in microseconds, so each daemon run uses its own range.
    /// A window that reconnects after a restart keeps its old view ID, and no
    /// new window takes it. The one exception is a clock that went back
    /// further than the last run counted.
    fn new() -> Self {
        Self {
            panes: Vec::new(),
            open: Vec::new(),
            closed: Vec::new(),
            link: (Reach::Stale, Some("reading Rex".to_owned())),
            clients: HashMap::new(),
            next: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|elapsed| u64::try_from(elapsed.as_micros()).ok())
                .unwrap_or(0),
            handoffs: HashMap::new(),
            going: HashSet::new(),
        }
    }

    /// Takes the client ID of a window that sent its hello, and returns it
    /// with the window's view: the view the hello kept, or the new ID. A kept
    /// view leaves `next` alone, so a hello cannot push the counter to its
    /// end and give every later window one ID.
    fn admit(&mut self, kept: Option<u64>) -> (u64, u64) {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        (id, kept.unwrap_or(id))
    }
}

/// A floating window going to a pane in another tab: the copy the daemon
/// started there opens with `carry`, then takes over from `old`.
struct Handoff {
    carry: Carry,
    old: Here,
    target: String,
    /// The client of the window it replaces.
    from: u64,
    /// Which go-to made it, so a deadline clears only its own hand-off.
    ticket: u64,
}

/// Ends the hand-off of `view` made under `ticket`, and the go-to of client
/// `id` with it, when the hand-off is still waiting. Returns whether it did.
/// A hand-off the new copy took, or a newer go-to made, stays.
fn expire(state: &mut State, view: u64, ticket: u64, id: u64) -> bool {
    if state
        .handoffs
        .get(&view)
        .is_none_or(|handoff| handoff.ticket != ticket)
    {
        return false;
    }
    state.handoffs.remove(&view);
    state.going.remove(&id);
    true
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
    let _ = setsid();
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
        state: Mutex::new(State::new()),
        facts: Mutex::new(HashMap::new()),
        catalog: Mutex::new(None),
    });
    {
        let hub = Arc::clone(&hub);
        thread::spawn(move || follow(&hub));
    }
    {
        // The closed sessions come from the transcripts, read on a thread
        // of their own so the open panes never wait for them.
        let hub = Arc::clone(&hub);
        thread::spawn(move || watch_closed(&hub));
    }
    let mut out = io::stdout();
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
                Update::Panes(open) => {
                    let mut state = hold(&hub.state);
                    state.open = open;
                    let panes = feed::with_closed(&state.open, &state.closed);
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

/// Reads the sessions whose pane was closed every [`feed::CLOSED_EVERY`],
/// and sends the panes again when they changed.
fn watch_closed(hub: &Hub) {
    loop {
        let closed = feed::recent().unwrap_or_default();
        {
            let mut state = hold(&hub.state);
            let same = closed.len() == state.closed.len()
                && closed
                    .iter()
                    .zip(&state.closed)
                    .all(|(new, old)| new.session_id == old.session_id && new.title == old.title);
            if !same {
                state.closed = closed;
                let panes = feed::with_closed(&state.open, &state.closed);
                state.panes.clone_from(&panes);
                if state.link.0 == Reach::Live {
                    broadcast(&mut state, &FromDaemon::Panes { panes });
                }
            }
        }
        thread::sleep(feed::CLOSED_EVERY);
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
///
/// The queue itself never closes while the client's workers and pager hold
/// its sender, so a full queue also shuts the client's socket. That ends the
/// window's reader, and its link reconnects with the same view. On
/// reconnecting, the app requests its facts and runs its search again, so
/// nothing is lost.
fn broadcast(state: &mut State, message: &FromDaemon) {
    state
        .clients
        .retain(|_, client| match client.send.try_send(message.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                let _ = client.socket.shutdown(Shutdown::Both);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        });
}

/// One client: the handshake, then its requests until it goes.
#[expect(
    clippy::too_many_lines,
    reason = "one loop over every request a client sends, with an arm for each"
)]
fn serve(hub: &Arc<Hub>, stream: UnixStream, socket: &Path) {
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let Ok(closer) = stream.try_clone() else {
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
        let (id, view) = state.admit(view);
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
        state.clients.insert(
            id,
            Client {
                send: send.clone(),
                socket: closer,
            },
        );
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
    let pages = pager(send.clone());
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
            ToDaemon::Older {
                path,
                list,
                before,
                count,
            } => {
                let _ = pages.send(Job::Page {
                    path,
                    way: Way::Older,
                    list,
                    at: before,
                    count,
                });
            }
            ToDaemon::Newer {
                path,
                list,
                after,
                count,
            } => {
                let _ = pages.send(Job::Page {
                    path,
                    way: Way::Newer,
                    list,
                    at: after,
                    count,
                });
            }
            ToDaemon::Around {
                path,
                list,
                at,
                count,
                ask,
            } => {
                let _ = pages.send(Job::Around {
                    path,
                    list,
                    at,
                    count,
                    ask,
                });
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
                let ticket = {
                    let mut state = hold(&hub.state);
                    if !state.going.insert(id) {
                        continue;
                    }
                    let ticket = state.next;
                    state.next = state.next.saturating_add(1);
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
                                ticket,
                            },
                        );
                    }
                    ticket
                };
                let (hub, send, here) = (Arc::clone(hub), send.clone(), here.clone());
                thread::spawn(move || {
                    let went = actions::go_to(here.as_ref(), &session, &block, view);
                    // A copy that started keeps the hand-off and the go-to
                    // until it takes over, or the wait ends; anything else
                    // ends both here.
                    let handed = matches!(went, Ok(actions::Went::Handed));
                    if !handed {
                        let mut state = hold(&hub.state);
                        state.handoffs.remove(&view);
                        state.going.remove(&id);
                    }
                    if let Err(error) = went {
                        notice(&send, &error);
                    }
                    if handed {
                        thread::sleep(HANDOFF_WAIT);
                        if expire(&mut hold(&hub.state), view, ticket, id) {
                            notice(&send, &anyhow!("the new window did not open"));
                        }
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
    let _ = writer.shutdown(Shutdown::Both);
}

fn shut_down(socket: &Path) -> ! {
    let _ = fs::remove_file(socket);
    process::exit(0);
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

/// One page a window asked for.
enum Job {
    /// The entries just before or after an entry.
    Page {
        path: PathBuf,
        way: Way,
        list: List,
        at: EntryId,
        count: usize,
    },
    /// The entries nearest a place in the transcript.
    Around {
        path: PathBuf,
        list: List,
        at: u64,
        count: usize,
        ask: u64,
    },
}

/// A thread that answers every page request, in order. It drops none, unlike
/// [`worker`], since a window waits for the answer to each. A transcript that
/// cannot be read is answered with a failure, so no request goes unanswered and
/// none looks like the start of the session.
fn pager(send: SyncSender<FromDaemon>) -> mpsc::Sender<Job> {
    let (jobs, inbox) = mpsc::channel::<Job>();
    thread::spawn(move || {
        for job in inbox {
            let answer = match job {
                Job::Page {
                    path,
                    way,
                    list,
                    at,
                    count,
                } => match facts::page(&path, way, list, at, count) {
                    Ok(page) => FromDaemon::Page {
                        list,
                        way,
                        at,
                        entries: page.entries,
                        more: page.more,
                        failed: None,
                        path,
                    },
                    Err(error) => FromDaemon::Page {
                        failed: Some(unreadable(&path, &error)),
                        list,
                        way,
                        at,
                        entries: Vec::new(),
                        more: true,
                        path,
                    },
                },
                Job::Around {
                    path,
                    list,
                    at,
                    count,
                    ask,
                } => match facts::around(&path, list, at, count) {
                    Ok(page) => FromDaemon::Around {
                        list,
                        ask,
                        entries: page.entries,
                        older: page.older,
                        place: page.place,
                        failed: None,
                        path,
                    },
                    Err(error) => FromDaemon::Around {
                        failed: Some(unreadable(&path, &error)),
                        list,
                        ask,
                        entries: Vec::new(),
                        older: true,
                        place: None,
                        path,
                    },
                },
            };
            if send.send(answer).is_err() {
                break;
            }
        }
    });
    jobs
}

/// What a window is told when a page of the transcript at `path` cannot be
/// read.
fn unreadable(path: &Path, error: &io::Error) -> String {
    format!("could not read the transcript {}: {error}", path.display())
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
    // The task list Claude Code keeps wins over the one the calls give.
    let tasks = facts::stored_tasks(&path);
    let mut sent_at: Option<Instant> = None;
    loop {
        let whole = reading.whole();
        let facts = || FromDaemon::Facts {
            path: path.clone(),
            facts: {
                let mut facts = reading.facts();
                if let Some(tasks) = &tasks {
                    facts.tasks.clone_from(tasks);
                }
                facts
            },
        };
        if whole {
            let _ = send.send(facts());
        } else if reading.begun() && sent_at.is_none_or(|at| at.elapsed() >= SEND_EVERY) {
            let _ = send.try_send(facts());
            sent_at = Some(Instant::now());
        }
        if whole || stale() || reading.step_back(&path).is_none() {
            break;
        }
    }
    hold(&hub.facts).insert(path, reading);
}

/// Whether a catalog loaded when `history.jsonl` was last modified at
/// `stored` still serves, with the file last modified at `now`. A missing
/// file has no time, so a catalog loaded without it serves until it appears.
fn fresh(stored: Option<SystemTime>, now: Option<SystemTime>) -> bool {
    stored == now
}

fn search(
    hub: &Hub,
    send: &SyncSender<FromDaemon>,
    generation: &AtomicU64,
    (id, query, pane): (u64, String, Option<String>),
) {
    let stale = || generation.load(Ordering::Acquire) != id;
    let terms = catalog::terms(&query);
    let Ok(dir) = session::claude_dir() else {
        return;
    };
    let modified = fs::metadata(dir.join("history.jsonl"))
        .and_then(|meta| meta.modified())
        .ok();
    let catalog = {
        let mut held = hold(&hub.catalog);
        match held.as_ref() {
            Some((stored, catalog)) if fresh(*stored, modified) => Arc::clone(catalog),
            Some(_) | None => {
                let Ok(found) = catalog::load(&dir, &HashSet::new()) else {
                    return;
                };
                let found: Arc<[Found]> = Arc::from(found);
                *held = Some((modified, Arc::clone(&found)));
                found
            }
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

#[cfg(test)]
mod tests {
    use std::env;
    use std::io::Read;

    use super::*;
    use crate::app::state::App;

    #[test]
    fn a_full_queue_drops_the_client_and_closes_its_socket() {
        let mut state = State::new();
        let (send, _queue) = mpsc::sync_channel(1);
        send.try_send(FromDaemon::Notice {
            text: "waiting".to_owned(),
        })
        .expect("the queue takes one message");
        let (socket, mut window) = UnixStream::pair().expect("a socket pair");
        // A socket left open fails the read by timeout instead of hanging.
        window
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("a read timeout");
        state.clients.insert(7, Client { send, socket });
        broadcast(
            &mut state,
            &FromDaemon::Notice {
                text: "one too many".to_owned(),
            },
        );
        assert!(state.clients.is_empty(), "the client is dropped");
        // The window's end reads the end of the stream, as its reader does
        // when the daemon shuts the socket.
        let mut buffer = [0_u8; 8];
        assert_eq!(window.read(&mut buffer).expect("the socket reads"), 0);
    }

    #[test]
    fn ids_start_from_the_clock_and_a_kept_view_leaves_them_alone() {
        let mut state = State::new();
        // After 2023-11-14, in microseconds since the epoch.
        assert!(state.next > 1_700_000_000_000_000, "{}", state.next);
        let start = state.next;
        let (id, view) = state.admit(Some(u64::MAX));
        assert_eq!((id, view), (start, u64::MAX));
        // A window with no view takes its own ID, and the next one another.
        let (other, own) = state.admit(None);
        assert_eq!((other, own), (start + 1, start + 1));
        let (third, _) = state.admit(None);
        assert_eq!(third, start + 2);
    }

    #[test]
    fn a_catalog_serves_only_while_the_history_keeps_its_modified_time() {
        let earlier = UNIX_EPOCH + Duration::from_secs(10);
        let later = UNIX_EPOCH + Duration::from_secs(20);
        assert!(fresh(Some(earlier), Some(earlier)));
        assert!(!fresh(Some(earlier), Some(later)));
        // A history that appeared, or went, since the catalog was loaded.
        assert!(!fresh(None, Some(earlier)));
        assert!(!fresh(Some(earlier), None));
        assert!(fresh(None, None));
    }

    fn handoff(ticket: u64) -> Handoff {
        Handoff {
            carry: App::new(None).carry(),
            old: Here {
                session: "session".to_owned(),
                block: "old".to_owned(),
            },
            target: "new".to_owned(),
            from: 3,
            ticket,
        }
    }

    #[test]
    fn a_deadline_clears_only_the_hand_off_it_was_made_for() {
        let mut state = State::new();
        state.handoffs.insert(5, handoff(10));
        state.going.insert(3);
        // A newer go-to's hand-off, and a view with none, stay.
        assert!(!expire(&mut state, 5, 9, 3));
        assert!(!expire(&mut state, 6, 10, 3));
        assert!(state.handoffs.contains_key(&5));
        assert!(state.going.contains(&3));
        // Its own hand-off ends, and the go-to with it.
        assert!(expire(&mut state, 5, 10, 3));
        assert!(state.handoffs.is_empty());
        assert!(state.going.is_empty());
        // A hand-off the new copy took is gone, so nothing is left to end.
        assert!(!expire(&mut state, 5, 10, 3));
    }

    #[test]
    fn a_transcript_that_cannot_be_read_is_answered_with_a_failure() {
        let (send, receive) = mpsc::sync_channel(4);
        let jobs = pager(send);
        let path = env::temp_dir().join(format!("rawr-pager-gone-{}.jsonl", process::id()));
        jobs.send(Job::Page {
            path,
            way: Way::Older,
            list: List::Activity,
            at: EntryId { line: 0, index: 0 },
            count: 100,
        })
        .expect("the pager takes the job");
        let FromDaemon::Page {
            way,
            at,
            entries,
            failed,
            ..
        } = receive.recv().expect("the pager answers")
        else {
            panic!("the answer is a page");
        };
        // The answer names the request it is for.
        assert_eq!((way, at), (Way::Older, EntryId { line: 0, index: 0 }));
        assert_eq!(entries.len(), 0);
        let reason = failed.clone().unwrap_or_default();
        assert!(reason.contains("No such file"), "{reason}");
        assert!(
            failed.is_some_and(|reason| reason.contains("could not read the transcript")),
            "a failure names its reason"
        );
    }
}

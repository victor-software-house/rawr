//! A window's side: connecting to the daemon, starting it when none
//! answers or when it runs another binary, and reconnecting when it goes.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::{Build, FromDaemon, ToDaemon, folder, socket};

/// How long a client waits for a daemon it started to say `ready`.
const READY: Duration = Duration::from_secs(5);

/// The first wait before reconnecting, and the longest.
const BACKOFF: (Duration, Duration) = (Duration::from_millis(100), Duration::from_secs(2));

/// What the link hands the window.
#[derive(Debug)]
pub(crate) enum Incoming {
    /// Connected and welcomed, at the start or again after a loss.
    Connected,
    Message(FromDaemon),
    /// The connection went; the link is reconnecting.
    Lost(String),
}

/// Sends requests to the daemon. A request sent while the link is down is
/// dropped; the window asks again on [`Incoming::Connected`].
#[derive(Debug, Clone)]
pub(crate) struct Link {
    requests: Sender<ToDaemon>,
}

impl Link {
    pub(crate) fn send(&self, request: ToDaemon) {
        let _ = self.requests.send(request);
    }
}

/// Starts the link: one thread keeps the connection, reading into `deliver`
/// and writing what [`Link::send`] queues.
pub(crate) fn start(deliver: impl Fn(Incoming) -> bool + Send + 'static) -> Result<Link> {
    let build = Build::current()?;
    let (requests, outbox) = mpsc::channel::<ToDaemon>();
    thread::spawn(move || keep(&build, &Mutex::new(outbox), &deliver));
    Ok(Link { requests })
}

fn keep(build: &Build, outbox: &Mutex<Receiver<ToDaemon>>, deliver: &dyn Fn(Incoming) -> bool) {
    let mut wait = BACKOFF.0;
    loop {
        let why = match connect(build) {
            Ok(stream) => {
                wait = BACKOFF.0;
                if !deliver(Incoming::Connected) {
                    return;
                }
                match pump(stream, outbox, deliver) {
                    Some(why) => why,
                    None => return,
                }
            }
            Err(error) => format!("{error:#}"),
        };
        if !deliver(Incoming::Lost(why)) {
            return;
        }
        thread::sleep(wait);
        wait = (wait * 2).min(BACKOFF.1);
    }
}

/// Moves messages both ways until the connection ends; `None` when the
/// window is gone.
fn pump(
    stream: UnixStream,
    outbox: &Mutex<Receiver<ToDaemon>>,
    deliver: &dyn Fn(Incoming) -> bool,
) -> Option<String> {
    let Ok(mut writer) = stream.try_clone() else {
        return Some("copy the connection".to_owned());
    };
    // Requests queued while the link was down are stale.
    {
        let held = outbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while held.try_recv().is_ok() {}
    }
    let (done, ended) = mpsc::channel::<()>();
    thread::scope(|scope| {
        scope.spawn(move || {
            let outbox = outbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            loop {
                match outbox.recv_timeout(Duration::from_millis(200)) {
                    Ok(request) => {
                        if write(&mut writer, &request).is_err() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if ended.try_recv().is_ok() {
                    break;
                }
            }
        });
        let mut lines = BufReader::new(stream);
        let mut line = String::new();
        let why = loop {
            line.clear();
            match lines.read_line(&mut line) {
                Ok(0) => break Some("the rawr daemon closed the connection".to_owned()),
                Err(error) => break Some(format!("read from the rawr daemon: {error}")),
                Ok(_) => {}
            }
            let Ok(message) = serde_json::from_str(&line) else {
                continue;
            };
            if !deliver(Incoming::Message(message)) {
                break None;
            }
        };
        let _ = done.send(());
        let _ = lines.get_ref().shutdown(std::net::Shutdown::Both);
        why
    })
}

fn write(stream: &mut UnixStream, request: &ToDaemon) -> Result<()> {
    let mut line = serde_json::to_vec(request).context("write a request")?;
    line.push(b'\n');
    stream.write_all(&line).context("send a request")
}

/// A welcomed connection: to the running daemon, or to one this starts.
fn connect(build: &Build) -> Result<UnixStream> {
    let path = socket()?;
    if let Ok(stream) = UnixStream::connect(&path) {
        match hello(stream, build)? {
            Hello::Welcome(stream) => return Ok(stream),
            Hello::Refused(mut stream) => {
                // Another binary: ask it to stop, and wait for its socket
                // to go quiet.
                write(&mut stream, &ToDaemon::Shutdown)?;
                drop(stream);
                let until = Instant::now() + READY;
                while UnixStream::connect(&path).is_ok() {
                    if Instant::now() > until {
                        bail!("the old rawr daemon did not stop");
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
    let dir = folder()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let lock = File::create(dir.join("start.lock")).context("open start.lock")?;
    lock.lock().context("wait for start.lock")?;
    // Another window may have started one while this one waited.
    if let Ok(stream) = UnixStream::connect(&path)
        && let Hello::Welcome(stream) = hello(stream, build)?
    {
        return Ok(stream);
    }
    spawn()?;
    let stream = UnixStream::connect(&path).context("connect to the rawr daemon")?;
    match hello(stream, build)? {
        Hello::Welcome(stream) => Ok(stream),
        Hello::Refused(_) => bail!("the rawr daemon it started runs another binary"),
    }
}

enum Hello {
    Welcome(UnixStream),
    Refused(UnixStream),
}

fn hello(mut stream: UnixStream, build: &Build) -> Result<Hello> {
    write(
        &mut stream,
        &ToDaemon::Hello {
            build: build.clone(),
        },
    )?;
    stream
        .set_read_timeout(Some(READY))
        .context("time the handshake")?;
    // Byte by byte: the daemon sends the panes right after the welcome, and
    // a buffered reader dropped here would take them with it.
    let mut line = Vec::new();
    let mut byte = [0_u8];
    while byte[0] != b'\n' {
        let read = std::io::Read::read(&mut stream, &mut byte).context("read the welcome")?;
        if read == 0 {
            bail!("the rawr daemon closed the connection");
        }
        line.push(byte[0]);
    }
    stream
        .set_read_timeout(None)
        .context("time the handshake")?;
    match serde_json::from_slice(&line).context("parse the welcome")? {
        FromDaemon::Welcome { .. } => Ok(Hello::Welcome(stream)),
        FromDaemon::Refused { .. } => Ok(Hello::Refused(stream)),
        _ => bail!("the rawr daemon did not answer the hello"),
    }
}

/// Starts `rawr daemon` and waits for its `ready` line.
fn spawn() -> Result<()> {
    let exe = std::env::current_exe().context("find the rawr binary")?;
    let mut child = Command::new(exe)
        .arg("daemon")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("start the rawr daemon")?;
    let out = child.stdout.take().context("read the rawr daemon")?;
    let (said, heard) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(out).read_line(&mut line);
        let _ = said.send(line);
    });
    match heard.recv_timeout(READY) {
        Ok(line) if line.trim() == "ready" => Ok(()),
        Ok(_) => bail!("the rawr daemon stopped before it was ready"),
        Err(_) => bail!("the rawr daemon was not ready within 5 seconds"),
    }
}

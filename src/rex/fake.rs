//! A fake Rex server that replays an exchange written by hand, for tests.
//!
//! A script is JSON lines under `testdata/rex/`, each one a [`Step`]: a message
//! the client must send next, a message the server sends, or a close.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::Value;
use tungstenite::Message;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http::header::SEC_WEBSOCKET_PROTOCOL;
use tungstenite::http::{HeaderValue, StatusCode};
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;

use super::Endpoint;

/// How long the fake server waits for the client before it fails the test.
const PATIENCE: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Step {
    /// The next message the client sends.
    Expect(Value),
    /// A message the server sends.
    Send(Value),
    /// The server closes the connection with this status.
    Close { code: u16, reason: String },
}

/// A server on a Unix socket in a folder of its own, running a script on a
/// thread.
#[derive(Debug)]
pub(crate) struct FakeServer {
    dir: PathBuf,
    pub(crate) endpoint: Endpoint,
    thread: JoinHandle<Result<()>>,
}

impl FakeServer {
    /// Starts a server that plays the lines of `parts`, in order.
    pub(crate) fn start(name: &str, parts: &[&str]) -> Self {
        let steps: Vec<Step> = parts
            .iter()
            .flat_map(|part| part.lines())
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let dir = std::env::temp_dir().join(format!("rawr-rex-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let thread = std::thread::spawn(move || serve(&listener, steps));
        Self {
            dir,
            endpoint: Endpoint::at(socket),
            thread,
        }
    }

    /// Waits for the script to end, and fails if the client broke it. Drop the
    /// client first: the server waits for it to hang up.
    pub(crate) fn finish(self) {
        let outcome = self.thread.join().unwrap();
        fs::remove_dir_all(&self.dir).unwrap();
        outcome.unwrap();
    }
}

/// Accepts only `GET /api/control` that offers `rex.control.v1`, and answers
/// with that subprotocol.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's Callback fixes the error type"
)]
fn accept_control(request: &Request, mut response: Response) -> Result<Response, ErrorResponse> {
    let offered = request
        .headers()
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|name| name.trim() == "rex.control.v1"));
    if request.uri().path() != "/api/control" || !offered {
        let mut refusal = ErrorResponse::new(Some(
            "expected GET /api/control with rex.control.v1".to_owned(),
        ));
        *refusal.status_mut() = StatusCode::BAD_REQUEST;
        return Err(refusal);
    }
    response.headers_mut().insert(
        SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("rex.control.v1"),
    );
    Ok(response)
}

/// Whether an I/O error says the peer is gone.
fn hung_up(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::BrokenPipe | ErrorKind::ConnectionReset)
}

fn serve(listener: &UnixListener, steps: Vec<Step>) -> Result<()> {
    let (stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(PATIENCE))?;
    let mut socket = tungstenite::accept_hdr(stream, accept_control)
        .map_err(|error| anyhow!("the fake server's handshake failed: {error}"))?;
    for step in steps {
        match step {
            Step::Expect(expected) => {
                let message = socket.read()?;
                let Message::Text(text) = message else {
                    bail!("expected a text message from the client, got {message:?}");
                };
                let actual: Value = serde_json::from_str(text.as_str())?;
                ensure!(
                    actual == expected,
                    "the client sent {actual}, the script expected {expected}"
                );
            }
            Step::Send(message) => match socket.send(Message::text(message.to_string())) {
                Ok(()) => {}
                // The client has what it came for and hung up: the exchange is
                // over.
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(());
                }
                Err(tungstenite::Error::Io(error)) if hung_up(error.kind()) => return Ok(()),
                Err(error) => return Err(error.into()),
            },
            Step::Close { code, reason } => socket.close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: reason.into(),
            }))?,
        }
    }
    while socket.read().is_ok() {}
    Ok(())
}

//! The connection: opening it, and calling methods on it.

use std::ffi::OsString;
use std::fmt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde::de::{DeserializeOwned, IgnoredAny};
use serde_json::Value;
use tungstenite::http::Uri;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{ClientRequestBuilder, HandshakeError, Message, WebSocket};

use super::events::EventMessage;
use super::protocol::{
    API_REVISION, AttachAll, Block, BlockArgs, BlockList, Hello, NoArgs, ProgramStatus, Request,
    Response, SUBPROTOCOL, ServerMessage, Session, SessionArgs, SessionList, StatusRecord, Welcome,
};

/// How long opening the connection and each call may wait for the server, the
/// default of the `rex` command.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Where the Rex server listens: a Unix socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    socket: PathBuf,
}

impl Endpoint {
    pub(super) fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    /// The server a pane's `REX_SERVER` names, else the default socket.
    pub(crate) fn from_environment() -> Result<Self> {
        match std::env::var_os("REX_SERVER") {
            Some(value) if !value.is_empty() => {
                Self::parse(value.to_str().context("REX_SERVER is not text")?)
            }
            _ => Self::default_socket(),
        }
    }

    /// Reads a `unix://` endpoint, whose path is percent-encoded, such as
    /// `unix:///home/dev/rex%20data/server.sock`.
    pub(super) fn parse(value: &str) -> Result<Self> {
        let path = value
            .strip_prefix("unix://")
            .with_context(|| format!("{value} is not a unix:// endpoint"))?;
        Ok(Self::at(percent_decode(path)?))
    }

    /// Rex.app's socket on macOS. Elsewhere the server has no documented
    /// default, so `REX_SERVER` is required.
    fn default_socket() -> Result<Self> {
        if !cfg!(target_os = "macos") {
            bail!("REX_SERVER is not set, and the Rex socket has no default on this system");
        }
        let home = std::env::home_dir().context("no home directory")?;
        Ok(Self::at(
            home.join("Library/Application Support/rex/server.sock"),
        ))
    }
}

/// `text` with each `%XX` escape replaced by its byte, as a path.
fn percent_decode(text: &str) -> Result<PathBuf> {
    let raw = text.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut at = 0;
    while let Some(&byte) = raw.get(at) {
        if byte == b'%' {
            let digits = text
                .get(at + 1..at + 3)
                .filter(|digits| digits.bytes().all(|digit| digit.is_ascii_hexdigit()))
                .with_context(|| format!("a broken %-escape in {text}"))?;
            bytes.push(u8::from_str_radix(digits, 16)?);
            at += 3;
        } else {
            bytes.push(byte);
            at += 1;
        }
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

/// A request the server answered with an error.
#[derive(Debug)]
pub(crate) struct ErrorResponse {
    method: String,
    code: String,
    message: String,
    status: u16,
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the Rex server refused {}: {} ({}), {}",
            self.method, self.code, self.status, self.message
        )
    }
}

impl std::error::Error for ErrorResponse {}

/// The server closed the connection.
#[derive(Debug)]
pub(super) struct Closed {
    code: Option<u16>,
    reason: String,
}

impl fmt::Display for Closed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "the Rex server closed the connection")?;
        if let Some(code) = self.code {
            write!(formatter, " with status {code}")?;
        }
        if !self.reason.is_empty() {
            write!(formatter, ": {}", self.reason)?;
        }
        Ok(())
    }
}

impl std::error::Error for Closed {}

/// The next message the server sends. Pings are answered by the socket library,
/// and a message that is not text is skipped, as the protocol sends only text.
pub(super) fn read_message(socket: &mut WebSocket<UnixStream>) -> Result<ServerMessage> {
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                return serde_json::from_str(text.as_str())
                    .context("read a message from the Rex server");
            }
            Ok(Message::Close(frame)) => {
                let code = frame.as_ref().map(|frame| u16::from(frame.code));
                let reason = frame
                    .map(|frame| frame.reason.as_str().to_owned())
                    .unwrap_or_default();
                return Err(Closed { code, reason }.into());
            }
            Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Err(Closed {
                    code: None,
                    reason: String::new(),
                }
                .into());
            }
            Err(error) => return Err(error).context("read from the Rex server"),
        }
    }
}

/// A control connection to the Rex server, attached to every session so that
/// block calls work.
#[derive(Debug)]
pub(crate) struct Client {
    pub(super) socket: WebSocket<UnixStream>,
    next_id: u64,
}

impl Client {
    /// Opens the control connection, sends the hello, checks the welcome, and
    /// attaches to every session.
    ///
    /// # Errors
    ///
    /// Fails when the socket does not answer, when the server speaks no
    /// revision rawr speaks, and when any message in between is refused.
    pub(crate) fn connect(endpoint: &Endpoint) -> Result<Self> {
        let stream = UnixStream::connect(&endpoint.socket).with_context(|| {
            format!("connect to the Rex server at {}", endpoint.socket.display())
        })?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .context("set the read timeout")?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .context("set the write timeout")?;
        let uri: Uri = "ws://localhost/api/control"
            .parse()
            .context("build the control connection address")?;
        let request = ClientRequestBuilder::new(uri).with_sub_protocol(SUBPROTOCOL);
        let (socket, _) = tungstenite::client(request, stream)
            .map_err(|error| match error {
                HandshakeError::Failure(failure) => anyhow::Error::new(failure),
                HandshakeError::Interrupted(_) => anyhow!("the WebSocket handshake did not finish"),
            })
            .context("open the control connection")?;
        let mut client = Self { socket, next_id: 1 };
        client.greet()?;
        client.call::<_, IgnoredAny>(
            "session.attach_all",
            &AttachAll {
                event_scope: "session",
            },
        )?;
        Ok(client)
    }

    /// Sends the hello and checks that the welcome names a revision rawr
    /// speaks.
    fn greet(&mut self) -> Result<()> {
        let hello = serde_json::to_string(&Hello::new()).context("encode the hello")?;
        self.socket
            .send(Message::text(hello))
            .context("send the hello")?;
        match read_message(&mut self.socket) {
            Ok(ServerMessage::Welcome(welcome)) => check_welcome(&welcome),
            Ok(ServerMessage::Response(_) | ServerMessage::Event(_) | ServerMessage::Unknown) => {
                bail!("the Rex server did not answer the hello with a welcome")
            }
            Err(error) => match error.downcast_ref::<Closed>() {
                Some(closed) if closed.code == Some(u16::from(CloseCode::Policy)) => bail!(
                    "rawr speaks Rex API revision {API_REVISION}, and the Rex server refused it: {}",
                    closed.reason
                ),
                Some(_) | None => Err(error),
            },
        }
    }

    /// Sends a request and waits for its response, handing each event that
    /// arrives meanwhile to `on_event`. Returns the result and the response's
    /// `seq`.
    pub(super) fn request_with(
        &mut self,
        method: &str,
        payload: Option<Value>,
        mut on_event: impl FnMut(EventMessage),
    ) -> Result<(Value, u64)> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let message = serde_json::to_string(&Request::new(&id, method, payload))
            .with_context(|| format!("encode {method}"))?;
        self.socket
            .send(Message::text(message))
            .with_context(|| format!("send {method}"))?;
        loop {
            match read_message(&mut self.socket).with_context(|| format!("wait for {method}"))? {
                ServerMessage::Response(response) if response.id == id => {
                    return finish(response, method);
                }
                ServerMessage::Event(event) => on_event(event),
                ServerMessage::Welcome(_) | ServerMessage::Response(_) | ServerMessage::Unknown => {
                }
            }
        }
    }

    /// Calls a method that takes `payload`, ignoring any event that arrives
    /// before the response.
    pub(super) fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        payload: &P,
    ) -> Result<R> {
        let payload =
            serde_json::to_value(payload).with_context(|| format!("encode {method} arguments"))?;
        let (value, _) = self.request_with(method, Some(payload), drop)?;
        serde_json::from_value(value).with_context(|| format!("read the {method} result"))
    }

    /// Calls a method that takes no arguments.
    fn call_bare<R: DeserializeOwned>(&mut self, method: &str) -> Result<R> {
        let (value, _) = self.request_with(method, None, drop)?;
        serde_json::from_value(value).with_context(|| format!("read the {method} result"))
    }

    /// Every session on the server, in the server's order.
    ///
    /// # Errors
    ///
    /// Fails when the call is refused or answered with something else.
    pub(crate) fn sessions(&mut self) -> Result<Vec<Session>> {
        let list: SessionList = self.call_bare("session.list")?;
        Ok(list.sessions.unwrap_or_default())
    }

    /// Every block in `session`, detached ones included, sorted by ID.
    ///
    /// # Errors
    ///
    /// Fails when the call is refused, and when any block has no ID, so a
    /// partial listing is never returned as a whole one.
    pub(crate) fn blocks(&mut self, session: &Session) -> Result<Vec<Block>> {
        let list: BlockList = self.call(
            "session.list_blocks",
            &SessionArgs {
                session_id: &session.id,
            },
        )?;
        list.blocks
            .unwrap_or_default()
            .into_iter()
            .map(Block::try_from)
            .collect()
    }

    /// The program status records of a terminal block, the root record first.
    /// `None` when the block closed since it was listed.
    ///
    /// # Errors
    ///
    /// Fails when the call is refused for any reason but a missing block.
    pub(crate) fn program_status(
        &mut self,
        session: &Session,
        block: &Block,
    ) -> Result<Option<Vec<StatusRecord>>> {
        let result = self.call::<_, ProgramStatus>(
            "com.superlogical.terminal.program_status",
            &BlockArgs {
                session_id: &session.id,
                block_id: &block.id,
                args: NoArgs {},
            },
        );
        match result {
            Ok(status) => Ok(Some(status.records.unwrap_or_default())),
            Err(error)
                if error
                    .downcast_ref::<ErrorResponse>()
                    .is_some_and(|refused| refused.code == "not_found") =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Stops the Rex server, ending every program in every pane, and drops the
    /// connection, which the server closes as it stops.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses: it takes the call only over its own
    /// socket.
    #[expect(
        dead_code,
        reason = "the supervisor restart in a later slice stops the server"
    )]
    pub(crate) fn stop_server(mut self) -> Result<()> {
        self.call::<_, IgnoredAny>("server.stop", &NoArgs {})?;
        Ok(())
    }
}

/// Checks that the welcome speaks the control protocol at the revision rawr
/// was written for.
fn check_welcome(welcome: &Welcome) -> Result<()> {
    if welcome.protocol != SUBPROTOCOL {
        bail!(
            "the Rex server answered with {}, not {SUBPROTOCOL}",
            welcome.protocol
        );
    }
    if welcome.api_revision != API_REVISION {
        bail!(
            "rawr speaks Rex API revision {API_REVISION}, and this Rex server speaks revisions {} to {}",
            welcome.api_min_revision,
            welcome.api_server_revision
        );
    }
    Ok(())
}

/// The result of a response, or its error.
fn finish(response: Response, method: &str) -> Result<(Value, u64)> {
    if let Some(error) = response.error {
        return Err(ErrorResponse {
            method: method.to_owned(),
            code: error.code,
            message: error.message,
            status: error.status,
        }
        .into());
    }
    Ok((response.payload.unwrap_or(Value::Null), response.seq))
}

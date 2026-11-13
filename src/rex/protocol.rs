//! The messages of the control protocol, and the values rawr reads from them.
//!
//! A type ignores the fields it does not name, as the protocol asks of a
//! client: a newer server adds fields.

use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::events::EventMessage;

/// The API revision rawr speaks, which the Rex server's `/llms.txt` describes.
/// The hello names it as both the newest and the oldest revision, so a server
/// outside it refuses the connection.
pub(super) const API_REVISION: u32 = 13;

/// The websocket subprotocol of the control connection.
pub(super) const SUBPROTOCOL: &str = "rex.control.v1";

/// The creator of a terminal block.
pub(super) const TERMINAL: &str = "com.superlogical.terminal";

/// The first message a client sends.
#[derive(Debug, Serialize)]
pub(super) struct Hello {
    #[serde(rename = "type")]
    kind: &'static str,
    api_revision: u32,
    api_min_revision: u32,
    client: ClientInfo,
}

#[derive(Debug, Serialize)]
struct ClientInfo {
    kind: &'static str,
    name: &'static str,
}

impl Hello {
    pub(super) const fn new() -> Self {
        Self {
            kind: "hello",
            api_revision: API_REVISION,
            api_min_revision: API_REVISION,
            client: ClientInfo {
                kind: "cli",
                name: "rawr",
            },
        }
    }
}

/// A method call. A method that takes no arguments sends no payload.
#[derive(Debug, Serialize)]
pub(super) struct Request<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    id: &'a str,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Value>,
}

impl<'a> Request<'a> {
    pub(super) fn new(id: &'a str, method: &'a str, payload: Option<Value>) -> Self {
        Self {
            kind: "request",
            id,
            method,
            payload,
        }
    }
}

/// A message from the server. A type this client does not know is
/// [`Self::Unknown`]: a newer server may add one.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ServerMessage {
    Welcome(Welcome),
    Response(Response),
    Event(EventMessage),
    #[serde(other)]
    Unknown,
}

/// The server's answer to the hello.
#[derive(Debug, Deserialize)]
pub(super) struct Welcome {
    pub(super) protocol: String,
    /// The revision every later message uses; 0 when left out.
    #[serde(default)]
    pub(super) api_revision: u32,
    #[serde(default)]
    pub(super) api_server_revision: u32,
    #[serde(default)]
    pub(super) api_min_revision: u32,
}

/// The answer to one request.
#[derive(Debug, Deserialize)]
pub(super) struct Response {
    pub(super) id: String,
    /// The `seq` of the last event queued for the connection before the
    /// server started the request.
    #[serde(default)]
    pub(super) seq: u64,
    pub(super) payload: Option<Value>,
    pub(super) error: Option<ErrorBody>,
}

/// Why a request failed.
#[derive(Debug, Deserialize)]
pub(super) struct ErrorBody {
    pub(super) code: String,
    #[serde(default)]
    pub(super) message: String,
    #[serde(default)]
    pub(super) status: u16,
}

/// A request with no arguments of its own, which the server reads as `{}`.
#[derive(Debug, Serialize)]
pub(super) struct NoArgs {}

/// The arguments of a method that names a session.
#[derive(Debug, Serialize)]
pub(super) struct SessionArgs<'a> {
    pub(super) session_id: &'a str,
}

/// The arguments of a block method, which holds the method's own under
/// `args`.
#[derive(Debug, Serialize)]
pub(super) struct BlockArgs<'a> {
    pub(super) session_id: &'a str,
    pub(super) block_id: &'a str,
    pub(super) args: NoArgs,
}

/// The arguments of `session.attach_all`.
#[derive(Debug, Serialize)]
pub(super) struct AttachAll {
    /// `session` or `all`.
    pub(super) event_scope: &'static str,
}

/// A Rex session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Session {
    #[serde(rename = "session_id")]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
}

/// The result of `session.list`; the server sends `null` for no sessions.
#[derive(Debug, Deserialize)]
pub(super) struct SessionList {
    pub(super) sessions: Option<Vec<Session>>,
}

/// A block in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) id: String,
    pub(crate) label: String,
    /// The creator that made the block, such as `com.superlogical.terminal`.
    pub(crate) creator: Option<String>,
}

impl Block {
    /// Whether the block is a terminal, the only kind that reports a program
    /// status.
    pub(crate) fn is_terminal(&self) -> bool {
        self.creator.as_deref() == Some(TERMINAL)
    }
}

/// A block as `session.list_blocks` reports it.
#[derive(Debug, Deserialize)]
pub(super) struct BlockRow {
    block_id: Option<String>,
    #[serde(default)]
    label: String,
    creator_name: Option<String>,
}

impl TryFrom<BlockRow> for Block {
    type Error = Error;

    /// Fails on a block with no ID, so a partial listing never counts as a
    /// whole one.
    fn try_from(row: BlockRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row
                .block_id
                .context("the Rex server listed a block with no id")?,
            label: row.label,
            creator: row.creator_name,
        })
    }
}

/// The result of `session.list_blocks`.
#[derive(Debug, Deserialize)]
pub(super) struct BlockList {
    pub(super) blocks: Option<Vec<BlockRow>>,
}

/// What a program in a terminal says it is doing.
///
/// Rex tells a client to treat a state it does not know as `idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub(crate) enum State {
    Idle,
    Working,
    Done,
    /// Waiting for the user.
    Blocked,
    Error,
}

impl From<String> for State {
    fn from(state: String) -> Self {
        match state.as_str() {
            "working" => Self::Working,
            "done" => Self::Done,
            "blocked" => Self::Blocked,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }
}

/// One record of a terminal's program status: what a program reported with
/// OSC 7501.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct StatusRecord {
    /// A slash separates a parent from a child; empty for the root record,
    /// which holds the reports that give no ID.
    #[serde(default)]
    pub(crate) id: String,
    pub(crate) state: State,
    pub(crate) app: Option<String>,
    /// The program the record belongs to: `app`, else the nearest parent's.
    pub(crate) effective_app: Option<String>,
    /// What a `blocked` record waits for; absent on other states.
    #[serde(default)]
    pub(crate) kind: Option<Kind>,
    /// How far a `working` or `blocked` record is, 0 to 100; absent when the
    /// program gives no percentage.
    #[serde(default)]
    pub(crate) progress: Option<u8>,
    /// The one line the program gave: what it is doing or waiting for.
    #[serde(default)]
    pub(crate) msg: Option<String>,
}

/// What a `blocked` program waits for. The protocol says an unknown kind is
/// treated as absent, so it reads as [`Kind::Unspecified`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub(crate) enum Kind {
    /// Approval to do something.
    Permission,
    /// An answer the user must type.
    Question,
    /// A login, token or credential.
    Auth,
    Unspecified,
}

impl From<String> for Kind {
    fn from(kind: String) -> Self {
        match kind.as_str() {
            "permission" => Self::Permission,
            "question" => Self::Question,
            "auth" => Self::Auth,
            _ => Self::Unspecified,
        }
    }
}

/// The result of `com.superlogical.terminal.program_status`.
#[derive(Debug, Deserialize)]
pub(super) struct ProgramStatus {
    pub(super) records: Option<Vec<StatusRecord>>,
}

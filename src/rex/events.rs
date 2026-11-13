//! The event stream: block and program status events from every session.

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tungstenite::WebSocket;

use super::client::{Client, read_message};
use super::protocol::{AttachAll, ServerMessage, Session, StatusRecord, TERMINAL};

/// An event rawr follows. Every other event, and every field of these that
/// rawr does not read, is skipped: the protocol tells a client to skip what it
/// does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    BlockCreated {
        session_id: String,
        block_id: String,
    },
    BlockClosed {
        session_id: String,
        block_id: String,
    },
    /// A program in the terminal reported a status; the record replaces the
    /// block's record of the same ID.
    ProgramStatusChanged {
        session_id: String,
        block_id: String,
        record: StatusRecord,
    },
    /// Records were removed from the terminal; the root record is the empty
    /// ID.
    ProgramStatusRemoved {
        session_id: String,
        block_id: String,
        ids: Vec<String>,
    },
}

/// An `event` message as the server sends it.
#[derive(Debug, Deserialize)]
pub(super) struct EventMessage {
    /// The event's place among the events sent on the connection.
    seq: u64,
    payload: Named,
}

/// A value the protocol names by `type`, with its fields in `payload`.
#[derive(Debug, Deserialize)]
struct Named {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: Value,
}

/// A session event: which session it came from, and the event itself.
#[derive(Debug, Deserialize)]
struct SessionEvent {
    session_id: String,
    event: Named,
}

#[derive(Debug, Deserialize)]
struct BlockRef {
    block_id: String,
}

#[derive(Debug, Deserialize)]
struct BlockEvent {
    block_id: String,
    creator_name: Option<String>,
    name: String,
    #[serde(default)]
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct StatusChanged {
    /// Always present, as the empty string for the root record.
    #[serde(default)]
    id: String,
    record: StatusRecord,
}

#[derive(Debug, Deserialize)]
struct StatusRemoved {
    #[serde(default)]
    ids: Vec<String>,
}

fn decode<T: DeserializeOwned>(value: Value, what: &str) -> Result<T> {
    serde_json::from_value(value).with_context(|| format!("read a {what} event"))
}

impl EventMessage {
    /// The typed event, or `None` for an event rawr does not follow.
    fn into_event(self) -> Result<Option<Event>> {
        if self.payload.kind != "session_event" {
            return Ok(None);
        }
        let SessionEvent { session_id, event } = decode(self.payload.payload, "session")?;
        match event.kind.as_str() {
            "block_created" => {
                let BlockRef { block_id } = decode(event.payload, "block_created")?;
                Ok(Some(Event::BlockCreated {
                    session_id,
                    block_id,
                }))
            }
            "block_closed" => {
                let BlockRef { block_id } = decode(event.payload, "block_closed")?;
                Ok(Some(Event::BlockClosed {
                    session_id,
                    block_id,
                }))
            }
            "block_event" => {
                decode::<BlockEvent>(event.payload, "block_event")?.into_event(session_id)
            }
            _ => Ok(None),
        }
    }
}

impl BlockEvent {
    fn into_event(self, session_id: String) -> Result<Option<Event>> {
        if self
            .creator_name
            .as_deref()
            .is_some_and(|creator| creator != TERMINAL)
        {
            return Ok(None);
        }
        let block_id = self.block_id;
        match self.name.as_str() {
            "program_status_changed" => {
                let changed: StatusChanged = decode(self.payload, "program_status_changed")?;
                Ok(Some(Event::ProgramStatusChanged {
                    session_id,
                    block_id,
                    record: StatusRecord {
                        id: changed.id,
                        ..changed.record
                    },
                }))
            }
            "program_status_removed" => {
                let removed: StatusRemoved = decode(self.payload, "program_status_removed")?;
                Ok(Some(Event::ProgramStatusRemoved {
                    session_id,
                    block_id,
                    ids: removed.ids,
                }))
            }
            _ => Ok(None),
        }
    }
}

/// Every session at the moment events started, and the events from then on.
#[derive(Debug)]
pub(crate) struct Watch {
    pub(crate) sessions: Vec<Session>,
    pub(crate) events: Events,
}

/// The events of every session, as they arrive. A read blocks until the
/// next one.
///
/// The iterator ends after the first error: the server closed the connection,
/// or sent something rawr cannot read. A caller that wants to go on opens a
/// new connection and lists the sessions again.
#[derive(Debug)]
pub(crate) struct Events {
    socket: WebSocket<UnixStream>,
    /// Events that arrived before the attach response, newer than it.
    backlog: VecDeque<EventMessage>,
    /// The `seq` of the attach response: an event at or below it is already in
    /// the snapshot.
    after: u64,
    ended: bool,
}

impl Client {
    /// Starts receiving every event from every session, including block
    /// events, and returns the sessions as they were then.
    ///
    /// The server's snapshot already holds what happened up to the attach
    /// response, so the events start after it.
    ///
    /// # Errors
    ///
    /// Fails when the server refuses to attach this connection.
    pub(crate) fn watch(mut self) -> Result<Watch> {
        let payload = serde_json::to_value(AttachAll { event_scope: "all" })
            .context("encode session.attach_all arguments")?;
        let mut arrived = Vec::new();
        let (value, seq) = self.request_with("session.attach_all", Some(payload), |event| {
            arrived.push(event);
        })?;
        let snapshot: Snapshot =
            serde_json::from_value(value).context("read the session.attach_all result")?;
        self.socket
            .get_ref()
            .set_read_timeout(None)
            .context("wait for events without a time limit")?;
        arrived.retain(|event| event.seq > seq);
        Ok(Watch {
            sessions: snapshot.sessions.unwrap_or_default(),
            events: Events {
                socket: self.socket,
                backlog: arrived.into(),
                after: seq,
                ended: false,
            },
        })
    }
}

/// The result of `session.attach_all`.
#[derive(Debug, Deserialize)]
struct Snapshot {
    sessions: Option<Vec<Session>>,
}

impl Events {
    /// The next message the server sends that is newer than the snapshot.
    fn next_message(&mut self) -> Result<EventMessage> {
        if let Some(event) = self.backlog.pop_front() {
            return Ok(event);
        }
        loop {
            match read_message(&mut self.socket)? {
                ServerMessage::Event(event) if event.seq > self.after => return Ok(event),
                ServerMessage::Event(_)
                | ServerMessage::Welcome(_)
                | ServerMessage::Response(_)
                | ServerMessage::Unknown => {}
            }
        }
    }
}

impl Iterator for Events {
    type Item = Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.ended {
            let message = match self.next_message() {
                Ok(message) => message,
                Err(error) => {
                    self.ended = true;
                    return Some(Err(error));
                }
            };
            match message.into_event() {
                Ok(Some(event)) => return Some(Ok(event)),
                Ok(None) => {}
                Err(error) => {
                    self.ended = true;
                    return Some(Err(error));
                }
            }
        }
        None
    }
}

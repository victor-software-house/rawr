//! A client for the Rex server's control protocol.
//!
//! The Rex server documents the protocol itself: `GET /llms.txt` on its socket
//! describes it, and `GET /api/help` lists every method with the JSON Schema of
//! its arguments and result. The control connection is a websocket at
//! `/api/control` with the subprotocol `rex.control.v1`, and every message is
//! one JSON object. rawr speaks API revision 13 only.
//!
//! Nothing outside this module sees JSON or websocket types.

mod client;
mod control;
#[cfg(test)]
pub(crate) mod fake;
// Block and session events are read by the daemon, which a later slice adds;
// this slice's verbs only list and read.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the supervisor daemon follows events in a later slice"
    )
)]
pub(crate) mod events;
mod protocol;
#[cfg(test)]
mod tests;

pub(crate) use client::{Client, Endpoint, session_is_gone};
pub(crate) use protocol::{Block, Kind, Session, State, StatusRecord};

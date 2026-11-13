//! The client against a fake server replaying the exchanges under
//! `testdata/rex/`, which are written by hand from the protocol's
//! documentation.

use std::path::PathBuf;

use anyhow::Result;

use super::events::Event;
use super::fake::{FakeServer, after_hang_up};
use super::{Block, Client, Endpoint, Session, State, StatusRecord};

const HANDSHAKE: &str = include_str!("../../testdata/rex/handshake.jsonl");
const LISTING: &str = include_str!("../../testdata/rex/listing.jsonl");
const PROGRAM_STATUS: &str = include_str!("../../testdata/rex/program-status.jsonl");
const REFUSED: &str = include_str!("../../testdata/rex/refused.jsonl");
const EVENTS: &str = include_str!("../../testdata/rex/events.jsonl");
const EVENTS_MALFORMED: &str = include_str!("../../testdata/rex/events-malformed.jsonl");
const HANGUP_SENDS: &str = include_str!("../../testdata/rex/hangup-sends.jsonl");
const HANGUP_EXPECT: &str = include_str!("../../testdata/rex/hangup-expect.jsonl");
const REVISION_WELCOME: &str = include_str!("../../testdata/rex/revision-welcome.jsonl");
const REVISION_REFUSED: &str = include_str!("../../testdata/rex/revision-refused.jsonl");

const WORK: &str = "session:0190a000-0000-7000-8000-00000000000a";
const SHELL: &str = "block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08";

fn session(id: &str, label: &str) -> Session {
    Session {
        id: id.to_owned(),
        label: label.to_owned(),
    }
}

fn terminal(id: &str, label: &str) -> Block {
    Block {
        id: id.to_owned(),
        label: label.to_owned(),
        creator: Some("com.superlogical.terminal".to_owned()),
    }
}

fn claude(id: &str, state: State, app: Option<&str>) -> StatusRecord {
    StatusRecord {
        id: id.to_owned(),
        state,
        app: app.map(str::to_owned),
        effective_app: Some("claude-code".to_owned()),
    }
}

/// A client connected to a fake server that plays `parts`.
fn open(name: &str, parts: &[&str]) -> (FakeServer, Client) {
    let server = FakeServer::start(name, parts);
    let client = Client::connect(&server.endpoint).unwrap();
    (server, client)
}

#[test]
fn an_endpoint_is_a_percent_decoded_unix_path() {
    let endpoint =
        Endpoint::parse("unix:///home/dev/Library/Application%20Support/rex/server.sock").unwrap();
    assert_eq!(
        endpoint,
        Endpoint::at(PathBuf::from(
            "/home/dev/Library/Application Support/rex/server.sock"
        ))
    );
    assert!(Endpoint::parse("tcp://127.0.0.1:7000").is_err());
    assert!(Endpoint::parse("unix:///rex%2").is_err());
    assert!(Endpoint::parse("unix:///rex%zz").is_err());
}

#[test]
fn a_hello_and_a_welcome_open_the_connection() {
    let (server, client) = open("hello", &[HANDSHAKE]);
    drop(client);
    server.finish();
}

#[test]
fn a_welcome_outside_revision_13_names_both_revisions() {
    let server = FakeServer::start("revision-welcome", &[REVISION_WELCOME]);
    let error = Client::connect(&server.endpoint).unwrap_err();
    assert_eq!(
        error.to_string(),
        "rawr speaks Rex API revision 13, and this Rex server speaks revisions 0 to 12"
    );
    server.finish();
}

#[test]
fn a_server_that_closes_on_the_hello_is_named_with_its_reason() {
    let server = FakeServer::start("revision-refused", &[REVISION_REFUSED]);
    let error = Client::connect(&server.endpoint).unwrap_err();
    assert_eq!(
        error.to_string(),
        "rawr speaks Rex API revision 13, and the Rex server refused it: \
         no common API revision: the server speaks 0 to 12, the client speaks 13 to 13"
    );
    server.finish();
}

#[test]
fn sessions_and_blocks_are_listed() {
    let (server, mut client) = open("listing", &[HANDSHAKE, LISTING]);
    let sessions = client.sessions().unwrap();
    assert_eq!(
        sessions,
        [
            session(WORK, "work"),
            session("session:0190a000-0000-7000-8000-00000000000b", "notes")
        ]
    );

    let blocks = client.blocks(&sessions[0]).unwrap();
    assert_eq!(
        blocks,
        [
            terminal(SHELL, "shell"),
            terminal("block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f09", "")
        ]
    );
    assert!(blocks.iter().all(Block::is_terminal));

    let blocks = client.blocks(&sessions[1]).unwrap();
    assert_eq!(blocks.len(), 1);
    assert!(!blocks[0].is_terminal());
    drop(client);
    server.finish();
}

#[test]
fn program_status_reads_the_records_and_a_closed_block_or_session_has_none() {
    let (server, mut client) = open("program-status", &[HANDSHAKE, PROGRAM_STATUS]);
    let work = session(WORK, "work");

    let records = client
        .program_status(&work, &terminal(SHELL, "shell"))
        .unwrap();
    let unknown = StatusRecord {
        id: "agent-2".to_owned(),
        state: State::Idle,
        app: None,
        effective_app: None,
    };
    assert_eq!(
        records,
        Some(vec![
            claude("", State::Blocked, Some("claude-code")),
            claude("agent-1", State::Working, None),
            unknown,
        ])
    );

    let silent = terminal("block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f09", "");
    assert_eq!(client.program_status(&work, &silent).unwrap(), Some(vec![]));

    let closed = terminal("block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f0f", "");
    assert_eq!(client.program_status(&work, &closed).unwrap(), None);

    let gone = session("session:0190a000-0000-7000-8000-0000000000ff", "gone");
    assert_eq!(
        client
            .program_status(&gone, &terminal(SHELL, "shell"))
            .unwrap(),
        None
    );
    drop(client);
    server.finish();
}

#[test]
fn a_refused_call_names_its_code_and_leaves_the_connection_usable() {
    let (server, mut client) = open("refused", &[HANDSHAKE, REFUSED]);
    let gone = session("session:0190a000-0000-7000-8000-0000000000ff", "gone");
    let error = client.blocks(&gone).unwrap_err();
    assert_eq!(
        error.to_string(),
        "the Rex server refused session.list_blocks: session_not_found (404), session not found"
    );
    assert_eq!(client.sessions().unwrap(), Vec::<Session>::new());
    drop(client);
    server.finish();
}

#[test]
fn events_skip_what_rawr_does_not_follow() {
    let (server, client) = open("events", &[HANDSHAKE, EVENTS]);
    let watch = client.watch().unwrap();
    assert_eq!(watch.sessions, [session(WORK, "work")]);

    let events: Vec<Event> = watch.events.take(5).collect::<Result<_>>().unwrap();
    let session_id = WORK.to_owned();
    let block_id = SHELL.to_owned();
    assert_eq!(
        events,
        [
            Event::BlockCreated {
                session_id: session_id.clone(),
                block_id: block_id.clone(),
            },
            Event::ProgramStatusChanged {
                session_id: session_id.clone(),
                block_id: block_id.clone(),
                record: claude("", State::Working, Some("claude-code")),
            },
            Event::ProgramStatusChanged {
                session_id: session_id.clone(),
                block_id: block_id.clone(),
                record: claude("agent-1", State::Done, None),
            },
            Event::ProgramStatusRemoved {
                session_id: session_id.clone(),
                block_id: block_id.clone(),
                ids: vec!["agent-1".to_owned(), String::new()],
            },
            Event::BlockClosed {
                session_id,
                block_id,
            },
        ]
    );
    server.finish();
}

#[test]
fn the_events_end_after_the_first_error() {
    let (server, client) = open("events-malformed", &[HANDSHAKE, EVENTS_MALFORMED]);
    let mut events = client.watch().unwrap().events;
    let error = events.next().unwrap().unwrap_err();
    assert_eq!(error.to_string(), "read a block_created event");
    assert!(events.next().is_none());
    drop(events);
    server.finish();
}

#[test]
fn a_client_that_hangs_up_with_sends_left_ends_the_exchange_cleanly() {
    after_hang_up(&[HANGUP_SENDS]).unwrap();
}

#[test]
fn a_client_that_hangs_up_with_an_expect_due_fails_the_exchange() {
    let error = after_hang_up(&[HANGUP_EXPECT]).unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"the client hung up with 1 expected message(s) unplayed: {"type":"request","id":"3","method":"session.list"}"#
    );
}

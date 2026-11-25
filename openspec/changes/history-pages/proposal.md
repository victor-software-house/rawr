# Proposal

## Why

The Conversation and Activity tabs hold only the newest 300 entries, and the list stops dead at the 300th even when the session goes back much further (operator, 2026-11-25).
The maintainer chose to load older entries on demand, and asked that they arrive before the reader reaches the end, so the loading is never seen (operator, 2026-11-25).

## What Changes

1. The daemon sends the newest 300 entries as now, and a window asks it for the 100 entries before the oldest it holds once the reader comes near them.
   How near depends on how fast the view moved lately, so a page lands before the view reaches it.
2. A window holds at most 3,000 entries of each list.
   Past that, it drops the entries farthest from the view and loads them again if the reader comes back.
   The daemon reads each page from the transcript on disk and keeps none.
3. The tab counts and the scrollbar show the whole session, not only the entries held.

## Capabilities

### New Capabilities

- `session-history`: how a session's conversation and activity load older entries, and how they are counted.

### Modified Capabilities

None.
The `daemon-link` capability is still added by the open `app-daemon` change, and its requirements stay true.

## Impact

1. Modules: `src/app/facts.rs` (totals and pages), `src/daemon.rs` and `src/daemon/server.rs` (the page messages), `src/app.rs` and `src/app/{state,view}.rs` (the held lists and the prefetch lead).
2. The window and the daemon gain one request and one answer each.
   A window and a daemon of different versions already refuse each other at the handshake.
3. It needs the entry IDs and the layout cache of the [smooth-scroll][smooth-scroll] change.

[smooth-scroll]: ../smooth-scroll/proposal.md

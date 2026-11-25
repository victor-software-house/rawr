# Proposal

## Why

Each list's scrollbar in rawr is drawn but does nothing: a press on it is ignored, and it sits in view at all times (operator, 2026-11-25).
The maintainer asked for a scrollbar that can be dragged, and that stays out of sight while it is not in use.

## What Changes

1. The scrollbar of every list (panes, sessions, search results and the session's tabs) takes the mouse.
   - A press on the thumb grabs it, and a drag moves the list with it.
   - A press on the track pages the list toward the press, a view's height at a time.
2. A scrollbar shows only while it is in use, then fades out.
   - It shows while its list scrolls, while the pointer is on its column, and while its thumb is held.
   - Once none of these hold, it fades out over 200 ms after 800 ms.
3. A drag past the entries a window holds in the Conversation and Activity tabs loads the place it reaches directly, instead of every page between.

## Capabilities

### New Capabilities

- `list-scrollbar`: how a list's scrollbar shows, hides and takes the mouse.

### Modified Capabilities

None.
The `list-scrolling` and `session-history` capabilities are still added by the open `smooth-scroll` and `history-pages` changes, and their requirements stay true.

## Impact

1. Modules: a new `src/app/scrollbar.rs` (geometry, press, fade), `src/app/view.rs` (drawing and hits), `src/app/state.rs` (press, drag, page), `src/app/history.rs`, `src/app/facts.rs`, `src/daemon.rs` and `src/daemon/server.rs` (the jump page).
2. The window and the daemon gain one request.

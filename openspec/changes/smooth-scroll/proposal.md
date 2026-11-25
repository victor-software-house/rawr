# Proposal

## Why

Scrolling the Conversation and Activity tabs is uneven.
A trackpad hands the terminal its wheel reports in batches, and rawr moves the list by a whole batch in one frame, then not at all until the next batch.
Each frame also lays out every entry again, so the list scrolls slower the more it holds, and a new entry moves the text the reader is on.
The maintainer asked for scrolling as smooth as a native list (operator, 2026-11-25).

## What Changes

1. The app draws on a fixed frame clock while anything moves, and spreads each batch of wheel reports evenly over the frames until the next batch is due.
2. Each entry is laid out once for a width and a fold state, and a frame draws only the entries in view, so the cost of a frame does not grow with the history held.
3. Each entry gets a stable ID, and the view keeps its place by entry: a new entry above it no longer moves the text being read.

## Capabilities

### New Capabilities

- `list-scrolling`: how the app paces the wheel and draws a long list.

### Modified Capabilities

None.
The `app` capability is still added by the open `rawr-app` change, and its requirements stay true.

## Impact

1. Modules: `src/app.rs` (the frame loop), a new `src/app/pacing.rs` (the clock, the wheel queue and the beat), `src/app/{state,view}.rs` (the scroll anchor and the layout cache), `src/app/facts.rs` (entry IDs).
2. The [history-pages][history-pages] change builds on the entry IDs and the layout cache to load older entries.

[history-pages]: ../history-pages/proposal.md

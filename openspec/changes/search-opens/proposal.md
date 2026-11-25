# Proposal

## Why

Enter on a search result, or a second click on it, resumes the session or switches Rex to its pane.
The maintainer wants a result to open in rawr first, where its conversation and activity can be read, and to go to the pane or resume it only from there (operator, 2026-11-25).
A result whose session no pane lists has nowhere to open today.

## What Changes

1. Enter, or one click, on a search result closes the search and selects its session in rawr.
   It never resumes the session or switches Rex to a pane.
2. A result that a pane lists selects that pane and that session.
3. A result that no pane lists opens as an opened pane at the top of the panes list, with its session view and Resume, and no Go to pane.
4. An opened pane closes with `x` or a click on its close mark, and leaves the list once a pane lists its session.

## Capabilities

### New Capabilities

- `search-open`: what choosing a search result does, and the opened panes it adds.

### Modified Capabilities

None.

## Impact

1. Modules: `src/app/state.rs` (choosing a hit, the opened panes, `x`), `src/app/view.rs` (the Opened section and the close mark), `src/app/feed.rs` (the opened flag on `Pane`).
2. No change to the daemon, the stored state, or `rawr find`.

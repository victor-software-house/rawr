# Proposal

## Why

The session column spends about eleven rows on its header: the pane, its status, the session's title, folder and state, and the buttons.
On a 32-row window that leaves the Conversation and Activity lists little more than half the height, and too little room to read an entry once it is expanded (operator, 2026-11-25).
The maintainer asked for the header to fold while the list is scrolled, and to come back while the pointer is on it, as the folded pane list does.

## What Changes

1. While a tab's list is scrolled away from its top, the header folds into one row, and the list takes the rows it frees.
2. While the pointer is on the folded row, the full header slides out over the list, and slides back when the pointer leaves.
3. Back at the top of the list, the header unfolds.

## Capabilities

### New Capabilities

- `session-header`: when the session column's header folds, and how it comes back.

### Modified Capabilities

None.

## Impact

1. Modules: `src/app/view.rs` (`pane_view` and a folded header row), `src/app/state.rs` (the fold and the drawer-like slide, hits under the pointer).
2. No change to the daemon, the stored state or the keys.

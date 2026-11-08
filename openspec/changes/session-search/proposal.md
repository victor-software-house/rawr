# Proposal

## Why

The picker offers only the sessions that started in the pane being restored.
A session that ran in a pane closed long ago, in another session, or before rawr was installed can still be resumed, but finding it means searching transcripts by hand.
Searching must cost the pane nothing until the user asks for it, and must answer each key press at once.

## What Changes

1. `/` in the picker opens a search line.
   Each key press filters every resumable local Claude Code session by the words typed, matched against the prompts the user typed in it, its folder and its ID.
2. The pane's own sessions that match come first, then every other match, newest first.
3. `rawr find [WORDS…]` opens the picker straight on the search line, in or out of a Rex pane, so any session can be looked up and resumed from a shell.
4. The zsh integration wraps `rawr find` so the chosen command runs in the shell that asked, as the restore hook does for `pick`.

## Capabilities

### New Capabilities

- `session-search`: which local sessions the picker can find, how a query matches them, and the order of the results.

### Modified Capabilities

None.

## Impact

1. Modules: a new `src/catalog.rs` (every resumable local session), `src/picker/state.rs` (the query and the filtered rows), `src/picker/view.rs` (the search line), `src/main.rs` (the `find` command).
2. Data read: Claude Code's prompt history, `~/.claude/history.jsonl`, and one listing of `~/.claude/projects/*/`.
   Each listed transcript's last 64 KiB is read once per load, for its title.
3. Dependencies: none.

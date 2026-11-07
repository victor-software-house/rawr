# Proposal

## Why

After a Rex server restart every pane comes back as a fresh shell, and the Claude Code sessions that ran in it are gone from view.
Finding each session again means searching transcripts by hand and typing `cd` and `claude --resume` lines, pane by pane.

A first picker drew a few inline rows under the restored output and took only arrow keys.
It had no mouse support, no hover, and lost the pane's restored output under its rows.

## What Changes

1. `rawr record` keeps, per Rex pane, the last five Claude Code sessions that started in it.
   A Claude Code `SessionStart` hook runs it.
2. `rawr pick` opens a full-screen picker on the alternate screen, so the restored output is untouched when it closes.
3. The picker takes the mouse as well as the keyboard: hover highlights a row and shows the pointer cursor, a click selects a row, a click on the selected row resumes it, and the wheel moves the selection.
4. Long titles, folders and session IDs wrap; nothing is cut.
5. The terminal is always given back as it was found: on choice, dismissal, error or panic.

## Capabilities

### New Capabilities

- `pane-index`: which Claude Code sessions ran in which Rex pane, and how that record stays one file per live pane.
- `session-picker`: how a restored pane offers its sessions and hands the chosen one to the shell.

### Modified Capabilities

None.

## Impact

1. Modules: `src/index.rs` (pane index), `src/session.rs` (resumable sessions), `src/picker.rs` and its submodules (the full-screen picker).
2. Dependencies: `ratatui` with its crossterm backend, and `textwrap`.
3. Shell integration: a zsh snippet runs `rawr pick` once before the first prompt and evaluates what it prints.

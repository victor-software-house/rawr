# Design

## Context

`src/app.rs` sent `own_status` on every frame: a line that counted the panes, the ones that need input and the working ones.
Every rawr window in every restored pane sent it, so each pane's change reached Rex once per window.
`rawr --startup` opened the same full app as bare `rawr`, with its own pane selected.

## Decisions

1. **A stand-in in each restored pane, and one floating app.**
   The maintainer separated the two after every stand-in rose to the top of Rex's Activity list at once (operator, 2026-11-23).
   - `rawr --startup` opens a stand-in: only the sessions recorded for its own pane, newest first, with the selected one in detail.
     It draws no panes column and no search, so `ctrl+f`, `1`, `g` and the panes drawer do nothing, and `esc` with no filter set quits.
     `r` resumes in the pane, as before.
   - Bare `rawr` is the full app: every pane, its sessions and the search.
     Rex opens it floating over the window in view on `cmd+alt+r`, through an `init.lua` action the README gives.
   - `main.rs` decides the mode once, and the stand-in's title with it: the newest resumable session's title, or its folder's name when it is untitled.
2. **Each window reports only its own state.**
   - Reports use `id=rawr` and `app=rawr`, so a change in another pane sends nothing.
   - A stand-in reports `idle` once, before its first frame, with its session's title as the record's title.
   - The full app reports `idle` with a constant line and the title `rawr`, and `blocked` with a constant line and no `kind` while any pane needs the user.
     It sends a report only when that choice flips, so a second waiting pane sends nothing.
   - The reporter returns the bytes to send, or none, so tests check what goes out.
3. **Only the origin says rawr** (operator, 2026-11-23).
   A stand-in also sets its pane's terminal title (OSC 2) to its session's title, and the full app leaves the title alone.
   Rex shows that title for the tab and keeps no title stack, and a new shell starts with an empty title, so the guard that gives the terminal back writes an empty title beside the status clear.
   The panic hook runs the same guard.

## Verified

1. 2026-11-25 (UTC), on an isolated Rex server: `rex block call title` read a title a program set with OSC 2; push and pop (`CSI 22;2t`, `CSI 23;2t`) left the title unchanged; an empty OSC 2 left an empty title; a new shell's block read an empty title.

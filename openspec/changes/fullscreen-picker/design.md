# Design

## Context

`pick` runs inside `$(rawr pick)`, so stdout is captured by the shell and only stderr and the terminal device reach the user.
Rex keeps block IDs, labels and scrollback across a server restart, but no running program, so the picker runs in a fresh login shell.
The binary is built from [ratatui] 0.30 on its [crossterm] 0.29 backend.

## Goals / Non-Goals

**Goals:**

1. A picker that reads well in any terminal colour theme, light or dark.
2. Mouse and keyboard parity: every action has a key and a pointer gesture.
3. The terminal is restored on every exit path, and no mouse report leaks into the shell.

**Non-Goals:**

1. Deleting or renaming sessions from the picker.
2. Previewing a transcript.
3. Resuming agents other than Claude Code.

## Decisions

1. **Alternate screen on stderr.**
   The picker enters the alternate screen (DEC mode 1049) on stderr and leaves it on exit, so the pane's restored scrollback is shown again unchanged.
   Alternative: an inline viewport under the restored output.
   It lost: it pushes the output up, leaves its rows in scrollback, and cannot use the whole pane for long lists.
2. **crossterm commands for the modes.**
   `EnterAlternateScreen`, `EnableMouseCapture` (modes 1000, 1002, 1003, 1015 and 1006) and `Hide` come from crossterm, and their counterparts undo them.
   Mode 1003 reports motion with no button held, which hover needs.
   Alternative: writing the escape sequences by hand.
   It lost: crossterm already sends the same sequences, and one source for enter and leave keeps them paired.
3. **Pointer shape by OSC 22.**
   Over a row the picker sets the pointer to `pointer` with `OSC 22 ; pointer ST`, and back to `default` elsewhere and on exit.
   Terminals that do not know OSC 22 ignore it.
   Alternative: no pointer change.
   It lost: the pointer is the cue that a row takes a click.
4. **One guard owns the terminal.**
   A guard value enters raw mode and the modes, and its `Drop` leaves them; a panic hook leaves them too, because the release profile aborts on panic and `Drop` does not run.
   After leaving the modes it reads and drops input for 50 ms before raw mode ends, so reports already in flight are not typed into the shell.
5. **Selection and hover are separate.**
   The selected row has the `❯` pointer and a bold title in bright blue (ANSI 12); a hovered row turns its dim text to the default foreground.
   Hover changes only the text colour, so it never reads as selected.
   Body text uses the terminal's default foreground and secondary text is dimmed, so both read in light and dark themes.
6. **A click selects, and a click on the selected row resumes.**
   A left click on a row that is not selected selects it; a left click on the selected row resumes it.
   Two clicks on a row therefore resume it at any speed.
   Alternative: a timed double click resumes, and a click on the selected row does nothing.
   It lost: the maintainer chose the faster path, and a click on the highlighted row reads as "open this one".
   Cost: the first row starts selected, so one click on it resumes at once.
7. **The wheel moves the selection.**
   One wheel step moves the selection one row and the view follows it.
   Alternative: the wheel scrolls the view and leaves the selection.
   It lost: with a short list, scrolling without moving the selection does nothing visible.
8. **Pure state, pure view, thin loop.**
   `picker/state.rs` turns a key or mouse event into a state change, `picker/view.rs` draws the state into a frame and returns which screen row belongs to which session, and `picker.rs` only reads events and draws.
   Tests drive the state with events and draw the view on ratatui's `TestBackend`.
9. **Titles and folders are sanitized before drawing.**
   They come from transcripts, hook input and the prompt history, so control characters are dropped and tabs and line breaks become spaces before any of them reaches the terminal.
10. **A title comes from the transcript's end.**
   Transcripts reach hundreds of megabytes, and Claude Code writes the title rows again every few dozen kilobytes, the user's title next to each generated one.
   rawr reads the last 64 KiB, then the last 1 MiB when those hold no title row.
   Of 162 local transcripts, 139 had a title row, and 99% of those had their last one within 33 KB of the end.
   Alternative: read the whole transcript.
   It lost: the picker runs as a shell starts, and local transcripts measured up to 417 MB, each read whole at every start.

11. **A session shows two lines: its title, then its folder and age.**
   The session ID drew a third line under every session, and nobody reads it to choose: the title and folder say which session it is.
   Search still matches the ID, and the resume still uses it.
   Alternative: show the ID on the selected session only.
   It lost: the selected block would grow and shrink as the selection moves, so the rows under it would jump.

12. **rawr resumes the chosen session itself.**
   On a choice it restores the terminal, then replaces its process with `claude --resume <session id>` in the session's folder, so the pane runs Claude Code with nothing in between.
   Nothing resumes without a choice: the picker only offers.
   Alternative: print a `cd` and `claude --resume` line for a shell hook to evaluate.
   It lost: it needs a wrapper and a quoting rule per shell, and run without the hook the picker printed a line and did nothing.

## Risks / Trade-offs

1. Mode 1003 makes the terminal report every pointer move; the picker redraws only when the hovered row changes.
2. A terminal without SGR mouse reports (mode 1006) sends legacy reports that crossterm still reads, but columns past 223 are lost there.
3. A pane narrower than 20 columns wraps every word onto its own line; the picker still works but reads poorly.

[ratatui]: https://docs.rs/ratatui/0.30.2/ratatui/
[crossterm]: https://docs.rs/crossterm/0.29.0/crossterm/

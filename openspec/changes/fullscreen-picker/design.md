# Design

## Context

`pick` runs inside `$(rex-resume pick)`, so stdout is captured by the shell and only stderr and the terminal device reach the user.
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
6. **Click selects, double click resumes.**
   A left click selects the row under it; a second click on the same row within 400 ms resumes it.
   Alternative: a click on the row already selected resumes it.
   It lost: the first row starts selected, so one stray click would start a session the user did not mean to open.
7. **The wheel moves the selection.**
   One wheel step moves the selection one row and the view follows it.
   Alternative: the wheel scrolls the view and leaves the selection.
   It lost: with a short list, scrolling without moving the selection does nothing visible.
8. **Pure state, pure view, thin loop.**
   `picker/state.rs` turns a key or mouse event into a state change, `picker/view.rs` draws the state into a frame and returns which screen row belongs to which session, and `picker.rs` only reads events and draws.
   Tests drive the state with events and draw the view on ratatui's `TestBackend`.
9. **Titles are sanitized before drawing.**
   Titles come from transcripts, so control characters are dropped and tabs and line breaks become spaces before a title reaches the terminal.

## Risks / Trade-offs

1. Mode 1003 makes the terminal report every pointer move; the picker redraws only when the hovered row changes.
2. A terminal without SGR mouse reports (mode 1006) sends legacy reports that crossterm still reads, but columns past 223 are lost there.
3. A pane narrower than 20 columns wraps every word onto its own line; the picker still works but reads poorly.

[ratatui]: https://docs.rs/ratatui/0.30.2/ratatui/
[crossterm]: https://docs.rs/crossterm/0.29.0/crossterm/

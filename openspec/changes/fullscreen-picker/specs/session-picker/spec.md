# Spec Delta

## Purpose

Offer a restored Rex pane's Claude Code sessions in a full-screen picker, and hand the chosen one to the shell as a command.

## ADDED Requirements

### Requirement: Offer only resumable sessions

`rawr pick` SHALL offer the sessions in the pane's index whose transcript still exists and which no running Claude Code process holds.

#### Scenario: One session is still running

- **WHEN** the index lists three sessions and `~/.claude/sessions/4242.json` names one of them with a live PID 4242
- **THEN** the picker offers the other two

#### Scenario: Nothing to offer

- **WHEN** every listed session is running or its transcript is gone
- **THEN** `pick` draws nothing, prints nothing and exits 1

### Requirement: Hand the choice to the shell

`pick` SHALL write only the chosen session's command to stdout, as `cd -- '<folder>' && claude --resume '<session id>'` with each value single-quoted, and SHALL draw everything else on stderr.

#### Scenario: The user resumes a session

- **WHEN** the user chooses the session `6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33` that started in `/home/dev/code/it's shop`
- **THEN** stdout holds exactly `cd -- '/home/dev/code/it'\''s shop' && claude --resume '6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33'` and a newline, and `pick` exits 0

#### Scenario: The user dismisses the picker

- **WHEN** the user presses Esc, `q` or Ctrl+C
- **THEN** stdout is empty and `pick` exits 1

### Requirement: Full screen, restored on exit

`pick` SHALL draw on the alternate screen with mouse reporting on and the cursor hidden, and SHALL restore the main screen, the cursor, the pointer shape and cooked mode on every exit, a panic included.

#### Scenario: The picker closes

- **WHEN** the user resumes a session, dismisses the picker, or `pick` fails after drawing
- **THEN** the pane shows its output as before the picker opened, the cursor is visible, and mouse reports no longer reach the shell

### Requirement: Keyboard control

The picker SHALL move the selection with Up, Down, `k`, `j`, Home, End, `g`, `G`, Page Up and Page Down, resume the selected session with Enter, and resume the n-th session with the digit n from 1 to 9.

#### Scenario: Pick by number

- **WHEN** the picker offers four sessions and the user presses `3`
- **THEN** `pick` prints the third session's command

### Requirement: Mouse control

The picker SHALL select the row under a left click when that row is not selected, resume the row under a left click when it is selected, and move the selection one row per wheel step.

#### Scenario: Click another row

- **WHEN** the first row is selected and the user clicks the second row
- **THEN** the picker stays open with the second row selected

#### Scenario: Click the selected row

- **WHEN** the second row is selected and the user clicks it
- **THEN** `pick` prints the second session's command

#### Scenario: Wheel

- **WHEN** the first row is selected and the user scrolls the wheel down twice
- **THEN** the third row is selected and visible

### Requirement: Hover

The picker SHALL show the row under the pointer in the default foreground instead of dimmed, and set the pointer shape to `pointer` with OSC 22 while it is over a row and to `default` elsewhere.

#### Scenario: The pointer moves over a row

- **WHEN** the pointer moves onto an unselected row
- **THEN** that row's folder and age turn from dim to the default foreground and the pointer shape becomes `pointer`

### Requirement: Nothing is cut

The picker SHALL wrap a title, folder or session ID that does not fit the pane's width at spaces, never at a hyphen, and break a word only when it is wider than the row.

#### Scenario: A long title in a narrow pane

- **WHEN** a title of 70 characters is shown in a pane 40 columns wide
- **THEN** every word of the title appears, across two or more rows, and the session ID appears whole on one row

### Requirement: Safe titles

The picker SHALL drop control characters from a title, and turn its tabs and line breaks into spaces, before drawing it.

#### Scenario: A title holds an escape sequence

- **WHEN** a transcript's title is `fix\u{1b}[2Jbuild`
- **THEN** the picker draws `fix[2Jbuild` and the screen is not cleared

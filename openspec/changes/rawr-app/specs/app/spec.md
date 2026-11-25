# Spec Delta

## Purpose

Bare `rawr` is the one command a person types: a full-screen app that shows every Rex pane's Claude, stays current, and gets the user back to a session.

## ADDED Requirements

### Requirement: Bare rawr opens the app

Running `rawr` with no verb on a terminal SHALL open the full-screen app on the alternate screen, drawn through stderr, and SHALL write nothing to stdout.

#### Scenario: Run in a Rex pane

- **WHEN** the user types `rawr` in a Rex pane and the server has three panes whose index lists a session
- **THEN** the app lists those three panes, each with its glyph, name and status word

#### Scenario: No terminal

- **WHEN** `rawr` runs with stderr not a terminal
- **THEN** it prints `rawr needs a terminal; rawr status --format json lists the panes` on stderr and exits 1

### Requirement: Panes stay current

The app SHALL update a pane's status word within one draw of the Rex event that changed it, with no timer and no repeated listing.

#### Scenario: A Claude asks for permission

- **WHEN** the app is open and Rex sends `program.status_changed` with state `blocked` for the root record of pane `work/shell`
- **THEN** `work/shell` reads `Needs input` and moves to the top of the list

#### Scenario: A pane closes

- **WHEN** Rex sends `block.closed` for a listed pane
- **THEN** the pane leaves the list, and the selection moves to the next pane

### Requirement: Panes that need the user come first

The app SHALL order panes by status, those that need the user first, then Failed, Done, Working, Idle and Stopped, and within each status by the newest session, newest first; sessions no open pane holds SHALL come last.

#### Scenario: Mixed statuses

- **WHEN** pane `a` is Idle, pane `b` is Needs input and pane `c` is Working
- **THEN** the list reads `b`, `c`, `a`

### Requirement: Every state draws something

The app SHALL show a line that names the state when the Rex server cannot be reached, when no pane has a session, and when the event stream fails after the first snapshot.

#### Scenario: No Rex server

- **WHEN** `rawr` runs and the Rex socket refuses the connection
- **THEN** the app shows `Rex is not running here`, and `ctrl+f` still searches every local session

#### Scenario: Nothing recorded

- **WHEN** the server answers and no pane's index lists a session
- **THEN** the app shows `No pane has a Claude session yet; rawr setup claude records them`

### Requirement: Resume a session

On `r`, a window tiled in its Rex tab SHALL give the terminal back as it found it, then replace its process with `claude --resume <session id>` in the session's folder; a floating window SHALL open the resume in a new tab of the pane's Rex session.
A session that a live Claude holds SHALL NOT be offered.

#### Scenario: Resume a stopped pane's session

- **WHEN** the window is tiled in its Rex tab, and the user selects session `Fix checkout` in folder `/work/shop` and presses `r`
- **THEN** the alternate screen closes and the process becomes `claude --resume <its id>` in `/work/shop`

### Requirement: Go to a pane

On `enter`, the app SHALL ask the Rex app to show the selected pane's session and window and focus its block, and SHALL show the app's refusal in the status line when it refuses.

#### Scenario: Remote Control is off

- **WHEN** the user presses `enter` on a pane and the Rex app refuses client actions
- **THEN** the status line shows the app's message, and the app stays open

### Requirement: Entries fold and open on a click

The Conversation and Activity tabs SHALL draw each entry folded until the user opens it: a prompt or a reply in at most 3 lines, the note that a task ended in 2, and anything else in 1, with a `+N` count of the lines left out.
A click on an entry SHALL open or fold it.
An open entry SHALL draw its text as Markdown, with fenced code highlighted, and a tool call's result under `⎿`.

#### Scenario: A long reply

- **WHEN** a reply of 40 lines holds a fenced `rust` block, and the user clicks it
- **THEN** the reply opens with its Markdown drawn and the Rust code highlighted, and a second click folds it back to 3 lines ending in `+37`

#### Scenario: A teammate's message

- **WHEN** a teammate named `reviewer` sent `Looks good`
- **THEN** the entry reads `@reviewer Looks good`, with no `Message from` lead

### Requirement: The pointer marks an entry without a background

No row under the pointer SHALL get a background.
A folded entry under the pointer SHALL show `▸` before its mark, an open one `▾`, and the entry's other rows a `▏` bar.

#### Scenario: Hover over an open reply

- **WHEN** the pointer rests on the second row of an open reply of 5 rows
- **THEN** the first row shows `▾`, the other four show `▏`, and no row's background changes

### Requirement: Files open in their default app

The Files tab SHALL list the files the session changed and read, each path cut to fit, and a click on a file SHALL open it in its default app, as a click on a `file://` link does.
A file gone since the session touched it SHALL be named in the status line instead.

#### Scenario: Open a changed file

- **WHEN** the session wrote `/work/shop/src/cart.rs` and the user clicks its row
- **THEN** `/work/shop/src/cart.rs` opens in the app the system assigns to `.rs` files, and rawr stays open

#### Scenario: A file in a temporary folder

- **WHEN** the session wrote 12 files under the system's temporary folder
- **THEN** they fold into one line, `+ 12 files in temporary folders`, which a click opens

### Requirement: Titles are drawn safe

The app SHALL drop control characters from a session's title, and turn its tabs and line breaks into spaces, before drawing it.

#### Scenario: A title holds an escape sequence

- **WHEN** a transcript's title is `fix\u{1b}[2Jbuild`
- **THEN** the app draws `fix[2Jbuild` and the screen is not cleared

### Requirement: The startup line offers only what can be resumed

`rawr --startup` SHALL open a stand-in on the current pane's sessions only when one of them can be resumed, and otherwise SHALL print nothing and exit 0.

#### Scenario: Fresh pane

- **WHEN** a new shell runs `rawr --startup` in a pane whose index is empty
- **THEN** nothing is printed and the exit status is 0

#### Scenario: Restored pane

- **WHEN** a shell runs `rawr --startup` in a pane whose index lists a session no live Claude holds
- **THEN** the stand-in opens with that pane's sessions listed, newest first, and the selected one in detail

### Requirement: A stand-in shows only its own pane

A stand-in SHALL list only the sessions recorded for its own pane, and SHALL NOT draw the panes column or open the search.

#### Scenario: Other panes have sessions

- **WHEN** the daemon lists three panes and the stand-in runs in one of them
- **THEN** the stand-in draws that pane's sessions and no other pane

#### Scenario: Keys that reach other panes

- **WHEN** the user presses `ctrl+f`, `1` or `g` in a stand-in
- **THEN** nothing changes on screen

#### Scenario: Back from the sessions

- **WHEN** the user presses `esc` in a stand-in with no filter set
- **THEN** the stand-in quits and the shell prompt returns

### Requirement: Each window reports only its own state

A rawr window SHALL send an OSC 7501 report only when its own state changes, and SHALL NOT send one because another pane changed.
A stand-in SHALL report `idle` once, with `app=rawr` and its session's title as the title.
The full app SHALL report `idle` with a constant line and the title `rawr`, or `blocked` with a constant line while any pane needs the user.

#### Scenario: Another pane starts working

- **WHEN** a pane's Claude goes from idle to working while a stand-in runs in another pane
- **THEN** the stand-in sends no report

#### Scenario: A second pane needs the user

- **WHEN** one pane needs the user and a second one starts to need the user too
- **THEN** the full app sends no report

### Requirement: A stand-in titles its pane after its session

While a stand-in runs, its pane's terminal title SHALL be the stand-in's session title, or the session folder's name when the session has no title; when the stand-in gives the terminal back, the title SHALL be empty again.

#### Scenario: Restored pane

- **WHEN** a stand-in opens on a pane whose newest resumable session is titled `Fix checkout`
- **THEN** the pane's title reads `Fix checkout` and not `rawr`

### Requirement: pick and find are gone

The verbs `pick` and `find` SHALL NOT exist; `rawr pick` SHALL fail as an unknown verb.

#### Scenario: Old muscle memory

- **WHEN** the user types `rawr pick`
- **THEN** clap reports an unknown subcommand and exits 2

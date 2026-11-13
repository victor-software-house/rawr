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

The app SHALL order panes Needs input, Failed, Working, Idle, Stopped, and within each status by the newest session, newest first.

#### Scenario: Mixed statuses

- **WHEN** pane `a` is Idle, pane `b` is Needs input and pane `c` is Working
- **THEN** the list reads `b`, `c`, `a`

### Requirement: Every state draws something

The app SHALL show a line that names the state when the Rex server cannot be reached, when no pane has a session, and when the event stream fails after the first snapshot.

#### Scenario: No Rex server

- **WHEN** `rawr` runs and the Rex socket refuses the connection
- **THEN** the app shows `Rex is not running here; / searches every local session` and the search works

#### Scenario: Nothing recorded

- **WHEN** the server answers and no pane's index lists a session
- **THEN** the app shows `No pane has a Claude session yet; rawr setup claude records them`

### Requirement: Resume in this terminal

On `r`, the app SHALL give the terminal back as it found it, then replace its process with `claude --resume <session id>` in the session's folder.
A session that a live Claude holds SHALL NOT be offered.

#### Scenario: Resume a stopped pane's session

- **WHEN** the user selects session `Fix checkout` in folder `/work/shop` and presses `r`
- **THEN** the alternate screen closes and the process becomes `claude --resume <its id>` in `/work/shop`

### Requirement: Go to a pane

On `enter`, the app SHALL ask the Rex app to show the selected pane's session and window and focus its block, and SHALL show the app's refusal in the status line when it refuses.

#### Scenario: Remote Control is off

- **WHEN** the user presses `enter` on a pane and the Rex app refuses client actions
- **THEN** the status line shows the app's message, and the app stays open

### Requirement: The startup line offers only what can be resumed

`rawr --startup` SHALL open the app on the current pane's sessions only when one of them can be resumed, and otherwise SHALL print nothing and exit 0.

#### Scenario: Fresh pane

- **WHEN** a new shell runs `rawr --startup` in a pane whose index is empty
- **THEN** nothing is printed and the exit status is 0

#### Scenario: Restored pane

- **WHEN** a shell runs `rawr --startup` in a pane whose index lists a session no live Claude holds
- **THEN** the app opens with that pane selected and its sessions listed

### Requirement: pick and find are gone

The verbs `pick` and `find` SHALL NOT exist; `rawr pick` SHALL fail as an unknown verb.

#### Scenario: Old muscle memory

- **WHEN** the user types `rawr pick`
- **THEN** clap reports an unknown subcommand and exits 2

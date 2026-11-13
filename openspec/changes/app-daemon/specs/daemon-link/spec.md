# Spec Delta

## Purpose

`rawr daemon` holds the Rex feed, the transcript facts and the search catalog for every `rawr` window, and each window is a client of it.

## ADDED Requirements

### Requirement: A window draws the daemon's panes first

A client SHALL draw the panes from the daemon's first `panes` message, without listing Rex itself.

#### Scenario: A second window opens

- **WHEN** the daemon follows a Rex server with panes `work/shell` and `notes/shell`, and the user runs `rawr` in a new pane
- **THEN** the first frame after `welcome` lists `work/shell` and `notes/shell` with their status words

### Requirement: The first client starts the daemon

A client that finds no daemon on `~/.local/state/rawr/daemon.sock` SHALL start `rawr daemon` under `daemon.lock`, wait for its `ready` line for at most 5 seconds, and connect; a second `rawr daemon` SHALL exit while the first holds the lock.

#### Scenario: No daemon runs

- **WHEN** no process listens on `~/.local/state/rawr/daemon.sock` and the user runs `rawr`
- **THEN** `rawr daemon` starts, prints `ready`, and the window connects to it

#### Scenario: Two windows start together

- **WHEN** two `rawr` windows find no daemon at the same moment
- **THEN** one daemon starts and both windows connect to it

### Requirement: A different binary replaces the daemon

The daemon SHALL refuse a `hello` whose build differs from its own, and the client SHALL then ask it to shut down and start its own daemon.

#### Scenario: The binary was rebuilt

- **WHEN** the daemon runs from `~/.cargo/bin/rawr` with modification time `2026-11-13T15:00:00Z`, and a client built at `2026-11-13T15:20:00Z` from the same path connects
- **THEN** the daemon answers `refused`, the client sends `shutdown`, and a new daemon from the new binary serves the client

### Requirement: Facts and hits stream from the daemon

The daemon SHALL answer a `facts` request once the transcript is read, and a `search` with `hits` batches as transcripts are read, ending an older search from the same client when a newer one arrives.

#### Scenario: A large transcript is selected

- **WHEN** the user selects a session whose transcript is 60 MB
- **THEN** the window keeps drawing and taking keys, shows `reading the transcript…`, and shows the facts when the daemon's `facts` message arrives

#### Scenario: The query changes mid-search

- **WHEN** the client sends `search` with ID 4 for `kodus`, then ID 5 for `kodus ci` before ID 4 has read every transcript
- **THEN** the daemon sends no more `hits` for ID 4

### Requirement: A slow client is dropped

The daemon SHALL close a connection whose queue holds 64 unsent messages, and the client SHALL reconnect and show `stale` until it is answered.

#### Scenario: A window stops reading

- **WHEN** a client stops reading while 64 `panes` messages wait for it
- **THEN** the daemon closes that connection, and other clients keep receiving messages

### Requirement: View state stays in the window

The daemon SHALL hold no selection, focus, query or scroll; the copy started by going to a pane SHALL open with the state of the copy it replaces.

#### Scenario: Going to a pane in another Rex session

- **WHEN** the floating window has query `ci`, the sidebar folded and pane `notes/shell` selected, and the user goes to `notes/shell` in session `notes`
- **THEN** the new window in `notes` opens with query `ci`, the sidebar folded and `notes/shell` selected

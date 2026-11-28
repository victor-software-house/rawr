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

A client that finds no daemon on `~/.local/state/rawr/daemon.sock` SHALL start `rawr daemon` under `start.lock`, wait for its `ready` line for at most 5 seconds, and connect; a second `rawr daemon` SHALL exit while the first holds the lock.

#### Scenario: No daemon runs

- **WHEN** no process listens on `~/.local/state/rawr/daemon.sock` and the user runs `rawr`
- **THEN** `rawr daemon` starts, prints `ready`, and the window connects to it

#### Scenario: Two windows start together

- **WHEN** two `rawr` windows find no daemon at the same moment
- **THEN** one daemon starts and both windows connect to it

#### Scenario: A daemon already holds the lock

- **WHEN** a `rawr daemon` holds `~/.local/state/rawr/daemon.lock` and a second `rawr daemon` starts
- **THEN** the second one prints `a rawr daemon already runs` on stderr and exits 1

### Requirement: A different binary replaces the daemon

The daemon SHALL refuse a `hello` whose build differs from its own, and the client SHALL then ask it to shut down and start its own daemon.

#### Scenario: The binary was rebuilt

- **WHEN** the daemon runs from `~/.cargo/bin/rawr` with modification time `2026-11-13T15:00:00Z`, and a client built at `2026-11-13T15:20:00Z` from the same path connects
- **THEN** the daemon answers `refused`, the client sends `shutdown`, and a new daemon from the new binary serves the client

### Requirement: Facts and hits stream from the daemon

The daemon SHALL read a transcript from its newest line back and send its facts as the read goes, and SHALL stop that read when the same client asks for another transcript.
It SHALL answer a `search` with `hits` batches as transcripts are read, ending an older search from the same client when a newer one arrives.

#### Scenario: A large transcript is selected

- **WHEN** the user selects a session whose transcript is 60 MB
- **THEN** the window keeps drawing and taking keys, shows the newest entries from the first `facts` message, and shows `reading older lines…` until the read reaches the start

#### Scenario: Another session is selected mid-read

- **WHEN** the daemon is reading session A's transcript for a client, and that client asks for session B's
- **THEN** the daemon stops reading A between parts and sends B's facts next, and asking for A again goes on from where its read stopped

#### Scenario: The query changes mid-search

- **WHEN** the client sends `search` with ID 4 for `kodus`, then ID 5 for `kodus ci` before ID 4 has read every transcript
- **THEN** the daemon sends no more `hits` for ID 4

### Requirement: Sessions no pane holds are listed

The daemon SHALL add to `panes`, after the open panes, a closed pane for each session whose transcript was written in the last day and that no open pane holds, up to 40, newest first.

#### Scenario: A pane closed with its Claude

- **WHEN** the user closes a Rex pane whose Claude wrote its transcript an hour ago
- **THEN** within 15 seconds `panes` lists that session as a closed pane, and the user can resume it

#### Scenario: A scripted run

- **WHEN** a Claude ran today from a folder under the system's temporary folder, and no pane holds it
- **THEN** `panes` does not list it

### Requirement: The daemon acts in Rex for the window

The daemon SHALL run `go_to` and `resume` in Rex for the window that sent them, and SHALL answer a failure with a `notice`.
A `resume` for a window that does not float SHALL be answered with `exec`, and the window SHALL then become `rawr <agent>` with the resume arguments and the recorded flags itself.

#### Scenario: Remote Control is off

- **WHEN** the window sends `go_to` and the Rex app refuses client actions
- **THEN** the daemon sends a `notice` with the app's message, and the window stays open

#### Scenario: Resume from a tiled pane

- **WHEN** a window tiled in its Rex tab sends `resume` for session `Fix checkout`
- **THEN** the daemon answers `exec`, and the window gives the terminal back and becomes `rawr claude --resume <its id>` with its recorded flags in its folder

### Requirement: A slow client is dropped

The daemon SHALL close a connection whose queue holds 64 unsent messages, and the client SHALL reconnect and show `stale` until it is answered.

#### Scenario: A window stops reading

- **WHEN** a client stops reading while 64 `panes` messages wait for it
- **THEN** the daemon closes that connection, and other clients keep receiving messages

### Requirement: View state stays in the window

The daemon SHALL hold no window's selection, focus, query or scroll, except the view a `go_to` carries, which it SHALL give in `welcome` to the copy it starts and then drop.

#### Scenario: Going to a pane in another Rex session

- **WHEN** the floating window has query `ci`, the panes column folded and pane `notes/shell` selected, and the user goes to `notes/shell` in session `notes`
- **THEN** the new window in `notes` opens with query `ci`, the panes column folded and `notes/shell` selected

# Spec Delta

## Purpose

Restart the Rex server only once every Claude Code session in it has settled, within a bound.

## ADDED Requirements

### Requirement: Restart waits for idle

`rawr restart` SHALL open a drain, and SHALL stop and start the Rex server only when every live Claude block's current session has acknowledged the current drain ID and the block reads `Idle` or `Failed` with no child record in `working` on a check made right before the stop, or when `--force` is given.
The set of live blocks SHALL change as Claude sessions start and exit during the drain.
On a timeout, an interrupt or any other non-zero exit, `rawr restart` SHALL close the drain it opened.

#### Scenario: Every pane was already idle

- **WHEN** every Claude block reads `Idle` when `rawr restart` opens drain `d-7a21`, and no block has acknowledged it yet
- **THEN** the Rex server keeps running until each block has acknowledged `d-7a21` and settled

#### Scenario: An idle block with no acknowledgement

- **WHEN** block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` reads `Idle` with no child record in `working` and has not acknowledged the current drain ID
- **THEN** the daemon does not stop the Rex server

#### Scenario: A second Claude in a pane that already acknowledged

- **WHEN** session `6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33` acknowledged `d-7a21` and exited, and a new session starts in the same block
- **THEN** the block stays unsettled until the new session acknowledges `d-7a21`

#### Scenario: A Claude starts during the drain

- **WHEN** a Claude starts in a new block while drain `d-7a21` is open
- **THEN** that block joins the wait, and its mod delivers and acknowledges `d-7a21` like the others

#### Scenario: A block whose Claude exits during the drain

- **WHEN** a block's Claude exits before acknowledging `d-7a21`, and the block reads `Stopped`
- **THEN** the block leaves the wait

#### Scenario: A pane waits on a permission prompt during the wait

- **WHEN** block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` reads `Needs input` while `rawr restart` waits
- **THEN** `rawr restart` names that block at once and keeps waiting

#### Scenario: Every pane acknowledged and settled

- **WHEN** every Claude block has acknowledged the drain and reads `Idle` or `Failed` with no child record in `working`
- **THEN** the daemon stops the Rex server, starts it again, and closes the drain

#### Scenario: One pane stays busy past the wait

- **WHEN** `restart.wait` is `5m` and block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` still reads `Working` 5 minutes after `rawr restart`
- **THEN** the Rex server keeps running, the drain is closed, and `rawr restart` exits non-zero naming that block

#### Scenario: The user interrupts the wait

- **WHEN** the user presses `ctrl+c` while `rawr restart` waits
- **THEN** the Rex server keeps running and the drain is closed

#### Scenario: The user restarts despite pending panes

- **WHEN** two blocks still read `Working` and the user presses `f` in the `rawr restart` view
- **THEN** the daemon stops and starts the Rex server at once and closes the drain

### Requirement: A drain left open is closed

`drain.yaml` SHALL record the daemon's process ID and start time.
The mod SHALL deliver nothing from a drain whose owner is no longer running.
At start, the daemon SHALL close any open drain unless its restart record says a restart is pending.

#### Scenario: The daemon was killed mid-drain

- **WHEN** `drain.yaml` names a daemon process that no longer runs
- **THEN** no session receives its hand-off request, and the next daemon start removes `drain.yaml`

#### Scenario: The daemon starts with an open drain

- **WHEN** the daemon starts, `drain.yaml` holds an open drain, and no restart is pending
- **THEN** the daemon closes the drain

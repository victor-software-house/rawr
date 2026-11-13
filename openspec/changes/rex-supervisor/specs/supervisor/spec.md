# Spec Delta

## Purpose

Show the state of every Claude Code session running in Rex, and restart the Rex server only once every session is idle.

## ADDED Requirements

### Requirement: Status per pane from Rex

`rawr status` SHALL list each Rex block whose index names a session, with one status word taken from that block's program status records.

#### Scenario: A session waits on a permission prompt

- **WHEN** block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` has a root record `state=blocked:app=claude-code:kind=permission`
- **THEN** its row reads `Needs input`

#### Scenario: A finished turn with a background task still running

- **WHEN** a block's root record is `state=done:app=claude-code` and it has one child record `state=working`
- **THEN** its row reads `Working`

#### Scenario: A pane whose Claude has exited

- **WHEN** a block has no program status record and its index lists a session
- **THEN** its row reads `Stopped`

### Requirement: Restart waits for idle

`rawr restart` SHALL open a drain, and SHALL stop and start the Rex server only when every live Claude block has acknowledged the drain and reads `Idle` or `Failed` with no child record since, or when `--force` is given.

#### Scenario: One pane stays busy past the wait

- **WHEN** `restart.wait` is `30m` and block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` still reads `Working` 30 minutes after `rawr restart`
- **THEN** the Rex server keeps running, and `rawr restart` exits non-zero naming that block

#### Scenario: Every pane was already idle

- **WHEN** every Claude block reads `Idle` when `rawr restart` opens drain `d-7a21`, and no block has acknowledged it yet
- **THEN** the Rex server keeps running until each block has acknowledged `d-7a21` and settled

#### Scenario: Every pane acknowledged and settled

- **WHEN** every Claude block has acknowledged the drain and reads `Idle` or `Failed` with no child record
- **THEN** the daemon stops the Rex server, starts it again, and closes the drain

#### Scenario: A pane waits on a permission prompt during the wait

- **WHEN** block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` reads `Needs input` while `rawr restart` waits
- **THEN** `rawr restart` names that block at once and keeps waiting

### Requirement: No pruning while Rex is down

The daemon SHALL delete no pane file on block events during a restart it runs, and SHALL prune at start only against a full listing from a running Rex server.

#### Scenario: The daemon starts while Rex is stopped

- **WHEN** the daemon starts and the Rex server does not answer
- **THEN** every pane file stays

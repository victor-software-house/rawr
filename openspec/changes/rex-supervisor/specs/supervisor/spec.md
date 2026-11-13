# Spec Delta

## Purpose

Show the state of every Claude Code session running in Rex.

## ADDED Requirements

### Requirement: Status per pane from Rex

`rawr status` SHALL list each Rex block whose index names a session, with one status word taken from that block's program status records.

#### Scenario: A session waits on a permission prompt

- **WHEN** block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08` has a root record `state=blocked:app=claude-code:kind=permission`
- **THEN** its row reads `Needs input`

#### Scenario: A finished turn with a background task still running

- **WHEN** a block's root record is `state=done:app=claude-code` and it has one child record `state=working`
- **THEN** its row reads `Working`

#### Scenario: A permission prompt while a background task runs

- **WHEN** a block's root record is `state=blocked:app=claude-code:kind=permission` and it has one child record `state=working`
- **THEN** its row reads `Needs input`

#### Scenario: A finished background task

- **WHEN** a block's root record is `state=idle:app=claude-code` and its one child record is `state=done`
- **THEN** its row reads `Idle`

#### Scenario: A pane whose Claude has exited

- **WHEN** a block has no program status record and its index lists a session
- **THEN** its row reads `Stopped`

### Requirement: Pruning only against a confirmed listing

A block event SHALL only mark a pane; the daemon SHALL delete a pane file only when a later full listing from a Rex server that reports its sessions restored confirms the block is gone, and a failed or empty listing SHALL delete nothing.

#### Scenario: The daemon starts while Rex is stopped

- **WHEN** the daemon starts and the Rex server does not answer
- **THEN** every pane file stays

#### Scenario: Rex emits removal events as it shuts down

- **WHEN** the Rex server crashes and emits a removal event for block `block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08`
- **THEN** that block's pane file stays until a later full listing from the restored server leaves it out

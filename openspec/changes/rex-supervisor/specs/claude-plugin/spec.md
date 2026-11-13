# Spec Delta

## Purpose

Record each Claude Code session under its Rex pane, and carry a drain request into each session, through one mod that uses only the documented mods API.

## ADDED Requirements

### Requirement: Record on session start

The rawr mod SHALL, on `session.start` in a process whose environment holds `REX_BLOCK`, run the rawr binary at the path stored in the plugin's data folder with the session's ID, folder and transcript path.

#### Scenario: A session starts outside Rex

- **WHEN** a session starts and `REX_BLOCK` is unset
- **THEN** the mod runs nothing

### Requirement: Carry a drain request once

The rawr mod SHALL read `$XDG_STATE_HOME/rawr/drain.yaml` every 2 seconds and deliver each request ID once: through `$.prompt.submit` when the session is idle, or through `$.session.append` before the next step when it is busy.

#### Scenario: An idle session during a drain

- **WHEN** `drain.yaml` holds `id: d-7a21` and the session is idle
- **THEN** the session starts one turn with the hand-off request, and a later read of the same ID delivers nothing

#### Scenario: A busy session during a drain

- **WHEN** `drain.yaml` holds `id: d-7a21` and the session is in a turn
- **THEN** the request is appended before the session's next step

### Requirement: Tell a session its drain was cancelled

When a drain the session acknowledged closes without a restart, the rawr mod SHALL clear its `ui.status` and append one notice before the session's next step that the restart was cancelled and work can go on.

#### Scenario: The restart timed out

- **WHEN** a session acknowledged `d-7a21` and the daemon closes `d-7a21` after `restart.wait` expires
- **THEN** the session's next turn starts with the cancellation notice

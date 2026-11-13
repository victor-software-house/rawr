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

When a drain the mod delivered to the session closes without a restart, whether or not the session has acknowledged it yet, the rawr mod SHALL clear its `ui.status` and append one notice before the session's next step that the restart was cancelled and work can go on.

#### Scenario: The restart timed out

- **WHEN** the mod delivered `d-7a21` to a session still mid-turn, and the daemon closes `d-7a21` after `restart.wait` expires
- **THEN** the session's next turn starts with the cancellation notice

### Requirement: Acknowledge when the carrying turn ends

The rawr mod SHALL acknowledge a drain request with the session's ID on `turn.complete` of the turn that carried it, whatever its `reason`: `answer`, `aborted`, `refusal` or `error`.

#### Scenario: The hand-off turn ends in an error

- **WHEN** the turn that carried `d-7a21` ends with an API error
- **THEN** the mod acknowledges `d-7a21`, and a block that reads `Failed` counts as settled

### Requirement: Interrupt a turn that runs too long

When a session that received a drain request through `$.session.append` has held it for `drain.interrupt` without its carrying turn ending, the rawr mod SHALL, once per request ID, cancel the running turn with `$.turn.abort` and submit the hand-off request with `$.prompt.submit`.
The submitted hand-off turn SHALL become the carrying turn and SHALL NOT be interrupted again.
Once it has called `$.turn.abort`, the mod SHALL ignore the cancelled turn's `turn.complete` whatever its `reason`, and SHALL acknowledge only when the hand-off turn completes.
When the turn completes before the abort takes effect, the mod SHALL submit no hand-off and SHALL acknowledge on that completion.

#### Scenario: A long build during a drain

- **WHEN** `drain.interrupt` is `2m` and a session is still running a foreground build 2 minutes after `d-7a21` was appended
- **THEN** the mod cancels the turn, the build stops, and the session starts its hand-off turn

#### Scenario: The hand-off turn itself runs long

- **WHEN** the mod cancelled a turn for `d-7a21` and the submitted hand-off turn runs past `drain.interrupt`
- **THEN** the mod does not cancel it, and acknowledges `d-7a21` only when that turn ends

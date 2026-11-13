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

The rawr mod SHALL acknowledge a drain request with the session's ID on `turn.complete` of the turn that carried it, whatever its `reason`: `answer`, `aborted`, `refusal` or `error`, except for a turn it cancelled under "Interrupt a turn that runs too long", which "Acknowledge an interrupted request after its hand-off" governs.

#### Scenario: The hand-off turn ends in an error

- **WHEN** the turn that carried `d-7a21` ends with an API error
- **THEN** the mod acknowledges `d-7a21`, and a block that reads `Failed` counts as settled

### Requirement: Background a running call at once

When the rawr mod appends a drain request through `$.session.append` while a Bash or Agent call of its session runs in the foreground, it SHALL at once run `rawr drain background`, which SHALL write the bytes `0x18 0x02` (`ctrl+x ctrl+b`) to the session's own block through Rex's `com.superlogical.terminal.write`.
The mod SHALL NOT send the key while no such call is in flight.

#### Scenario: A long build during a drain

- **WHEN** a session is running a foreground build when `d-7a21` is appended
- **THEN** the build moves to the background and keeps running, and the session reads the request at its next step

#### Scenario: No call in flight

- **WHEN** a session is streaming a model response with no tool call running when `d-7a21` is appended
- **THEN** no key is sent

### Requirement: Interrupt a turn that runs too long

When a session that received a drain request through `$.session.append` has held it for `drain.interrupt` without its carrying turn ending, the rawr mod SHALL, once per request ID, cancel the running turn with `$.turn.abort`, and SHALL then submit the hand-off as "Acknowledge an interrupted request after its hand-off" sets out.
The submitted hand-off turn SHALL become the carrying turn and SHALL NOT be interrupted again.

#### Scenario: A long tool call that cannot move to the background

- **WHEN** `drain.interrupt` is `2m` and a session is still waiting on a long MCP tool call 2 minutes after `d-7a21` was appended
- **THEN** the mod cancels the turn, the call stops, and, once the cancelled turn completes with `reason` `aborted` or `error`, the session starts its hand-off turn

#### Scenario: The hand-off turn itself runs long

- **WHEN** the mod cancelled a turn for `d-7a21` and the submitted hand-off turn runs past `drain.interrupt`
- **THEN** the mod does not cancel it, and acknowledges `d-7a21` only when that turn ends

### Requirement: Acknowledge an interrupted request after its hand-off

Once it has called `$.turn.abort`, the mod SHALL wait for the cancelled turn's `turn.complete` before submitting the hand-off.
If that completion's `reason` is `aborted` or `error`, the mod SHALL submit the hand-off with `$.prompt.submit` and SHALL acknowledge only when the hand-off turn completes.
If its `reason` is `answer` or `refusal`, the turn ended before the abort took effect: the mod SHALL submit no hand-off and SHALL acknowledge on that completion.

#### Scenario: The cancelled turn reports the abort

- **WHEN** the mod cancels the turn carrying `d-7a21` and that turn's `turn.complete` has `reason` `aborted`
- **THEN** the mod submits the hand-off and does not acknowledge `d-7a21` until the hand-off turn completes

#### Scenario: The turn ends just before the abort

- **WHEN** the turn carrying `d-7a21` completes with `reason` `answer` before the mod's `$.turn.abort` takes effect
- **THEN** the mod submits no hand-off and acknowledges `d-7a21`

#### Scenario: The cancelled turn ends in an error

- **WHEN** the mod cancels the turn carrying `d-7a21` and that turn's `turn.complete` has `reason` `error`
- **THEN** the mod submits the hand-off and does not acknowledge `d-7a21` until the hand-off turn completes

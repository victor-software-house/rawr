# Proposal

## Why

A Rex server restart or upgrade cuts Claude Code turns, background tasks and monitors without warning.
Today the only safe restart is to check every pane by hand and wait.

The user gains `rawr restart`: it asks each session to finish its step and write a hand-off, waits until every pane is idle, then restarts the Rex server.
`rawr drain` asks for the same wind-down without a restart.

This change builds on the daemon, mod and Rex client in [rex-supervisor][supervisor], split out to keep each change within its budget.

## What Changes

1. `rawr drain` writes one request that every session's mod delivers once, as a prompt to an idle session or a notice to a busy one.
2. A busy session's foreground Bash or Agent call moves to the background at once, through `rawr drain background`, so the request reaches the session at its next step.
3. A turn still running after `drain.interrupt` is cancelled with `$.turn.abort`, and the hand-off follows.
4. `rawr restart` waits until every live Claude block has acknowledged and settled, within `restart.wait`, then stops and starts the Rex server.
   `f`, `--force` or a second `rawr restart` skips the wait, and Esc or `ctrl+c` cancels it.

## Capabilities

### New Capabilities

- `drain`: `rawr drain`, `rawr drain ack`, `rawr drain background`, and what the mod does with a drain.
- `restart`: `rawr restart`, its wait and the drain it closes.

### Modified Capabilities

None.

## Impact

1. Modules: new `src/daemon/drain.rs` and `src/daemon/restart.rs`, and the mod's drain handling under `plugin/`.
2. Dependencies: none beyond rex-supervisor's.

[supervisor]: ../rex-supervisor/proposal.md

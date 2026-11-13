# Proposal

## Why

rawr brings Claude Code sessions back into their Rex panes.
Before a restart, no command shows what the agents in Rex are doing.
A Rex server restart or upgrade then cuts turns, background tasks and monitors without warning.

The user gains:

1. `rawr status`: every Claude Code session running in Rex, per pane, with the words Claude Code's agents view uses.
2. `rawr restart`: asks each session to finish its step and write a hand-off, waits until every pane is idle, then restarts the Rex server.

## What Changes

1. `rawr daemon`, a long-running process outside every Rex pane, started by `brew services`.
   It follows each Rex pane's program status, owns the index and its cleanup, and runs drains and restarts.
2. A Claude Code plugin in this repository, installed by `rawr setup`, holding one mod.
   The mod records each session under its Rex pane and carries drain requests into the session.
   It replaces the `SessionStart` hook in `settings.json`.
3. `rawr status`, `rawr drain` and `rawr restart`.
4. The picker's rows take Claude Code's status words and glyphs from Rex's program status.
5. The picker layout, the config file and the index cleanup on read come first, in [picker-and-index][picker-and-index]; this change reads the drain and restart waits from that config file.
6. The Homebrew formula, the shell line and `rawr setup` come last, in [setup-and-tap][setup-and-tap].

## Capabilities

### New Capabilities

- `supervisor`: `rawr daemon`, `rawr status`, `rawr drain` and `rawr restart`.
- `claude-plugin`: the plugin and mod `rawr setup` installs, and what the mod does in a session.

### Modified Capabilities

- `pane-index`: the daemon prunes a pane's file once a confirmed Rex listing leaves the pane out.
- `session-picker`: the status words.

## Impact

1. Modules: `src/index.rs`, `src/picker/*`, `src/session.rs`, `src/main.rs`, and new `src/daemon/*` and `src/rex/*`.
2. New files: `.claude-plugin/marketplace.json` and `plugin/` with the mod's TypeScript and tests.
3. Dependencies: a WebSocket client for Rex's control protocol (design decision 8).

[picker-and-index]: ../picker-and-index/proposal.md
[setup-and-tap]: ../setup-and-tap/proposal.md

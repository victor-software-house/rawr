# Proposal

## Why

rawr brings Claude Code sessions back into their Rex panes, but only once the user has wired it up by hand.
Today that takes three pieces:

- a hook line in Claude Code's settings;
- a snippet in the shell's startup file;
- rawr somewhere on `PATH`.

Before a restart, no command shows what the agents in Rex are doing.
A Rex server restart or upgrade then cuts turns, background tasks and monitors without warning.

The picker also falls short.
Its search line sits at the top, and Esc leaves no hint of how to come back.
The history kept per pane is a number fixed in the code, with no way to see less or more.

The user gains:

1. One install, `brew install victor-software-house/tap/rawr`, then `rawr setup`, after which every piece is in place and `rawr setup status` reports each one as current.
2. `rawr status`: every Claude Code session running in Rex, per pane, with the words Claude Code's agents view uses.
3. `rawr restart`: asks each session to finish its step and write a hand-off, waits until every pane is idle, then restarts the Rex server.
4. A picker laid out like Claude Code's own screen, with the search box at the bottom, a short list that expands on request, and a hint after Esc.
5. An index that cleans itself, bounded by a setting, never by a number fixed in the code.

## What Changes

1. A Homebrew tap, `victor-software-house/homebrew-tap`, with a `rawr` formula that installs the binary, completions, and a `brew services` service for `rawr daemon`.
2. `rawr daemon`, a long-running process outside every Rex pane, started by `brew services`.
   It follows each Rex pane's program status, owns the index and its cleanup, and runs drains and restarts.
3. A Claude Code plugin in this repository, installed by `rawr setup`, holding one mod.
   The mod records each session under its Rex pane and carries drain requests into the session.
   It replaces the `SessionStart` hook in `settings.json`.
4. `rawr status`, `rawr drain` and `rawr restart`.
5. The picker redesign: Claude Code's row layout and status words, the search box at the bottom, `ctrl+o` to show every remembered session, and a one-line hint after Esc.
6. A user config file, `$XDG_CONFIG_HOME/rawr/config.yaml`, holding the history kept per pane, how many rows show before `ctrl+o`, and the drain wait.
7. `rawr setup`, `rawr setup status` and `rawr setup remove`, after `rex skill setup`: each reports what it installed and touches nothing it did not install.

## Capabilities

### New Capabilities

- `supervisor`: `rawr daemon`, `rawr status`, `rawr drain` and `rawr restart`.
- `claude-plugin`: the plugin and mod `rawr setup` installs, and what the mod does in a session.
- `setup`: what `rawr setup` installs, reports and removes.
- `config`: the user config file and its keys.

### Modified Capabilities

- `pane-index`: entries whose transcript is gone are dropped.
  The history per pane comes from config, and the daemon prunes when Rex removes a pane.
- `session-picker`: the layout, status words, progressive disclosure and Esc hint.
- `install` and `shell-integration` (install-and-shells): Homebrew becomes the main install, `rawr setup` replaces `rawr setup claude`, and the shell snippet becomes optional.

## Impact

1. Modules: `src/index.rs`, `src/picker/*`, `src/session.rs`, `src/setup.rs`, `src/main.rs`, and new `src/daemon/*`, `src/rex/*`, `src/config.rs`.
2. New files: `.claude-plugin/marketplace.json` and `plugin/` with the mod's TypeScript and tests.
3. A new repository: `victor-software-house/homebrew-tap`.
4. Dependencies: a Rex API client (see the design's open point), YAML config through ctl-core's `input` feature.

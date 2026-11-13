# Proposal

## Why

rawr brings Claude Code sessions back into their Rex panes, but only once the user has wired it up by hand.
Today that takes three pieces:

- a hook line in Claude Code's settings;
- a snippet in the shell's startup file;
- rawr somewhere on `PATH`.

The user gains one install, `brew install victor-software-house/tap/rawr`, then `rawr setup`, after which every piece is in place and `rawr setup status` reports each one as current.

This change is the last part of [rex-supervisor][supervisor], split out to keep each change within its budget.
It installs the plugin and service that change builds.

## What Changes

1. A Homebrew tap, `victor-software-house/homebrew-tap`, with a `rawr` formula that installs the binary, completions, and a `brew services` service for `rawr daemon`.
2. `rawr init <shell>`, which prints the shell hook that offers a restored pane its sessions.
3. `rawr setup`, `rawr setup status` and `rawr setup remove`, after `rex skill setup`: each reports what it installed and touches nothing it did not install.

## Capabilities

### New Capabilities

- `setup`: what `rawr setup` installs, reports and removes.

### Modified Capabilities

- `install` and `shell-integration` (install-and-shells): Homebrew becomes the main install, `rawr setup` replaces `rawr setup claude`, and the shell line printed by `rawr setup` offers restored panes their sessions in every supported shell.

## Impact

1. Modules: `src/setup.rs`, `src/main.rs`.
2. A new repository: `victor-software-house/homebrew-tap`.
3. Dependencies: ctl-core's shell detection (CTC-057).

[supervisor]: ../rex-supervisor/proposal.md

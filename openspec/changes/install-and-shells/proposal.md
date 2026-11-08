# Proposal

## Why

rawr only helps once two hooks are in place: one in the shell, to offer a restored pane its sessions, and one in Claude Code, to record them.
Today both are snippets in the README, for zsh only, and there is no way to install rawr but building it.
Installing must not touch the user's shell files or settings unasked, and must not add a process to every shell start for those who keep their configuration in files.

## What Changes

1. `rawr init <bash|zsh|fish>` prints the shell integration for that shell: the one-time offer in a restored Rex pane.
   The text is fixed for a given rawr version, so it can be evaluated at start or saved once into a dotfiles repository.
2. `rawr setup claude` adds the `SessionStart` hook that runs `rawr record` to Claude Code's user settings, and only when the user runs it; `--dry-run` shows the change.
3. `rawr completion <bash|zsh|fish>` prints static completions, from ctl-core's shared completion verb.
4. Releases ship `darwin-arm64`, `linux-x64` and `linux-arm64` tarballs, each with a `.sha256`.
5. Three ways to install, none of which edits a shell file:
   - `curl -fsSL https://github.com/victor-software-house/rawr/releases/latest/download/install.sh | sh`
   - `brew install victor-software-house/tap/rawr`
   - `mise use -g github:victor-software-house/rawr`

   Each ends by printing the one line to add for the user's shell.

## Capabilities

### New Capabilities

- `shell-integration`: what `init`, `setup claude` and `completion` print or change, per shell.
- `install`: the release targets and what each installer does and leaves alone.

### Modified Capabilities

None.

## Impact

1. Modules: `src/main.rs` (the `init`, `setup` and `completion` commands), a new `src/shell.rs` (per-shell scripts and quoting), a new `src/claude_settings.rs` (the hook entry), `install.sh`, `.ctl/ver.yaml` (targets).
2. A new public repository, `victor-software-house/homebrew-tap`, holds the formula; each release updates it.
3. Depends on a ctl-core release with a `completion` verb; until then `rawr completion` is not offered.

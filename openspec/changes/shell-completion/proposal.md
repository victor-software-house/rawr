# Proposal

## Why

rawr has no shell completion, so its verbs and their flags have to be typed in full.
ctl-core, which rawr already builds on, ships a `completion <SHELL>` verb that other ctl CLIs use.

## What Changes

1. `rawr completion bash|zsh|fish` prints a short registration script for that shell.
2. The script holds no grammar: at each Tab the shell runs the installed `rawr` again, and ctl-core answers with the candidates before parsing.
   Candidates therefore always match the installed binary.
3. The script prints plain, with no colour and never hidden by `--quiet`, so `source <(rawr completion zsh)` loads it.

## Capabilities

### New Capabilities

- `shell-completion`: the `completion` verb and the candidates it serves.

### Modified Capabilities

None.

## Impact

1. Modules: `src/main.rs`.
2. Dependencies: ctl-core's `completion` feature, which brings `clap_complete`.

# Spec Delta

## Purpose

Give each supported shell and Claude Code the hooks rawr needs, printed or written only when the user asks.

## ADDED Requirements

### Requirement: Print the shell integration

`rawr init <SHELL>` SHALL print, for `bash`, `zsh` or `fish`, a script that runs `rawr pick --shell <SHELL>` once before the first prompt in a Rex pane and evaluates its output, and that defines a `rawr` function evaluating the output of `rawr find`.

#### Scenario: zsh

- **WHEN** the user runs `rawr init zsh`
- **THEN** stdout holds a zsh script that uses `add-zsh-hook precmd`, and two runs print the same bytes

#### Scenario: An unknown shell

- **WHEN** the user runs `rawr init tcsh`
- **THEN** rawr prints a usage error naming `bash`, `zsh` and `fish`, and exits 2

### Requirement: Quote for the shell

`pick` and `find` SHALL quote the printed folder and session ID for the shell named by `--shell`, defaulting to POSIX quoting.

#### Scenario: fish

- **WHEN** the chosen session started in `/home/dev/code/it's shop` and the shell is `fish`
- **THEN** the command reads `cd -- '/home/dev/code/it\'s shop' && claude --resume '<session id>'`

### Requirement: Add the Claude Code hook on request

`rawr setup claude` SHALL add a `SessionStart` hook running `rawr record` to Claude Code's user settings when none is there, keep every other setting unchanged, and report `added` or `unchanged`; with `--dry-run` it SHALL change nothing.

#### Scenario: Run twice

- **WHEN** the user runs `rawr setup claude` twice
- **THEN** the first run reports `added`, the second `unchanged`, and the settings hold one such hook

### Requirement: Print completions

`rawr completion <SHELL>` SHALL print a static completion script for `bash`, `zsh` or `fish`.

#### Scenario: bash

- **WHEN** the user runs `rawr completion bash`
- **THEN** stdout holds a bash completion script that offers `record`, `pick`, `find`, `init`, `setup` and `completion`

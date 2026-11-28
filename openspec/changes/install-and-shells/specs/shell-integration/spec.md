# Spec Delta

## Purpose

Give each supported shell and Claude Code the hooks rawr needs, printed or written only when the user asks.

## ADDED Requirements

### Requirement: Print the shell integration

`rawr init <SHELL>` SHALL print, for `bash`, `zsh` or `fish`, a script that runs `rawr resume` once before the first prompt in a Rex pane and, in a Rex pane, defines one function per supported agent that runs `rawr <agent>` with its arguments, unless a function of that name exists; it SHALL remove an alias of that name first, and SHALL do nothing when `rawr` is not on `PATH`.

#### Scenario: zsh

- **WHEN** the user runs `rawr init zsh`
- **THEN** stdout holds a zsh script that uses `add-zsh-hook precmd`, and two runs print the same bytes

#### Scenario: Agent functions inside Rex

- **WHEN** a zsh with `REX_BLOCK` set and an alias `claude` evaluates `rawr init zsh`
- **THEN** `claude --model opus` runs `rawr claude --model opus`

#### Scenario: A function of that name exists

- **WHEN** a zsh in a Rex pane defines its own function `claude`, then evaluates `rawr init zsh`
- **THEN** `claude` still runs the user's function

#### Scenario: rawr was removed

- **WHEN** a zsh in a Rex pane evaluates the saved script and `rawr` is not on `PATH`
- **THEN** `claude` runs the user's own `claude`

#### Scenario: Outside Rex

- **WHEN** a zsh without `REX_BLOCK` evaluates `rawr init zsh`
- **THEN** `whence -w claude` reports a command, not a function

#### Scenario: An unknown shell

- **WHEN** the user runs `rawr init tcsh`
- **THEN** rawr prints a usage error naming `bash`, `zsh` and `fish`, and exits 2

### Requirement: Add the Claude Code hook on request

`rawr setup claude` SHALL add a `SessionStart` hook running `rawr record` to Claude Code's user settings when none is there, keep every other setting unchanged, and report `added` or `unchanged`; with `--dry-run` it SHALL change nothing.

#### Scenario: Run twice

- **WHEN** the user runs `rawr setup claude` twice
- **THEN** the first run reports `added`, the second `unchanged`, and the settings hold one such hook

### Requirement: Print completions

`rawr completion <SHELL>` SHALL print ctl-core's completion script for `bash`, `zsh` or `fish`.

#### Scenario: bash

- **WHEN** the user runs `rawr completion bash`
- **THEN** stdout holds a bash completion script that offers `record`, `daemon`, `status`, `init`, `setup` and `completion`

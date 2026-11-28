# Spec Delta

## Purpose

Install, report and remove everything rawr needs after the binary, and nothing else.

## ADDED Requirements

### Requirement: Setup reports each piece

`rawr setup status` SHALL report the plugin, its stored binary path, the service and any old `SessionStart` hook as `current`, `outdated`, `missing` or `foreign`, and SHALL change nothing.

#### Scenario: A machine with only the old hook

- **WHEN** `~/.claude/settings.json` holds a `SessionStart` hook running `rawr record` and the plugin is not installed
- **THEN** the plugin reads `missing` and the old hook reads `outdated`

### Requirement: Remove only what setup installed

`rawr setup remove` SHALL remove the plugin, its stored path and the service it started, and SHALL leave every other hook, plugin and setting as it was.

#### Scenario: Another plugin is installed

- **WHEN** another plugin is installed beside rawr's and `rawr setup remove` runs
- **THEN** the other plugin is still installed

### Requirement: Setup writes one marked block per file

`rawr setup` SHALL write the shell line and the Rex module's `require` line each between rawr's start and end markers, and a second run SHALL leave each file with one such block.

#### Scenario: Run twice

- **WHEN** the user runs `rawr setup --yes` twice with zsh as the shell
- **THEN** `~/.zshrc` holds one block between `# >>> rawr >>>` and `# <<< rawr <<<`, and `~/.config/rex/init.lua` holds one `require("rawr")` block

### Requirement: The checklist is asked once

When no setup state exists, the daemon SHALL give the checklist to the first rawr window that connects, and every other window SHALL wait until it is answered.

#### Scenario: Three windows after an install

- **WHEN** rawr was installed with mise and three rawr windows open at once
- **THEN** one window shows the checklist, and after the answer all three draw their panes

### Requirement: Setup status reports agents that bypass the launcher

`rawr setup status` SHALL report each supported agent whose name, in the user's shell, is a function other than rawr's, or an alias that does not reach rawr's function.

#### Scenario: An alias that names a path

- **WHEN** the user's zsh has `alias claude=~/.claude/local/claude`
- **THEN** `rawr setup status` reports that `claude` bypasses the launcher

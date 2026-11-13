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

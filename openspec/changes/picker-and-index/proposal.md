# Proposal

## Why

The picker falls short.
Its search line sits at the top, and Esc leaves no hint of how to come back.
The history kept per pane is a number fixed in the code, with no way to see less or more.
The index also grows without bound: an entry stays after Claude Code has deleted its transcript, so the picker offers sessions that cannot resume.

The user gains:

1. A picker laid out like Claude Code's own screen, with the search box at the bottom, a short list that expands on request, and a hint after Esc.
2. An index that cleans itself, bounded by a setting, never by a number fixed in the code.

This change is the first part of [rex-supervisor][supervisor], split out to keep each change within its budget.
The daemon's pruning against Rex's listing stays there.

## What Changes

1. The picker redesign: the search box at the bottom, `ctrl+o` to show every remembered session, and a one-line hint after Esc.
2. A user config file, `$XDG_CONFIG_HOME/rawr/config.yaml`, holding the history kept per pane, how many rows show before `ctrl+o`, and the drain and restart waits that rex-supervisor reads.
3. Index entries carry `recorded_at` and `transcript_seen`, and reading a pane's index drops the entries that can no longer resume.

## Capabilities

### New Capabilities

- `config`: the user config file and its keys.

### Modified Capabilities

- `pane-index`: entries whose transcript is gone are dropped, and the history per pane comes from config.
- `session-picker`: the layout, progressive disclosure and Esc hint.

## Impact

1. Modules: `src/config.rs` (new), `src/index.rs`, `src/picker/*`, `src/main.rs`.
2. Dependencies: YAML config through ctl-core's `input` feature, and `garde` for its validation derive.

[supervisor]: ../rex-supervisor/proposal.md

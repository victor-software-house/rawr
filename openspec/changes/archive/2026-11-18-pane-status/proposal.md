# Proposal

## Why

After a Rex server restart, every restored pane that has a session to offer opens the picker.
Panes in background tabs give no sign of it, so a session waits until someone happens to open its tab.

## What Changes

1. While the picker is open, `rawr pick` and `rawr find` report the pane as blocked on a question through the Rex Program Status Protocol (OSC 7501), under the record ID `rawr`.
   Rex shows this as a needs-attention dot on the pane's tab and in Activity.
2. The record is cleared whenever the picker gives the terminal back: on a choice, a dismissal, an error or a panic.
3. Terminals that do not know OSC 7501 ignore the sequence.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `session-picker`: the open picker marks its pane as waiting for the user.

## Impact

1. Modules: `src/picker/terminal.rs`.
2. Dependencies: none.

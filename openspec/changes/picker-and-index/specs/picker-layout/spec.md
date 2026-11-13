# Spec Delta

## Purpose

Lay the picker out like Claude Code's own screen, show a few sessions at a time, and leave a way back after a dismissal.

## ADDED Requirements

### Requirement: Few rows, expand on request

The picker SHALL show at most `picker.rows` sessions and, only when more sessions exist, one row `+N earlier · ctrl+o`; `ctrl+o` SHALL show and hide the rest.

#### Scenario: Seven sessions, three rows

- **WHEN** a pane's index lists seven sessions and `picker.rows` is 3
- **THEN** three rows show, then `+4 earlier · ctrl+o`, and `ctrl+o` shows all seven

#### Scenario: No more sessions than rows

- **WHEN** a pane's index lists two sessions and `picker.rows` is 3
- **THEN** two rows show, and no `earlier` row

#### Scenario: As many sessions as rows

- **WHEN** a pane's index lists three sessions and `picker.rows` is 3
- **THEN** three rows show, and no `earlier` row

### Requirement: Search box at the bottom

The picker SHALL draw its search box at the bottom of the screen, under the list, with the key line under the box.

#### Scenario: Typing a search

- **WHEN** the user types `shop` in the picker
- **THEN** the text appears in the bottom box and the list above narrows to matching sessions

### Requirement: A hint after a dismissal

After Esc, `q` or `ctrl+c`, `rawr pick` SHALL print one line on stderr naming `rawr pick` and `rawr find`.

#### Scenario: The user presses Esc

- **WHEN** the picker is open and the user presses Esc
- **THEN** stderr ends with `rawr pick reopens this pane's sessions · rawr find searches all`

# Spec Delta

## Purpose

Keep the pane index bounded without a number fixed in the code.

## ADDED Requirements

### Requirement: Drop entries whose transcript is gone

rawr SHALL drop an index entry whose transcript file existed and no longer exists, or that has had no transcript file for more than 24 hours since it was recorded, whenever it reads that pane's index.

#### Scenario: Claude Code swept an old transcript

- **WHEN** a pane's index lists session `6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33` and its transcript file is gone
- **THEN** the picker does not offer it, and the index no longer lists it

### Requirement: Bound each pane by config

A pane index SHALL hold at most `history.per_pane` sessions, newest first.

#### Scenario: A smaller bound

- **WHEN** `history.per_pane` is 4 and a fifth session starts in a pane
- **THEN** the oldest of the five is dropped

#### Scenario: A session just started

- **WHEN** a session was recorded a minute ago and Claude Code has not written its transcript yet
- **THEN** the index keeps the entry

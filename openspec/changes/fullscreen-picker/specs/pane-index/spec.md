# Spec Delta

## Purpose

Record which Claude Code sessions started in which Rex pane, so a restored pane can offer them again.

## ADDED Requirements

### Requirement: Record a session per pane

`rex-resume record` SHALL read the Claude Code `SessionStart` hook input on stdin and `REX_BLOCK` from the environment, and add the session to that pane's index file `$XDG_STATE_HOME/rex-agents/<block id>.json`, or `~/.local/state/rex-agents/<block id>.json` when `XDG_STATE_HOME` is unset.

#### Scenario: A session starts in a Rex pane

- **WHEN** the hook input is `{"session_id":"6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33","transcript_path":"/home/dev/.claude/projects/-home-dev-code-shop/6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d33.jsonl","cwd":"/home/dev/code/shop"}` and `REX_BLOCK=block:0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08`
- **THEN** `rex-agents/0190f3a2-7c41-7e10-8b2d-5d9e6a4c1f08.json` lists that session first, with its folder and transcript path

### Requirement: Keep the five newest sessions

A pane index SHALL hold at most five sessions, newest first, with no session listed twice.

#### Scenario: A sixth session starts

- **WHEN** a pane index lists five sessions and a new one starts in that pane
- **THEN** the new session is first and the oldest is dropped

#### Scenario: A listed session starts again

- **WHEN** a session already in the index starts again in the same pane
- **THEN** it moves to the front and the index still lists it once

### Requirement: One index file per live pane

After recording, `record` SHALL delete the index files of blocks the Rex server no longer lists, and SHALL delete nothing when any Rex listing fails.

#### Scenario: A pane was closed

- **WHEN** the Rex server lists every session and block, and one index file names a block it does not list
- **THEN** that file is deleted and the others stay

#### Scenario: The Rex server does not answer

- **WHEN** `rex ls --json` fails
- **THEN** no index file is deleted

### Requirement: Accept only Rex block IDs

`record` SHALL accept a block ID only when, after an optional `block:` prefix, it holds only hexadecimal digits and `-`, so the ID cannot name a path outside the index folder.

#### Scenario: A block ID that holds a path

- **WHEN** `REX_BLOCK=../../etc/passwd`
- **THEN** `record` writes nothing and exits non-zero

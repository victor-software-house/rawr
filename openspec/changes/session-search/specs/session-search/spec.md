# Spec Delta

## Purpose

Let the picker find any resumable local Claude Code session by the words the user typed in it.

## ADDED Requirements

### Requirement: Search every resumable local session

The picker SHALL search the sessions named in `~/.claude/history.jsonl` whose transcript exists under `~/.claude/projects/` and which no running Claude Code process holds.

#### Scenario: A session whose transcript is gone

- **WHEN** the history names sessions `6f1c2a90` and `b8e4d7c2` and only `b8e4d7c2.jsonl` exists under `~/.claude/projects/-home-dev-code-shop/`
- **THEN** a search can find `b8e4d7c2` and never `6f1c2a90`

### Requirement: Match every word

A session SHALL match a query when each whitespace-separated word of the query, ignoring case, occurs in one of its prompts, its title, its folder or its session ID.

#### Scenario: Two words in different prompts

- **WHEN** one prompt of a session reads `Fix the login page` and a later one reads `the redirect loops`, and the user types `LOGIN redirect`
- **THEN** that session is listed

#### Scenario: One word missing

- **WHEN** the user types `login checkout` and no prompt, folder or ID of a session contains `checkout`
- **THEN** that session is not listed

### Requirement: Order the results

The results SHALL list the pane's own matching sessions first, in the pane's order, then every other match by its latest prompt, newest first, each session once.

#### Scenario: A pane session and an older session match

- **WHEN** the pane's index lists `b8e4d7c2`, and both `b8e4d7c2` and a session with a newer prompt match the query
- **THEN** `b8e4d7c2` is listed first

### Requirement: Open the search line

In the picker, `/` SHALL open a search line whose text filters the list on each key press; `Backspace` SHALL delete a character, `Ctrl+U` SHALL clear the line, and `Esc` SHALL close it and show the pane's sessions.

#### Scenario: Typing on the search line

- **WHEN** the user presses `/`, then types `q`
- **THEN** the picker stays open and filters by `q` instead of closing

#### Scenario: Leaving the search line

- **WHEN** the search line is open and the user presses `Esc`
- **THEN** the search line closes and the list shows the pane's sessions; a second `Esc` closes the picker

#### Scenario: Leaving the search line with no pane sessions

- **WHEN** `rawr find` runs outside a Rex pane and the user presses `Esc`
- **THEN** the picker closes, prints nothing and exits 1

### Requirement: Find from any shell

`rawr find [WORDS…]` SHALL open the picker on the search line with `WORDS` as the query, inside or outside a Rex pane, and SHALL resume the chosen session as `pick` does.

#### Scenario: Find outside Rex

- **WHEN** `REX_BLOCK` is unset and the user runs `rawr find login`, then presses `Enter`
- **THEN** the first match resumes in its folder, as `pick` resumes a session

### Requirement: Titles from the transcript's end

Searching SHALL read `~/.claude/history.jsonl`, the session records under `~/.claude/sessions/`, one listing of `~/.claude/projects/*/`, and the last 64 KiB of each listed session's transcript, or its last 1 MiB when those 64 KiB hold no line break, once per catalog load.
A row's title SHALL be the last title the user set in its transcript, else the last one Claude Code generated, else the session's first prompt that is not a slash command.

#### Scenario: A large transcript

- **WHEN** a matching session's transcript is 50 MB and its last 64 KiB hold a generated title `Weekly notes draft`
- **THEN** the search reads only that end of it, and the row's title is `Weekly notes draft`

#### Scenario: A transcript with no title row

- **WHEN** a session's transcript holds no title row and its first prompts are `/config` and `Fix the login page`
- **THEN** the row's title is `Fix the login page`

# Design

## Context

Claude Code appends one JSON line per prompt the user types to `~/.claude/history.jsonl`, with `display` (the prompt), `project` (the folder Claude Code started in), `sessionId` and `timestamp`.
Transcripts live at `~/.claude/projects/<folder slug>/<session id>.jsonl`, and Claude Code deletes old ones on its own schedule, so the history names many sessions that can no longer be resumed.

## Decisions

1. **The prompt history is the search index.**
   It is one file, read once, and it already holds the words a user remembers a session by: what they typed.
   Alternative: read each transcript for its title and messages.
   It lost: transcripts run to tens of megabytes each, and reading a hundred of them is seconds, not milliseconds.
2. **One directory listing decides what is resumable.**
   The catalog lists `~/.claude/projects/*/` once and keeps the sessions whose `<session id>.jsonl` is there and which no live Claude Code process holds.
   Alternative: derive each transcript's path from the folder slug.
   It lost: the slug rule belongs to Claude Code and shortens long paths, and a listing finds the file whatever the rule.
3. **A session's title is Claude Code's title, else its first prompt.**
   Search rows show the title the pane rows show: the last one the user set, else the last one Claude Code generated, found in Claude Code's order once when the catalog loads (fullscreen-picker design, decision 10).
   A session whose transcript holds no title row shows the first line of its first prompt that is not a slash command such as `/config`, up to 120 characters, ending in `…` when cut.
   The title is searched as well as the prompts.
   Alternative: always the first prompt.
   It lost: first prompts such as `hi` or a line pointing at a brief file say nothing about the session, and they made the list hard to read.
   Reading the ends costs at most 64 KiB at each end per session, once per catalog load, not per key press: 17.2 MB for the last 64 KiB of 272 local transcripts, read in about 5 ms from a warm cache.
   Alternative: read titles only for the rows on screen, or cache them by path and modification time.
   It lost: a title found only for shown rows could not be searched, and 3 ms does not repay a cache.
4. **Words match by substring, all of them, ignoring case.**
   A query of `login redirect` matches a session whose prompts, folder or ID contain both words.
   Alternative: fuzzy subsequence matching, as fzf does.
   It lost: over long prompts nearly every short query matches every session, and the order stops meaning anything.
5. **`/` opens the search line; the other keys keep their meaning until then.**
   On the search line, printable keys edit the query, `Backspace` deletes, `Ctrl+U` clears, and `Esc` closes the line and shows the pane's sessions again.
   Arrows, `PgUp`, `PgDn`, `Enter` and the mouse still move and resume.
   Alternative: every printable key starts a search, as fzf does.
   It lost: `j`, `k`, `q`, `g`, `G` and `1` to `9` are the picker's keys, and a restored pane should keep them.
6. **The catalog loads on the first `/`, not at start.**
   `pick` runs in every new shell in a Rex pane, so it reads nothing it does not need; `find` loads the catalog at once because it starts on the search line.
7. **`find` resumes the chosen session, as `pick` does.**
   It replaces its process with `claude --resume` in the session's folder, so it works from any shell with no wrapper.

## Sources

- Claude Code [settings and files][claude-settings], for `~/.claude` and `CLAUDE_CONFIG_DIR`.

[claude-settings]: https://docs.anthropic.com/en/docs/claude-code/settings

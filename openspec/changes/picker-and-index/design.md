# Design

## Context

rawr keeps a short history of Claude Code sessions per Rex pane and offers it in a full-screen picker.
The tools that restore agent sessions keep one session per pane ([rex-supervisor's design][supervisor] compares them); tmux-resurrect bounds its saves by count and age.
Claude Code deletes a transcript with no message after `cleanupPeriodDays`, 30 days by default ([settings][cc-settings]), so an index entry can outlive what it points at.

## Decisions

1. **The picker follows Claude Code's screen.**
   - The list is on top.
     The search box sits at the bottom, drawn like Claude Code's prompt box, with the pane's name in its top rule and the key line under it.
   - Each row holds the title, the folder, and a right-aligned age, after the agents view's columns.
     rex-supervisor adds the glyph and status word in front, once rawr reads Rex's program status.
   - The newest rows show first, up to `picker.rows` (default 3).
     When more sessions exist, a dim `+N earlier · ctrl+o` row stands for the rest, and `ctrl+o` expands and collapses them, as it does for detail in Claude Code.
   - Esc, `q` and `ctrl+c` leave one dim line in the shell: `rawr pick reopens this pane's sessions · rawr find searches all`.
2. **The history per pane is a setting.**
   `history.per_pane` (default 5) bounds each pane's index.
   `picker.rows` bounds what shows before `ctrl+o`.
   Both live in `$XDG_CONFIG_HOME/rawr/config.yaml`, read through ctl-core's `input` loader, so a bad value names its file and line.
   The same file holds rex-supervisor's drain and restart waits, so the user has one place for every setting.
3. **The index cleans itself on read.**
   - Each entry carries `recorded_at`, set when `rawr record` writes it, and `transcript_seen`, set the first time rawr finds its transcript.
   - An entry goes when `transcript_seen` is set and the transcript is gone.
   - An entry with no transcript goes 24 hours after it was recorded, so a session that has not written its first message yet stays.
   - The per-pane bound is `history.per_pane`.
   - Whole pane files go only through the daemon's pruning in rex-supervisor, which waits for a confirmed listing from Rex.
4. **`AGENTS.md` rule 3 changes.**
   After a dismissal, `pick` prints the one hint line on stderr.

[supervisor]: ../rex-supervisor/design.md
[cc-settings]: https://code.claude.com/docs/en/settings

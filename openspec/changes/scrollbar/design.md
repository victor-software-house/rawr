# Design

## Context

1. `view::scrollbar` draws ratatui's `Scrollbar` in four lists: panes, sessions, search results and the session's tabs.
   It draws a `┃` thumb in the list's last column, with a blank track, whenever the list does not fit.
2. No hit is recorded for it, so a press there does nothing.
3. The app already drags a column's edge (`Target::Edge`, `App::dragging`), and redraws on the frame clock while something moves.
4. The Conversation and Activity tabs hold at most 3,000 entries of a session, and load older and newer ones in pages of 100 relative to an entry they hold.

## Decisions

1. **The geometry follows Claude Code's list scroll bar.**
   - The thumb's size is the share of the list in view, at least one row.
   - The thumb touches an end of the track only when the view is at that end, so a thumb at the top always means the start.
   - Alternative: ratatui's `Scrollbar`, as now.
     It lost: it has no inverse from a thumb row to a list position, which a drag needs.
2. **The thumb can be held.**
   - A press on the thumb grabs it at the row pressed, and a drag keeps that row under the pointer, so the list follows the pointer at once.
   - The thumb is drawn `█`, the whole cell, while the pointer is on the bar or the thumb is held, and `┃` otherwise, so what can be grabbed shows how wide it is.
   - A press on the track pages the list by a view's height toward the press.
   - The pointer shows a hand over the bar, as over other targets.
3. **A scrollbar shows only while it is in use.**
   - It shows while its list scrolls by the wheel, a key or a drag, while the pointer is on its column, and while its thumb is held.
   - 800 ms after the last of these, it fades out over 200 ms in four steps of dimmer colour on the frame clock, and then its column is blank.
   - Its column stays reserved while it is hidden, so text never moves when it shows.
   - A list that fits draws no bar.
   - Alternative: always show it, as now.
     It lost: the maintainer asked for it out of sight (operator, 2026-11-25).
4. **A drag past the held entries jumps.**
   - In the Conversation and Activity tabs, the thumb's position stands for a place in the whole session, by the counts the tabs already show.
   - When a drag ends, or rests for 100 ms, on a place no held entry covers, the window asks the daemon `around` with the list and that place as a share of the transcript's bytes.
   - The share is in millionths from the transcript's first byte.
     The tabs list the newest entry first, so the bottom of the track is share 0, and a drag to the bottom asks for the first calls.
   - The daemon reads the 100 entries of that list nearest that byte, as it reads any page, and answers `around`, a message of its own that names the request it answers.
     Each request carries a number, and only the answer to the newest one lands, so a thumb that moved on drops the older answer.
   - The window keeps its view until the answer lands, then shows the entry at that place at the top.
     The answer joins the held entries with gaps on both sides, which pages fill as for any gap.
   - The daemon cannot count the entries it skips, so the window shares the entries not held there between the two new gaps by the bytes each side spans.
     The counts of the whole session stay as they were.
   - While the thumb is held, or a place is waiting for its entries, the window asks for no older or newer page, so a drag through the held entries loads nothing on its way.
   - Alternative: load every page between the view and the place.
     It lost: the 3,000-entry bound would drop each page soon after it lands, and a long session would take seconds.

# Design

## Context

1. `src/app.rs` draws once per batch of input, and every 16 ms only while a column moves.
   A wheel report moves its list by `WHEEL` (2) lines at once, in the frame that reads it.
2. The maintainer's own trackpad scroll, recorded on 2026-11-25, sent 1,083 reports at about 120 a second in 8 gestures.
   Each gesture rose to 20 to 40 reports in 50 ms within about 100 ms, then fell by about 15% every 50 ms.
3. `view::said` wraps every entry and renders the markdown of every longer text on every frame, folded entries included.
4. `Scroll::top` is a line number from the top.
   Lists are newest first, so a new entry pushes every line below it down while the reader is scrolled.
5. The daemon reads a transcript from its newest line back to its first and sends `Facts` with the last `KEEP` (300) entries of the conversation and of the activity.
   `entry_key` hashes an entry's time and text, and the open entries are kept by that key.

## Decisions

1. **A fixed frame clock while anything moves.**
   - While a wheel report waits, a list moves or a spinner turns, the loop draws on a clock of 125 frames a second, a frame every 8 ms.
     125 Hz is the polling rate of a USB mouse, so a held wheel never outruns the clock.
   - The clock keeps an even beat: a frame a key draws outside it moves the next tick one period on, and a wake after a rest takes the next tick no sooner than a period after the last.
   - At rest the loop sleeps until input, as now.
   - Alternative: draw as soon as input arrives, as now.
     It lost: frames then land wherever the reports land, so their spacing follows the trackpad's jitter.
2. **Wheel reports wait in a queue and are spread over frames.**
   - Reports that arrive less than 2 ms apart came in one write from the terminal, as a batch.
   - A batch that comes within 2 frames of the one before is spread over the frames until the next is due.
     So batches every other frame move the list in even halves, and batches once a frame move it on the next frame.
   - The first batch of a gesture moves the list on the next frame, so motion starts at once.
     The list stops at most 2 frames after the last report, and the gesture's own slowing gives the motion its tail.
   - Reports read since the last frame that span 2 ms or more are a stream faster than the clock, such as a held wheel, and that frame takes every report queued.
   - A stream as fast as the clock or faster moves the clock's beat into the gap between two reports, fitted to the last 16 reports.
     Without it, a tick that lands on a report takes it or leaves it by a few microseconds of jitter, so a frame takes one report more or fewer than the one before.
     A trackpad at 120 reports a second against a 125 Hz clock is that case.
   - One report still moves 2 lines.
   - Alternative: ease each report over several frames.
     It lost: it adds lag to every gesture, and the trackpad already eases its own motion.
   - Alternative: wheel acceleration, a larger step for reports that follow each other quickly.
     It lost: the trackpad already sends more reports when it moves faster, and the maintainer asked for the motion to follow their hand.
3. **Each entry is laid out once.**
   - A window keeps each entry's lines for the width they were laid out at and its fold state.
     The hover bar and the fold marker are drawn over them, so hovering lays nothing out again.
   - A width change lays out the entries in view first, and the rest in the time left before each next tick.
     Until an entry is laid out, its height counts as the average height of the entries laid out.
   - A frame draws only the entries in view.
   - Alternative: lay out every entry on every frame, as now.
     It lost: its cost grows with the history, which load on demand makes unbounded.
4. **The view keeps its place by entry.**
   - The scroll position is an entry and a line within it.
   - A new entry above the view adds to the lines above it and moves nothing on screen.
   - At the top with nothing scrolled, the list follows the newest entry, as now.
   - Entries are identified by the byte offset of their transcript line and their place within that line.
     The open entries and the scroll anchor use that ID, so an entry stays open while its tool result arrives.

## Risks

1. The frame clock costs CPU while anything moves.
   It runs only while something moves, and task 1.4 measures a frame's cost with 3,000 entries held.
2. Pacing values come from another client's measurements and from one trackpad recording.
   Task 3.1 checks them against the maintainer's own scrolling in Rex.

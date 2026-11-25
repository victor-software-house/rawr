# Design

## Context

1. The daemon reads a transcript from its newest line back to its first and sends `Facts` with the last `KEEP` (300) entries of the conversation and of the activity.
2. The [smooth-scroll][smooth-scroll] change gives each entry a stable ID, lays out each entry once, and keeps the view anchored by entry.

## Decisions

1. **Older entries load in pages.**
   - `Facts` keeps the newest 300 entries of each list, and marks whether older ones exist.
   - A window asks `older` with the transcript, the list, the oldest entry ID it holds, and a count of 100.
     The daemon reads the transcript back from that line until it has 100 entries of that list or reaches the start, and answers `page` with them and whether older ones remain.
   - A tool call in the page whose result is written after the page's last line still shows its result.
     The daemon also reads forward from the page's end until every call in the page has its result, or 256 KiB.
   - The daemon keeps no page, so its memory stays that of the live facts.
   - One page per list is in flight at a time.
   - The page size bounds how long the daemon takes to read a page.
     Its round trip is measured in task 1.4 before the size is fixed.
   - Alternative: the daemon pushes older pages ahead of the view.
     It lost: the daemon does not know where any window's view is.
2. **A page is asked for before the view needs it.**
   - The lead is the lines the view moved, either way, in the last 250 ms, and at least 200.
   - A window asks for the next page once the lines it holds past the view toward the oldest held entry are no more than the lead.
     So at the speed the view moves, the page lands before the view reaches the end of what is held.
   - The view never draws a line it does not hold.
     If it reaches the oldest held entry while a page is in flight, it stops there and moves on when the page lands.
   - At the true start of the session, the list ends at its oldest entry, with nothing drawn below it.
3. **A window holds a bounded number of entries.**
   - A window holds at most 3,000 entries of each list, the newest 300 always among them.
   - Past that, a page that lands drops the entries farthest from the view.
     It drops beyond the lead on the side the view leaves, and never within the view or the lead.
   - Dropped entries load again as pages when the view comes back.
     The daemon reads a newer page the same way, forward from the newest dropped entry's line.
   - Alternative: keep every entry a window ever loaded.
     It lost: a session can hold hundreds of thousands of entries.
4. **The counts and the scrollbar show the whole session.**
   - The daemon counts every entry of each list as it reads the transcript, past the 300 it keeps.
     `Facts` carries the two totals, and the tab shows the total once the whole transcript is read.
     Until then it shows the count so far with a `+`, such as `Activity 300+`.
   - The scrollbar counts the entries not held at the average height of the entries laid out.
     So the thumb does not jump when a page lands.
5. **The `/` filter covers the entries held.**
   - A filter shows the matching entries among those held.
     Pages keep loading as the view nears the oldest match held, so older matches appear as the reader scrolls.
   - Searching the whole transcript stays with `ctrl+f`, whose results already come from the daemon.

[smooth-scroll]: ../smooth-scroll/proposal.md

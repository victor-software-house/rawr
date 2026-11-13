---
rawr: patch
---

The picker puts the session list on top and a search box at the bottom, drawn like Claude Code's prompt box.
Printable keys type into the box, so `/`, `q`, `j`, `k`, `g`, `G` and `1` to `9` no longer act on the list.
`Esc` clears the text, then dismisses; `Ctrl+C` dismisses.
The list shows `picker.rows` sessions, 3 by default, then `+N earlier · ctrl+o`, and `Ctrl+O` shows and hides the rest.
After a dismissal, `rawr pick` and `rawr find` print one line on stderr that names the way back.

# Design

## Context

`open_hit` in `src/app/state.rs` turns Enter on a result, or a click on the selected result, into `Action::GoTo` when a live Claude holds the session, and into `Action::Resume` otherwise.
The panes list comes from the daemon through `set_panes`, sorted by attention, with closed panes last under Closed.
A closed pane already draws as this change needs an opened one to: its session view has Resume and no Go to pane.

## Decisions

1. **Choosing selects.**
   Enter on a result, or a click on any result, chooses it.
   Choosing closes the search, as Esc does, and focuses the session column on the chosen session.
   It returns `Action::None`, so no Claude starts and Rex keeps its pane.
2. **A listed session selects its pane.**
   The first pane, open or closed, whose sessions include the result's session ID is selected, with that session selected in it.
3. **An unlisted session opens a pane.**
   The app keeps the opened sessions in a list of its own.
   `set_panes` adds one `Pane` per opened session that no daemon pane lists: `opened` set, `closed` set, the session as its only session, and a block ID of `opened:` and the session ID.
   Opened panes sort first and draw under an Opened header.
   Choosing an already opened session selects its pane again.
4. **Closing.**
   `x` on a selected opened pane, or a click on the `×` at the right of its row, removes it from the opened list.
   The selection moves to the pane that is first in the list.
   When a daemon pane comes to list an opened session, the opened pane goes, and the selection follows the session to that pane.
5. **Not kept.**
   The opened list lives only as long as the app.

# Design

## Context

1. `view::pane_view` draws the header rows before the tabs: the pane's name and tab, its status pill and message, the session list when one is shown, the title, the folder and session ID, the session's state, and the Go to pane and Resume buttons.
2. A folded column already works as a drawer: `Drawer` slides it out over the others while the pointer is on its rail, with `GRACE` and `AWAY`, eased by `eased` over `span`.
3. Each tab keeps its scroll in `Scroll`, with an anchor entry for Conversation and Activity, so a list can change height and keep its place.

## Decisions

1. **The header folds while the list is scrolled.**
   - The header is folded whenever the shown tab's list is not at its top, and unfolded at the top.
   - Folded, it is one row: the status pill, the pane's name, and the session's title, cut to the width.
   - The tabs row and its rule stay below it, so the list starts three rows from the top of the column.
   - The fold and the unfold move over `span` of the rows they change, eased as a column move is, and the list keeps its anchor entry on its top row.
   - Alternative: fold on any scroll and unfold after a pause.
     It lost: the header would come and go while reading at one place.
2. **The pointer draws the header out.**
   - While the pointer is on the folded row, the full header slides down over the list, as the pane rail's drawer slides over the pane.
   - It shuts when the pointer leaves it and the folded row, with the drawer's `GRACE` and `AWAY`.
   - The list does not move under it, and the header's buttons and session list take clicks while it is out.
   - Alternative: unfold in place, pushing the list down.
     It lost: the rows under the pointer would move as the pointer reached them.
3. **Keys and focus do not fold or unfold it.**
   - Only the list's position decides, so `Home` and a scroll back to the top bring the full header back.

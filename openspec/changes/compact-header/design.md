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
4. **A column that folds or opens slides; it never switches to its rail mid-move.**
   - Today `rails` draws any column narrower than `FULL` as a rail. While the panes drawer shuts or a focus move folds the panes, the list turns into the rail in one frame, and a blank gap then closes (operator, 2026-11-25).
   - While a column's width moves, it is drawn at the width it has when open and cut at its moving edge, so its rows slide with the edge. It draws as a rail only once the move has ended at the rail's width.
   - The rail's marks sit in the same columns as the list's marks, so the last frames of a fold already look like the rail.
   - The header's slide in decision 2 draws the same way, cut at its moving bottom edge.

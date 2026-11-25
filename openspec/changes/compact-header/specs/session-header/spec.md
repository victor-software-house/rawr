# Session header

## ADDED Requirements

### Requirement: The header folds while the list is scrolled

While the shown tab's list is not at its top, the session column's header SHALL fold into one row with the status, the pane's name and the session's title, and the list SHALL take the rows it frees.
At the top of the list, the header SHALL unfold.

#### Scenario: Scrolling the Activity tab folds the header

- **WHEN** the reader scrolls the Activity tab of a long session down from its top
- **THEN** the header folds into one row above the tabs, and the list shows eight more rows than at the top

#### Scenario: Back at the top

- **WHEN** the reader scrolls the list back to its top
- **THEN** the full header returns and the list keeps its first entry on its top row

### Requirement: The pointer draws the folded header out

While the pointer is on the folded header, the full header SHALL slide out over the list without moving it, and SHALL slide back once the pointer leaves it.

#### Scenario: Hovering the folded header

- **WHEN** the header is folded and the pointer moves onto it
- **THEN** the full header is drawn over the top of the list, and its Resume button takes a click
- **AND** the list's rows stay where they were

### Requirement: A folding column slides

While the panes column folds or opens, by its drawer or by a move of focus, its rows SHALL slide with its moving edge, and it SHALL draw as a rail only once the move ends at the rail's width.

#### Scenario: The drawer shuts

- **WHEN** the pointer leaves the open panes drawer
- **THEN** each frame of the shut shows the pane list cut at the moving edge, and no frame shows a rail with a blank gap beside it

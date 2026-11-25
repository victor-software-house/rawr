# Search open

## ADDED Requirements

### Requirement: Choosing a result selects its session

Enter on a search result, or a click on a result, SHALL close the search and show the result's session in the session column.
It SHALL NOT resume the session or switch Rex to a pane.

#### Scenario: A result a pane lists

- **WHEN** the search shows a session that a running pane lists, and the user presses Enter on it
- **THEN** the search closes, and that pane and that session are selected
- **AND** no Claude starts, and Rex keeps the pane it showed

### Requirement: A result no pane lists opens as a pane

A chosen result whose session no pane lists SHALL open as an opened pane at the top of the panes list, under an Opened header.
Its session view SHALL offer Resume and SHALL NOT offer Go to pane.

#### Scenario: Opening an old session

- **WHEN** the user clicks a result whose session no pane lists
- **THEN** the panes list starts with an Opened section that holds it, selected
- **AND** the session column shows its tabs and a Resume button, and no Go to pane button

### Requirement: An opened pane closes

An opened pane SHALL close on `x` while it is selected, or on a click on its close mark.
It SHALL leave the list once a pane from Rex lists its session.

#### Scenario: Closing an opened pane

- **WHEN** an opened pane is selected and the user presses `x`
- **THEN** it leaves the list and the first pane in the list is selected

#### Scenario: Resumed in a pane

- **WHEN** an opened session is resumed and a pane comes to list it
- **THEN** the opened pane leaves the list and that pane is selected

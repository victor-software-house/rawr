# List scrolling

## ADDED Requirements

### Requirement: Frame clock while moving

While a wheel report waits or a list moves, the app SHALL draw on a clock of 125 frames a second, and at rest it SHALL draw only when input or a daemon message arrives.

#### Scenario: A gesture draws evenly spaced frames

- **WHEN** wheel reports arrive at 120 a second for 400 ms
- **THEN** the frames that move the list are 8 ms apart, give or take 1 ms
- **AND** no frame is drawn once the list has stopped and no input arrives

### Requirement: Wheel batches spread over frames

Reports that arrive less than 2 ms apart SHALL count as one batch.
A batch that comes within 2 frames of the one before SHALL be spread evenly over the frames until the next batch is due.
The first batch of a gesture SHALL move the list on the next frame.

#### Scenario: Batches every other frame move in halves

- **WHEN** a batch of 6 reports arrives every 16 ms
- **THEN** the list moves 6 lines on each of the 2 frames between batches, never 12 and then 0

#### Scenario: The first batch moves at once

- **WHEN** a batch of 4 reports arrives after the list has rested for 1 s
- **THEN** the next frame moves the list 8 lines

#### Scenario: The list stops soon after the last report

- **WHEN** the last report of a gesture arrives
- **THEN** the list moves no more than 2 frames later

### Requirement: Streams faster than the clock

Reports read since the last frame that span 2 ms or more SHALL all move the list in that frame.
For a stream as fast as the clock or faster, the clock SHALL move its beat between two reports so each frame takes the same number of reports.

#### Scenario: A trackpad at the clock's rate moves one report a frame

- **WHEN** reports arrive every 8.3 ms with up to 1 ms of jitter for 1 s
- **THEN** every frame that moves the list moves it by one report, 2 lines

### Requirement: Frame cost independent of history

A frame SHALL lay out only entries not yet laid out for the current width and fold state, and SHALL draw only the entries in view.

#### Scenario: Hovering lays out nothing

- **WHEN** the pointer moves from one entry to the next in a list of 3,000 entries
- **THEN** no entry is laid out again

#### Scenario: Opening an entry lays out that entry

- **WHEN** the reader clicks a folded entry
- **THEN** only that entry is laid out again

### Requirement: The view keeps its place

The scroll position SHALL be an entry and a line within it, and a new entry above the view SHALL not move the entries on screen.
At the top with nothing scrolled, the list SHALL follow the newest entry.

#### Scenario: A new entry while scrolled

- **WHEN** the reader has scrolled 40 lines into the Activity tab and Claude calls a tool
- **THEN** the entries on screen stay on the same rows

#### Scenario: A new entry at the top

- **WHEN** the reader has not scrolled the Activity tab and Claude calls a tool
- **THEN** the new call shows as the first entry

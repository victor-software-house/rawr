# List scrollbar

## ADDED Requirements

### Requirement: The thumb can be dragged

A press on a list's scrollbar thumb SHALL grab it, and a drag SHALL keep the grabbed row of the thumb under the pointer and move the list to match.

#### Scenario: Dragging the thumb down

- **WHEN** the reader presses the Activity tab's thumb and drags it 5 rows down
- **THEN** the thumb is drawn 5 rows lower and the list shows the entries that thumb position stands for

### Requirement: A press on the track pages

A press on a scrollbar's track SHALL scroll its list by one view's height toward the press.

#### Scenario: A press below the thumb

- **WHEN** the reader presses the track below the thumb of a list 20 rows tall
- **THEN** the list moves 20 rows down

### Requirement: The bar shows only while in use

A scrollbar SHALL show while its list scrolls, while the pointer is on its column, and while its thumb is held.
800 ms after the last of these it SHALL fade out over 200 ms, and its column SHALL then be blank.
A list that fits SHALL draw no bar.

#### Scenario: The bar fades after a scroll

- **WHEN** the reader scrolls the Activity tab with the wheel and stops
- **THEN** the bar shows while it scrolls, and is gone 1 s after the last step

#### Scenario: Hovering reveals the bar

- **WHEN** the pointer moves onto the last column of a list that does not fit
- **THEN** the bar shows with the thumb drawn as a full cell

### Requirement: A drag past the held entries jumps

In the Conversation and Activity tabs, a drag that ends or rests for 100 ms on a place no held entry covers SHALL load the entries at that place directly, and show them once they land.

#### Scenario: Dragging to the start of a long session

- **WHEN** a window holds the newest 300 of 5,800 calls and the reader drags the Activity thumb to the bottom of the track
- **THEN** the window asks the daemon for the entries at the end of the list's bytes, and shows the first calls of the session when they land
- **AND** it asks for no page between them

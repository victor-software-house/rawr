# Session history

## ADDED Requirements

### Requirement: Older entries load in pages

The daemon SHALL answer `older` for a transcript, a list, an entry ID and a count with up to that many entries of that list from before the entry, and whether older ones remain.
It SHALL read them from the transcript on disk and keep none of them.

#### Scenario: A page of older activity

- **WHEN** a session has 1,000 tool calls and a window that holds the newest 300 asks `older` before the oldest of them, with a count of 100
- **THEN** the daemon answers with calls 301 to 400 counted from the newest, oldest first, and that older ones remain

#### Scenario: The first entries of a session

- **WHEN** a window asks `older` before the second tool call of a session, with a count of 100
- **THEN** the daemon answers with the first call and that no older ones remain

### Requirement: A page that cannot be read

The daemon SHALL answer `older` or `newer` for a transcript it cannot read with a failure that names the reason, never with an empty page.
The window SHALL show the reason as a notice and keep the list open, and SHALL ask again only when the view moves.

#### Scenario: An unreadable transcript

- **WHEN** the transcript is gone when a window asks for a page
- **THEN** the daemon answers with a failure and its reason, and the window shows it
- **AND** the list does not end, and the window asks again only after the view moves

### Requirement: Results across a page's edge

A tool call in a page SHALL show the result written after the page's last line, when it is written within 256 KiB of that line.

#### Scenario: A result just past the page

- **WHEN** a page ends with a `Bash` call whose result is the next line of the transcript
- **THEN** the page's call shows that result

### Requirement: Pages load ahead of the view

A window SHALL ask for the next page once the lines it holds past the view toward the oldest held entry are no more than the lines the view moved in the last 250 ms, and no fewer than 200.
One page per list SHALL be in flight at a time.

#### Scenario: A fast scroll asks early

- **WHEN** the view moved 600 lines in the last 250 ms and holds 550 lines past its bottom
- **THEN** the window asks for the next page

#### Scenario: A slow scroll asks at 200 lines

- **WHEN** the view moved 20 lines in the last 250 ms and holds 250 lines past its bottom
- **THEN** the window asks for nothing

### Requirement: The view never draws what it does not hold

When the view reaches the oldest entry held while a page is in flight, it SHALL stop there and move on when the page lands.
At the start of the session the list SHALL end at its oldest entry.

#### Scenario: The view waits for a page

- **WHEN** the reader scrolls past the oldest held entry while its page is in flight
- **THEN** the oldest held entry stays at the bottom of the view and nothing is drawn below it

### Requirement: Held entries are bounded

A window SHALL hold at most 3,000 entries of each list.
Past that, it SHALL drop the entries farthest from the view, outside the view and the lead, and load them again as pages when the view comes back.

#### Scenario: Scrolling far back

- **WHEN** the reader scrolls back through 5,000 tool calls
- **THEN** the window holds no more than 3,000 of them at any time

#### Scenario: Coming back

- **WHEN** the reader scrolls back to the newest entries after the window dropped some between them and the view
- **THEN** the dropped entries load again before the view reaches them

### Requirement: Whole-session counts

Each tab SHALL show the number of entries in the whole transcript once it is read, and the count so far with a `+` until then.
The scrollbar SHALL count entries not held at the average height of those laid out.

#### Scenario: A long session

- **WHEN** the daemon has read the whole transcript of a session with 2,400 tool calls
- **THEN** the tab reads `Activity 2400` while the window holds 300 of them

#### Scenario: A session still being read

- **WHEN** the daemon has read the newest 300 tool calls of a session and not its start
- **THEN** the tab reads `Activity 300+`

### Requirement: The filter covers held entries

The `/` filter SHALL show the matching entries among those held.
Pages SHALL keep loading as the view nears the oldest match held, and while no match is held, until the list holds 3,000 entries.
Then the tab SHALL end with one dim line that says older entries are not searched and that `ctrl+f` searches the whole session.

#### Scenario: Filtering a partly loaded list

- **WHEN** the reader filters the Activity tab for `cargo` while 300 of 2,400 calls are held
- **THEN** the tab shows the matching calls among the 300
- **AND** older matching calls appear as the reader scrolls down

#### Scenario: No match held

- **WHEN** the reader filters for a word that none of the 300 held calls of 5,000 contains
- **THEN** pages load until 3,000 calls are held, or a match comes
- **AND** if none comes, the tab reads that nothing matches, and ends with the dim line

#### Scenario: A filter at the bound

- **WHEN** every match is among the newest calls, and 3,000 calls are held
- **THEN** the window asks for no more pages, and holds no more than 3,000
- **AND** the tab ends with the dim line

## ADDED Requirements

### Requirement: A stand-in shows only its own pane

`rawr --startup` SHALL open a stand-in that lists only the sessions recorded for its own pane, newest first, and SHALL NOT draw the panes column or open the search.

#### Scenario: Other panes have sessions

- **WHEN** the daemon lists three panes and the stand-in runs in one of them
- **THEN** the stand-in draws that pane's sessions and no other pane

#### Scenario: Keys that reach other panes

- **WHEN** the user presses `ctrl+f`, `1` or `g` in a stand-in
- **THEN** nothing changes on screen

#### Scenario: Back from the sessions

- **WHEN** the user presses `esc` in a stand-in with no filter set
- **THEN** the stand-in quits and the shell prompt returns

### Requirement: Each window reports only its own state

A rawr window SHALL send an OSC 7501 report only when its own state changes, and SHALL NOT send one because another pane changed.
A stand-in SHALL report `idle` once, with `app=rawr` and its session's title as the title.
The full app SHALL report `idle` with a constant line and the title `rawr`, or `blocked` with a constant line while any pane needs the user.

#### Scenario: Another pane starts working

- **WHEN** a pane's Claude goes from idle to working while a stand-in runs in another pane
- **THEN** the stand-in sends no report

#### Scenario: A second pane needs the user

- **WHEN** one pane needs the user and a second one starts to need the user too
- **THEN** the full app sends no report

### Requirement: A stand-in titles its pane after its session

While a stand-in runs, its pane's terminal title SHALL be the stand-in's session title, or the session folder's name when the session is untitled; when the stand-in gives the terminal back, the title SHALL be empty again.

#### Scenario: Restored pane

- **WHEN** a stand-in opens on a pane whose newest resumable session is titled `Fix checkout`
- **THEN** the pane's title reads `Fix checkout` and not `rawr`

#### Scenario: Back to the shell

- **WHEN** the user quits the stand-in
- **THEN** the pane's title is empty, as a new shell leaves it

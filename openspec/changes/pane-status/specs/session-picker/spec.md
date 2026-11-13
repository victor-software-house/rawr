# Spec Delta

## Purpose

Mark a pane whose picker waits for the user, so a restored pane in a background tab is easy to find.

## ADDED Requirements

### Requirement: Report a waiting picker to Rex

While the picker holds the terminal, `rawr` SHALL report an OSC 7501 record with `id=rawr`, `state=blocked`, `kind=question`, `app=rawr` and a message, and SHALL clear that record when it gives the terminal back.

#### Scenario: The picker opens

- **WHEN** `pick` takes the terminal
- **THEN** stderr carries `ESC ] 7501 ; state=blocked:kind=question:app=rawr:id=rawr:msg=<base64 message> ESC \` before the first frame

#### Scenario: The picker closes

- **WHEN** the user resumes a session, dismisses the picker, or `pick` fails or panics after drawing
- **THEN** stderr carries `ESC ] 7501 ; state=clear:id=rawr ESC \` before the main screen returns

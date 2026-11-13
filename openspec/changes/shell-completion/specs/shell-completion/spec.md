# Spec Delta

## Purpose

Let bash, zsh and fish complete rawr's verbs and flags from the installed binary.

## ADDED Requirements

### Requirement: Print a registration script

`rawr completion <SHELL>` SHALL print the registration script for bash, zsh or fish on stdout and exit 0.

#### Scenario: zsh

- **WHEN** the user runs `rawr completion zsh`
- **THEN** stdout holds a zsh completion script for `rawr`, with no ANSI escapes, and the exit status is 0

### Requirement: Complete from the installed binary

At each Tab, the registered script SHALL ask the installed `rawr` for candidates, and `rawr` SHALL answer before it parses its arguments.

#### Scenario: Verbs

- **WHEN** zsh has loaded `rawr completion zsh` and the user types `rawr ` then Tab
- **THEN** zsh offers `record`, `pick`, `find`, `setup` and `completion`

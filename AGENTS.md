# rawr

A small Rust binary that brings Claude Code agents back into their Rex panes after a Rex server restart.
The README describes the behaviour; this file holds the rules for changing it.

## Rules

1. The pane index format is owned here: one JSON file per Rex block ID under `$XDG_STATE_HOME/rawr/` (default `~/.local/state/rawr/`).
   `record` is the only writer; `pick` only reads.
2. The index stays one file per live block.
   `record` deletes a file only after the Rex server has answered with its full block list; a failed or partial listing deletes nothing.
3. `pick` writes only the chosen command to stdout and draws everything else on the terminal through stderr, so `$(rawr pick)` captures nothing but the command.
   It exits non-zero, printing nothing, when there is no terminal, no index, nothing to resume, or the user dismisses it.
4. The binary starts only `rex`.
   It never starts `claude`; the shell runs the printed command.
5. The repository is public.
   Examples, fixtures and docs use invented session titles, folders and IDs, never ones taken from a real machine.

## Development

The pull request's CI verifies: `mise run verify` is the one gate.
Behaviour changes start as an OpenSpec change under `openspec/changes/`.
Releases are patch bumps, and a release is cut only when the maintainer asks for one.

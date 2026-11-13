# rawr

A small Rust binary that brings Claude Code agents back into their Rex panes after a Rex server restart.
The README describes the behaviour; this file holds the rules for changing it.

## Rules

1. The pane index format is owned here: one JSON file per Rex block ID under `$XDG_STATE_HOME/rawr/` (default `~/.local/state/rawr/`).
   `record` is the only writer; `pick` only reads.
2. The index stays one file per live block.
   `record` deletes a file only after the Rex server has answered with its full block list; a failed or partial listing deletes nothing.
3. `pick` and `find` draw on the terminal through stderr and write nothing to stdout.
   On a choice they restore the terminal, then replace their own process with `claude --resume <session id>` in the session's folder.
   They exit non-zero, printing nothing, when there is no terminal, no index or nothing to resume.
   When the user dismisses the picker, they exit non-zero and print one hint line on stderr.
4. The binary talks to the Rex server over its control protocol instead of starting `rex`, and on a choice becomes `claude`; it starts nothing else.
   Nothing resumes without a choice.
5. The repository is public.
   Examples, fixtures and docs use invented session titles, folders and IDs, never ones taken from a real machine.

## Development

The pull request's CI verifies: `mise run verify` is the one gate.
Behaviour changes start as an OpenSpec change under `openspec/changes/`.
Releases are patch bumps, and a release is cut only when the maintainer asks for one.

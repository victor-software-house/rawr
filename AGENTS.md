# rawr

A small Rust binary that brings Claude Code agents back into their Rex panes after a Rex server restart.
The README describes the behaviour; this file holds the rules for changing it.

## Rules

1. The pane index format is owned here: one JSON file per Rex block ID under `$XDG_STATE_HOME/rawr/` (default `~/.local/state/rawr/`).
   `record` is the only writer; the app and the daemon only read.
2. The index stays one file per live block.
   `record` deletes a file only after the Rex server has answered with its full block list; a failed or partial listing deletes nothing.
3. Bare `rawr` and `rawr --startup` draw on the terminal through stderr and write nothing to stdout.
   On a resume in their own pane they restore the terminal, then replace their own process with `claude --resume <session id>` in the session's folder.
   `rawr --startup` prints nothing and exits 0 when the pane has nothing to resume, because it runs in every new shell.
4. The binary talks to the Rex server over its control protocol instead of starting `rex`.
   It starts three programs and nothing else: its own `rawr daemon`, `claude` on a resume, and `/usr/bin/open` (`xdg-open` off macOS) for a file the user clicks.
   Nothing resumes and nothing opens without the user's choice.
5. The repository is public.
   Examples, fixtures and docs use invented session titles, folders and IDs, never ones taken from a real machine.

## Development

The pull request's CI verifies: `mise run verify` is the one gate.
Behaviour changes start as an OpenSpec change under `openspec/changes/`.
Releases are patch bumps, and a release is cut only when the maintainer asks for one.

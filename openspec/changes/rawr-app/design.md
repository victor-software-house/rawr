# Design

## Context

rawr already holds every piece the app needs.
`src/status.rs` turns Rex's program status records into a word per pane, `src/rex/events.rs` follows block and status events, `src/catalog.rs` searches every local session, and `src/picker/` draws a ratatui list with a Claude Code style search box.
Each piece is reached through its own verb today.

The maintainer's private herdr plugins, a session board and a file explorer, are the prior art for the screen.
They keep state in one struct and draw from it with a pure function, feed one channel from producer threads, and draw once after draining it.
herdr orders agents by attention, blocked first ([model.rs][herdr-model]).
The `gh` CLI is the counter-example: dozens of verbs, each printing once, and no screen that shows the state of everything.

## Decisions

1. **One entrypoint.**
   - Bare `rawr` opens the app.
     `--help` lists it first, and the scripting verbs after it.
   - `pick` and `find` go with no alias: the project has one user, and the dotfiles are its only caller.
   - `rawr --startup` serves the shell line in `56-rawr.zsh`.
     It opens the app on the pane's sessions only when one can be resumed; otherwise it prints nothing and exits 0, because it runs in every new shell.
   - Considered: keeping `pick` and `find` beside the app.
     It lost because two ways to do one thing is what made rawr hard to learn.
2. **Three layouts, chosen from running code.**
   - *Two columns*: panes on the left with glyph, name and status word; the selected pane's details and session history on the right.
   - *Stacked*: panes on top, the selected pane's sessions below, the search box at the bottom as in today's picker.
   - *Tabs*: Panes, Sessions and Search as full-width views, one at a time.
   - `ctrl+l` cycles them in the preview build.
     The maintainer chooses, and the pull request ships only that one, with `ctrl+l` removed.
   - Considered: choosing from mockups.
     It lost because the maintainer asked to see each option running (operator, 2026-11-13).
3. **State, view and feed are separate.**
   - `App` holds the panes, the selection, the query and the mode.
     `view::draw` reads it and writes widgets, and records where each row landed for mouse hits.
   - A feed thread connects to Rex, sends one full snapshot (sessions, blocks, status records, pane index), then one message per event.
     The loop drains the channel and draws once if anything changed, with no timer.
   - The first frame draws from the pane index files alone, so it never waits for Rex.
   - Considered: polling `session.list` every second.
     It lost because Rex already pushes every status change, and a poll is late by up to its interval.
4. **Order and glyphs.**

   | Status | Glyph | Colour role |
   |:--|:--|:--|
   | Needs input | `◐` | warning |
   | Failed | `✘` | error |
   | Working | `●` | accent |
   | Idle | `○` | muted |
   | Stopped | `·` | muted |

   Panes sort by that order, then by the newest session's age.
   Colours are roles in `theme.rs` set to ANSI slots, so the terminal's palette shows through, as herdr's explorer does.
   The words are the ones `rawr status` already prints.
5. **Actions.**
   - `enter` on a pane goes to it: the Rex app selects that session and window and focuses the block.
     That is a client action, which needs Remote Control turned on in the app; when the app refuses, the status line shows the app's message.
   - `r` resumes the selected session in this terminal, as `pick` did: the terminal is given back, then rawr becomes `claude --resume`.
     A session a live Claude holds is not offered.
   - `/` opens the search box over every local session, through `catalog::search`.
   - `?` shows the keys; `q`, `esc` and `ctrl+c` quit.
   - The mouse does what the keys do: a click selects, a click on the selected row runs `enter`, the wheel moves the selection.
6. **Every state draws something.**
   - No Rex server: a one-line notice, and the search still works.
   - No pane has a session: a line naming `rawr setup claude`.
   - A feed error after the first snapshot: the last panes stay, marked stale, with the error in the status line.
7. **The preview build.**
   The maintainer watches the app while it is built (operator, 2026-11-13).
   The lead builds `cargo build` on the Mac and runs the binary in a Rex pane beside the maintainer, rebuilt after each change.
   The repository's config allows a local build only for a preview the maintainer asked for.
   CI still verifies the pull request.
8. **`AGENTS.md` rule 3 changes.**
   It names `rawr` and `rawr --startup` in place of `pick` and `find`, and keeps the rest: draw through stderr, nothing on stdout, become `claude` on a choice.

## Not verified

1. Which control protocol method the Rex CLI's `rex -C CLIENT do session.select` sends, and whether a program in a pane can send it without the CLI.
   Task 2.3 reads it from the protocol before the go-to-pane action is written.
2. Whether `block.focus` or a `window_id` argument to `session.select` focuses the exact pane in a split tab.

[herdr-model]: https://github.com/victor-software-house/herdr-vsh/blob/88334d5fe80ebe7b6db3f4bba5a21ac83a119f1e/src/model.rs#L86-L95

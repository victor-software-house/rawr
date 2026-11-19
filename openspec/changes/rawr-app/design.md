# Design

## Context

rawr already held every piece the app needs.
`src/status.rs` turns Rex's program status records into a word per pane, `src/rex/events.rs` follows block and status events, `src/catalog.rs` searches every local session, and `src/picker/` draws a ratatui list with a Claude Code style search box.
Each piece was reached through its own verb.

The maintainer's private herdr plugins, a session board and a file explorer, are the prior art for the screen.
They keep state in one struct and draw from it with a pure function, feed one channel from producer threads, and draw once after draining it.
herdr orders agents by attention, blocked first ([model.rs][herdr-model]).
The maintainer's `a-tui` is the prior art for each entry: folded to one line, opened by a click, with a marker under the pointer instead of a background.
Claude Code itself is the prior art for how a prompt, a reply and a tool call read.
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
2. **Two columns, each folding to a rail.**
   - The left column lists the panes.
     The right column shows the selected pane: its sessions when it has more than one, and the session shown, in four tabs.
   - The focused column takes the width, and the other folds to a rail one cell wide.
     While the pointer rests on a rail, its column draws out over the other as a drawer, and closes when the pointer leaves.
   - Each column's edge is dragged to set its width.
   - `ctrl+l` switches to one column at a time across the whole width, under a line that names where the user is.
   - Considered: three layouts behind one key, two columns, a stacked column and tabs, chosen from running code.
     The preview built them first; the maintainer's corrections during it led to this one, with one column at a time kept behind `ctrl+l`.
3. **State and view are separate; the daemon feeds them.**
   - `App` holds the panes, the selection, the focus, the arrangement, the tab, the query and filters, the column widths and the scroll.
     `view::draw` reads it and writes widgets, and records where each row and each clickable span landed for mouse hits.
   - The panes, the transcript facts and the search hits come from `rawr daemon`, as [app-daemon][app-daemon] describes.
     The loop drains the link's messages and draws once if anything changed.
   - Considered: polling `session.list` every second.
     It lost because Rex already pushes every status change, and a poll is late by up to its interval.
4. **Order and glyphs.**

   | Status | Glyph | Colour role |
   |:--|:--|:--|
   | Needs permission, Needs input | `▲` | warning, bold |
   | Needs an answer | `?` | warning, bold |
   | Needs login | `◆` | warning, bold |
   | Failed | `✘` | error |
   | Done | `✔` | |
   | Working | a braille spinner | working |
   | Idle | `○` | dim |
   | Stopped | `·` | dim |

   - Panes sort by that order, then by the newest session's age, and the sessions no open pane holds come last.
   - Status colours are roles in `theme.rs` set to ANSI slots, so the terminal's palette shows through, as herdr's explorer does.
   - The words are the ones `rawr status` prints.
5. **The session's four tabs.**
   - *Overview*: the session's model, branch, prompt and reply counts, tokens out and sub-agents, when it started and last wrote, and its task list.
   - *Conversation*: what the user typed and what Claude answered, newest first.
   - *Activity*: everything, with each tool call and its result.
   - *Files*: the files the session changed and read, with chips that show all, the changed, the read, or one file type.
     Each path is cut to fit, the folders before the last shortened as fish shortens them.
     A click on a file opens it in its default app, and a click on its folder narrows the tab to that folder.
     Files under the system's temporary folders fold into one line that a click opens.
6. **Entries read as Claude Code draws them.**
   - Each entry is folded until it is opened: its age, its mark, and as much text as fits in 3 lines for a prompt or a reply, 2 for the note that a task ended, and 1 for anything else, with a `+N` count of the lines left out.
   - Under the pointer, a folded entry shows `▸` before its mark and an open one `▾`, and the entry's other rows show a `▏` bar, as `a-tui` draws them.
     No row under the pointer gets a background.
   - A click opens or folds an entry.
     An open entry draws its text as Markdown, with code in fences highlighted, and a tool call's result under `⎿`, at most 12 lines.
   - Marks: `⏺` for a reply, `@name` for a message from a teammate, and `Name(argument)` for a tool call, its name red when the call failed.
     A teammate's message has no `Message from` lead.
   - Code is highlighted with syntect and bat's syntaxes in Monokai Extended, the theme Claude Code uses for code under a dark theme.
     Text in the theme's foreground, or near white, draws in the terminal's default colour, and entry text uses a warm grey, so long transcripts do not glare.
7. **Actions.**
   - `enter` on a pane, or `g` anywhere, goes to it: the daemon brings the block into view in its tab, then asks the Rex app to show that session and window.
     When the window floats over another tab, the daemon starts a copy of the app over the pane instead, which takes over once it has drawn.
     That is a client action, which needs Remote Control turned on in the app; when the app refuses, the status line shows the app's message.
   - `enter` on a session, or `r`, resumes it.
     In a window tiled in its tab, the terminal is given back and rawr becomes `claude --resume`; a floating window opens the resume in a new tab of the pane's session.
     A session a live Claude holds is not offered.
   - `/` filters what has focus; `ctrl+f` searches every local session through the daemon.
   - `1` and `2` focus a column, `tab` walks the columns, and `←` `→` or `[` `]` walk the session's tabs.
   - `?` shows the keys; `esc` goes back to the panes and `q`, `esc` there, or `ctrl+c` quits.
   - The mouse does what the keys do: a click selects or focuses, a click on the selected row acts, and the wheel scrolls the list under the pointer.
8. **Every state draws something.**
   - No Rex server: `Rex is not running here`, and the search still works.
   - No pane has a session: `No pane has a Claude session yet`.
   - A link error after the first snapshot: the last panes stay, marked stale, with the error in the status line.
9. **The preview build.**
   The maintainer watches the app while it is built (operator, 2026-11-13).
   The lead builds `cargo build` on the Mac and runs the binary in a Rex pane beside the maintainer, rebuilt after each change.
   The repository's config allows a local build only for a preview the maintainer asked for.
   CI still verifies the pull request.
10. **`AGENTS.md` rules 1, 3 and 4 change.**
    - Rule 1 names the app and the daemon as readers of the index.
    - Rule 3 names `rawr` and `rawr --startup` in place of `pick` and `find`, and keeps the rest: draw through stderr, nothing on stdout, become `claude` on a choice.
    - Rule 4 names the three programs rawr starts: its own daemon, `claude` on a resume, and `/usr/bin/open` for a file the user clicks.

## Verified

1. Asking the Rex app to show a pane sends `client.perform` with action `session.select`, the session and the window, to the app attached to the window's session, else the first app connected; `session.focus_block` focuses the block in a split tab (`src/rex/control.rs`).

[herdr-model]: https://github.com/victor-software-house/herdr-vsh/blob/88334d5fe80ebe7b6db3f4bba5a21ac83a119f1e/src/model.rs#L86-L95
[app-daemon]: ../app-daemon/design.md

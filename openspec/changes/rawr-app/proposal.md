# Proposal

## Why

rawr has four verbs a person types: `pick`, `find`, `status` and `setup`.
Each does one thing and leaves.
`rawr pick` exits with status 1 and prints nothing when the pane has nothing to resume, so it reads as broken.
`rawr status` prints a table once and is out of date a second later.
Nobody learns four verbs to answer two questions: which Claude needs me, and how do I get back to a session.

## What Changes

1. Bare `rawr` opens one full-screen app, drawn with ratatui.
   It lists every Rex pane that has a Claude session, with what that Claude is doing, kept current from Rex's event stream.
   The selected pane shows its session history; `/` searches every local session.
   From the app the user goes to a pane in the Rex app, or resumes a session in the terminal that ran `rawr`.
2. Panes that need the user come first: Needs input, then Failed, Working, Idle and Stopped.
3. The app always draws something.
   With no Rex server it says so and still offers search; with no sessions anywhere it says that.
4. `rawr pick` and `rawr find` go.
   The shell line that offers a restored pane's sessions runs `rawr --startup`, which opens the app on the pane's sessions only when one can be resumed, and otherwise prints nothing and exits 0.
5. `rawr record`, `rawr status --format json`, `rawr setup` and `rawr completion` stay, for hooks and scripts.
6. Three layouts are built first behind one key, so the maintainer chooses from running code: two columns, one stacked column, and tabs.
   The two not chosen are removed before the pull request opens.

## Capabilities

### New Capabilities

- `app`: what bare `rawr` shows, how it stays current, its keys, and what each action does.

### Modified Capabilities

None.
The `fullscreen-picker` and `session-search` changes describe `rawr pick` and `rawr find`; task 3.2 removes those requirements from them.

## Impact

1. Modules: a new `src/app.rs` with `src/app/{state,view,feed,theme}.rs`; `src/main.rs` (verbs); `src/picker.rs` and its submodules, whose search and row drawing move into the app; `src/status.rs` (rows and order); `src/rex/client.rs` (the call that selects a pane in the Rex app).
2. `AGENTS.md` rule 3 names `rawr` instead of `pick` and `find`.
3. The dotfiles' `56-rawr.zsh` runs `rawr --startup` once the release ships.
4. Dependencies: none new; ratatui and crossterm are already there.

# Tasks

## 1. Preview

- [x] 1.1 The preview built three layouts (5b8e17f), and one stayed after the maintainer watched them (5385180, 2026-11-13).
  The maintainer's corrections since folded its columns to rails with drawers, with one column at a time behind `ctrl+l`.

## 2. The app

- [ ] 2.1 `src/app/` holds `App` state, a pure `view::draw` and `theme.rs` roles, and reads the daemon's link; proved by `TestBackend` tests in CI for the order and status scenarios, and one for each arrangement.
- [ ] 2.2 The empty, no-server and stale states draw their lines; proved by `TestBackend` tests in CI.
- [ ] 2.3 Going to a pane sends `client.perform` with `session.select`, as design.md records; proved by a `rex::fake` test in CI.
- [ ] 2.4 `r` resumes through `Agent::resume`; proved by the existing resume tests in CI.
- [ ] 2.5 A click opens and folds an entry, an open entry draws Markdown and highlighted code, and a file click opens the file; proved by tests of `markup` and the click map in CI.
- [ ] 2.6 The maintainer's preview of the finished app gives the go-ahead, including whether one column at a time stays; recorded here.

## 3. One entrypoint

- [ ] 3.1 Bare `rawr` opens the app, `--startup` serves the shell line, `pick` and `find` are gone; proved by CLI parse tests in CI.
- [ ] 3.2 The `fullscreen-picker` and `session-search` changes no longer name `rawr pick` or `rawr find`; proved by `rg -n 'rawr (pick|find)' openspec/changes` matching only this change.
- [ ] 3.3 `AGENTS.md` rules 1, 3 and 4 and the README name `rawr`, `rawr --startup` and the programs rawr starts; proved by review.

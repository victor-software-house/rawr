# Tasks

## 1. Preview: three layouts

- [ ] 1.1 `src/app/` holds `App` state, a pure `view::draw`, `theme.rs` roles and a feed thread with the snapshot then events; proved by `TestBackend` tests in CI for the order and status scenarios.
- [ ] 1.2 The two-column, stacked and tabs layouts draw the same `App`, and `ctrl+l` cycles them; proved by one `TestBackend` test per layout in CI.
- [ ] 1.3 The maintainer watches a local preview build in a Rex pane and chooses a layout; the choice and its date are recorded here.

## 2. The chosen app

- [ ] 2.1 The layouts not chosen and `ctrl+l` are removed; proved by `mise run verify` in CI.
- [ ] 2.2 The empty, no-server and stale states draw their lines; proved by `TestBackend` tests in CI.
- [ ] 2.3 The control protocol method for selecting a pane in the Rex app is read from the protocol and recorded in design.md; `enter` sends it, proved by a `rex::fake` test in CI.
- [ ] 2.4 `r` resumes through `Agent::resume`; proved by the existing resume tests in CI.
- [ ] 2.5 The maintainer's preview of the finished app gives the go-ahead; recorded here.

## 3. One entrypoint

- [ ] 3.1 Bare `rawr` opens the app, `--startup` serves the shell line, `pick` and `find` are gone; proved by CLI parse tests in CI.
- [ ] 3.2 The `fullscreen-picker` and `session-search` changes no longer name `rawr pick` or `rawr find`; proved by `rg -n 'rawr (pick|find)' openspec/changes` matching only this change.
- [ ] 3.3 `AGENTS.md` rule 3 and the README name `rawr` and `rawr --startup`; proved by review.

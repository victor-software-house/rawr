# Tasks

## 1. Config and index cleanup

- [x] 1.1 `src/config.rs` reads `$XDG_CONFIG_HOME/rawr/config.yaml` through ctl-core's `input` feature, with the defaults in the config spec; proved by its unit tests in CI, including a bad value naming its file and line.
  - 2026-11-13: [CI run 43412237486][ci-27] on [pull request 27][pr-27] passed `mise run verify`, with the unit tests for this task.
- [x] 1.2 `history.per_pane` replaces the fixed `KEEP` in `src/index.rs`; proved by the "A smaller bound" scenario's unit test in CI.
  - 2026-11-13: [CI run 43412237486][ci-27] on [pull request 27][pr-27] passed `mise run verify`, with the unit tests for this task.
- [x] 1.3 Entries carry `recorded_at` and `transcript_seen`; reading a pane's index drops the entries the pane-cleanup spec names; proved by unit tests for each of its scenarios in CI.
  - 2026-11-13: [CI run 43412237486][ci-27] on [pull request 27][pr-27] passed `mise run verify`, with the unit tests for this task.

## 2. Picker layout

- [x] 2.1 The list sits on top and the search box at the bottom, with the key line under it; printable keys type into the box; proved by `TestBackend` tests in CI.
  - 2026-11-13: [CI run 43413195925][ci-32] on [pull request 32][pr-32] passed `mise run verify`, with the `TestBackend` tests for this task.
- [x] 2.2 At most `picker.rows` sessions show, then `+N earlier · ctrl+o` only when more exist, and `ctrl+o` toggles the rest; proved by tests for the three row-count scenarios in CI.
  - 2026-11-13: [CI run 43413195925][ci-32] on [pull request 32][pr-32] passed `mise run verify`, with the row-count tests for this task.
- [x] 2.3 A dismissal prints the hint line on stderr; proved by a unit test of the outcome in CI.
  - 2026-11-13: [CI run 43413195925][ci-32] on [pull request 32][pr-32] passed `mise run verify`, with the dismissal test for this task.
- [ ] 2.4 The maintainer's preview in a Rex pane shows the new layout; the go-ahead is recorded here.

[pr-27]: https://github.com/victor-software-house/rawr/pull/27
[ci-27]: https://github.com/victor-software-house/rawr/actions/runs/43412237486
[pr-32]: https://github.com/victor-software-house/rawr/pull/32
[ci-32]: https://github.com/victor-software-house/rawr/actions/runs/43413195925

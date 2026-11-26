# Tasks

The open tasks below ask for a maintainer preview of the picker, which shipped in v0.0.1 to v0.0.3 and was replaced by the rawr app before a preview was recorded. The change is archived as shipped with them open (operator, 2026-11-18).

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
  - 2026-11-18 (UTC): The layout shipped in [pull request 32][pr-32] and is in [v0.0.3][tag-v003]. Left unchecked: no commit message, task note or ledger row in [tasks.yaml][tasks-yaml] records the maintainer's preview or go-ahead. Not verified: whether the maintainer saw the layout in a Rex pane without recording it.

[pr-27]: https://github.com/victor-software-house/rawr/pull/27
[ci-27]: https://github.com/victor-software-house/rawr/actions/runs/43412237486
[pr-32]: https://github.com/victor-software-house/rawr/pull/32
[ci-32]: https://github.com/victor-software-house/rawr/actions/runs/43413195925
[tag-v003]: https://github.com/victor-software-house/rawr/releases/tag/v0.0.3
[tasks-yaml]: https://github.com/victor-software-house/rawr/blob/b86160589fca4451e5fdead89e0596fd4a3f44dc/tasks.yaml

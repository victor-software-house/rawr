# Tasks

## 1. Config and index cleanup

- [ ] 1.1 `src/config.rs` reads `$XDG_CONFIG_HOME/rawr/config.yaml` through ctl-core's `input` feature, with the defaults in the config spec; proved by its unit tests in CI, including a bad value naming its file and line.
- [ ] 1.2 `history.per_pane` replaces the fixed `KEEP` in `src/index.rs`; proved by the "A smaller bound" scenario's unit test in CI.
- [ ] 1.3 Entries carry `recorded_at` and `transcript_seen`; reading a pane's index drops the entries the pane-cleanup spec names; proved by unit tests for each of its scenarios in CI.

## 2. Picker layout

- [ ] 2.1 The list sits on top and the search box at the bottom, with the key line under it; printable keys type into the box; proved by `TestBackend` tests in CI.
- [ ] 2.2 At most `picker.rows` sessions show, then `+N earlier · ctrl+o` only when more exist, and `ctrl+o` toggles the rest; proved by tests for the three row-count scenarios in CI.
- [ ] 2.3 A dismissal prints the hint line on stderr; proved by a unit test of the outcome in CI.
- [ ] 2.4 The maintainer's preview in a Rex pane shows the new layout; the go-ahead is recorded here.

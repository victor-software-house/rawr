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

## 3. Rex client

- [ ] 3.1 A `rex` module speaks the Rex server socket and exposes typed calls and an event stream; proved by a test against a recorded exchange in CI.
- [ ] 3.2 `rawr status` lists each pane's word from program status; proved by tests for the status scenarios in CI.

## 4. Daemon, drain and restart

- [ ] 4.1 `rawr daemon` follows Rex events and prunes only against a confirmed listing; proved by tests for the pruning scenarios in CI.
- [ ] 4.2 `rawr drain`, `rawr drain ack` and `rawr restart` follow the supervisor spec, with the live pane view, `f` and Esc; proved by tests for each scenario in CI.

## 5. Claude Code mod

- [ ] 5.1 The plugin and mod in this repository record sessions and carry drains, typed against Claude Code's generated declarations; proved by `claude plugin test` in CI.

## 6. Setup and install

- [ ] 6.1 `rawr init <shell>` and `rawr setup`, `setup status`, `setup remove` follow the setup spec; proved by unit tests in CI.
- [ ] 6.2 `victor-software-house/homebrew-tap` holds the `rawr` formula with completions and the `rawr daemon` service; proved by `brew test rawr` in that repository's CI.

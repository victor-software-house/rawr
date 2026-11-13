# Tasks

## 1. Rex client

- [x] 1.1 A `rex` module speaks the Rex server socket and exposes typed calls and an event stream; proved by a test against a recorded exchange in CI.
  - 2026-11-13 BRT: [CI run 43413079651][run-rex] at `5f6dd73` passed `verify`, with the fake-server tests in `src/rex/tests.rs` replaying the exchanges under `testdata/rex/`.
- [x] 1.2 `rawr status` lists each pane's word from program status; proved by tests for the status scenarios in CI.
  - 2026-11-13 BRT: the same run passed the status tests in `src/status.rs`.

## 2. Daemon

- [ ] 2.1 `rawr daemon` follows Rex events and prunes only against a confirmed listing; proved by tests for the pruning scenarios in CI.

## 3. Claude Code mod

- [ ] 3.1 The plugin and mod in this repository record sessions, typed against Claude Code's generated declarations; proved by `claude plugin test` in CI.

[run-rex]: https://github.com/victor-software-house/rawr/actions/runs/43413079651

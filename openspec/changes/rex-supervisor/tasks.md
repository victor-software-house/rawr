# Tasks

## 1. Rex client

- [ ] 1.1 A `rex` module speaks the Rex server socket and exposes typed calls and an event stream; proved by a test against a recorded exchange in CI.
- [ ] 1.2 `rawr status` lists each pane's word from program status; proved by tests for the status scenarios in CI.

## 2. Daemon, drain and restart

- [ ] 2.1 `rawr daemon` follows Rex events and prunes only against a confirmed listing; proved by tests for the pruning scenarios in CI.
- [ ] 2.2 `rawr drain`, `rawr drain ack` and `rawr restart` follow the supervisor spec, with the live pane view, `f` and Esc; proved by tests for each scenario in CI.

## 3. Claude Code mod

- [ ] 3.1 The plugin and mod in this repository record sessions and carry drains, typed against Claude Code's generated declarations; proved by `claude plugin test` in CI.

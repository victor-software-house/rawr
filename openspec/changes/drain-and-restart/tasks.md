# Tasks

## 1. Drain and restart

- [ ] 1.1 `rawr drain`, `rawr drain ack`, `rawr drain background` and `rawr restart` follow the drain and restart specs, with the live pane view, `f` and Esc; proved by tests for each scenario in CI.
- [ ] 1.2 The rawr mod carries drains, asks for the background key and acknowledges as the drain spec says, typed against Claude Code's generated declarations; proved by `claude plugin test` in CI.

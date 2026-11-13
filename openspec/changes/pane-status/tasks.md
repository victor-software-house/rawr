# Tasks

## 1. Status report

- [ ] 1.1 `Screen::enter` writes the blocked record and `leave` clears it; proved by a unit test of both sequences in CI.

## 2. Verify

- [ ] 2.1 `mise run verify` passes in the pull request's CI.
- [ ] 2.2 In a Rex pane, a background tab whose pane runs `rawr pick` shows the needs-attention dot, and the dot is gone after Esc.
- [ ] 2.3 `openspec validate --all --strict` passes.

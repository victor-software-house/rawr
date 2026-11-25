# Tasks

## 1. App

- [x] 1.1 Enter, or a click on a result, closes the search and selects the result's pane and session, with no action sent; proved by `App` state tests of the "A result a pane lists" scenario in CI.
  - Tasks 1.1 to 1.3 passed in CI run [45693489676][ci-run], 2026-11-25 (UTC).
- [x] 1.2 A result no pane lists opens as an opened pane under Opened, first in the list, with Resume and no Go to pane; proved by state and `TestBackend` tests of the "Opening an old session" scenario in CI.
- [x] 1.3 `x` and the close mark close an opened pane, and a pane that comes to list its session replaces it; proved by state tests of the "Closing an opened pane" and "Resumed in a pane" scenarios in CI.
- [x] 1.4 The key help and the README name `x` and say what choosing a result does; proved by the diff.

## 2. Review

- [ ] 2.1 The maintainer chooses results in the demo and judges it; proved by their review.

[ci-run]: https://github.com/victor-software-house/rawr/actions/runs/45693489676

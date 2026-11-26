# Tasks

## 1. Stand-in and floating app

- [ ] 1.1 `rawr --startup` opens a stand-in that lists only its pane's sessions, with no panes column and no search; proved by `App` state tests and a `TestBackend` test in CI.
- [ ] 1.2 Each window reports only its own state, and the full app's report flips only between idle and blocked; proved by tests of the bytes each report sends in CI.
- [ ] 1.3 A stand-in sets its pane's title to its session's title and leaves it empty on exit; proved by a test of the sequences in CI, and by `rex block call title` on a demo Rex server.
  - 2026-11-25 (UTC): on an isolated Rex server, a stand-in set the pane title and an idle `rawr` record to its session's title; `ctrl+f` did nothing; `esc` quit, leaving an empty title and no record.
- [ ] 1.4 The README leads with `cmd+alt+r` and gives the `init.lua` action that opens the floating app; proved by review.

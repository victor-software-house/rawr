# Tasks

## 1. Pacing

- [ ] 1.1 A new `src/app/pacing.rs` holds the frame clock, the wheel queue and the beat fitted to a stream, with unit tests for each scenario of the "Frame clock while moving", "Wheel batches spread over frames" and "Streams faster than the clock" requirements; proved by those tests in CI.
- [ ] 1.2 `src/app.rs` draws on the clock while a report waits, and sleeps at rest; wheel reports go into the queue, and each frame moves each list by what the queue gives it; proved by a loop test that feeds timed reports and reads the frame times and moves, in CI.
- [ ] 1.3 Each entry is laid out once for a width and a fold state, and a frame draws only the entries in view; proved by `TestBackend` tests that count layouts for the "Frame cost independent of history" scenarios, in CI.
- [x] 1.4 A frame with 3,000 entries held, at 120 columns, takes less than 2 ms at p99 while the view scrolls, and the frame after a width change is measured too; proved by a release-build timing test run on a Mac, with the figures recorded under this task.
  - 2026-11-25 (UTC), release build in a 4 CPU, 8 GiB box on a Mac, `frame_time_with_three_thousand_entries`, three runs:
    - 2,000 scrolling frames: p99 199 to 210 µs.
    - The frame after a width change, laying out all 3,000 entries again: 3.7 to 3.9 ms.

## 2. Scroll anchor and entry IDs

- [ ] 2.1 Each entry carries the byte offset of its transcript line and its place within that line; the open set and the scroll position use that ID; proved by `facts.rs` tests that read the same line twice and get the same IDs, in CI.
- [ ] 2.2 A new entry above the view moves nothing on screen, and an unscrolled list follows the newest entry; proved by `TestBackend` tests of the two "The view keeps its place" scenarios, in CI.

## 3. Review

- [ ] 3.1 The maintainer scrolls a long session's activity in Rex with a trackpad and judges the motion; proved by their review of a preview binary they asked for.

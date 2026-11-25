# Tasks

## 1. Scrollbar

- [ ] 1.1 A new `src/app/scrollbar.rs` holds the geometry (thumb from position, position from thumb), the press (thumb or track) and the fade, with unit tests of each; proved by those tests in CI.
- [ ] 1.2 Every list draws its bar through it, records its hits, and takes a press, a drag and a track press; proved by `TestBackend` and state tests of the "The thumb can be dragged" and "A press on the track pages" scenarios in CI.
- [ ] 1.3 The bar shows only while in use and fades out on the frame clock; proved by state tests of the "The bar shows only while in use" scenarios in CI.
- [ ] 1.4 The daemon answers `around`, and a drag past the held entries jumps; proved by tests of the "A drag past the held entries jumps" scenario on a fixture transcript in CI.

## 2. Review

- [ ] 2.1 The README's mouse row names dragging the scrollbar; proved by review.
- [ ] 2.2 The maintainer drags a long session's bar in the browser demo and judges it; proved by their review.

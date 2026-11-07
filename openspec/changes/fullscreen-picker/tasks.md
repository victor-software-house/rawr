# Tasks

## 1. Pane index

- [ ] 1.1 `record` adds the hook's session to the pane's index, keeps five, and prunes only after a full Rex listing; proved by the unit tests in `src/index.rs` passing in CI.
- [ ] 1.2 Fixtures use invented block and session IDs; proved by `rg` finding no ID from the maintainer's machine in `src/`.

## 2. Picker state and view

- [ ] 2.1 `picker/state.rs` maps keys, clicks, double clicks and wheel steps to selection changes and choices; proved by its unit tests in CI.
- [ ] 2.2 `picker/view.rs` draws the header, the wrapped rows, the scrollbar and the key line, and returns the row map; proved by `TestBackend` tests of selection, hover, wrapping and scrolling in CI.
- [ ] 2.3 Titles are sanitized before drawing; proved by a test with an escape sequence in a title.

## 3. Terminal

- [ ] 3.1 A guard enters the alternate screen, mouse capture and the hidden cursor on stderr, and leaves them on drop and from a panic hook, then drains input for 50 ms; proved by the maintainer's preview in a Rex pane: the restored output is back after Esc, Enter and a double click.
- [ ] 3.2 The pointer shape follows hover with OSC 22 and returns to `default` on exit; proved by the same preview.

## 4. Verify and preview

- [ ] 4.1 `mise run verify` passes in the pull request's CI.
- [ ] 4.2 A macOS preview binary is built from the pull request's head and shown to the maintainer in a Rex pane; the maintainer's go-ahead is recorded here.
- [ ] 4.3 `openspec validate --all --strict` passes.

# Tasks

## 1. Catalog

- [ ] 1.1 `src/catalog.rs` reads the prompt history and one projects listing into resumable sessions with their prompts, folder, first prompt and latest time; proved by unit tests over fixture files in CI.
- [ ] 1.2 Matching takes every word, ignores case, and orders pane sessions first; proved by unit tests in CI.

## 2. Picker

- [ ] 2.1 `picker/state.rs` opens, edits and closes the search line and keeps the selection inside the filtered rows; proved by unit tests in CI.
- [ ] 2.2 `picker/view.rs` draws the search line and the key line for it; proved by `TestBackend` tests in CI.

## 3. Commands and shell

- [ ] 3.1 `rawr find [WORDS…]` opens the picker on the search line; proved by the maintainer's preview.
- [ ] 3.2 The README's zsh integration evaluates `rawr find` through a `rawr` function; proved by the maintainer's preview resuming a session found by a word.

## 4. Verify

- [ ] 4.1 `mise run verify` passes in the pull request's CI.
- [ ] 4.2 `openspec validate --all --strict` passes.

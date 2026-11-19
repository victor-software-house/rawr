# Tasks

Task 1.1 stays open because no test covers `prune`. The other open tasks ask for a maintainer preview of the picker, which shipped in v0.0.1 to v0.0.3 and was replaced by the rawr app before a preview was recorded. The change is archived as shipped with them open (operator, 2026-11-18).

## 1. Pane index

- [ ] 1.1 `record` adds the hook's session to the pane's index, keeps five, and prunes only after a full Rex listing; proved by the unit tests in `src/index.rs` passing in CI.
  - 2026-11-18 (UTC): Read the tests in [src/index.rs][index-rs] and the [CI run on the commit that merged the stack][ci-40abc93]. `add_moves_a_session_first_and_keeps_five` passed there and in the [CI run for v0.0.1][ci-v001]. Left unchecked: no unit test covers `prune` or its rules (a file written after the listing stays, nothing is deleted when a Rex listing fails), because `live_blocks` runs the `rex` command. Not verified: the pruning behaviour by any test or run.
- [x] 1.2 Fixtures use invented block and session IDs; proved by `rg` finding no ID from the maintainer's machine in `src/`.
  - 2026-11-18 (UTC): Collected the 17 distinct ID-like strings in [`src/` at v0.0.1][src-v001], at the [merge of the stack][c-40abc93] and at the [head of main][c-b861605], such as `0190a000-0000-7000-8000-00000000000a`. Searched for each as a fixed string in the 2,164 IDs on the maintainer's main machine (transcript file names, prompt history, the rawr index folder and the Rex block listing) and in the 11 transcript and index names on a second machine. No match. Not verified: any other machine, or an ID deleted before this check. This compares known IDs; it is not a search for unknown ones.

## 2. Picker state and view

- [x] 2.1 `picker/state.rs` maps keys, clicks and wheel steps to selection changes and choices; proved by its unit tests in CI.
  - 2026-11-18 (UTC): The tests in [src/picker/state.rs][state-rs] passed in the [CI run on the merge of the stack][ci-40abc93] (42 passed, 0 failed), among them `keys_move_the_selection_within_the_list`, `enter_digits_and_escape_end_the_picker`, `a_click_selects_another_row_and_resumes_the_selected_one`, `a_click_outside_the_rows_changes_nothing`, `the_wheel_moves_the_selection` and `hover_follows_the_pointer_and_redraws_only_on_change`. The same names passed in the [CI run for v0.0.1][ci-v001].
- [x] 2.2 `picker/view.rs` draws the header, the wrapped rows, the scrollbar and the key line, and returns the row map; proved by `TestBackend` tests of selection, hover, wrapping and scrolling in CI.
  - 2026-11-18 (UTC): The `TestBackend` tests in [src/picker/view.rs][view-rs] passed in the [CI run on the merge of the stack][ci-40abc93] and in the [CI run for v0.0.1][ci-v001]: `the_selected_session_has_the_pointer_and_the_accent` (also asserts the header text, the key line and the row map), `hover_undims_a_session_without_selecting_it`, `long_text_wraps_and_nothing_is_cut` and `the_list_scrolls_to_keep_the_selection_whole` (also asserts a scrollbar thumb). Judged against the rows as shipped then, with the session ID on a third line.
- [x] 2.3 Titles are sanitized before drawing; proved by a test with an escape sequence in a title.
  - 2026-11-18 (UTC): [src/session.rs][session-rs] passes each title through `sanitize`, and `sanitize_drops_escape_sequences` feeds it `fix<ESC>[2Jbuild` and expects `fix[2Jbuild`; it passed in the [CI run on the merge of the stack][ci-40abc93]. [src/catalog.rs][catalog-rs] has the same check for a prompt title in `a_long_first_prompt_is_cut_with_an_ellipsis`. Not verified: a test that draws a title with an escape sequence through the view; the view test for an escape uses a folder.

## 3. Terminal

- [ ] 3.1 A guard enters the alternate screen, mouse capture and the hidden cursor on stderr, and leaves them on drop and from a panic hook, then drains input for 50 ms; proved by the maintainer's preview in a Rex pane: the restored output is back after Esc, Enter and a click on the selected row.
  - 2026-11-18 (UTC): Read the guard in [src/picker/terminal.rs][terminal-rs]: `Screen::enter` sets a panic hook, raw mode, the alternate screen, mouse capture and a hidden cursor on stderr, and `leave` runs from `Drop` and the hook and drains input for `DRAIN` (50 ms). Left unchecked: the clause is proved by the maintainer's preview in a Rex pane, and no commit message, task note or ledger row records one. No unit test covers the guard.
- [ ] 3.2 The pointer shape follows hover with OSC 22 and returns to `default` on exit; proved by the same preview.
  - 2026-11-18 (UTC): Read [src/picker/terminal.rs][terminal-rs]: `set_pointer` writes OSC 22 `pointer` or `default` when the hovered row changes, and `leave` writes `default` first. Left unchecked: the same preview is not recorded anywhere, and no test covers the sequences.

## 4. Verify and preview

- [x] 4.1 `mise run verify` passes in the pull request's CI.
  - 2026-11-18 (UTC): `verify` passed in the [CI run of PR 4][ci-pr4], the top of the stack, and in the [CI run on the merge of the stack][ci-40abc93]. The last run of [PR 1][pr-1], the bottom of the stack, failed: [its CI][ci-pr1] stopped at the `deps` step in both the static and the build jobs, while its tests and the `specs` step passed. The later runs pass, so the stack merged green. Not verified: which commit fixed `deps`.
- [ ] 4.2 A macOS preview binary is built from the pull request's head and shown to the maintainer in a Rex pane; the maintainer's go-ahead is recorded here.
  - 2026-11-18 (UTC): Searched the commit messages on main, every `tasks.md`, the README, `openspec/config.yaml` and [tasks.yaml][tasks-yaml] for a preview or a go-ahead and found none. The ledger row RAWR-001 lists the preview as an acceptance line. Left unchecked. Not verified: whether the maintainer saw a preview that nobody recorded.
- [x] 4.3 `openspec validate --all --strict` passes.
  - 2026-11-18 (UTC): The `specs` step of `verify` runs `openspec validate --all --strict --no-interactive` ([mise-tasks/specs.sh][specs-sh]). It listed `change/fullscreen-picker` as passed in the [CI run of PR 4][ci-pr4], in the [CI run on the merge of the stack][ci-40abc93] and in the [CI run for v0.0.3][ci-v003].

[pr-1]: https://github.com/victor-software-house/rawr/pull/1
[ci-pr1]: https://github.com/victor-software-house/rawr/actions/runs/42430619195
[ci-pr4]: https://github.com/victor-software-house/rawr/actions/runs/42431573127
[ci-40abc93]: https://github.com/victor-software-house/rawr/actions/runs/42432176573
[ci-v001]: https://github.com/victor-software-house/rawr/actions/runs/43357263135
[ci-v003]: https://github.com/victor-software-house/rawr/actions/runs/43414046460
[c-40abc93]: https://github.com/victor-software-house/rawr/commit/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9
[c-b861605]: https://github.com/victor-software-house/rawr/commit/b86160589fca4451e5fdead89e0596fd4a3f44dc
[src-v001]: https://github.com/victor-software-house/rawr/tree/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src
[index-rs]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/src/index.rs
[state-rs]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/src/picker/state.rs
[view-rs]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/src/picker/view.rs
[terminal-rs]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/src/picker/terminal.rs
[session-rs]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/src/session.rs
[catalog-rs]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/src/catalog.rs
[specs-sh]: https://github.com/victor-software-house/rawr/blob/40abc93a3ef4bb293540bfce6320a69d4ebbc5d9/mise-tasks/specs.sh
[tasks-yaml]: https://github.com/victor-software-house/rawr/blob/b86160589fca4451e5fdead89e0596fd4a3f44dc/tasks.yaml

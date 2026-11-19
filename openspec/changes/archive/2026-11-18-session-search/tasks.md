# Tasks

The open tasks below ask for a maintainer preview of the picker, which shipped in v0.0.1 to v0.0.3 and was replaced by the rawr app before a preview was recorded. The change is archived as shipped with them open (operator, 2026-11-18).

## 1. Catalog

- [x] 1.1 `src/catalog.rs` reads the prompt history and one projects listing into resumable sessions with their prompts, folder, title and latest time, the title read from the transcript's ends and its `custom-title.json`, else the first prompt; proved by unit tests over fixture files in CI.
  - 2026-11-18 (UTC): The tests in [src/catalog.rs][catalog-rs] write a fixture folder (a history file and two transcripts) and passed in the [CI run for v0.0.1][ci-v001]: `only_sessions_with_a_transcript_are_found_newest_first` checks the order, the title (a transcript title row for one session, the first prompt for the other) and the folder, and `a_live_session_is_not_found` and `a_long_first_prompt_is_cut_with_an_ellipsis` cover the rest. The transcript-ends and `custom-title.json` order is tested in [src/session.rs][session-rs] by `a_title_file_beats_a_generated_title_in_the_tail`, `a_user_title_in_the_tail_beats_the_title_file` and four more title tests, all passed in the same run. Not verified: that a test asserts the single projects listing; only the code reads one.
- [x] 1.2 Matching takes every word, ignores case, and orders pane sessions first; proved by unit tests in CI.
  - 2026-11-18 (UTC): `every_word_must_match_in_any_prompt_ignoring_case` and `the_pane_s_sessions_come_first_and_once` in [src/catalog.rs][catalog-rs] passed in the [CI run on the merge of the stack][ci-40abc93] and in the [CI run for v0.0.1][ci-v001].

## 2. Picker

- [x] 2.1 `picker/state.rs` opens, edits and closes the search line and keeps the selection inside the filtered rows; proved by unit tests in CI.
  - 2026-11-18 (UTC): In [src/picker/state.rs][state-rs], `slash_opens_the_search_line_and_letters_type_into_it` (open, type, backspace, clear), `escape_closes_the_search_line_then_the_picker` (close) and `a_new_list_selects_its_first_row_and_enter_needs_a_row` (a new filtered list resets the selection to the first row, and zero rows leave nothing to choose) passed in the [CI run on the merge of the stack][ci-40abc93] and in the [CI run for v0.0.1][ci-v001]. This is the `/` search line as shipped in v0.0.1; the picker redesign in a later change replaced it.
- [x] 2.2 `picker/view.rs` draws the search line and the key line for it; proved by `TestBackend` tests in CI.
  - 2026-11-18 (UTC): `the_search_line_replaces_the_header` in [src/picker/view.rs][view-rs] draws a search on a `TestBackend`, expects the first row to read ` / login▏  1 match` and the last row to contain `close search`. `no_match_says_so_and_offers_no_row` covers an empty result. Both passed in the [CI run on the merge of the stack][ci-40abc93] and in the [CI run for v0.0.1][ci-v001].

## 3. Commands and shell

- [ ] 3.1 `rawr find [WORDS…]` opens the picker on the search line; proved by the maintainer's preview.
  - 2026-11-18 (UTC): Read [src/main.rs][main-rs] in v0.0.1: `find` opens the picker on the search line with the words typed, and does nothing when stderr is not a terminal. Left unchecked: the clause is proved by the maintainer's preview, and no commit message, task note or ledger row records one. No test covers the command beyond the command-line shape.
- [ ] 3.2 `rawr find` resumes the chosen session itself, with no shell wrapper; proved by the maintainer's preview resuming a session found by a word.
  - 2026-11-18 (UTC): The code does this since [3ef8d9f][c-3ef8d9f], which is in v0.0.1: `choose` calls `resume` on the chosen session. Left unchecked: no record of the maintainer's preview resuming a session found by a word. Not verified: a live resume.

## 4. Verify

- [x] 4.1 `mise run verify` passes in the pull request's CI.
  - 2026-11-18 (UTC): `verify` passed in the [CI run of PR 4][ci-pr4], the top of the stack that carried this change, and in the [CI run on the merge of the stack][ci-40abc93] (42 tests passed, 0 failed). The last run of [PR 1][pr-1], the bottom of the stack, failed: [its CI][ci-pr1] stopped at the `deps` step, while its tests and the `specs` step passed. The later runs pass. That answers the draft-pull-request caveat in the note under 4.2. Not verified: which commit fixed `deps`.
- [x] 4.2 `openspec validate --all --strict` passes.
  - 2026-11-07 BRT: `openspec validate --all --strict` reported 3 passed, 0 failed.
  - Not verified: tasks 1.1 to 2.2 pass their unit tests on a maintainer's Mac (30 passed), but the pull request is a draft, so CI has not run them.

[pr-1]: https://github.com/victor-software-house/rawr/pull/1
[ci-pr1]: https://github.com/victor-software-house/rawr/actions/runs/42430619195
[ci-pr4]: https://github.com/victor-software-house/rawr/actions/runs/42431573127
[ci-40abc93]: https://github.com/victor-software-house/rawr/actions/runs/42432176573
[ci-v001]: https://github.com/victor-software-house/rawr/actions/runs/43357263135
[c-3ef8d9f]: https://github.com/victor-software-house/rawr/commit/3ef8d9ff7bbdc47bad58414f1f4873e8105aec3d
[catalog-rs]: https://github.com/victor-software-house/rawr/blob/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src/catalog.rs
[session-rs]: https://github.com/victor-software-house/rawr/blob/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src/session.rs
[state-rs]: https://github.com/victor-software-house/rawr/blob/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src/picker/state.rs
[view-rs]: https://github.com/victor-software-house/rawr/blob/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src/picker/view.rs
[main-rs]: https://github.com/victor-software-house/rawr/blob/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src/main.rs

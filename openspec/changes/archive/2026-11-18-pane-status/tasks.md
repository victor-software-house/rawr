# Tasks

The open tasks below ask for a maintainer preview of the picker, which shipped in v0.0.1 to v0.0.3 and was replaced by the rawr app before a preview was recorded. The change is archived as shipped with them open (operator, 2026-11-18).

## 1. Status report

- [x] 1.1 `Screen::enter` writes the blocked record and `leave` clears it; proved by a unit test of both sequences in CI.
  - 2026-11-18 (UTC): `status_records_match_the_protocol` in [src/picker/terminal.rs][terminal-rs] checks both sequences: the blocked record (`state=blocked`, `kind=question`, `app=rawr`, `id=rawr`, a base64 message) and the clear record `state=clear:id=rawr`. It passed in the [CI run of PR 15][ci-pr15] and in the [CI run for v0.0.1][ci-v001]. Read the code at the same commit: `enter` writes the blocked record and `leave` writes the clear record. The test checks the two constants, not the writes in `enter` and `leave`.

## 2. Verify

- [x] 2.1 `mise run verify` passes in the pull request's CI.
  - 2026-11-18 (UTC): `verify` passed in the [CI run of PR 15][ci-pr15] (46 tests passed, 0 failed), the pull request that added the status report. [PR 15][pr-15] merged into the code that [v0.0.1][tag-v001] carries, and the [CI run for v0.0.1][ci-v001] passed too.
- [ ] 2.2 In a Rex pane, a background tab whose pane runs `rawr pick` shows the needs-attention dot, and the dot is gone after Esc.
  - 2026-11-18 (UTC): Left unchecked: the clause needs a person to watch a Rex pane, and no commit message, task note or ledger row records that check; the row RAWR-011 in [tasks.yaml][tasks-yaml] lists it as an acceptance line. Not verified: the dot in a live Rex tab.
- [x] 2.3 `openspec validate --all --strict` passes.
  - 2026-11-18 (UTC): The `specs` step of `verify` runs `openspec validate --all --strict --no-interactive`. It listed `change/pane-status` as passed (4 passed, 0 failed) in the [CI run of PR 15][ci-pr15], and again in the [CI run for v0.0.3][ci-v003].

[pr-15]: https://github.com/victor-software-house/rawr/pull/15
[ci-pr15]: https://github.com/victor-software-house/rawr/actions/runs/43356394854
[ci-v001]: https://github.com/victor-software-house/rawr/actions/runs/43357263135
[ci-v003]: https://github.com/victor-software-house/rawr/actions/runs/43414046460
[tag-v001]: https://github.com/victor-software-house/rawr/releases/tag/v0.0.1
[terminal-rs]: https://github.com/victor-software-house/rawr/blob/b111e99ab4f0ef218ce10260ee161a8fe8fce910/src/picker/terminal.rs
[tasks-yaml]: https://github.com/victor-software-house/rawr/blob/b86160589fca4451e5fdead89e0596fd4a3f44dc/tasks.yaml

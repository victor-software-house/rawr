# Tasks

## 1. Pages

- [x] 1.1 The daemon counts every entry of each list as it reads, and `Facts` carries the totals and whether older entries exist; the tabs show the totals or the count with `+`; proved by tests of the "Whole-session counts" scenarios, in CI.
  - Tasks 1.1 to 1.3 passed in CI run [45651550054][ci-run], 2026-11-25 (UTC).
- [x] 1.2 `older` and `page` join the daemon protocol; the daemon reads a page back from the given entry, reads forward past its end for results, and keeps nothing; proved by tests of the "Older entries load in pages" and "Results across a page's edge" scenarios on fixture transcripts, in CI.
- [x] 1.3 A window merges pages into its held lists, asks ahead by the lead, holds at most 3,000 entries a list and loads dropped entries again; proved by state tests of the "Pages load ahead of the view", "The view never draws what it does not hold", "Held entries are bounded" and "The filter covers held entries" scenarios, in CI.
- [x] 1.4 A page of 100 entries answers within 20 ms at p95 on a transcript of 50 MB; proved by a release-build timing test run on a Mac, with the figures recorded under this task.
  - 2026-11-25 (UTC), release build on a Mac with an M4 Pro, 4 CPUs and 8 GiB given to the run, `page_round_trip_on_a_transcript_of_fifty_megabytes`, three runs.
  - Each run made 400 requests for the 100 entries before an entry of either list, at pseudo-random places past the first 150 turns.
    It timed each from the request written to the `Page` decoded, over a socket pair to a thread that reads the page from disk.
  - The invented transcript has 9,491 turns of 50 MiB: a prompt, a reply with one call, and the call's result, whose size is 120 KB every 50th turn, 12 KB for 5 of the next 49 and 1.5 KB for the rest.
    The page size of 100 stays.
    - Median 2.0 ms, p95 2.26 to 2.35 ms, slowest 2.46 to 2.89 ms.
  - Not measured: a transcript that is not in the file cache, and a page in a busy daemon with other windows asking.

## 2. Review

- [ ] 2.1 The README and the key help describe scrolling and loading as they now behave; proved by review.
- [ ] 2.2 The maintainer scrolls back through a long session in Rex and judges whether the loading shows; proved by their review of a preview binary they asked for.

[ci-run]: https://github.com/victor-software-house/rawr/actions/runs/45646806991

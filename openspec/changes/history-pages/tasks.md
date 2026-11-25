# Tasks

## 1. Pages

- [ ] 1.1 The daemon counts every entry of each list as it reads, and `Facts` carries the totals and whether older entries exist; the tabs show the totals or the count with `+`; proved by tests of the "Whole-session counts" scenarios, in CI.
- [ ] 1.2 `older` and `page` join the daemon protocol; the daemon reads a page back from the given entry, reads forward past its end for results, and keeps nothing; proved by tests of the "Older entries load in pages" and "Results across a page's edge" scenarios on fixture transcripts, in CI.
- [ ] 1.3 A window merges pages into its held lists, asks ahead by the lead, holds at most 3,000 entries a list and loads dropped entries again; proved by state tests of the "Pages load ahead of the view", "The view never draws what it does not hold", "Held entries are bounded" and "The filter covers held entries" scenarios, in CI.
- [ ] 1.4 A page of 100 entries answers within 20 ms at p95 on a transcript of 50 MB; proved by a release-build timing test run on a Mac, with the figures recorded under this task.

## 2. Review

- [ ] 2.1 The README and the key help describe scrolling and loading as they now behave; proved by review.
- [ ] 2.2 The maintainer scrolls back through a long session in Rex and judges whether the loading shows; proved by their review of a preview binary they asked for.

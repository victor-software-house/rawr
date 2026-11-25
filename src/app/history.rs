//! The entries of each list that a window holds for a transcript.
//!
//! The daemon's facts carry the newest [`KEEP`] entries of each list. A window
//! asks for older ones in pages as the view nears the oldest it holds, and for
//! newer ones to reload what it dropped. The held entries are the facts' and
//! the pages' together, by entry ID, and at most [`HELD`] of each list. The
//! window drops the ones farthest from the view first, so the held entries may
//! have gaps, each recorded after the newest entry below it.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::mem;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::app::facts::{EntryId, Facts, KEEP, List, Said, Way};

/// The most entries a window holds of one list.
pub(crate) const HELD: usize = 3000;

/// How many entries a page asks for.
pub(crate) const PAGE: usize = 100;

/// How far back the lines the view moved count toward the lead.
const LEAD_WINDOW: Duration = Duration::from_millis(250);

/// The fewest lines the lead is.
const LEAD_MIN: usize = 200;

/// The most entries each side of the view that are kept from being dropped.
const NEAR: usize = 1000;

/// The lines the view moved, either way, in the last [`LEAD_WINDOW`]: how far
/// ahead of the view a page must be asked for to land before the view reaches
/// its end.
#[derive(Debug, Default)]
pub(crate) struct Lead {
    moves: VecDeque<(Instant, usize)>,
    sum: usize,
}

impl Lead {
    /// The view was asked to scroll `lines` at `now`, up when below 0.
    pub(crate) fn scrolled(&mut self, lines: i64, now: Instant) {
        if lines != 0 {
            let moved = usize::try_from(lines.unsigned_abs()).unwrap_or(usize::MAX);
            self.moves.push_back((now, moved));
            self.sum = self.sum.saturating_add(moved);
        }
    }

    /// The lines to hold past the view's edges at `now`.
    pub(crate) fn lines(&mut self, now: Instant) -> usize {
        while let Some(&(at, moved)) = self.moves.front()
            && now.saturating_duration_since(at) > LEAD_WINDOW
        {
            self.moves.pop_front();
            self.sum -= moved;
        }
        self.sum.max(LEAD_MIN)
    }
}

/// Where a frame left the view in a list of entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reach {
    pub(crate) list: List,
    /// The newest and the oldest entry in view.
    pub(crate) first: EntryId,
    pub(crate) last: EntryId,
    /// The lines held past the view's bottom, and above its top, up to the
    /// edges of the run of held entries it is in.
    pub(crate) below: usize,
    pub(crate) above: usize,
    /// Whether a filter hides entries, so only the ones shown count.
    pub(crate) filtered: bool,
}

/// A page the window asks the daemon for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ask {
    pub(crate) way: Way,
    pub(crate) list: List,
    /// The entry the page reads from: it holds the entries just before or just
    /// after this one.
    pub(crate) at: EntryId,
    pub(crate) count: usize,
}

/// For each gap in the held entries, the newest entry held below it and how
/// many entries are not held in it.
pub(crate) type Gaps = BTreeMap<EntryId, usize>;

/// What the view needs to know of the entries that are not held.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape<'held> {
    pub(crate) gaps: &'held Gaps,
    /// How many entries older than the oldest held are not held.
    pub(crate) older: usize,
}

/// Keeps the held entries of the transcript `shown` alone: every other history
/// shrinks to the facts' newest entries, so the entries held stay bounded
/// across the sessions a window has shown.
pub(crate) fn keep_only(histories: &mut HashMap<PathBuf, History>, shown: &Path) {
    for (path, history) in histories {
        if path != shown {
            history.shrink();
        }
    }
}

/// A transcript's facts, with the entries of each list that the window holds.
#[derive(Debug, Default)]
pub(crate) struct History {
    /// The facts as the daemon sent them last, except that `log` and
    /// `activity` are the held entries.
    pub(crate) facts: Facts,
    lists: [Held; 2],
}

#[derive(Debug)]
struct Held {
    gaps: Gaps,
    /// Whether entries older than the oldest held exist.
    older: bool,
    /// The page asked for and not yet answered.
    flight: Option<Ask>,
    /// Where the last frame left the view, and the lead then.
    view: Option<(Reach, usize)>,
    /// Where the view stood when the last page failed, so the window asks
    /// again only once the view has moved.
    stalled: Option<Reach>,
}

impl Default for Held {
    fn default() -> Self {
        Self {
            gaps: Gaps::new(),
            older: true,
            flight: None,
            view: None,
            stalled: None,
        }
    }
}

impl History {
    /// Takes the daemon's facts in. Returns the entries whose layout is stale.
    pub(crate) fn merge(&mut self, mut read: Facts) -> Vec<EntryId> {
        let mut stale = Vec::new();
        for list in List::BOTH {
            let older = read.older(list);
            let incoming = mem::take(read.entries_mut(list));
            let kept = mem::take(self.facts.entries_mut(list));
            *read.entries_mut(list) =
                self.lists[list.index()].take(kept, incoming, older, &mut stale);
        }
        self.facts = read;
        stale
    }

    /// The page asked for in `list` has come. Returns the entries whose layout
    /// is stale: those it changed and those it dropped.
    pub(crate) fn land(&mut self, list: List, entries: Vec<Said>, more: bool) -> Vec<EntryId> {
        let held = &mut self.lists[list.index()];
        let Some(ask) = held.flight.take() else {
            return Vec::new();
        };
        held.land(self.facts.entries_mut(list), ask, entries, more)
    }

    /// A frame left the view at `reach`, with `lead` lines to hold past it.
    /// Returns the page to ask for, unless one is in flight already.
    pub(crate) fn settle(&mut self, reach: Reach, lead: usize) -> Option<Ask> {
        let held = &mut self.lists[reach.list.index()];
        held.view = Some((reach, lead));
        if held.stalled == Some(reach) {
            return None;
        }
        held.stalled = None;
        if held.flight.is_some() {
            return None;
        }
        let mine = self.facts.entries(reach.list);
        // The gap below the run the view is in, and the run's oldest entry.
        let below = held.gaps.range(..reach.last).next_back().map(|(id, _)| *id);
        let oldest = match below {
            Some(newest) => mine.get(mine.partition_point(|said| said.id <= newest)),
            None => mine.first(),
        }?;
        // A filter at the bound loads no more: a page that landed would only
        // push another out, and the entries older than the bound go unsearched.
        let room = !reach.filtered || mine.len() < HELD;
        let ask = if reach.below <= lead && room && (below.is_some() || held.older) {
            Ask {
                way: Way::Older,
                list: reach.list,
                at: oldest.id,
                count: PAGE,
            }
        } else if reach.above <= lead
            && let Some((newest, _)) = held.gaps.range(reach.first..).next()
        {
            Ask {
                way: Way::Newer,
                list: reach.list,
                at: *newest,
                count: PAGE,
            }
        } else {
            return None;
        };
        held.flight = Some(ask);
        Some(ask)
    }

    /// The page asked for in `list` could not be read. The list stays open, and
    /// the page is asked for again when the view moves.
    pub(crate) fn fail(&mut self, list: List) {
        let held = &mut self.lists[list.index()];
        if held.flight.take().is_some() {
            held.stalled = held.view.map(|(reach, _)| reach);
        }
    }

    /// The window shows another transcript: cuts the held entries back to the
    /// newest [`KEEP`] of the facts, and forgets the pages, the gaps and the
    /// view. Coming back to this one loads pages again.
    fn shrink(&mut self) {
        for list in List::BOTH {
            let mine = self.facts.entries_mut(list);
            mine.drain(..mine.len().saturating_sub(KEEP));
            self.lists[list.index()] = Held {
                older: self.facts.older(list),
                ..Held::default()
            };
        }
    }

    /// The daemon connection was lost, and with it the pages in flight.
    pub(crate) fn lost(&mut self) {
        for held in &mut self.lists {
            held.flight = None;
        }
    }

    /// Whether `list` has older entries than the held ones, which a filter does
    /// not search because the window holds as many as it may.
    pub(crate) fn unsearched(&self, list: List) -> bool {
        self.lists[list.index()].older && self.facts.entries(list).len() >= HELD
    }

    /// What the view needs to know of the entries of `list` not held.
    pub(crate) fn shape(&self, list: List) -> Shape<'_> {
        let held = &self.lists[list.index()];
        let missing: usize = held.gaps.values().sum();
        let have = self.facts.entries(list).len();
        let older = if held.older {
            self.facts.total(list).saturating_sub(have + missing)
        } else {
            0
        };
        Shape {
            gaps: &held.gaps,
            older,
        }
    }
}

impl Held {
    /// Joins the facts' `incoming` entries to the `kept` ones, which are the
    /// facts' earlier and the pages'. `older` is whether the facts say entries
    /// older than theirs exist.
    fn take(
        &mut self,
        kept: Vec<Said>,
        incoming: Vec<Said>,
        older: bool,
        stale: &mut Vec<EntryId>,
    ) -> Vec<Said> {
        if kept.is_empty() {
            self.gaps.clear();
            self.older = older;
            return incoming;
        }
        if !older {
            self.older = false;
        }
        let (Some(newest_held), Some(newest_new)) = (
            kept.last().map(|said| said.id),
            incoming.last().map(|said| said.id),
        ) else {
            return kept;
        };
        // A transcript written again numbers its entries again.
        if newest_new < newest_held {
            stale.extend(kept.iter().map(|said| said.id));
            self.gaps.clear();
            self.older = older;
            // A page in flight reads the old numbering.
            self.flight = None;
            self.view = None;
            self.stalled = None;
            return incoming;
        }
        let oldest_new = incoming.first().map(|said| said.id);
        let mut all: BTreeMap<EntryId, Said> =
            kept.into_iter().map(|said| (said.id, said)).collect();
        let mut shared = false;
        for said in incoming {
            if let Some(held) = all.get(&said.id) {
                shared = true;
                if *held != said {
                    stale.push(said.id);
                }
            }
            all.insert(said.id, said);
        }
        // Facts that share no entry with the held ones may leave entries
        // between them, which a page will tell.
        if !shared && oldest_new.is_some_and(|oldest| oldest > newest_held) {
            self.gaps.insert(newest_held, 0);
        }
        let mut merged: Vec<Said> = all.into_values().collect();
        self.trim(&mut merged, Way::Newer, None, stale);
        merged
    }

    /// Joins the page `entries`, the answer to `ask`, to `mine`.
    fn land(
        &mut self,
        mine: &mut Vec<Said>,
        ask: Ask,
        entries: Vec<Said>,
        more: bool,
    ) -> Vec<EntryId> {
        let mut stale = Vec::new();
        let (first, last) = (
            entries.first().map(|said| said.id),
            entries.last().map(|said| said.id),
        );
        let mut added = Vec::new();
        for said in entries {
            match mine.binary_search_by_key(&said.id, |held| held.id) {
                // The facts' entry is the live one, unless it lacks a result.
                Ok(at) => {
                    if mine[at].result.is_none() && said.result.is_some() {
                        stale.push(said.id);
                        mine[at] = said;
                    }
                }
                Err(_) => added.push(said),
            }
        }
        let fresh = added.len();
        match ask.way {
            Way::Older => {
                let below = self.gaps.range(..ask.at).next_back().map(|(id, _)| *id);
                if let Some(newest) = below {
                    if !more || first.is_none_or(|first| first <= newest) {
                        self.gaps.remove(&newest);
                    } else if let Some(missing) = self.gaps.get_mut(&newest) {
                        *missing = missing.saturating_sub(fresh);
                    }
                }
                if !more {
                    self.older = false;
                }
            }
            Way::Newer => {
                let next = mine
                    .get(mine.partition_point(|said| said.id <= ask.at))
                    .map(|said| said.id);
                if let Some(missing) = self.gaps.remove(&ask.at)
                    && more
                    && let (Some(last), Some(next)) = (last, next)
                    && last < next
                {
                    self.gaps.insert(last, missing.saturating_sub(fresh));
                }
            }
        }
        mine.extend(added);
        mine.sort_by_key(|said| said.id);
        self.trim(mine, ask.way, first.zip(last), &mut stale);
        stale
    }

    /// Drops entries from `mine` while it holds more than [`HELD`], the ones
    /// farthest from the view first, and on the side the view leaves, which is
    /// the one opposite the page that `landed`. The view and the lead past it
    /// stay, and so do the newest [`KEEP`] and the `page` that just landed,
    /// from its first entry to its last.
    fn trim(
        &mut self,
        mine: &mut Vec<Said>,
        landed: Way,
        page: Option<(EntryId, EntryId)>,
        stale: &mut Vec<EntryId>,
    ) {
        let len = mine.len();
        let excess = len.saturating_sub(HELD);
        if excess == 0 {
            return;
        }
        let index = |id: EntryId| mine.partition_point(|said| said.id < id);
        let ((lo, hi), lead) = match &self.view {
            Some((reach, lead)) => ((index(reach.last), index(reach.first).min(len - 1)), *lead),
            None => (
                match landed {
                    Way::Older => (0, 0),
                    Way::Newer => (len - 1, len - 1),
                },
                LEAD_MIN,
            ),
        };
        let margin = lead.min(NEAR);
        let pinned = len.saturating_sub(KEEP);
        let older_side = 0..lo.saturating_sub(margin).min(pinned);
        let newer_side = (hi.saturating_add(margin) + 1).min(pinned)..pinned;
        let order: Vec<usize> = match landed {
            Way::Older => newer_side.rev().chain(older_side).collect(),
            Way::Newer => older_side.chain(newer_side.rev()).collect(),
        };
        let page = page.map(|(first, last)| index(first)..=index(last));
        let mut gone = vec![false; len];
        for at in order
            .into_iter()
            .filter(|at| !page.as_ref().is_some_and(|page| page.contains(at)))
            .take(excess)
        {
            gone[at] = true;
        }
        // The entries left, and a gap after the newest of those below each run
        // dropped, which takes in the gaps inside the run.
        let mut kept: Vec<Said> = Vec::with_capacity(len);
        let mut dropped = 0;
        for (at, said) in mem::take(mine).into_iter().enumerate() {
            if gone[at] {
                dropped += 1 + self.gaps.remove(&said.id).unwrap_or(0);
                stale.push(said.id);
                continue;
            }
            if dropped > 0 {
                match kept.last() {
                    Some(below) => *self.gaps.entry(below.id).or_default() += dropped,
                    None => self.older = true,
                }
                dropped = 0;
            }
            kept.push(said);
        }
        *mine = kept;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ops::Range;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::{Ask, HELD, History, LEAD_MIN, Lead, PAGE, Reach, keep_only};
    use crate::app::facts::{EntryId, Facts, Kind, List, Said, Way};

    /// The tool call number `place` of a transcript.
    fn said(place: usize) -> Said {
        Said {
            id: EntryId {
                line: u64::try_from(place).expect("a small number") * 100,
                index: 0,
            },
            at: None,
            text: format!("call {place}"),
            kind: Kind::Tool {
                name: "Read".to_owned(),
            },
            more: None,
            call: None,
            result: None,
        }
    }

    fn world(count: usize) -> Vec<Said> {
        (0..count).map(said).collect()
    }

    /// The facts of a transcript read whole, which keep the entries `window`.
    fn facts_of(all: &[Said], window: Range<usize>) -> Facts {
        Facts {
            activity: all[window].to_vec(),
            activity_total: all.len(),
            whole: true,
            ..Facts::default()
        }
    }

    /// What the daemon answers: the entries of `all` the ask names.
    fn answer(all: &[Said], ask: Ask) -> (Vec<Said>, bool) {
        match ask.way {
            Way::Older => {
                let end = all.partition_point(|said| said.id < ask.at);
                let from = end.saturating_sub(ask.count);
                (all[from..end].to_vec(), from > 0)
            }
            Way::Newer => {
                let from = all.partition_point(|said| said.id <= ask.at);
                let to = (from + ask.count).min(all.len());
                (all[from..to].to_vec(), to < all.len())
            }
        }
    }

    /// The view on the held entries `from` to `to`, by place among them.
    fn view(
        history: &History,
        (from, to): (usize, usize),
        (below, above): (usize, usize),
    ) -> Reach {
        let mine = &history.facts.activity;
        Reach {
            list: List::Activity,
            first: mine[to].id,
            last: mine[from].id,
            below,
            above,
            filtered: false,
        }
    }

    fn held(history: &History) -> usize {
        history.facts.activity.len()
    }

    #[test]
    fn the_lead_is_the_lines_scrolled_either_way_lately() {
        let mut lead = Lead::default();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        assert_eq!(lead.lines(start), LEAD_MIN);
        for ms in 0..40 {
            lead.scrolled(-30, at(ms));
            lead.scrolled(30, at(ms));
        }
        assert_eq!(lead.lines(at(40)), 2400);
        // The first 20 ms are past the window of 250 ms.
        assert_eq!(lead.lines(at(270)), 60 * 20);
        assert_eq!(lead.lines(at(500)), LEAD_MIN);
    }

    #[test]
    fn a_fast_scroll_asks_early() {
        let all = world(2400);
        let mut history = History::default();
        history.merge(facts_of(&all, 2100..2400));
        // The view moved 600 lines in the last 250 ms and holds 550 past its
        // bottom.
        let ask = history.settle(view(&history, (100, 110), (550, 100)), 600);
        assert_eq!(
            ask,
            Some(Ask {
                way: Way::Older,
                list: List::Activity,
                at: all[2100].id,
                count: PAGE,
            })
        );
    }

    #[test]
    fn a_slow_scroll_asks_at_200_lines() {
        let all = world(2400);
        let mut history = History::default();
        history.merge(facts_of(&all, 2100..2400));
        // 20 lines moved, so the lead is the least, 200, and 250 are held.
        assert_eq!(
            history.settle(view(&history, (100, 110), (250, 100)), LEAD_MIN),
            None
        );
        assert!(
            history
                .settle(view(&history, (100, 110), (200, 100)), LEAD_MIN)
                .is_some()
        );
    }

    #[test]
    fn one_page_of_a_list_is_in_flight_at_a_time() {
        let all = world(2400);
        let mut history = History::default();
        history.merge(facts_of(&all, 2100..2400));
        let near = view(&history, (0, 10), (0, 100));
        let ask = history.settle(near, LEAD_MIN).expect("a page is asked for");
        assert_eq!(history.settle(near, LEAD_MIN), None);
        let (entries, more) = answer(&all, ask);
        history.land(List::Activity, entries, more);
        assert_eq!(held(&history), 400);
        assert!(
            history
                .settle(view(&history, (0, 10), (0, 100)), LEAD_MIN)
                .is_some()
        );
        // A connection lost takes the pages in flight with it.
        history.lost();
        assert!(
            history
                .settle(view(&history, (0, 10), (0, 100)), LEAD_MIN)
                .is_some()
        );
    }

    #[test]
    fn showing_another_session_cuts_the_held_entries_back_to_the_facts() {
        let all = world(5000);
        let mut a = History::default();
        a.merge(facts_of(&all, 4700..5000));
        while held(&a) < HELD {
            let ask = a
                .settle(view(&a, (0, 39), (0, 1000)), LEAD_MIN)
                .expect("a page is asked for");
            let (entries, more) = answer(&all, ask);
            a.land(List::Activity, entries, more);
        }
        let other = world(400);
        let mut b = History::default();
        b.merge(facts_of(&other, 100..400));
        let mut histories =
            HashMap::from([(PathBuf::from("a.jsonl"), a), (PathBuf::from("b.jsonl"), b)]);
        // The window shows B: A holds the facts' newest 300 and nothing else.
        keep_only(&mut histories, Path::new("b.jsonl"));
        let a = &histories[Path::new("a.jsonl")];
        let newest: Vec<_> = all[4700..].iter().map(|said| said.id).collect();
        let kept: Vec<_> = a.facts.activity.iter().map(|said| said.id).collect();
        assert_eq!(kept, newest);
        assert!(a.shape(List::Activity).gaps.is_empty());
        assert_eq!(a.shape(List::Activity).older, 4700);
        assert_eq!(held(&histories[Path::new("b.jsonl")]), 300);
        // Coming back to A pages again.
        let a = histories
            .get_mut(Path::new("a.jsonl"))
            .expect("history of A");
        let ask = a.settle(view(a, (0, 39), (0, 1000)), LEAD_MIN);
        assert_eq!(ask.map(|ask| ask.way), Some(Way::Older));
    }

    #[test]
    fn a_filter_at_the_bound_neither_asks_for_a_page_again_nor_holds_more() {
        let all = world(5000);
        let mut history = History::default();
        history.merge(facts_of(&all, 4700..5000));
        let mut asked = Vec::new();
        // Every match is among the newest entries, as the view shows.
        loop {
            let newest = held(&history) - 1;
            let seen = Reach {
                filtered: true,
                ..view(&history, (newest - 9, newest), (0, 0))
            };
            let Some(ask) = history.settle(seen, LEAD_MIN) else {
                break;
            };
            assert!(!asked.contains(&ask.at), "asked twice for {:?}", ask.at);
            asked.push(ask.at);
            let (entries, more) = answer(&all, ask);
            let page = (entries[0].id, entries[entries.len() - 1].id);
            history.land(List::Activity, entries, more);
            assert!(held(&history) <= HELD);
            // The page that landed stays.
            let mine = &history.facts.activity;
            assert!(mine.iter().any(|said| said.id == page.0));
            assert!(mine.iter().any(|said| said.id == page.1));
            assert!(asked.len() < 100);
        }
        assert_eq!(asked.len(), 27);
        assert_eq!(held(&history), HELD);
        assert!(history.unsearched(List::Activity));
    }

    #[test]
    fn a_page_that_failed_leaves_the_list_open_until_the_view_moves() {
        let all = world(2400);
        let mut history = History::default();
        history.merge(facts_of(&all, 2100..2400));
        let near = view(&history, (0, 10), (0, 100));
        assert!(history.settle(near, LEAD_MIN).is_some());
        history.fail(List::Activity);
        // The same view asks for nothing, and the list has not ended.
        assert_eq!(history.settle(near, LEAD_MIN), None);
        assert_eq!(history.shape(List::Activity).older, 2100);
        // The view moves, and the page is asked for again.
        let moved = view(&history, (0, 10), (0, 99));
        assert!(history.settle(moved, LEAD_MIN).is_some());
    }

    #[test]
    fn a_page_at_the_start_ends_the_list() {
        let all = world(350);
        let mut history = History::default();
        history.merge(facts_of(&all, 50..350));
        let ask = history
            .settle(view(&history, (0, 10), (0, 100)), LEAD_MIN)
            .expect("a page is asked for");
        let (entries, more) = answer(&all, ask);
        assert!(!more);
        history.land(List::Activity, entries, more);
        assert_eq!(held(&history), 350);
        assert_eq!(
            history.settle(view(&history, (0, 10), (0, 100)), LEAD_MIN),
            None
        );
        assert_eq!(history.shape(List::Activity).older, 0);
    }

    #[test]
    fn scrolling_far_back_holds_no_more_than_the_bound() {
        let all = world(5000);
        let mut history = History::default();
        history.merge(facts_of(&all, 4700..5000));
        let mut pages = 0;
        // The view stays at the oldest entries held, as it does while it
        // scrolls back as fast as pages come.
        loop {
            let seen = view(&history, (0, 39), (0, 1000));
            let Some(ask) = history.settle(seen, LEAD_MIN) else {
                break;
            };
            assert_eq!(ask.way, Way::Older);
            let (entries, more) = answer(&all, ask);
            history.land(List::Activity, entries, more);
            pages += 1;
            let mine = &history.facts.activity;
            assert!(mine.len() <= HELD, "{} held", mine.len());
            // The newest 300 and the view stay.
            assert_eq!(mine.last().map(|said| said.id), Some(all[4999].id));
            assert!(mine.iter().any(|said| said.id == all[4700].id));
            assert!(mine.iter().any(|said| said.id == seen.first));
            assert!(mine.iter().any(|said| said.id == seen.last));
            assert!(pages < 100);
        }
        assert_eq!(held(&history), HELD);
        assert_eq!(history.facts.activity[0].id, all[0].id);
        assert_eq!(history.shape(List::Activity).older, 0);
    }

    #[test]
    fn dropped_entries_load_again_when_the_view_comes_back() {
        let all = world(5000);
        let mut history = History::default();
        history.merge(facts_of(&all, 4700..5000));
        while let Some(ask) = history.settle(view(&history, (0, 39), (0, 1000)), LEAD_MIN) {
            let (entries, more) = answer(&all, ask);
            history.land(List::Activity, entries, more);
        }
        let gap = |history: &History| history.lists[List::Activity.index()].gaps.clone();
        assert_eq!(gap(&history).len(), 1);
        // The view climbs to the newest entry below the gap.
        let mut pages = 0;
        while let Some((&newest, _)) = gap(&history).iter().next() {
            let mine = &history.facts.activity;
            let at = mine.partition_point(|said| said.id < newest);
            let seen = view(&history, (at - 39, at), (1000, 0));
            let ask = history.settle(seen, LEAD_MIN).expect("a page is asked for");
            assert_eq!((ask.way, ask.at), (Way::Newer, newest));
            let (entries, more) = answer(&all, ask);
            history.land(List::Activity, entries, more);
            pages += 1;
            assert!(held(&history) <= HELD);
            assert!(pages < 100);
        }
        // The entries between are held again, one after another.
        let mine = &history.facts.activity;
        assert!(
            mine.windows(2)
                .all(|pair| pair[1].id.line == pair[0].id.line + 100)
        );
        assert_eq!(mine.last().map(|said| said.id), Some(all[4999].id));
        assert!(history.shape(List::Activity).gaps.is_empty());
    }

    #[test]
    fn facts_that_slide_keep_the_entries_that_leave_them() {
        let all = world(400);
        let mut history = History::default();
        history.merge(facts_of(&all, 0..300));
        // A call arrives and one leaves the daemon's newest 300.
        let stale = history.merge(facts_of(&all, 1..301));
        assert_eq!(stale, Vec::new());
        assert_eq!(held(&history), 301);
        // A call whose result came later is changed, and its layout stale.
        let mut read = facts_of(&all, 1..301);
        read.activity[10].text = "changed".to_owned();
        assert_eq!(history.merge(read), [all[11].id]);
        assert_eq!(held(&history), 301);
        assert!(history.shape(List::Activity).gaps.is_empty());
    }

    #[test]
    fn facts_that_share_no_entry_with_the_held_ones_leave_a_gap_to_fill() {
        let all = world(1000);
        let mut history = History::default();
        history.merge(facts_of(&all, 0..300));
        history.merge(facts_of(&all, 700..1000));
        assert_eq!(held(&history), 600);
        let ask = history
            .settle(view(&history, (290, 299), (1000, 0)), LEAD_MIN)
            .expect("a page is asked for");
        assert_eq!((ask.way, ask.at), (Way::Newer, all[299].id));
        let (entries, more) = answer(&all, ask);
        history.land(List::Activity, entries, more);
        assert_eq!(held(&history), 700);
        // Four more pages follow: three of the missing entries, and one that
        // finds the held ones next to its last.
        let mut pages = 0;
        while !history.shape(List::Activity).gaps.is_empty() {
            let ask = history
                .settle(view(&history, (290, 299), (1000, 0)), LEAD_MIN)
                .expect("a page is asked for");
            let (entries, more) = answer(&all, ask);
            history.land(List::Activity, entries, more);
            pages += 1;
            assert!(pages < 10);
        }
        assert_eq!(pages, 4);
        assert_eq!(held(&history), 1000);
        assert!(history.shape(List::Activity).gaps.is_empty());
    }

    #[test]
    fn a_session_that_goes_on_never_pushes_the_held_entries_past_the_bound() {
        let all = world(4000);
        let mut history = History::default();
        for from in (0..=3700).step_by(100) {
            history.merge(facts_of(&all, from..from + 300));
            assert!(held(&history) <= HELD, "{} held", held(&history));
        }
        let mine = &history.facts.activity;
        assert_eq!(mine.last().map(|said| said.id), Some(all[3999].id));
        assert!(mine.iter().any(|said| said.id == all[3700].id));
    }

    #[test]
    fn a_transcript_written_again_starts_over() {
        let all = world(400);
        let mut history = History::default();
        history.merge(facts_of(&all, 100..400));
        let ask = history
            .settle(view(&history, (0, 10), (0, 100)), LEAD_MIN)
            .expect("a page is asked for");
        let again = world(50);
        let stale = history.merge(facts_of(&again, 0..50));
        assert_eq!(stale.len(), 300);
        assert_eq!(held(&history), 50);
        // The page asked for in the old numbering is nobody's answer now.
        let (entries, more) = answer(&all, ask);
        assert_eq!(history.land(List::Activity, entries, more), Vec::new());
        assert_eq!(held(&history), 50);
    }

    #[test]
    fn a_page_nobody_asked_for_is_ignored() {
        let all = world(400);
        let mut history = History::default();
        history.merge(facts_of(&all, 100..400));
        assert_eq!(
            history.land(List::Activity, all[..100].to_vec(), true),
            Vec::new()
        );
        assert_eq!(held(&history), 300);
    }

    #[test]
    fn the_entries_not_held_are_counted() {
        let all = world(2400);
        let mut history = History::default();
        history.merge(facts_of(&all, 2100..2400));
        assert_eq!(history.shape(List::Activity).older, 2100);
        assert_eq!(history.facts.total(List::Activity), 2400);
    }
}

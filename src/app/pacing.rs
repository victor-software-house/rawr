//! Frame pacing: a fixed clock for the frames that move a list, and the
//! wheel reports that wait for it.
//!
//! A terminal hands a trackpad's wheel reports over in batches, and a
//! mouse wheel's at its own rate. Moving a list by each batch in the frame
//! that reads it gives frames spaced as the reports are, so the motion
//! follows the trackpad's jitter. Here the loop draws on a clock, spreads
//! each batch over the frames until the next is due, and places the
//! clock's beat between the reports of a fast stream.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::app::state::{self, Focus};

/// Frames a second while a list moves. 125 is the polling rate of a USB
/// mouse, so a held wheel never outruns the clock.
const FPS: u32 = 125;

/// How many frames of the clock a batch may wait for the one before it to
/// finish: a batch that comes within this many frames of the last is
/// spread over the frames until the next is due.
const CATCH_UP_FRAMES: usize = 2;

/// A report that comes this many periods after the last frame that moved a
/// list starts a gesture, and draws at once.
const REST_FRAMES: u32 = 2;

/// Reports less than this far apart came in one write, as a batch.
const TOGETHER: Duration = Duration::from_millis(2);

/// How many reports the beat's line is fitted to.
const BEAT_REPORTS: usize = 16;

/// The beat engages after this many reports of a stream.
const BEAT_FIRST_REPORTS: usize = 4;

/// Where in the gap between two reports the beat falls: 0 just after the
/// latest a report can come, 1 at the earliest the next can.
const BEAT_SHARE: f64 = 0.5;

/// The beat moves later by at most this much a frame, so no gap between
/// frames grows enough to leave a display refresh with nothing new.
const BEAT_STEP: Duration = Duration::from_micros(300);

/// A stream whose spacing is less than this many microseconds from the
/// clock's period counts as one at the clock's rate. Keep it at or under
/// [`BEAT_STEP`].
const BEAT_SAME_RATE_US: u32 = 300;

/// How far past the latest a report of a stream at the clock's rate came
/// the beat falls, in widths of the band its reads jitter over.
const BEAT_SAME_RATE_MARGIN: f64 = 0.5;

/// The lists a frame can move, each with its own queue of reports.
const LISTS: usize = Focus::LISTS.len();

/// The lines each list moves in one frame, in the order of
/// [`Focus::LISTS`].
pub(crate) type Moves = [i64; LISTS];

/// The fixed frame clock: ticks at an even period, whatever else wakes the
/// loop.
#[derive(Debug)]
pub(crate) struct Clock {
    period: Duration,
    next: Instant,
}

impl Clock {
    pub(crate) fn new(fps: u32, now: Instant) -> Self {
        Self {
            period: Duration::from_secs_f64(1.0 / f64::from(fps)),
            next: now,
        }
    }

    pub(crate) fn next(&self) -> Instant {
        self.next
    }

    pub(crate) fn period(&self) -> Duration {
        self.period
    }

    /// A frame drawn outside the clock moves the beat to one period after
    /// it, so frames stay evenly spaced instead of landing a tick apart and
    /// then a tick and a half.
    pub(crate) fn rephase(&mut self, now: Instant) {
        self.next = now + self.period;
    }

    /// A wake after a rest keeps the beat: the tick comes no sooner than one
    /// period after the last, and the beat counts from it.
    ///
    /// Without it a batch that came just after a tick nobody waited for
    /// drew at once and the next tick stayed on the old beat.
    /// So a batch's two frames came 6 to 10 ms apart,
    /// and a 120 Hz display showed no motion on some refreshes.
    pub(crate) fn wake(&mut self, now: Instant) {
        self.next = self.next.max(now);
    }

    /// Move the next tick toward `at`, where a fast wheel stream's beat falls:
    /// at once when that is sooner, else by at most `step` a frame, so no
    /// gap grows enough to leave a display refresh with nothing new.
    pub(crate) fn align(&mut self, at: Instant, step: Duration) {
        self.next = at.min(self.next + step);
    }

    /// When a tick is due at `now`, the tick after it, without moving there:
    /// one period on, or one period after `now` when the clock fell behind.
    pub(crate) fn peek(&self, now: Instant) -> Option<Instant> {
        if now < self.next {
            return None;
        }
        let after = self.next + self.period;
        Some(if after < now {
            now + self.period
        } else {
            after
        })
    }

    /// Move to `after`, the tick `peek` gave.
    pub(crate) fn take(&mut self, after: Instant) {
        self.next = after;
    }

    /// True when a tick is due; moves to the following tick.
    #[cfg(test)]
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let Some(after) = self.peek(now) else {
            return false;
        };
        self.take(after);
        true
    }
}

/// Wheel reports that wait for a frame of the clock to move a list.
///
/// A terminal hands a trackpad's reports over in batches,
/// several at one moment, once per event of the trackpad driver.
/// Applying each batch in the next frame moves the list by a dozen lines,
/// then by none, when the batches come slower than the clock.
///
/// A batch that comes within `catch_up_frames` frames of the one before
/// is spread over the frames until the next is due instead.
/// Each of its reports is shown within as many frames as the gap.
///
/// So batches every other frame move the list in even halves,
/// and batches once a frame move it on the next frame.
///
/// Any other batch, the first of a gesture, moves the list on the next frame,
/// so motion starts with its biggest step and stops at most
/// `catch_up_frames` frames after the last report.
///
/// Each frame takes the fewest whole reports
/// that keep every report within its frames, so the steps stay even.
/// Reports read since the last frame that span `together` or more
/// are a stream faster than the clock, not a batch.
/// That frame takes every report queued.
#[derive(Debug, Default)]
pub(crate) struct WheelQueue {
    reports: VecDeque<WheelReport>,
    stepped: Option<Instant>,
    /// When the last batch began.
    moment: Option<Instant>,
    /// The frames the last batch's reports must be shown within.
    within: usize,
}

#[derive(Debug)]
struct WheelReport {
    rows: i32,
    at: Instant,
    /// Frames stepped since the report was queued.
    frames: usize,
    within: usize,
}

impl WheelQueue {
    /// Queue a report of `rows` read at `at`.
    pub(crate) fn push(&mut self, rows: i32, at: Instant) {
        self.reports.push_back(WheelReport {
            rows,
            at,
            frames: 0,
            within: 1,
        });
    }

    pub(crate) fn pending(&self) -> bool {
        !self.reports.is_empty()
    }

    /// Drops the reports waiting for a frame.
    pub(crate) fn clear(&mut self) {
        self.reports.clear();
    }

    /// The reports waiting for a frame.
    pub(crate) fn queued(&self) -> usize {
        self.reports.len()
    }

    /// True when a report no frame has stepped yet came less than
    /// `together` after the batch the last step began, as the rest of a
    /// batch a terminal handed over in two reads does.
    pub(crate) fn joins_batch(&self, together: Duration) -> bool {
        self.moment.is_some_and(|m| {
            self.reports
                .iter()
                .any(|r| r.frames == 0 && r.at.saturating_duration_since(m) < together)
        })
    }

    /// The signed rows to move in the frame drawn at `now`, on a clock of
    /// `period`. Reports less than `together` apart came at one moment, as
    /// a batch.
    pub(crate) fn step(
        &mut self,
        catch_up_frames: usize,
        together: Duration,
        period: Duration,
        now: Instant,
    ) -> i64 {
        for r in &mut self.reports {
            if r.frames == 0 {
                if self
                    .moment
                    .is_none_or(|m| r.at.saturating_duration_since(m) >= together)
                {
                    let gap = self.moment.map(|m| {
                        usize::try_from(
                            (r.at.saturating_duration_since(m) + period / 2).as_nanos()
                                / period.as_nanos(),
                        )
                        .unwrap_or(usize::MAX)
                    });
                    // The first batch of a gesture has no gap within the
                    // catch-up frames.
                    self.within = gap
                        .filter(|&n| n <= catch_up_frames)
                        .map_or(1, |n| n.max(1));
                    self.moment = Some(r.at);
                }
                r.within = self.within;
            }
            r.frames += 1;
        }
        let mut fresh = self
            .reports
            .iter()
            .map(|r| r.at)
            .filter(|&at| self.stepped.is_none_or(|s| at >= s));
        let first = fresh.next();
        let stream = first
            .zip(fresh.next_back())
            .is_some_and(|(first, last)| last.saturating_duration_since(first) >= together);
        self.stepped = Some(now);
        // The first `i + 1` reports go in the frames report `i` has left.
        let reports = if stream {
            self.reports.len()
        } else {
            self.reports
                .iter()
                .enumerate()
                .map(|(i, r)| (i + 1).div_ceil((r.within + 1).saturating_sub(r.frames).max(1)))
                .max()
                .unwrap_or(0)
        };
        (0..reports)
            .filter_map(|_| self.reports.pop_front())
            .map(|r| i64::from(r.rows))
            .sum()
    }
}

/// Where the clock ticks in a wheel stream as fast as it or faster.
///
/// A tick that lands on a report takes it or leaves it for the next frame by a
/// few microseconds of jitter, so a frame takes one report fewer or more than
/// the one before.
///
/// Once a stream gives a frame two reports or more at a steady spacing, the
/// beat moves into the gap between two reports, [`BEAT_SHARE`] of the way from
/// the latest a report can come to the earliest the next can.
///
/// The spacing and each report's offset from it are fitted to the read times
/// of the last [`BEAT_REPORTS`] reports.
/// So the gap is what the jitter leaves of the spacing.
///
/// A stream whose spacing is less than [`BEAT_SAME_RATE_US`] from the
/// period gives a frame one report, and the clock rests between reports.
/// So a report read past its tick draws on its read,
/// and the frame gaps take the reads' jitter.
///
/// Its beat moves into the gap after each report,
/// [`BEAT_SAME_RATE_MARGIN`] times the jitter past the latest a report can
/// come, and by at most [`BEAT_STEP`] a frame either way.
/// So every report waits for an even tick.
///
/// Any other stream keeps its tick.
/// The beat engages after [`BEAT_FIRST_REPORTS`].
/// A stream after a rest whose first two reports keep the spacing of the last
/// stream that filled the window takes that stream's spacing and jitter at
/// once.
#[derive(Debug, Default)]
pub(crate) struct WheelBeat {
    reads: VecDeque<Instant>,
    /// The spacing and the width of the jitter about it, in seconds, of the
    /// last stream that filled the window. A stream after a rest starts from it
    /// on its first report, and keeps it while its reads stay within that
    /// width.
    known: Option<(f64, f64)>,
}

impl WheelBeat {
    /// Forget the reports before a rest; keep what the last full window found.
    pub(crate) fn restart(&mut self) {
        self.reads.clear();
    }

    /// A report read at `at`, keeping the last [`BEAT_REPORTS`].
    pub(crate) fn read(&mut self, at: Instant) {
        self.reads.push_back(at);
        while self.reads.len() > BEAT_REPORTS {
            self.reads.pop_front();
        }
    }

    /// The tick near `next` that falls in the gap between reports, by the rule
    /// above for a clock of `period`, or None while the stream is too short
    /// to know, jitters across its whole spacing, or keeps its tick.
    pub(crate) fn tick_near(&mut self, next: Instant, period: Duration) -> Option<Instant> {
        let first = *self.reads.front()?;
        let ys: Vec<f64> = self
            .reads
            .iter()
            .map(|at| at.duration_since(first).as_secs_f64())
            .collect();
        let (base, spacing, lo, hi, band) = if ys.len() >= BEAT_REPORTS {
            let (base, spacing, lo, hi) = fit(&ys, None);
            self.known = Some((spacing, hi - lo));
            (base, spacing, lo, hi, hi - lo)
        } else if let Some((spacing, band)) = self.known
            && let (base, _, lo, hi) = fit(&ys, Some(spacing))
            && hi - lo <= band
        {
            (base, spacing, lo, hi, band)
        } else if ys.len() >= BEAT_FIRST_REPORTS {
            let (base, spacing, lo, hi) = fit(&ys, None);
            (base, spacing, lo, hi, hi - lo)
        } else {
            return None;
        };
        // The reads sit somewhere in a band `band` wide: its top is no later
        // than `lo + band`, and the next report's band starts no sooner
        // than `hi - band + spacing`.
        let gap = spacing - 2.0 * band + (hi - lo);
        let frame = period.as_secs_f64();
        let twice = (frame / spacing).round() >= 2.0;
        let same_rate = (spacing - frame).abs() * 1e6 < f64::from(BEAT_SAME_RATE_US);
        if spacing <= 0.0 || gap <= 0.0 || !(twice || same_rate) {
            return None;
        }
        if !twice {
            // The tick before a report never fires while the clock rests, so
            // the tick is the one after the report still to come. A
            // tick too late only keeps one report waiting, so it moves sooner
            // no faster than later.
            let offset = base + lo + band + (BEAT_SAME_RATE_MARGIN * band).min(BEAT_SHARE * gap);
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "a count of wheel reports in one batch, far below 2^53"
            )]
            let tick = first + Duration::from_secs_f64(offset + ys.len() as f64 * spacing);
            #[expect(
                clippy::unchecked_time_subtraction,
                reason = "next is a tick on a monotonic clock that began long before the app, and the step is a fraction of a frame"
            )]
            let floor = next - BEAT_STEP;
            return Some(tick.max(floor));
        }
        let offset = base + lo + band + BEAT_SHARE * gap;
        let k = (next.duration_since(first).as_secs_f64() - offset) / spacing;
        // On a stream's first report the tick is a period after it, on the
        // report two spacings on. The gap before that report would leave the
        // next frame one report, so the tick moves toward the gap after it.
        let k = if ys.len() == 1 { k.ceil() } else { k.round() };
        Some(first + Duration::from_secs_f64(offset + k * spacing))
    }
}

/// A least-squares line through (report, read time) for read times `ys` in
/// seconds, with the slope `spacing` when given: its value at the first report,
/// its slope, and the lowest and highest residual.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a count of wheel reports in one batch, far below 2^53"
)]
fn fit(ys: &[f64], spacing: Option<f64>) -> (f64, f64, f64, f64) {
    let n = ys.len() as f64;
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = ys.iter().sum::<f64>() / n;
    let spacing = spacing.unwrap_or_else(|| {
        let (sxy, sxx) = ys
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(sxy, sxx), (i, y)| {
                let dx = i as f64 - mean_x;
                (sxy + dx * (y - mean_y), sxx + dx * dx)
            });
        sxy / sxx
    });
    let base = mean_y - spacing * mean_x;
    let (lo, hi) = ys
        .iter()
        .enumerate()
        .map(|(i, y)| y - base - spacing * i as f64)
        .fold((f64::MAX, f64::MIN), |(lo, hi), r| (lo.min(r), hi.max(r)));
    (base, spacing, lo, hi)
}

/// A wheel report: the list it scrolls, how many lines, and when the input
/// thread read it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Report {
    pub(crate) list: Focus,
    pub(crate) rows: i32,
    pub(crate) at: Instant,
}

/// What the loop woke with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wake<'a> {
    /// When the loop woke.
    pub(crate) now: Instant,
    /// The wheel reports read since the last wake, oldest first.
    pub(crate) reports: &'a [Report],
    /// A key or a click asks for a frame at once.
    pub(crate) urgent: bool,
    /// Something changed that the next frame shows, such as a message from
    /// the daemon, the pointer's hover or a spinner's turn. It draws on the
    /// clock's next tick: at once when the clock rests, since its tick is
    /// then due.
    pub(crate) dirty: bool,
    /// Focus or a drawer is on the move. It draws every 16 ms, a change waits
    /// for those frames, and the wake after the move ends draws its last frame.
    pub(crate) moving: bool,
}

/// How long a frame that moves focus or a drawer waits for the next.
const MOVE_FRAME: Duration = Duration::from_millis(16);

/// A frame the wheel moves sleeps until this long before its tick, then spins
/// to the tick, so it draws on the beat whatever the sleep overshoots by.
pub(crate) const SPIN_LEAD: Duration = Duration::from_micros(1400);

/// A sleep shorter than this costs more than it saves, so the loop spins
/// instead.
pub(crate) const MIN_SLEEP: Duration = Duration::from_micros(200);

/// When the loop next wakes with nothing to read, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Wait {
    pub(crate) at: Instant,
    /// Whether to sleep until [`SPIN_LEAD`] before then and spin the rest. Only
    /// a frame the wheel moves does; every other frame only sleeps.
    pub(crate) spin: bool,
}

/// The loop's pacing decisions: when it wakes, whether a wake draws, and how
/// many lines each list moves in the frame.
#[derive(Debug)]
pub(crate) struct Pacer {
    clock: Clock,
    beat: WheelBeat,
    queues: [WheelQueue; LISTS],
    /// When the last frame drew.
    drew: Instant,
    /// When the last frame that moved a list drew.
    moved: Option<Instant>,
    /// Whether something changed since the last frame, which waits for the
    /// clock's tick.
    dirty: bool,
    /// Whether focus or a drawer was on the move at the last wake.
    was_moving: bool,
}

impl Pacer {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            clock: Clock::new(FPS, now),
            beat: WheelBeat::default(),
            queues: Default::default(),
            drew: now,
            moved: None,
            dirty: false,
            was_moving: false,
        }
    }

    /// Whether any list has a report waiting for a frame.
    fn pending(&self) -> bool {
        self.queues.iter().any(WheelQueue::pending)
    }

    /// A scroll was set another way, so the wheel reports read before it, for
    /// the lists whose scroll it was, no longer apply: those in the queues and
    /// those in `reports`, which this wake has yet to queue. Reports read
    /// after it stay.
    pub(crate) fn reset(&mut self, lists: &[Focus], reports: &mut Vec<Report>) {
        for list in lists {
            self.queues[state::slot(*list)].clear();
        }
        reports.retain(|report| !lists.contains(&report.list));
    }

    /// When the loop must wake next, and how: at the clock's tick while a
    /// report waits or a change waits for it, a frame of a move on from the
    /// last frame while `moving`, and at each of `others`, such as the
    /// spinner's next turn. At rest, never.
    pub(crate) fn wait(&self, moving: bool, others: &[Option<Instant>]) -> Option<Wait> {
        // A change waits for a move's own frames while one runs.
        let tick = (self.pending() || (self.dirty && !moving)).then(|| self.clock.next());
        let frame = moving.then(|| self.drew + MOVE_FRAME);
        let other = others.iter().flatten().copied().min();
        let soonest = [tick, frame, other].into_iter().flatten().min()?;
        Some(Wait {
            at: soonest,
            spin: self.pending() && tick == Some(soonest),
        })
    }

    /// The loop woke: queue the wheel reports it read, and decide whether to
    /// draw. A frame comes with the lines each list moves in it.
    ///
    /// A report that ends a rest, and a key or a click, draw at once; a frame
    /// drawn outside the clock moves its beat. Reports that come while the
    /// clock runs wait for its next tick, which draws when a report waits.
    /// So does a change, such as a message from the daemon.
    pub(crate) fn wake(&mut self, wake: &Wake<'_>) -> Option<Moves> {
        let now = wake.now;
        let period = self.clock.period();
        let clock_resting = !self.pending();
        let mut restarted = false;
        for report in wake.reports {
            let resting = !self.pending()
                && self.moved.is_none_or(|at| {
                    report.at.saturating_duration_since(at) >= period * REST_FRAMES
                });
            if resting {
                self.beat.restart();
            }
            self.beat.read(report.at);
            self.queues[state::slot(report.list)].push(report.rows, report.at);
            restarted |= resting;
        }
        // A batch that ends a rest is spread over frames counted from the
        // wake that read it. The rest of a batch read in two parts keeps the
        // beat of the frame its first part drew. A single report keeps the
        // beat: counting from each late read moved the beat later through a
        // held wheel, and every report read before it waited, spinning, for
        // the latest read so far.
        if clock_resting {
            if self.queues.iter().any(|queue| queue.joins_batch(TOGETHER)) {
                self.clock.rephase(self.drew);
            } else if self.queues.iter().map(WheelQueue::queued).sum::<usize>() >= 2 {
                self.clock.wake(now);
            }
        }
        // A move draws every `MOVE_FRAME`, and once more when it ends, which
        // shows where it stopped. Until then a change waits for those frames.
        let move_frame = wake.moving && now >= self.drew + MOVE_FRAME;
        let settled = self.was_moving && !wake.moving;
        self.was_moving = wake.moving;
        self.dirty |= wake.dirty || move_frame || settled;
        let changed = self.dirty && (!wake.moving || move_frame);
        // A tick due now is taken whether or not it draws.
        let tick = self.clock.peek(now);
        let on_clock = tick.is_some() && (self.pending() || changed);
        let at_once = wake.urgent || restarted;
        if let Some(after) = tick {
            self.clock.take(after);
        }
        if !on_clock && !at_once {
            return None;
        }
        let mut moves = Moves::default();
        for (queue, rows) in self.queues.iter_mut().zip(&mut moves) {
            *rows = queue.step(CATCH_UP_FRAMES, TOGETHER, period, now);
        }
        self.drew = now;
        self.dirty = false;
        let scrolled = moves.iter().any(|rows| *rows != 0);
        if scrolled {
            self.moved = Some(now);
        }
        if restarted || (at_once && !on_clock) {
            self.clock.rephase(now);
        }
        if scrolled && let Some(at) = self.beat.tick_near(self.clock.next(), period) {
            self.clock.align(at, BEAT_STEP);
        }
        Some(moves)
    }
}

#[cfg(test)]
mod tests;

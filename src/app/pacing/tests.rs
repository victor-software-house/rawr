use std::mem;
use std::time::{Duration, Instant};

use super::{
    BEAT_REPORTS, BEAT_STEP, Clock, FPS, Focus, Pacer, Report, Wait, Wake, WheelBeat, WheelQueue,
    state,
};

/// Lines one wheel report moves in these tests.
const LINES: i32 = 2;

/// The first of `batches` of `(µs, n, rows)` is read before the first 8 ms beat
/// after it. Pushes each of them, and steps the queue on every beat up to
/// `until_ms`; the rows each beat moved.
fn play_wheel(
    q: &mut WheelQueue,
    catch_up: usize,
    batches: &[(u64, usize, i32)],
    until_ms: u64,
) -> Vec<i64> {
    let (together, period) = (Duration::from_micros(2000), Duration::from_millis(8));
    let t0 = Instant::now();
    let mut batches = batches.iter().peekable();
    (1..=until_ms / 8)
        .map(|k| {
            while let Some(&(us, n, rows)) = batches.next_if(|b| b.0 < k * 8000) {
                (0..n).for_each(|_| q.push(rows, t0 + Duration::from_micros(us)));
            }
            q.step(
                catch_up,
                together,
                period,
                t0 + Duration::from_millis(k * 8),
            )
        })
        .collect()
}

/// The first batch of a gesture moves the list on the next frame. A batch that
/// comes within `catch_up_frames` of the one before is shared evenly over the
/// frames of that gap.
#[test]
fn a_wheel_batch_is_spread_until_the_next_is_due() {
    let mut q = WheelQueue::default();
    // 4 reports, then 24 reports 48 ms later: 6 frames of the clock, within the
    // 8 of catch-up.
    assert_eq!(
        play_wheel(&mut q, 8, &[(1000, 4, 3), (49000, 24, -3)], 104),
        [12, 0, 0, 0, 0, 0, -12, -12, -12, -12, -12, -12, 0]
    );
}

/// A batch, even one the reader splits, is paced, and reports since the last
/// frame that span `together` or more drain in one frame.
#[test]
fn a_stream_faster_than_the_clock_drains_in_one_frame() {
    let mut q = WheelQueue::default();
    let batches = [
        (1000, 4, 3), // the first batch: at once
        (18000, 1, 3),
        (20000, 1, 3),
        (22000, 1, 3),
        (23900, 1, 3), // 2 ms apart: a stream, at once
        (47000, 1, 3), // a lone report after a rest
        (62000, 3, 3),
        (63000, 3, 3), // 15 ms later, split over two reads 1 ms apart: one batch, in halves
    ];
    assert_eq!(
        play_wheel(&mut q, 2, &batches, 88),
        [12, 0, 12, 0, 0, 3, 0, 9, 9, 0, 0]
    );
}

/// As a gesture slows, the steps shrink with its batches, and the list stops
/// moving within `catch_up_frames` frames of the last batch. Taking a share of
/// what was left on every frame moved a batch of eight as 4, 2, 1 and 1
/// reports.
///
/// So the list crept on one report a frame after the fingers stopped, and it
/// held batches that came once a frame a frame late.
#[test]
fn a_wheel_gesture_stops_within_the_catch_up_frames() {
    // Batches of 12, 9, 6 and 3 every 16 ms, as a 60 Hz trackpad sends a
    // slowing swipe.
    let mut q = WheelQueue::default();
    assert_eq!(
        play_wheel(
            &mut q,
            2,
            &[(1000, 12, 1), (17000, 9, 1), (33000, 6, 1), (49000, 3, 1)],
            72
        ),
        [12, 0, 5, 4, 3, 3, 2, 1, 0]
    );
    // Batches of 8, 7 and 6 once a frame, as a 120 Hz trackpad sends them.
    let mut q = WheelQueue::default();
    assert_eq!(
        play_wheel(&mut q, 2, &[(1000, 8, 1), (9000, 7, 1), (17000, 6, 1)], 32),
        [8, 7, 6, 0]
    );
}

/// A tick that comes late in a run keeps the beat, and a wake after a rest
/// counts the beat from the wake. So a batch that comes just after a tick
/// nobody waited for gets its next frame one period later, not on the old
/// beat.
#[test]
fn a_wake_after_a_rest_counts_the_beat_from_the_wake() {
    let us = Duration::from_micros;
    let t0 = Instant::now();
    let mut clock = Clock::new(FPS, t0);
    assert!(clock.tick(t0));
    assert!(clock.tick(t0 + us(9000)));
    assert_eq!(clock.next(), t0 + us(16000));
    // The queue emptied at 8 ms and the next batch comes at 16.7 ms.
    clock.wake(t0 + us(16700));
    assert!(clock.tick(t0 + us(16700)));
    assert_eq!(clock.next(), t0 + us(24700));
    // A wake before the beat waits for it.
    clock.wake(t0 + us(20000));
    assert!(!clock.tick(t0 + us(20000)));
    assert_eq!(clock.next(), t0 + us(24700));
}

/// The loop looks at a due tick before it takes it, so input that stops a frame
/// build keeps the tick for the build that follows. A tick far behind moves the
/// beat to a period after now.
#[test]
fn a_peeked_tick_stays_due_until_it_is_taken() {
    let us = Duration::from_micros;
    let t0 = Instant::now();
    let mut clock = Clock::new(FPS, t0);
    assert_eq!(clock.peek(t0), Some(t0 + us(8000)));
    assert_eq!(clock.peek(t0 + us(10)), Some(t0 + us(8000)));
    assert_eq!(clock.next(), t0);
    clock.take(t0 + us(8000));
    assert_eq!(clock.peek(t0 + us(7999)), None);
    assert_eq!(clock.peek(t0 + us(30_000)), Some(t0 + us(38_000)));
}

/// What reaches the loop.
#[derive(Debug, Clone, Copy)]
enum Input {
    /// A wheel report of this many lines.
    Wheel(i32),
    /// A key.
    Key,
    /// A message from the daemon.
    Daemon,
}

/// What else is on screen, as offsets from the start.
#[derive(Debug, Default, Clone, Copy)]
struct Scene {
    /// Focus is on the move until then.
    moving_until: Duration,
    /// A pane is at work until then, so the spinner turns.
    spinner_until: Duration,
}

/// How often the spinner turns.
const SPIN: Duration = Duration::from_millis(100);

/// Plays `script` through `pacer` as the loop does, from `t0`: it wakes at the
/// next input when that comes before the pacer's own wake, and at that wake
/// otherwise, and an input wake takes the inputs at that moment. Returns each
/// frame drawn: when, and the lines it moved the list.
fn play_scene(
    pacer: &mut Pacer,
    t0: Instant,
    script: &[(Instant, Input)],
    scene: Scene,
) -> Vec<(Instant, i64)> {
    let spin_at = |at: Instant| {
        usize::try_from(at.saturating_duration_since(t0).as_millis() / SPIN.as_millis())
            .unwrap_or(0)
    };
    let mut script = script.iter().copied().peekable();
    let mut frames = Vec::new();
    let mut now = t0;
    let mut drawn_spin = usize::MAX;
    for _ in 0..100_000 {
        let spinner = (now < t0 + scene.spinner_until)
            .then(|| t0 + SPIN * u32::try_from(spin_at(now) + 1).expect("a small number"));
        let wait = pacer.wait(now < t0 + scene.moving_until, &[spinner]);
        let next = script.peek().map(|input| input.0);
        let (at, by_input) = match (next, wait) {
            (Some(input), Some(wait)) if input >= wait.at => (wait.at, false),
            (Some(input), _) => (input, true),
            (None, Some(wait)) => (wait.at, false),
            (None, None) => return frames,
        };
        let mut reports = Vec::new();
        let (mut urgent, mut dirty) = (false, false);
        while by_input && let Some((read, input)) = script.next_if(|input| input.0 <= at) {
            match input {
                Input::Wheel(rows) => reports.push(Report {
                    list: Focus::Session,
                    rows,
                    at: read,
                }),
                Input::Key => urgent = true,
                Input::Daemon => dirty = true,
            }
        }
        let spun = at < t0 + scene.spinner_until && spin_at(at) != drawn_spin;
        let wake = Wake {
            now: at,
            reports: &reports,
            urgent,
            dirty: dirty || spun,
            moving: at < t0 + scene.moving_until,
        };
        if let Some(moves) = pacer.wake(&wake) {
            frames.push((at, moves[state::slot(Focus::Session)]));
            drawn_spin = spin_at(at);
        }
        now = at;
    }
    panic!("the pacer never came to rest");
}

/// Plays wheel `reads` (when each was read, and its lines) through `pacer`.
fn play(pacer: &mut Pacer, reads: &[(Instant, i32)]) -> Vec<(Instant, i64)> {
    let t0 = reads.first().map_or_else(Instant::now, |read| read.0);
    let script: Vec<_> = reads
        .iter()
        .map(|&(at, rows)| (at, Input::Wheel(rows)))
        .collect();
    play_scene(pacer, t0, &script, Scene::default())
}

/// The frames of a gesture, played from a fresh clock, and the pacer at its
/// end. Times come as offsets from the start.
fn gesture(reads: &[(Duration, i32)]) -> (Vec<(Duration, i64)>, Pacer) {
    let t0 = Instant::now();
    let mut pacer = Pacer::new(t0);
    let reads: Vec<_> = reads.iter().map(|&(at, rows)| (t0 + at, rows)).collect();
    let frames = play(&mut pacer, &reads)
        .into_iter()
        .map(|(at, rows)| (at.duration_since(t0), rows))
        .collect();
    (frames, pacer)
}

/// The time from each frame to the next.
fn gaps(frames: &[(Duration, i64)]) -> Vec<Duration> {
    frames
        .windows(2)
        .map(|pair| pair[1].0.saturating_sub(pair[0].0))
        .collect()
}

/// `n` reports of `LINES` lines at `at`.
fn batch(at: Duration, n: usize) -> Vec<(Duration, i32)> {
    vec![(at, LINES); n]
}

/// While a trackpad's reports come at 120 a second, frames that move the list
/// come on the clock, 8 ms apart give or take 1 ms, and the clock rests once
/// the list has stopped.
#[test]
fn a_gesture_draws_evenly_spaced_frames() {
    let reads: Vec<_> = (0..48)
        .map(|i| (Duration::from_micros(8333) * i, LINES))
        .collect();
    let (frames, pacer) = gesture(&reads);
    assert_eq!(frames.len(), reads.len());
    for gap in gaps(&frames) {
        assert!(
            gap.abs_diff(Duration::from_millis(8)) <= Duration::from_millis(1),
            "{gap:?}"
        );
    }
    assert_eq!(pacer.wait(false, &[None]), None);
}

/// A batch of 6 reports every 16 ms moves 6 lines on each of the 2 frames
/// between batches, never 12 and then 0.
#[test]
fn batches_every_other_frame_move_in_halves() {
    let reads: Vec<_> = (0..8)
        .flat_map(|k| batch(Duration::from_millis(16) * k, 6))
        .collect();
    let (frames, _) = gesture(&reads);
    // The first batch moves at once; the rest are spread over the 2 frames
    // before the next.
    assert_eq!(frames[0].1, 12);
    let rest: Vec<i64> = frames[1..].iter().map(|frame| frame.1).collect();
    assert!(rest.len() >= 14, "{frames:?}");
    assert!(rest[..14].iter().all(|&lines| lines == 6), "{rest:?}");
}

/// A batch that comes after the list has rested moves it on the next frame,
/// all at once.
#[test]
fn the_first_batch_moves_at_once() {
    let rested = Duration::from_millis(1000);
    let mut reads = batch(Duration::ZERO, 1);
    reads.extend(batch(rested, 4));
    let (frames, _) = gesture(&reads);
    assert_eq!(frames.last(), Some(&(rested, 8)), "{frames:?}");
}

/// The list moves no more than 2 frames after the last report.
#[test]
fn the_list_stops_soon_after_the_last_report() {
    let last = Duration::from_millis(16) * 7;
    let reads: Vec<_> = (0..8)
        .flat_map(|k| batch(Duration::from_millis(16) * k, 6))
        .collect();
    let (frames, _) = gesture(&reads);
    let (end, _) = *frames.last().expect("a frame");
    assert!(
        end <= last + Duration::from_millis(16),
        "{end:?} after {last:?}"
    );
}

/// A trackpad at 120 reports a second against the 125 Hz clock gives each
/// frame one report, up to 1 ms of jitter in when it is read.
#[test]
fn a_trackpad_at_the_clocks_rate_moves_one_report_a_frame() {
    let reads: Vec<_> = (0..120)
        .map(|i| {
            (
                Duration::from_micros(8300) * i + jitter(i, Duration::from_millis(1)),
                LINES,
            )
        })
        .collect();
    let (frames, _) = gesture(&reads);
    assert_eq!(frames.len(), reads.len());
    assert!(
        frames.iter().all(|frame| frame.1 == i64::from(LINES)),
        "{frames:?}"
    );
}

/// How late report `i` is read: a fixed hash of `i` below `range`, so a run is
/// the same every time.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a hash scaled to 0 to 1; the low bits an f64 drops do not matter"
)]
fn jitter(i: u32, range: Duration) -> Duration {
    let mut x = u64::from(i).wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    range.mul_f64((x ^ (x >> 31)) as f64 / u64::MAX as f64)
}

/// A held wheel played through a clock and the pacer, with `beat` carried in
/// and out so a second hold starts from what the first found. Report `i` is
/// read `late(i)` after `spacing` times `i`. Returns each frame after the
/// first, which the first report draws at once: when it was drawn, and the
/// reports it took.
fn held_wheel_frames(
    beat: &mut WheelBeat,
    spacing: Duration,
    reports: u32,
    late: impl Fn(u32) -> Duration,
) -> Vec<(Instant, i64)> {
    let t0 = Instant::now();
    let mut pacer = Pacer::new(t0);
    pacer.beat = mem::take(beat);
    let reads: Vec<_> = (0..reports)
        .map(|i| (t0 + spacing * i + late(i), 1))
        .collect();
    let frames = play(&mut pacer, &reads);
    *beat = pacer.beat;
    frames.into_iter().skip(1).collect()
}

/// The reports each frame took in `held_wheel_frames`.
fn held_wheel(
    beat: &mut WheelBeat,
    spacing: Duration,
    reports: u32,
    late: impl Fn(u32) -> Duration,
) -> Vec<i64> {
    held_wheel_frames(beat, spacing, reports, late)
        .into_iter()
        .map(|(_, rows)| rows)
        .collect()
}

/// A wheel held at 4 ms, twice as fast as the 8 ms clock. With the tick on a
/// report, a few microseconds of jitter put that report in this frame or the
/// next, so the frames took one report, then three.
///
/// Once the beat knows the spacing it ticks between reports, and every frame
/// takes two, with reads jittered 50 µs either way or up to 2 ms late only.
///
/// A second hold after a rest starts from the first one's spacing on its first
/// report, so with reads 50 µs either way it is even from its first frame of
/// the clock.
#[test]
fn a_held_wheel_twice_as_fast_as_the_clock_moves_two_reports_a_frame() {
    let spacing = Duration::from_millis(4);
    let reports = 375;
    // The beat knows the stream after `BEAT_REPORTS` reports, and moves later
    // by `BEAT_STEP` a frame.
    let in_place = 24;
    let settled = |steps: &[i64], from: usize| steps[from..steps.len() - 1].iter().all(|&n| n == 2);
    // The first report sets the clock, so it lands mid-band: without the beat,
    // later reports fall either side of the tick.
    let both_ways = |i: u32| {
        if i == 0 {
            Duration::from_micros(50)
        } else {
            jitter(i, Duration::from_micros(100))
        }
    };
    let late_only = |i: u32| jitter(i, Duration::from_millis(2));
    let both_ways: &dyn Fn(u32) -> Duration = &both_ways;
    for (name, late, even_from) in [
        ("50 µs either way", both_ways, 0),
        ("up to 2 ms late", &late_only, in_place / 2),
    ] {
        let mut beat = WheelBeat::default();
        let first = held_wheel(&mut beat, spacing, reports, late);
        assert!(settled(&first, in_place), "{name}: {first:?}");
        assert_eq!(first.iter().sum::<i64>(), i64::from(reports) - 1, "{name}");
        // The second hold starts on its first report with the first hold's
        // spacing and jitter.
        let second = held_wheel(&mut beat, spacing, reports, |i| late(i + reports));
        assert!(
            settled(&second, even_from),
            "{name}, second hold: {second:?}"
        );
    }
}

/// A wheel held at the clock's own rate gives each frame one report, and the
/// clock rests between reports. So without the beat a report read past its tick
/// draws on its read, and each late read moves its frame.
///
/// The beat puts the tick in the gap after each report, just past the latest a
/// report came, so every report waits for an even tick. With reads 50 µs either
/// way and every eighth up to 2 ms late, every frame takes one report. No frame
/// gap reaches a period and a half.
///
/// Once the beat is in place, 19 gaps in 20 are a period, give or take
/// `BEAT_STEP`. A stream one report a frame but more than
/// `BEAT_SAME_RATE_US` slower, as a trackpad's 120 Hz events, keeps its tick.
#[test]
fn a_wheel_held_at_the_clock_rate_waits_for_an_even_tick() {
    let period = Clock::new(FPS, Instant::now()).period();
    let reports = 375;
    // The beat engages after `BEAT_FIRST_REPORTS` reports and moves later by
    // `BEAT_STEP` a frame.
    let in_place = 24;
    let both_ways = |i: u32| {
        if i == 0 {
            Duration::from_micros(50)
        } else {
            jitter(i, Duration::from_micros(100))
        }
    };
    let late = |i: u32| {
        if i % 8 == 5 {
            jitter(i, Duration::from_millis(2))
        } else {
            both_ways(i)
        }
    };
    let frames = held_wheel_frames(&mut WheelBeat::default(), period, reports, late);
    assert_eq!(
        frames.len(),
        usize::try_from(reports).expect("in range") - 1
    );
    let gaps: Vec<_> = frames.windows(2).map(|w| w[1].0 - w[0].0).collect();
    let uneven = gaps[in_place..]
        .iter()
        .filter(|g| g.abs_diff(period) > BEAT_STEP)
        .count();
    assert!(frames.iter().all(|&(_, rows)| rows == 1), "{gaps:?}");
    assert!(gaps.iter().all(|&g| g < period * 3 / 2), "{gaps:?}");
    assert!(uneven * 20 <= frames.len() - in_place, "{gaps:?}");

    let mut beat = WheelBeat::default();
    let t0 = Instant::now();
    let held = u32::try_from(BEAT_REPORTS).expect("in range");
    for i in 0..held {
        beat.read(t0 + Duration::from_micros(8333) * i + both_ways(i));
    }
    assert_eq!(beat.tick_near(t0 + period * held, period), None);
}

/// A wheel batch read just after a tick nobody waited for, while the clock
/// rested, is spread over two frames a period apart, counted from the wake
/// that read it. On the old beat its second frame came 7.3 ms after its first.
#[test]
fn a_wheel_batch_after_a_rest_is_spread_a_period_apart() {
    let us = Duration::from_micros;
    // Report 0 draws at once and sets the beat. Report 1, read 1 ms later in
    // the same batch, draws on the beat at 8 ms and the queue empties. Reports
    // 2 and 3, read together two frames after the first batch, share the frames
    // until the next.
    let reads = [us(0), us(1000), us(16700), us(16700)];
    let frames = held_wheel_frames(
        &mut WheelBeat::default(),
        Duration::ZERO,
        u32::try_from(reads.len()).expect("in range"),
        |i| reads[usize::try_from(i).expect("in range")],
    );
    assert_eq!(
        frames.iter().map(|&(_, rows)| rows).collect::<Vec<_>>(),
        [1, 1, 1]
    );
    assert_eq!(
        frames
            .iter()
            .map(|&(at, _)| at - frames[0].0)
            .collect::<Vec<_>>(),
        [us(0), us(8700), us(16700)]
    );
}

/// A batch whose first report is read alone and drawn at once, with the rest
/// read 0.3 ms later, as a terminal that splits a burst hands it over, counts
/// the beat from its first frame. It does so like a batch read whole.
///
/// Counting from a wake only when it read two reports left the rest of the
/// batch on the old beat, 7.3 ms after its first frame.
#[test]
fn a_wheel_batch_read_in_two_parts_keeps_the_beat_of_its_first_frame() {
    let us = Duration::from_micros;
    // As above, the queue empties on the beat at 8 ms. Then report 2 is read
    // alone at 16.7 ms and reports 3 and 4 at 17 ms.
    let reads = [us(0), us(1000), us(16700), us(17000), us(17000)];
    let frames = held_wheel_frames(
        &mut WheelBeat::default(),
        Duration::ZERO,
        u32::try_from(reads.len()).expect("in range"),
        |i| reads[usize::try_from(i).expect("in range")],
    );
    assert_eq!(
        frames.iter().map(|&(_, rows)| rows).collect::<Vec<_>>(),
        [1, 1, 1, 1]
    );
    assert_eq!(
        frames
            .iter()
            .map(|&(at, _)| at - frames[0].0)
            .collect::<Vec<_>>(),
        [us(0), us(8700), us(16700), us(24700)]
    );
}

/// A held wheel at 4 ms, so a report waits at every tick, as a script, with
/// §start§ as the time of the first.
fn stream(t0: Instant) -> Vec<(Instant, Input)> {
    (0..100)
        .map(|i| (t0 + Duration::from_millis(4) * i, Input::Wheel(LINES)))
        .collect()
}

/// A change from the daemon while a list scrolls draws on the clock's next
/// tick, which draws anyway: no frame is added or moved, and no report is
/// lost. With the clock at rest its tick is due, so it draws at once, and so
/// does a key.
#[test]
fn a_daemon_change_mid_gesture_adds_no_frame() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let baseline = play_scene(&mut Pacer::new(t0), t0, &stream(t0), Scene::default());
    let mut script = stream(t0);
    script.extend((1..8).map(|k| (t0 + ms(50) * k + ms(3), Input::Daemon)));
    script.sort_by_key(|input| input.0);
    let changed = play_scene(&mut Pacer::new(t0), t0, &script, Scene::default());
    assert_eq!(changed, baseline);
    assert_eq!(
        baseline.iter().map(|frame| frame.1).sum::<i64>(),
        100 * i64::from(LINES)
    );
    // Once the list has stopped, a daemon change and a key each draw at once.
    let later = t0 + ms(1000);
    let at_rest = [(later, Input::Daemon), (later + ms(50), Input::Key)];
    let frames = play_scene(&mut Pacer::new(t0), t0, &at_rest, Scene::default());
    assert_eq!(frames, [(later, 0), (later + ms(50), 0)]);
}

/// A key draws at once, outside the clock, and the next tick comes a period
/// after it, so frames keep an even spacing. No report is lost.
#[test]
fn a_key_mid_gesture_draws_at_once_and_moves_the_beat() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut script = stream(t0);
    let key = t0 + ms(203);
    script.push((key, Input::Key));
    script.sort_by_key(|input| input.0);
    let frames = play_scene(&mut Pacer::new(t0), t0, &script, Scene::default());
    let at = frames
        .iter()
        .position(|frame| frame.0 == key)
        .expect("the key drew a frame");
    // The next frame comes within a period of the key, and no sooner than it.
    assert!(frames[at + 1].0 > key && frames[at + 1].0 <= key + ms(8));
    assert_eq!(
        frames.iter().map(|frame| frame.1).sum::<i64>(),
        100 * i64::from(LINES)
    );
}

/// Focus on the move draws every 16 ms, and once more when it has stopped, so
/// the last frame shows where it came to rest.
#[test]
fn the_last_frame_of_a_move_draws() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let scene = Scene {
        moving_until: ms(50),
        ..Scene::default()
    };
    let frames = play_scene(&mut Pacer::new(t0), t0, &[], scene);
    assert_eq!(
        frames,
        [
            (t0 + ms(16), 0),
            (t0 + ms(32), 0),
            (t0 + ms(48), 0),
            (t0 + ms(64), 0)
        ]
    );
}

/// A spinner draws each turn and wakes the loop for nothing else.
#[test]
fn a_spinner_draws_each_turn_and_nothing_between() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let scene = Scene {
        spinner_until: ms(450),
        ..Scene::default()
    };
    let frames = play_scene(&mut Pacer::new(t0), t0, &[], scene);
    assert_eq!(
        frames.iter().map(|frame| frame.0 - t0).collect::<Vec<_>>(),
        [ms(100), ms(200), ms(300), ms(400)]
    );
}

/// While a list scrolls, a spinner's turn waits for the clock's next tick,
/// which draws anyway.
#[test]
fn a_spinner_turn_while_a_list_scrolls_adds_no_frame() {
    let t0 = Instant::now();
    let baseline = play_scene(&mut Pacer::new(t0), t0, &stream(t0), Scene::default());
    let scene = Scene {
        spinner_until: Duration::from_secs(1),
        ..Scene::default()
    };
    let spun = play_scene(&mut Pacer::new(t0), t0, &stream(t0), scene);
    assert_eq!(spun[..baseline.len()], baseline);
}

/// Only a frame the wheel moves spins to its tick. Any other sleeps to its
/// time, and a list at rest never wakes the loop.
#[test]
fn only_a_frame_the_wheel_moves_spins() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut pacer = Pacer::new(t0);
    assert_eq!(pacer.wait(false, &[None]), None);
    assert_eq!(
        pacer.wait(true, &[None]),
        Some(Wait {
            at: t0 + ms(16),
            spin: false
        })
    );
    let report = Report {
        list: Focus::Session,
        rows: LINES,
        at: t0 + ms(1),
    };
    let wake = Wake {
        now: t0 + ms(1),
        reports: &[report],
        urgent: false,
        dirty: false,
        moving: false,
    };
    pacer.wake(&wake);
    let next = pacer.clock.next();
    assert_eq!(pacer.wait(false, &[None]), None);
    pacer.queues[state::slot(Focus::Session)].push(LINES, t0 + ms(2));
    assert_eq!(
        pacer.wait(false, &[Some(next + ms(5))]),
        Some(Wait {
            at: next,
            spin: true
        })
    );
    assert_eq!(
        pacer.wait(false, &[Some(t0 + ms(2))]),
        Some(Wait {
            at: t0 + ms(2),
            spin: false
        })
    );
    // A list whose scroll was set another way has no report left to move it.
    pacer.reset(&[Focus::Session], &mut Vec::new());
    assert_eq!(pacer.wait(false, &[None]), None);
}

/// While focus is on the move it draws every 16 ms. The pointer's wakes every
/// 4 ms mark changes, which wait for those frames instead of drawing between
/// them, and the move's last frame follows its end.
#[test]
fn a_move_draws_only_on_its_own_frames() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let scene = Scene {
        moving_until: ms(100),
        ..Scene::default()
    };
    let pointer: Vec<_> = (0..25)
        .map(|k| (t0 + ms(1) + ms(4) * k, Input::Daemon))
        .collect();
    let frames = play_scene(&mut Pacer::new(t0), t0, &pointer, scene);
    assert_eq!(
        frames.iter().map(|frame| frame.0 - t0).collect::<Vec<_>>(),
        [16, 32, 48, 64, 80, 96, 112].map(ms)
    );
    // A key still draws between them.
    let key = [(t0 + ms(20), Input::Key)];
    let frames = play_scene(&mut Pacer::new(t0), t0, &key, scene);
    assert!(
        frames.iter().any(|frame| frame.0 == t0 + ms(20)),
        "{frames:?}"
    );
}

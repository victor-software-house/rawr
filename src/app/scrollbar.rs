//! A list's scrollbar: where its thumb stands, what a press on it does, and
//! when it shows. It shows while its list scrolls, while the pointer is on
//! it and while its thumb is held; once none of these holds it stays for
//! [`HOLD`], fades out in [`STEPS`] steps over [`FADE`], and leaves its
//! column blank.
//!
//! The thumb's size is the share of the list in view, at least one row. It
//! touches an end of the track only when the view is at that end, so a thumb
//! at the top always means the first line.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::app::state::Focus;
use crate::app::theme;

/// How long the bar stays in full once nothing holds it.
pub(crate) const HOLD: Duration = Duration::from_millis(800);

/// How long the bar takes to fade out after [`HOLD`].
pub(crate) const FADE: Duration = Duration::from_millis(200);

/// How many steps of dimmer colour the fade takes.
pub(crate) const STEPS: u8 = 4;

/// Where a list stands, in lines: the first line in view, how many lines the
/// view holds, which is also the track's height, and how many the list has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub(crate) offset: u32,
    pub(crate) visible: u32,
    pub(crate) total: u32,
}

impl Geometry {
    pub(crate) fn new(offset: usize, visible: u16, total: usize) -> Self {
        let line = |lines: usize| u32::try_from(lines).unwrap_or(u32::MAX);
        Self {
            offset: line(offset),
            visible: u32::from(visible),
            total: line(total),
        }
    }

    /// Whether the list does not fit, so a bar is drawn.
    pub(crate) const fn shown(&self) -> bool {
        self.total > self.visible && self.visible > 0
    }

    /// The thumb's first row in the track and its size.
    pub(crate) fn thumb(&self) -> (u32, u32) {
        if !self.shown() {
            return (0, 0);
        }
        let (visible, total) = (u64::from(self.visible), u64::from(self.total));
        // Integer rounding of a share of the track, halves up.
        let size = ((2 * visible * visible + total) / (2 * total))
            .clamp(1, visible.saturating_sub(1).max(1));
        let room = visible - size;
        let scrollable = total - visible;
        let offset = u64::from(self.offset);
        let top = if offset == 0 {
            0
        } else if offset >= scrollable {
            room
        } else {
            let top = (2 * offset * room + scrollable) / (2 * scrollable);
            // Only the ends of the view touch the ends of the track.
            if room < 2 {
                top
            } else {
                top.clamp(1, room - 1)
            }
        };
        (
            u32::try_from(top).unwrap_or(u32::MAX),
            u32::try_from(size).unwrap_or(u32::MAX),
        )
    }

    /// The first line in view for a thumb whose first row is `top`, which may
    /// lie outside the track: the inverse of [`Geometry::thumb`].
    pub(crate) fn offset_for_top(&self, top: i64) -> u32 {
        if !self.shown() {
            return 0;
        }
        let (_, size) = self.thumb();
        let room = i64::from(self.visible - size);
        let scrollable = i64::from(self.total - self.visible);
        let line = |lines: i64| u32::try_from(lines).unwrap_or(u32::MAX);
        if top <= 0 || room == 0 {
            return 0;
        }
        if top >= room {
            return line(scrollable);
        }
        let offset = (2 * top * scrollable + room) / (2 * room);
        line(if scrollable < 2 {
            offset
        } else {
            offset.clamp(1, scrollable - 1)
        })
    }
}

/// A list's scrollbar column as a frame drew it, with the geometry it drew
/// the thumb for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Track {
    pub(crate) list: Focus,
    /// The column, one cell wide and as tall as the list.
    pub(crate) area: Rect,
    pub(crate) geometry: Geometry,
}

/// Where a press on the bar landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    /// On the thumb, `grab` rows below its top.
    Thumb { grab: u32 },
    /// On the track, above the thumb or below it.
    Track { above: bool },
}

/// Where a press on row `row` of the track lands, against the thumb as drawn,
/// `(top, size)`.
pub(crate) const fn press((top, size): (u32, u32), row: u32) -> Press {
    if row >= top && row < top + size {
        Press::Thumb { grab: row - top }
    } else {
        Press::Track { above: row < top }
    }
}

/// How the bar draws at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shade {
    /// Nothing: the column is blank.
    Hidden,
    /// The thumb at rest.
    Rest,
    /// The thumb fading out, from step 1 to step [`STEPS`].
    Fading(u8),
    /// The thumb under the pointer or held.
    Lit,
}

/// When a list's scrollbar was last in use.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Fade {
    used: Option<Instant>,
}

impl Fade {
    /// The list scrolled, or the pointer or the thumb let go, at `now`.
    pub(crate) fn touch(&mut self, now: Instant) {
        self.used = Some(now);
    }

    /// How the bar draws at `now`; `lit` is whether the pointer is on it or
    /// its thumb is held.
    pub(crate) fn shade(&self, now: Instant, lit: bool) -> Shade {
        if lit {
            return Shade::Lit;
        }
        let Some(used) = self.used else {
            return Shade::Hidden;
        };
        let idle = now.saturating_duration_since(used);
        if idle < HOLD {
            Shade::Rest
        } else if idle < HOLD + FADE {
            let step =
                idle.saturating_sub(HOLD).as_millis() / (FADE.as_millis() / u128::from(STEPS));
            Shade::Fading(u8::try_from(step).map_or(STEPS, |step| step + 1).min(STEPS))
        } else {
            Shade::Hidden
        }
    }

    /// When the bar next draws differently with nothing else changing: where
    /// the hold ends, then each step of the fade.
    pub(crate) fn next_change(&self, now: Instant, lit: bool) -> Option<Instant> {
        let used = self.used.filter(|_| !lit)?;
        let idle = now.saturating_duration_since(used);
        if idle < HOLD {
            return Some(used + HOLD);
        }
        let step = FADE / u32::from(STEPS);
        (idle < HOLD + FADE).then(|| {
            let next = idle.saturating_sub(HOLD).as_millis() / step.as_millis() + 1;
            used + HOLD + step * u32::try_from(next).unwrap_or(u32::from(STEPS))
        })
    }
}

/// Draws the thumb, whose first row in the track is `top` and which takes
/// `size` rows, in `shade`. The track itself is blank.
pub(crate) fn draw(buf: &mut Buffer, track: Rect, (top, size): (u32, u32), shade: Shade) {
    let (glyph, style) = match shade {
        Shade::Hidden => return,
        Shade::Rest => ("┃", theme::line()),
        Shade::Fading(step) => ("┃", theme::fading(step)),
        Shade::Lit => ("█", theme::lit()),
    };
    for row in top..top.saturating_add(size) {
        let Ok(row) = u16::try_from(row) else {
            break;
        };
        if row >= track.height {
            break;
        }
        if let Some(cell) = buf.cell_mut((track.x, track.y + row)) {
            cell.set_symbol(glyph).set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// The thumb touches an end of the track only when the view is at that
    /// end, and its size rounds the share in view.
    #[test]
    fn the_thumb_touches_an_end_only_with_the_view_there() {
        // offset, rows in view, lines, thumb's first row, thumb's size.
        let cases = [
            (0, 15, 400, 0, 1),
            (1, 15, 400, 1, 1),
            (200, 15, 400, 7, 1),
            (384, 15, 400, 13, 1),
            (385, 15, 400, 14, 1),
            (1969, 30, 2000, 28, 1),
            (5, 30, 60, 3, 15),
            (4, 20, 29, 3, 14),
            (9, 20, 29, 6, 14),
        ];
        for (offset, visible, total, start, size) in cases {
            let geometry = Geometry {
                offset,
                visible,
                total,
            };
            assert_eq!(
                geometry.thumb(),
                (start, size),
                "offset {offset} of {total} in {visible} rows"
            );
        }
    }

    #[test]
    fn a_thumb_row_gives_back_the_offset_that_puts_it_there() {
        let geometry = Geometry {
            offset: 0,
            visible: 15,
            total: 400,
        };
        for (top, offset) in [
            (-3, 0),
            (0, 0),
            (1, 28),
            (7, 193),
            (13, 358),
            (14, 385),
            (20, 385),
        ] {
            assert_eq!(
                geometry.offset_for_top(top),
                offset,
                "offset_for_top({top})"
            );
        }
    }

    #[test]
    fn a_list_that_fits_has_no_bar() {
        let geometry = Geometry::new(0, 20, 20);
        assert!(!geometry.shown());
        assert_eq!(geometry.thumb(), (0, 0));
        assert_eq!(geometry.offset_for_top(5), 0);
    }

    #[test]
    fn a_press_lands_on_the_thumb_with_its_grab_row_or_on_the_track() {
        let thumb = (7, 3);
        assert_eq!(press(thumb, 7), Press::Thumb { grab: 0 });
        assert_eq!(press(thumb, 9), Press::Thumb { grab: 2 });
        assert_eq!(press(thumb, 6), Press::Track { above: true });
        assert_eq!(press(thumb, 10), Press::Track { above: false });
    }

    #[test]
    fn the_bar_holds_then_fades_in_four_steps_then_goes() {
        let t0 = Instant::now();
        let mut fade = Fade::default();
        assert_eq!(fade.shade(t0, false), Shade::Hidden);
        fade.touch(t0);
        let shades: Vec<Shade> = [0, 799, 800, 849, 850, 899, 900, 949, 950, 999, 1000, 5000]
            .into_iter()
            .map(|at| fade.shade(t0 + ms(at), false))
            .collect();
        assert_eq!(
            shades,
            [
                Shade::Rest,
                Shade::Rest,
                Shade::Fading(1),
                Shade::Fading(1),
                Shade::Fading(2),
                Shade::Fading(2),
                Shade::Fading(3),
                Shade::Fading(3),
                Shade::Fading(4),
                Shade::Fading(4),
                Shade::Hidden,
                Shade::Hidden,
            ]
        );
    }

    #[test]
    fn the_pointer_or_a_held_thumb_lights_the_bar_whatever_its_age() {
        let t0 = Instant::now();
        let mut fade = Fade::default();
        assert_eq!(fade.shade(t0, true), Shade::Lit);
        fade.touch(t0);
        assert_eq!(fade.shade(t0 + ms(5000), true), Shade::Lit);
        assert_eq!(fade.next_change(t0, true), None);
    }

    #[test]
    fn the_bar_asks_to_be_drawn_where_its_shade_changes() {
        let t0 = Instant::now();
        let mut fade = Fade::default();
        assert_eq!(fade.next_change(t0, false), None);
        fade.touch(t0);
        let mut wakes = Vec::new();
        let mut now = t0;
        while let Some(next) = fade.next_change(now, false) {
            wakes.push(next.duration_since(t0));
            now = next;
        }
        assert_eq!(wakes, [ms(800), ms(850), ms(900), ms(950), ms(1000)]);
    }

    #[test]
    fn the_thumb_draws_in_its_rows_only() {
        let track = Rect::new(4, 2, 1, 6);
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 10));
        draw(&mut buf, track, (1, 2), Shade::Rest);
        let column: Vec<&str> = (2..8).map(|y| buf[(4, y)].symbol()).collect();
        assert_eq!(column, [" ", "┃", "┃", " ", " ", " "]);
        draw(&mut buf, track, (3, 1), Shade::Lit);
        assert_eq!(buf[(4, 5)].symbol(), "█");
        let mut blank = Buffer::empty(Rect::new(0, 0, 8, 10));
        draw(&mut blank, track, (1, 2), Shade::Hidden);
        assert_eq!(blank, Buffer::empty(Rect::new(0, 0, 8, 10)));
    }
}

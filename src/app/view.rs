//! How the app looks: three columns, every pane, the selected pane's
//! sessions, and one session. The focused column takes the width it needs
//! and the others fold to rails, or, in the single arrangement, one column
//! fills the screen under a path back. Drawing reads the state and writes
//! cells, and records where each thing that takes a click landed, so a
//! click never disagrees with the screen.

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroU16;
use std::ops::Range;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, mem};

use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::app::facts::{EntryId, Facts, Kind, List, MILLION, Outcome, Pr, Said};
use crate::app::feed::Pane;
use crate::app::history::{Gaps, History, NEAR, Reach, Shape};
use crate::app::layouts::{Laid, Layouts};
use crate::app::scrollbar::{self, Geometry, Track};
use crate::app::state::{
    self, Anchor, App, Arrangement, Button, Drag, FULL, Focus, Grip, Link, RAIL, Scroll, Seek, Tab,
    Target, Under,
};
use crate::app::{markup, theme};
use crate::catalog;
use crate::session::Agent;
use crate::status::Status;

/// How many columns past a drawer's edge the pointer may go before the
/// drawer shuts.
const SLACK: u16 = 2;

/// How many columns a view slides in from in the single arrangement.
const SLIDE: f32 = 10.0;

/// Where the last frame put each thing that takes a click, and which list
/// each area scrolls.
#[derive(Debug, Default)]
pub(crate) struct Hits {
    targets: Vec<(Rect, Target)>,
    lists: Vec<(Rect, Focus)>,
    /// The folded columns, which draw out when the pointer comes over them.
    rails: Vec<(Rect, Focus)>,
    /// Where the pointer keeps the drawer out: as wide as it opens, and a
    /// little past its edge, so a pointer that slips over the edge stays.
    keep: Option<Rect>,
    /// The rail the drawer comes out of.
    drawer_rail: Option<Rect>,
    /// The screen's size.
    pub(crate) width: u16,
    pub(crate) height: u16,
    /// Where the terminal's cursor goes: after the text in the search box.
    pub(crate) cursor: Option<Position>,
    /// Where this frame scrolled each list to, for the next to start from.
    pub(crate) tops: Vec<(Focus, u16)>,
    /// The entry the session list's first line is in, when it lists entries.
    pub(crate) anchor: Option<Anchor>,
    /// Where the view stands among the entries held, when it lists entries.
    pub(crate) reach: Option<Reach>,
    /// The scrollbars of the lists that do not fit, with where each stands.
    pub(crate) bars: Vec<Track>,
    /// What this frame found at the place the session's thumb asked for.
    pub(crate) seek: Option<Seek>,
}

impl Hits {
    pub(crate) fn target(&self, column: u16, row: u16) -> Option<Target> {
        let at = Position::new(column, row);
        self.targets
            .iter()
            .find(|(area, _)| area.contains(at))
            .map(|(_, target)| *target)
    }

    /// What lies at `column`, `row`.
    pub(crate) fn under(&self, column: u16, row: u16) -> Under {
        let at = Position::new(column, row);
        let find = |areas: &[(Rect, Focus)]| {
            areas
                .iter()
                .find(|(area, _)| area.contains(at))
                .map(|(_, focus)| *focus)
        };
        Under {
            target: self.target(column, row),
            list: find(&self.lists),
            rail: find(&self.rails),
            keep: self.keep.is_some_and(|area| area.contains(at)),
            on_rail: self.drawer_rail.is_some_and(|area| area.contains(at)),
            size: (self.width, self.height),
        }
    }

    fn add(&mut self, area: Rect, target: Target) {
        self.targets.push((area, target));
    }

    /// Forgets what lies under `area`, which something now covers.
    fn cover(&mut self, area: Rect) {
        self.targets.retain(|(under, _)| !under.intersects(area));
        self.lists.retain(|(under, _)| !under.intersects(area));
        self.rails.retain(|(under, _)| !under.intersects(area));
        self.bars.retain(|track| !track.area.intersects(area));
    }
}

/// The session the session column shows, with its facts once read.
pub(crate) type Shown<'a> = Option<(&'a Agent, Option<&'a History>)>;

pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut App, shown: Shown<'_>) -> Hits {
    // The layouts are the app's, and the drawing reads the rest of it.
    let mut layouts = mem::take(&mut app.layouts);
    let hits = draw_with(frame, app, &mut layouts, shown);
    app.layouts = layouts;
    hits
}

fn draw_with(frame: &mut Frame<'_>, app: &App, layouts: &mut Layouts, shown: Shown<'_>) -> Hits {
    let mut hits = Hits {
        width: frame.area().width,
        height: frame.area().height,
        ..Hits::default()
    };
    let [body, search] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(frame.area());
    // One column and one row in from the outermost ring, which never hovers.
    let body = pad(body, 1, 1);
    // A stand-in draws its pane's column across the whole body, with no
    // panes column to fold, in either arrangement.
    if app.is_stand_in() {
        view(frame, app, layouts, Focus::Session, body, shown, &mut hits);
    } else {
        match app.arrangement {
            Arrangement::Rails => rails(frame, app, layouts, body, shown, &mut hits),
            Arrangement::Single => single(frame, app, layouts, body, shown, &mut hits),
        }
    }
    if app.focus == Focus::Search {
        search_overlay(frame, app, body, &mut hits);
    }
    search_box(frame, app, search, &mut hits);
    if app.help {
        hits.targets.clear();
        hits.rails.clear();
        hits.bars.clear();
        hits.cursor = None;
        help(frame, app.is_stand_in());
    }
    if let Some(cursor) = hits.cursor {
        frame.set_cursor_position(cursor);
    }
    hits
}

/// `area` less `x` columns on each side and `y` rows at the top.
fn pad(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x + x,
        y: area.y + y,
        width: area.width.saturating_sub(2 * x),
        height: area.height.saturating_sub(y),
    }
}

/// Writes `line` into the one-row `area`.
fn put(frame: &mut Frame<'_>, area: Rect, line: Line<'_>) {
    frame.render_widget(Paragraph::new(line), area);
}

fn row_at(area: Rect, y: u16) -> Rect {
    Rect {
        y,
        height: 1,
        ..area
    }
}

/// `area` from `rows` rows down.
fn below(area: Rect, rows: u16) -> Rect {
    Rect {
        y: area.y + rows.min(area.height),
        height: area.height.saturating_sub(rows),
        ..area
    }
}

/// A rule across the one-row `area`.
fn rule(frame: &mut Frame<'_>, area: Rect, style: Style) {
    put(
        frame,
        area,
        Line::styled("─".repeat(usize::from(area.width)), style),
    );
}

// The rails arrangement.

/// Each column's width when `focus` has settled, of `total` columns.
fn rails(
    frame: &mut Frame<'_>,
    app: &App,
    layouts: &mut Layouts,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    let [mut a, mut b] = app.layout();
    // The rail the panes fold to, before a drawer widens it.
    let folded = Rect { width: a, ..area };
    // A drawn-out panes column pushes the pane aside rather than covering
    // it, so the pane stays in sight.
    let drawn = app
        .drawn()
        .filter(|(column, _)| *column == Focus::Panes && a < FULL);
    if let Some((column, out)) = drawn {
        let full = app.drawer_full(column).max(a);
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the value is rounded and floored at zero, and lies between two u16 values"
        )]
        let wide = (f32::from(a) + (f32::from(full) - f32::from(a)) * out)
            .round()
            .max(0.0) as u16;
        let total = a + b;
        a = wide.min(total.saturating_sub(RAIL));
        b = total - a;
    }
    let [first, edge, second] = Layout::horizontal([
        Constraint::Length(a),
        Constraint::Length(1),
        Constraint::Length(b),
    ])
    .areas(area);
    let rects = [first, second];
    // The edge moves while the panes are drawn in full; the pane takes
    // what is left.
    let grip = (first.width >= FULL).then_some(Grip {
        column: Focus::Panes,
        anchor: first.x,
        rightward: true,
    });
    edge_line(frame, app, edge, grip, hits);
    for (column, rect) in Focus::COLUMNS.into_iter().zip(rects) {
        if rect.width == 0 || rect.height < 3 {
            continue;
        }
        if rect.width < FULL {
            rail(frame, app, column, rect, shown, hits);
        } else {
            header(frame, app, column, rect, hits);
            view(frame, app, layouts, column, below(rect, 2), shown, hits);
        }
    }
    // A drawer that has shut keeps its area a moment, so a pointer that
    // slipped out can bring it back. The area runs the screen's height, so
    // a pointer that leaves past the top or the bottom keeps it too.
    if let Some(column) = app.drawer.map(|drawer| drawer.column)
        && column == Focus::Panes
        && folded.width < FULL
    {
        let keep = keep_area(app, area, folded, column);
        hits.keep = Some(Rect {
            y: 0,
            height: hits.height,
            ..keep
        });
        hits.drawer_rail = Some(folded);
    }
}

/// Where the pointer keeps the drawer of `column` out: the open drawer's
/// area and a little past its edge, whatever the animation has drawn, so a
/// pointer that was in it stays in it as it shuts.
fn keep_area(app: &App, area: Rect, rail: Rect, column: Focus) -> Rect {
    let reach = app.drawer_full(column) + 1 + SLACK;
    if column.column() < app.lead().column() {
        Rect {
            width: reach.min(area.right().saturating_sub(rail.x)),
            ..rail
        }
    } else {
        let x = rail.right().saturating_sub(reach).max(area.x);
        Rect {
            x,
            width: rail.right() - x,
            ..rail
        }
    }
}

/// A column's edge: a thin rule, which lights while hovered or dragged
/// when `grip` lets it move.
fn edge_line(frame: &mut Frame<'_>, app: &App, area: Rect, grip: Option<Grip>, hits: &mut Hits) {
    let lit = grip.is_some_and(|grip| {
        matches!(app.dragging, Some(Drag::Edge(held)) if held.column == grip.column)
            || app.hover == Some(Target::Edge(grip))
    });
    let (glyph, style) = if lit {
        ("┃", Style::new().fg(theme::ACCENT))
    } else {
        ("│", theme::line())
    };
    let lines: Vec<Line<'static>> = (0..area.height)
        .map(|_| Line::styled(glyph, style))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
    if let Some(grip) = grip {
        // Before the rows, so the edge's own column always takes the drag.
        hits.targets.insert(0, (area, Target::Edge(grip)));
    }
}

/// A column's name, its key, and what it holds, over a rule that lights
/// when the column has focus.
fn header(frame: &mut Frame<'_>, app: &App, column: Focus, area: Rect, hits: &mut Hits) {
    let focused = app.at().column() == column.column();
    let hovered = app.hover == Some(Target::Column(column));
    let (key, name) = names(column);
    let key_style = if focused {
        Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    let name_style = if focused {
        Style::new().add_modifier(Modifier::BOLD)
    } else if hovered {
        Style::new()
    } else {
        theme::dim()
    };
    let mut spans = vec![
        Span::styled(format!(" {key} "), key_style),
        Span::styled(name, name_style),
    ];
    if column == Focus::Panes {
        spans.extend(summary(app).spans);
    }
    // The pane's header shows the filter of whichever of its parts has
    // focus, and none while the panes do.
    let filter = match (column, app.at()) {
        (Focus::Panes, _) => app.filter(Focus::Panes),
        (_, at @ (Focus::Sessions | Focus::Session)) => app.filter(at),
        _ => "",
    };
    if !filter.is_empty() {
        spans.push(Span::styled(
            format!("  / {filter}"),
            Style::new().fg(theme::ACCENT),
        ));
    }
    let top = row_at(area, area.y);
    put(frame, top, Line::from(spans));
    rule(
        frame,
        row_at(area, area.y + 1),
        if focused {
            Style::new().fg(theme::ACCENT)
        } else {
            theme::line()
        },
    );
    hits.add(top, Target::Column(column));
}

const fn names(column: Focus) -> (&'static str, &'static str) {
    match column {
        Focus::Panes | Focus::Search => ("1", "Panes"),
        Focus::Sessions | Focus::Session => ("2", "Pane"),
    }
}

/// A folded column, which draws out while the pointer is on it.
fn rail(
    frame: &mut Frame<'_>,
    app: &App,
    column: Focus,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    rail_view(frame, app, column, area, shown, hits);
    hits.rails.push((area, column));
}

/// A folded column's face: its key, then a glyph per row, or the
/// session's tabs. A click on a row selects it; anywhere else focuses the
/// column.
#[expect(
    clippy::too_many_lines,
    reason = "draws a folded column's face for each of the three columns in one pass"
)]
fn rail_view(
    frame: &mut Frame<'_>,
    app: &App,
    column: Focus,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    let hovered = app.hover == Some(Target::Column(column));
    let (key, _) = names(column);
    put(
        frame,
        row_at(area, area.y),
        Line::styled(
            format!(" {key}"),
            if hovered {
                Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD)
            } else {
                theme::dim()
            },
        ),
    );
    rule(frame, row_at(area, area.y + 1), theme::line());
    let list = below(area, 2);
    let height = usize::from(list.height);
    let mut counter = None;
    let glyphs: Vec<(Target, bool, Span<'static>)> = match column {
        Focus::Panes | Focus::Search => app
            .visible_panes()
            .into_iter()
            .filter_map(|index| {
                let pane = app.panes.get(index)?;
                Some((
                    Target::Pane(index),
                    index == app.pane,
                    Span::styled(
                        theme::glyph(pane.status, app.frame),
                        theme::status(pane.status),
                    ),
                ))
            })
            .collect(),
        Focus::Sessions => app
            .visible_sessions()
            .into_iter()
            .filter_map(|index| {
                let (agent, held) = app.selected()?.sessions.get(index)?;
                let (glyph, style) = session_glyph(app, agent, *held);
                Some((
                    Target::Session(index),
                    index == app.session,
                    Span::styled(glyph, style),
                ))
            })
            .collect(),
        Focus::Session => {
            let rows: Vec<(Target, bool, Span<'static>)> = Tab::ALL
                .iter()
                .map(|tab| {
                    let style = if *tab == app.tab {
                        Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        theme::dim()
                    };
                    (
                        Target::Tab(*tab),
                        *tab == app.tab,
                        Span::styled(tab.name().get(..1).unwrap_or_default().to_owned(), style),
                    )
                })
                .collect();
            if let Some((_, Some(history))) = shown
                && let facts = &history.facts
                && !facts.tasks.is_empty()
            {
                let done = facts
                    .tasks
                    .iter()
                    .filter(|task| task.status == "completed")
                    .count();
                counter = Some(format!("{done}/{}", facts.tasks.len()));
            }
            rows
        }
    };
    let selected = glyphs.iter().position(|(_, on, _)| *on).unwrap_or(0);
    let skip = selected.saturating_sub(height.saturating_sub(1));
    for (offset, (target, on, glyph)) in glyphs.into_iter().skip(skip).take(height).enumerate() {
        let cell = row_at(list, list.y + u16::try_from(offset).unwrap_or(0));
        let hovered = app.hover == Some(target);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if on { "▌" } else { " " }, Style::new().fg(theme::ACCENT)),
                Span::raw(" "),
                glyph,
            ]))
            .style(theme::row(false, hovered)),
            cell,
        );
        hits.add(cell, target);
    }
    // The tasks done, under the tabs, across the rail's whole width.
    if let Some(counter) = counter {
        let y = list.y + u16::try_from(Tab::ALL.len() + 1).unwrap_or(0);
        if y < list.bottom() {
            let text = if counter.chars().count() < usize::from(area.width) {
                counter
            } else {
                counter.split('/').next().unwrap_or_default().to_owned()
            };
            put(
                frame,
                row_at(list, y),
                Line::styled(format!(" {text}"), theme::dim()),
            );
        }
    }
    hits.add(area, Target::Column(column));
}

/// One column's content.
fn view(
    frame: &mut Frame<'_>,
    app: &App,
    layouts: &mut Layouts,
    column: Focus,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    match column {
        Focus::Panes | Focus::Search => panes_list(frame, app, area, hits),
        Focus::Sessions | Focus::Session => pane_view(frame, app, layouts, area, shown, hits),
    }
}

// The single arrangement.

fn single(
    frame: &mut Frame<'_>,
    app: &App,
    layouts: &mut Layouts,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    if area.height < 4 {
        return;
    }
    crumbs(frame, app, row_at(area, area.y), hits);
    rule(frame, row_at(area, area.y + 1), theme::line());
    let body = below(area, 2);
    let at = app.at();
    view(frame, app, layouts, at, body, shown, hits);
    // The view slides in from the side it came from: deeper from the right,
    // back from the left.
    if let Some(was) = app.moved.map(|moved| moved.was)
        && was.column() != at.column()
    {
        let sign: f32 = if at.column() > was.column() {
            1.0
        } else {
            -1.0
        };
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "the value is rounded and lies within the slide, a few columns"
        )]
        let by = ((1.0 - app.progress()) * SLIDE * sign).round() as i32;
        shift(frame.buffer_mut(), body, by);
    }
}

/// The path to the view: each column's key and what it shows, the current
/// one raised. A click goes back to any of them.
fn crumbs(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let pane = app
        .selected()
        .map(|pane| format!("{} · {}", pane.name, pane.tab));
    let session = app.current().map(|agent| agent.title.clone());
    let pane_and_session = match (pane, session) {
        (Some(pane), Some(session)) => format!("{pane} · {session}"),
        (Some(name), None) | (None, Some(name)) => name,
        (None, None) => "Pane".to_owned(),
    };
    let parts = [
        (Focus::Panes, "1", "Panes".to_owned()),
        (Focus::Session, "2", pane_and_session),
    ];
    let mut x = area.x;
    for (index, (column, key, name)) in parts.into_iter().enumerate() {
        if index > 0 {
            let gap = Rect {
                x,
                width: 3.min(area.right().saturating_sub(x)),
                ..area
            };
            put(frame, gap, Line::styled(" › ", theme::dim()));
            x += gap.width;
        }
        let current = app.at().column() == column.column();
        let hovered = app.hover == Some(Target::Column(column));
        let text = format!(" {key} {} ", truncate(&name, 60));
        let width = u16::try_from(text.chars().count())
            .unwrap_or(0)
            .min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let cell = Rect { x, width, ..area };
        let style = if current {
            theme::raised()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            theme::hover()
        } else {
            theme::dim()
        };
        put(frame, cell, Line::styled(text, style));
        hits.add(cell, Target::Column(column));
        x += width;
    }
    let rest = Rect {
        x,
        width: area.right().saturating_sub(x),
        ..area
    };
    if app.at() == Focus::Panes && rest.width > 8 {
        put(frame, rest, summary(app));
    }
}

/// Moves the cells of `area` `by` columns, right when positive, and blanks
/// the columns left behind.
fn shift(buf: &mut Buffer, area: Rect, by: i32) {
    if by == 0 {
        return;
    }
    let width = i32::from(area.width);
    for y in area.top()..area.bottom() {
        let row: Vec<Cell> = (area.left()..area.right())
            .map(|x| buf.cell((x, y)).cloned().unwrap_or(Cell::EMPTY))
            .collect();
        for (offset, x) in (area.left()..area.right()).enumerate() {
            let from = i32::try_from(offset).unwrap_or(0) - by;
            let cell = usize::try_from(from)
                .ok()
                .filter(|_| from < width)
                .and_then(|from| row.get(from).cloned())
                .unwrap_or(Cell::EMPTY);
            if let Some(slot) = buf.cell_mut((x, y)) {
                *slot = cell;
            }
        }
    }
}

// The panes.

/// The panes the filter lets through, one section per status, two lines a
/// pane, as Rex's own sidebar draws its tabs.
fn panes_list(frame: &mut Frame<'_>, app: &App, list: Rect, hits: &mut Hits) {
    hits.lists.push((list, Focus::Panes));
    let visible = app.visible_panes();
    if visible.is_empty() {
        let text = if app.panes.is_empty() {
            empty_text(app).to_owned()
        } else {
            format!("No pane matches “{}”", app.filter(Focus::Panes))
        };
        put(
            frame,
            row_at(list, list.y),
            Line::styled(format!(" {text}"), theme::dim()),
        );
        return;
    }
    let mut lines: Vec<Line_> = Vec::new();
    let mut previous = None;
    for index in &visible {
        let Some(pane) = app.panes.get(*index) else {
            continue;
        };
        let section = section_of(pane);
        if previous != Some(section) {
            if previous.is_some() {
                lines.push(Line_::Blank);
            }
            let count = visible
                .iter()
                .filter_map(|other| app.panes.get(*other))
                .filter(|other| section_of(other) == section)
                .count();
            lines.push(Line_::Header(section, count));
            previous = Some(section);
        }
        lines.push(Line_::Pane(*index));
    }
    let tall = |line: &Line_| -> u16 {
        match line {
            Line_::Pane(index) => app
                .panes
                .get(*index)
                .map_or(1, |pane| pane_height(app, pane)),
            Line_::Header(..) | Line_::Blank => 1,
        }
    };
    let total: u16 = lines.iter().map(tall).sum();
    let selection = lines
        .iter()
        .position(|line| *line == Line_::Pane(app.pane))
        .map(|at| {
            let start: u16 = lines.iter().take(at).map(tall).sum();
            (start, start + lines.get(at).map_or(1, tall))
        });
    let skip = top_of(app.scroll_of(Focus::Panes), total, list.height, selection);
    hits.tops.push((Focus::Panes, skip));
    draw_bar(frame, app, hits, Focus::Panes, list, skip, total);
    let list = Rect {
        width: list.width.saturating_sub(u16::from(total > list.height)),
        ..list
    };
    let mut y: u16 = 0;
    for line in &lines {
        let height = tall(line);
        let top = y;
        y += height;
        if top < skip {
            continue;
        }
        let row = list.y + (top - skip);
        if row + height > list.bottom() {
            break;
        }
        let cell = Rect {
            y: row,
            height,
            ..list
        };
        match line {
            Line_::Pane(index) => {
                pane_row(frame, app, *index, cell);
                hits.add(cell, Target::Pane(*index));
            }
            Line_::Header(section, count) => put(
                frame,
                cell,
                Line::from(vec![
                    Span::styled(format!(" {section}"), theme::header()),
                    Span::styled(format!("  {count}"), theme::dim()),
                ]),
            ),
            Line_::Blank => {}
        }
    }
}

/// The section a pane goes in: by its status, or Closed when its pane is
/// gone.
fn section_of(pane: &Pane) -> &'static str {
    if pane.closed {
        "Closed"
    } else {
        theme::section(pane.status)
    }
}

/// One line of the panes list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line_ {
    Header(&'static str, usize),
    Pane(usize),
    Blank,
}

fn summary(app: &App) -> Line<'static> {
    let waiting = app
        .panes
        .iter()
        .filter(|pane| pane.status.needs_user())
        .count();
    let listed = app
        .panes
        .iter()
        .filter(|pane| app.moved_to(pane).is_none())
        .count();
    let mut spans = vec![Span::styled(format!("  {listed}"), theme::dim())];
    if waiting > 0 {
        spans.push(Span::styled(" · ", theme::dim()));
        spans.push(Span::styled(
            format!("{waiting} need you"),
            Style::new().fg(theme::WARNING),
        ));
    }
    match &app.link {
        Link::Connecting => spans.push(Span::styled(" · reading Rex…", theme::dim())),
        Link::Live => {}
        Link::Offline(_) => spans.push(Span::styled(
            " · Rex is not running",
            Style::new().fg(theme::ERROR),
        )),
        Link::Stale(_) => spans.push(Span::styled(" · stale", Style::new().fg(theme::WARNING))),
    }
    Line::from(spans)
}

fn empty_text(app: &App) -> &'static str {
    match app.link {
        Link::Connecting => "Reading Rex…",
        Link::Offline(_) => "Rex is not running here",
        Link::Live | Link::Stale(_) => {
            "No pane has a Claude session yet; rawr setup claude records them"
        }
    }
}

/// The pane that runs a stopped pane's last session now; see
/// [`App::moved_to`].
fn moved_to<'a>(app: &'a App, pane: &Pane) -> Option<&'a Pane> {
    app.moved_to(pane)
}

/// How many lines a pane's row takes: one for a quiet pane, which needs
/// nothing and says nothing; two for the rest, whose second line says
/// where the pane is and what its Claude says.
fn pane_height(app: &App, pane: &Pane) -> u16 {
    let quiet = matches!(pane.status, Status::Idle | Status::Stopped)
        && pane.msg.as_deref().is_none_or(str::is_empty)
        && moved_to(app, pane).is_none();
    if quiet { 1 } else { 2 }
}

/// One pane row: the glyph, the newest session's title and its age; then,
/// when the row has two lines, the Rex session and tab and what Claude
/// last said.
fn pane_row(frame: &mut Frame<'_>, app: &App, index: usize, cell: Rect) {
    let Some(pane) = app.panes.get(index) else {
        return;
    };
    let selected = index == app.pane;
    let focused = app.at() == Focus::Panes || app.out(Focus::Panes);
    let hovered = app.hover == Some(Target::Pane(index));
    let age = short_age(pane.sessions.first().and_then(|(agent, _)| agent.idle));
    let width = usize::from(cell.width);
    let room = width.saturating_sub(4 + age.chars().count() + 2);
    let title = truncate(pane.title(), room);
    let gap = room.saturating_sub(title.chars().count());
    let bar = Span::styled(
        if selected { "▌" } else { " " },
        Style::new().fg(theme::ACCENT),
    );
    let title_style = if selected {
        Style::new().add_modifier(Modifier::BOLD)
    } else if pane.status == Status::Stopped {
        theme::dim()
    } else {
        Style::new()
    };
    let mine = app.here.as_deref() == Some(pane.block_id.as_str());
    let mut place = format!("{} · {}", pane.name, pane.tab);
    if mine {
        place.push_str(" · here");
    }
    let under = match (moved_to(app, pane), &pane.msg) {
        (Some(other), _) => format!("{place} · resumed in {}", unique_name(other)),
        (None, Some(msg)) if !msg.is_empty() => format!("{place} · {msg}"),
        (None, _) => place,
    };
    let mut lines = vec![Line::from(vec![
        bar.clone(),
        Span::raw(" "),
        Span::styled(
            theme::glyph(pane.status, app.frame),
            theme::status(pane.status),
        ),
        Span::raw(" "),
        Span::styled(title, title_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(format!(" {age} "), theme::dim()),
    ])];
    if cell.height > 1 {
        lines.push(Line::from(vec![
            bar,
            Span::raw("   "),
            Span::styled(truncate(&under, width.saturating_sub(5)), theme::dim()),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).style(theme::row(selected && focused, hovered)),
        cell,
    );
}

/// A session's glyph: its pane's status when a pane runs it, else whether
/// a Claude holds it.
fn session_glyph(app: &App, agent: &Agent, held: bool) -> (&'static str, Style) {
    match app.running(&agent.session_id) {
        Some(index) => {
            let status = app
                .panes
                .get(index)
                .map_or(Status::Idle, |pane| pane.status);
            (theme::glyph(status, app.frame), theme::status(status))
        }
        None if held => ("●", Style::new().fg(theme::WORKING)),
        None => ("○", theme::dim()),
    }
}

// The pane.

/// The selected pane and the session it shows: where the pane is and what
/// its Claude says, its sessions when it has more than one, then the
/// session's title, the actions, and a tab of its tasks and pull requests,
/// its prompts, its tool calls, or the files it touched.
#[expect(
    clippy::too_many_lines,
    reason = "draws the pane's header, sessions and tabs in one pass over the area"
)]
fn pane_view(
    frame: &mut Frame<'_>,
    app: &App,
    layouts: &mut Layouts,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    let inner = pad(area, 1, 0);
    let width = usize::from(inner.width);
    let bottom = inner.bottom();
    let mut y = inner.y;
    // The pane, unless the session shown is a search hit.
    let pane = app.selected().filter(|_| app.focus != Focus::Search);
    if let Some(pane) = pane {
        if bottom.saturating_sub(y) < 3 {
            return;
        }
        let mut place = vec![
            Span::styled(pane.name.clone(), Style::new().add_modifier(Modifier::BOLD)),
            Span::styled(format!(" · {}", pane.tab), theme::dim()),
        ];
        if app.here.as_deref() == Some(pane.block_id.as_str()) {
            place.push(Span::styled(" · here", theme::dim()));
        }
        put(frame, row_at(inner, y), Line::from(place));
        let mut status = vec![
            pill(pane.status, app.frame),
            Span::styled(
                pane.progress
                    .map_or_default(|percent| format!(" {percent}%")),
                theme::status(pane.status),
            ),
        ];
        if let Some(other) = moved_to(app, pane) {
            status.push(Span::styled(
                format!("   its session was resumed in {}", unique_name(other)),
                theme::dim(),
            ));
        } else if let Some(msg) = pane.msg.as_deref().filter(|msg| !msg.is_empty()) {
            status.push(Span::styled(
                format!("   {}", truncate(msg, width.saturating_sub(24))),
                theme::dim(),
            ));
        }
        put(frame, row_at(inner, y + 1), Line::from(status));
        y += 3;
        if app.lists_sessions() {
            let room = Rect {
                y,
                height: bottom.saturating_sub(y),
                ..area
            };
            y += session_list(frame, app, room, pane, hits) + 1;
        }
    }
    let Some((agent, history)) = shown else {
        let text = if app.panes.is_empty() {
            empty_text(app)
        } else {
            "No session selected"
        };
        put(
            frame,
            row_at(inner, y),
            Line::styled(text.to_owned(), theme::dim()),
        );
        return;
    };
    if bottom.saturating_sub(y) < 8 {
        return;
    }
    put(
        frame,
        row_at(inner, y),
        Line::styled(
            truncate(&agent.title, width),
            Style::new().add_modifier(Modifier::BOLD),
        ),
    );
    let id = agent.session_id.get(..8).unwrap_or(&agent.session_id);
    put(
        frame,
        row_at(inner, y + 1),
        Line::styled(
            truncate(&format!("{} · {id}", agent.short_cwd()), width),
            theme::dim(),
        ),
    );
    y += 2;
    let held = app
        .selected()
        .and_then(|pane| {
            pane.sessions
                .iter()
                .find(|(other, _)| other.session_id == agent.session_id)
        })
        .is_some_and(|(_, held)| *held);
    let running = app.running(&agent.session_id);
    // The pane's own live session says nothing the pane has not said.
    let status = match running.and_then(|index| app.panes.get(index).map(|pane| (index, pane))) {
        Some((index, _)) if pane.is_some() && index == app.pane => None,
        Some((_, other)) => Some(Line::from(vec![
            pill(other.status, app.frame),
            Span::styled(format!("  in {}", unique_name(other)), theme::dim()),
        ])),
        None if held => Some(Line::styled(
            "● running in a pane rawr has not recorded",
            theme::dim(),
        )),
        None => Some(Line::styled("○ resumable", theme::dim())),
    };
    if let Some(status) = status {
        put(frame, row_at(inner, y), status);
        y += 1;
    }
    y += 1;
    let actions = row_at(inner, y);
    let mut x = actions.x;
    if !app.is_stand_in() && pane.is_some_and(|pane| !pane.closed) {
        let width = 16.min(actions.width);
        action(
            frame,
            app,
            Rect {
                x,
                width,
                ..actions
            },
            (Button::GoTo, "g", "Go to pane"),
            hits,
        );
        x += width + 2;
    }
    if !held && x + 13 <= actions.right() {
        action(
            frame,
            app,
            Rect {
                x,
                width: 13,
                ..actions
            },
            (Button::Resume, "r", "Resume"),
            hits,
        );
    }
    y += 2;
    tabs(
        frame,
        app,
        row_at(inner, y),
        history.map(|history| &history.facts),
        hits,
    );
    rule(frame, row_at(area, y + 1), theme::line());
    let content = Rect {
        y: y + 2,
        height: bottom.saturating_sub(y + 2),
        ..inner
    };
    let Some(history) = history else {
        put(
            frame,
            row_at(content, content.y),
            Line::styled("reading the transcript…", theme::dim()),
        );
        return;
    };
    let facts = &history.facts;
    // The text leaves the scrollbar's columns free, so it keeps its width
    // as the tab grows past the screen.
    let text = Rect {
        width: content.width.saturating_sub(2),
        ..content
    };
    layouts.show(&agent.transcript, usize::from(text.width));
    let body = tab_lines(
        app,
        layouts,
        agent,
        history,
        usize::from(text.width),
        content.height,
    );
    hits.lists.push((content, Focus::Session));
    let skip = body.top;
    hits.tops.push((Focus::Session, skip));
    hits.anchor = body.anchor;
    hits.reach = body.reach;
    hits.seek = body.seek;
    let prs_at = body.prs_at;
    for (at, span, target) in body.rows {
        if let Some(row) = u16::try_from(at)
            .ok()
            .and_then(|at| at.checked_sub(skip))
            .filter(|row| *row < text.height)
        {
            let mut cell = row_at(text, text.y + row);
            if let Some((from, to)) = span {
                let from = u16::try_from(from).unwrap_or(u16::MAX).min(cell.width);
                let to = u16::try_from(to).unwrap_or(u16::MAX).min(cell.width);
                cell.x += from;
                cell.width = to.saturating_sub(from);
            }
            hits.add(cell, target);
        }
    }
    draw_bar(
        frame,
        app,
        hits,
        Focus::Session,
        content,
        body.bar.0,
        body.bar.1,
    );
    frame.render_widget(
        Paragraph::new(body.lines)
            .style(Style::new().fg(theme::TEXT))
            .scroll((body.offset, 0)),
        text,
    );
    if let Some(at) = prs_at.and_then(|at| u16::try_from(at).ok())
        && at >= skip
        && at - skip < text.height
    {
        pr_links(
            frame.buffer_mut(),
            row_at(text, text.y + at - skip),
            &facts.prs,
        );
    }
}

/// The pane's sessions, one line each, under a header; at most a third of
/// `area`, scrolled to the selection. Returns the rows it took.
#[expect(
    clippy::too_many_lines,
    reason = "draws the sessions header and every row under it in one pass"
)]
fn session_list(frame: &mut Frame<'_>, app: &App, area: Rect, pane: &Pane, hits: &mut Hits) -> u16 {
    let focused = app.focus == Focus::Sessions;
    let visible = app.visible_sessions();
    let filter = app.filter(Focus::Sessions);
    let scratch = pane
        .sessions
        .iter()
        .filter(|(agent, _)| {
            state::is_scratch(agent)
                && state::passes(filter, &format!("{} {}", agent.title, agent.cwd))
        })
        .count();
    let inner = pad(area, 1, 0);
    put(
        frame,
        row_at(inner, inner.y),
        Line::from(vec![
            Span::styled(
                "Sessions",
                if focused {
                    theme::header().fg(theme::ACCENT)
                } else {
                    theme::header()
                },
            ),
            Span::styled(format!("  {}", visible.len()), theme::dim()),
        ]),
    );
    let total = u16::try_from(visible.len()).unwrap_or(u16::MAX);
    let height = total
        .min((area.height / 3).max(3))
        .min(area.height.saturating_sub(2));
    let list = Rect {
        y: area.y + 1,
        height,
        ..area
    };
    hits.lists.push((list, Focus::Sessions));
    let selection = visible
        .iter()
        .position(|index| *index == app.session)
        .and_then(|at| u16::try_from(at).ok())
        .map(|at| (at, at + 1));
    let skip = top_of(
        app.scroll_of(Focus::Sessions),
        total,
        list.height,
        selection,
    );
    hits.tops.push((Focus::Sessions, skip));
    draw_bar(frame, app, hits, Focus::Sessions, list, skip, total);
    let rows = Rect {
        width: list.width.saturating_sub(u16::from(total > list.height)),
        ..list
    };
    if visible.is_empty() && !filter.is_empty() {
        put(
            frame,
            row_at(rows, rows.y),
            Line::styled(format!(" No session matches “{filter}”"), theme::dim()),
        );
    }
    for (offset, index) in visible
        .iter()
        .skip(usize::from(skip))
        .take(usize::from(rows.height))
        .enumerate()
    {
        let Some((agent, held)) = pane.sessions.get(*index) else {
            continue;
        };
        let cell = row_at(rows, rows.y + u16::try_from(offset).unwrap_or(0));
        let selected = *index == app.session;
        let hovered = app.hover == Some(Target::Session(*index));
        let (glyph, style) = session_glyph(app, agent, *held);
        let age = short_age(agent.idle);
        let room = usize::from(cell.width).saturating_sub(4 + age.chars().count() + 2);
        let title = truncate(&agent.title, room);
        let gap = room.saturating_sub(title.chars().count());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if selected { "▌" } else { " " },
                    Style::new().fg(theme::ACCENT),
                ),
                Span::raw(" "),
                Span::styled(glyph, style),
                Span::raw(" "),
                Span::styled(
                    title,
                    if selected {
                        Style::new().add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    },
                ),
                Span::raw(" ".repeat(gap)),
                Span::styled(format!(" {age} "), theme::dim()),
            ]))
            .style(theme::row(selected && focused, hovered)),
            cell,
        );
        hits.add(cell, Target::Session(*index));
    }
    let mut used = 1 + list.height;
    if scratch > 0 {
        let cell = row_at(area, list.bottom());
        let hovered = app.hover == Some(Target::Scratch);
        let text = if app.show_scratch {
            format!("  − hide {scratch} untitled sessions in temporary folders")
        } else {
            format!("  + {scratch} untitled sessions in temporary folders")
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                text,
                if hovered {
                    Style::new().fg(theme::ACCENT)
                } else {
                    theme::dim()
                },
            ))
            .style(theme::row(false, hovered)),
            cell,
        );
        hits.add(cell, Target::Scratch);
        used += 1;
    }
    used
}

/// How many entries `list` has in the whole transcript, or in the part read so
/// far with a `+` while the rest is still being read.
fn counted(facts: &Facts, list: List) -> String {
    let plus = if facts.whole { "" } else { "+" };
    format!("{}{plus}", facts.total(list))
}

/// The tab bar: each tab's name and how many rows it holds.
fn tabs(frame: &mut Frame<'_>, app: &App, area: Rect, facts: Option<&Facts>, hits: &mut Hits) {
    let mut x = area.x;
    for tab in Tab::ALL {
        let count = facts.map(|facts| match tab {
            Tab::Overview => None,
            Tab::Prompts => Some(counted(facts, List::Log)),
            Tab::Activity => Some(counted(facts, List::Activity)),
            Tab::Files => Some(
                (facts.files.len()
                    + facts
                        .read
                        .iter()
                        .filter(|(path, _)| !facts.files.iter().any(|(file, _)| file == path))
                        .count())
                .to_string(),
            ),
        });
        let text = match count.flatten() {
            Some(count) => format!(" {} {count} ", tab.name()),
            None => format!(" {} ", tab.name()),
        };
        let width = u16::try_from(text.chars().count())
            .unwrap_or(0)
            .min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let cell = Rect { x, width, ..area };
        let current = tab == app.tab;
        let hovered = app.hover == Some(Target::Tab(tab));
        let style = if current {
            theme::raised()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            theme::hover()
        } else {
            theme::dim()
        };
        put(frame, cell, Line::styled(text, style));
        hits.add(cell, Target::Tab(tab));
        x += width + 1;
    }
    // The newest lines show first; the counts grow as the older ones come.
    if facts.is_some_and(|facts| !facts.whole) {
        let text = "reading older lines…";
        let width = u16::try_from(text.chars().count()).unwrap_or(0);
        if x + width <= area.right() {
            let cell = Rect {
                x: area.right() - width,
                width,
                ..area
            };
            put(frame, cell, Line::styled(text, theme::dim()));
        }
    }
}

/// What the session column's current tab shows.
struct Body {
    /// The lines to draw, from line `offset` of them on.
    lines: Vec<Line<'static>>,
    offset: u16,
    /// The scrollbar's position and length: the lines of the whole tab, with
    /// the entries not held counted in.
    bar: (usize, usize),
    /// The first line of the tab on screen.
    top: u16,
    /// The entry that line belongs to, when the tab lists entries.
    anchor: Option<Anchor>,
    /// Where the view stands among the entries held, when the tab lists them.
    reach: Option<Reach>,
    /// What the frame found at the place the scrollbar's thumb asked for.
    seek: Option<Seek>,
    /// Which line holds the pull requests.
    prs_at: Option<usize>,
    /// Which lines take a click.
    rows: Rows,
}

/// The current tab's lines, scrolled to `height` lines of room.
#[expect(
    clippy::too_many_lines,
    reason = "draws each of the session column's tabs in one pass"
)]
fn tab_lines(
    app: &App,
    layouts: &mut Layouts,
    agent: &Agent,
    history: &History,
    width: usize,
    height: u16,
) -> Body {
    let facts = &history.facts;
    let cwd = agent.cwd.as_str();
    let filter = app.filter(Focus::Session);
    let open = app.open.get(&agent.transcript);
    let mut lines = Vec::new();
    let mut rows = Rows::new();
    let mut prs_at = None;
    // Where the view stands among the held entries even when none is shown, and
    // whether a filter ended the loading of older ones.
    let mut reach = None;
    let mut unsearched = false;
    match app.tab {
        Tab::Overview => {
            let mut first = Vec::new();
            if let Some(model) = &facts.model {
                first.push(model.trim_start_matches("claude-").to_owned());
            }
            if let Some(branch) = &facts.branch {
                first.push(format!("on {branch}"));
            }
            first.push(format!("{} prompts", facts.prompts));
            first.push(format!("{} replies", facts.replies));
            if facts.output_tokens > 0 {
                first.push(format!("{} tokens out", thousands(facts.output_tokens)));
            }
            if facts.agents > 0 {
                first.push(format!("{} sub-agents", facts.agents));
            }
            // The facts wrap between parts rather than lose the last ones.
            let mut line = String::new();
            for part in first {
                if !line.is_empty() && line.chars().count() + 3 + part.chars().count() > width {
                    lines.push(Line::raw(mem::take(&mut line)));
                }
                if !line.is_empty() {
                    line.push_str(" · ");
                }
                line.push_str(&truncate(&part, width));
            }
            lines.push(Line::raw(line));
            if let (Some(started), Some(last)) = (&facts.started, &facts.last) {
                lines.push(Line::styled(
                    format!("started {} · active {}", when(started), when(last)),
                    theme::dim(),
                ));
            }
            if !facts.tasks.is_empty() {
                let done = facts
                    .tasks
                    .iter()
                    .filter(|task| task.status == "completed")
                    .count();
                lines.push(Line::default());
                lines.push(Line::from(vec![
                    Span::styled("Tasks", theme::header()),
                    Span::styled(format!("  {done} of {}", facts.tasks.len()), theme::dim()),
                ]));
                for task in &facts.tasks {
                    let (glyph, glyph_style, text) = match task.status.as_str() {
                        "completed" => ("✔", Style::new().fg(theme::ACCENT), theme::dim()),
                        "in_progress" => (
                            theme::glyph(Status::Working, app.frame),
                            theme::status(Status::Working),
                            Style::new(),
                        ),
                        _ => ("○", theme::dim(), Style::new()),
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(glyph, glyph_style),
                        Span::raw(" "),
                        Span::styled(truncate(&task.subject, width.saturating_sub(5)), text),
                    ]));
                }
            }
            if !facts.prs.is_empty() {
                lines.push(Line::default());
                lines.push(Line::default());
                prs_at = Some(lines.len() - 1);
            }
            if let Some(prompt) = &facts.last_prompt {
                lines.push(Line::default());
                lines.push(Line::styled("Last prompt", theme::header()));
                for part in wrap(prompt, width.saturating_sub(2), 6) {
                    lines.push(Line::raw(format!("  {part}")));
                }
            }
        }
        Tab::Prompts | Tab::Activity => {
            let list = if app.tab == Tab::Prompts {
                List::Log
            } else {
                List::Activity
            };
            let source = Source {
                list,
                items: facts.entries(list),
                shape: history.shape(list),
                unsearched: !filter.is_empty() && history.unsearched(list),
            };
            unsearched = source.unsearched;
            let entries = said(app, layouts, open, source, width, height);
            reach = entries.reach;
            if entries.total > 0 {
                return Body {
                    lines: entries.lines,
                    offset: 0,
                    bar: entries.bar,
                    top: entries.top,
                    anchor: entries.anchor,
                    reach: entries.reach,
                    seek: entries.seek,
                    prs_at: None,
                    rows: entries.rows,
                };
            }
        }
        Tab::Files => files(&mut lines, &mut rows, app, facts, cwd, width),
    }
    if lines.is_empty() {
        let text = if filter.is_empty() {
            "Nothing yet".to_owned()
        } else {
            format!("Nothing matches “{filter}”")
        };
        lines.push(Line::styled(text, theme::dim()));
        if unsearched {
            lines.push(Line::styled(UNSEARCHED, theme::dim()));
        }
    }
    let total = lines.len();
    let top = top_of(
        app.scroll_of(Focus::Session),
        u16::try_from(total).unwrap_or(u16::MAX),
        height,
        None,
    );
    Body {
        lines,
        offset: top,
        bar: (usize::from(top), total),
        top,
        anchor: None,
        reach,
        seek: None,
        prs_at,
        rows,
    }
}

/// The width of the age column and the fold marker before every entry.
const AGE: usize = 10;

/// What a tab of entries shows: the rows in view, with where the view stands.
struct Entries {
    /// The rows in view, from the first line on screen.
    lines: Vec<Line<'static>>,
    /// Which line of the list each row that takes a click is.
    rows: Rows,
    /// How many lines the held entries have.
    total: usize,
    /// The first line on screen.
    top: u16,
    /// The entry that line belongs to, unless it is the first line.
    anchor: Option<Anchor>,
    /// Where the view stands among the held entries, when it shows any.
    reach: Option<Reach>,
    /// The scrollbar's position and length, which count the entries not held.
    bar: (usize, usize),
    /// What was found at the place the scrollbar's thumb asked for, if it did.
    seek: Option<Seek>,
}

/// The entries a tab lists, and what is known of those not held.
struct Source<'a> {
    list: List,
    items: &'a [Said],
    shape: Shape<'a>,
    /// Whether the list stopped loading older entries under a filter.
    unsearched: bool,
}

/// An entry of a list, placed by the lines before it.
struct Placed {
    /// Its place among the items.
    index: usize,
    id: EntryId,
    open: bool,
    /// The run of held entries it is in, counted from the newest; the entries
    /// not held in a gap separate two runs.
    run: usize,
    /// The line of the list its first row is.
    start: usize,
    height: usize,
}

/// The line of a tab of entries that ends a filtered list the window stopped
/// loading.
const UNSEARCHED: &str = "Older entries are not searched; ctrl+f searches the whole session.";

/// Where the view stands among the lines of the held entries.
struct Stand {
    /// The first line on screen.
    top: usize,
    /// The lines of the run of held entries the view is in.
    lo: usize,
    hi: usize,
    /// Whether the run ends with the line that says older entries are not
    /// searched, which `hi` counts.
    tail: bool,
}

/// Where the view of `height` lines stands among the `placed` entries: at the
/// line the scrollbar's thumb asks for, `seek`, as a run and a line of the held
/// entries, else at the scroll's anchor entry, or at its line when it has none,
/// and never out of the run of held entries that entry is in. `tail` is whether
/// the oldest run ends with one more line.
fn stand(
    app: &App,
    placed: &[Placed],
    height: u16,
    tail: bool,
    seek: Option<(usize, usize)>,
) -> Stand {
    let scroll = app.scroll_of(Focus::Session);
    let anchored = seek
        .map(|(run, line)| (run, 0, i64::try_from(line).unwrap_or(i64::MAX)))
        .or_else(|| {
            scroll.anchor.and_then(|anchor| {
                // The entry itself, else the nearest newer one still listed.
                let (entry, line) = match placed.iter().find(|entry| entry.id == anchor.id) {
                    Some(entry) => (entry, anchor.line),
                    None => (placed.iter().rfind(|entry| entry.id > anchor.id)?, 0),
                };
                // The wheel's steps since the last frame count wherever the
                // view stands.
                let line = i64::from(line).saturating_add(i64::from(anchor.moved));
                Some((entry.run, entry.start, line))
            })
        });
    let current = anchored
        .map(|(run, ..)| run)
        .or_else(|| placed.first().map(|entry| entry.run))
        .unwrap_or(0);
    let wanted = anchored.map_or_else(
        || i64::from(scroll.top),
        |(_, start, line)| {
            i64::try_from(start)
                .unwrap_or(i64::MAX)
                .saturating_add(line)
        },
    );
    let mut in_run = placed.iter().filter(|entry| entry.run == current);
    let lo = in_run.next().map_or(0, |entry| entry.start);
    let end = in_run
        .next_back()
        .map_or(lo, |entry| entry.start + entry.height);
    let tail = tail && placed.last().is_some_and(|last| last.run == current);
    let hi = end.max(lo) + usize::from(tail);
    let top = usize::try_from(wanted.max(0))
        .unwrap_or(0)
        .clamp(lo, hi.saturating_sub(usize::from(height)).max(lo));
    Stand { top, lo, hi, tail }
}

/// The entries of the prompts or the activity, newest first, as Claude
/// Code draws them. A folded entry shows its first lines with how many it
/// leaves out; a click opens it.
///
/// Each entry is laid out once for the width and its fold state. The rows of
/// the entries in the view of `height` lines are built from that, and the view
/// stands where the scroll's anchor entry now lies. It never leaves the run of
/// held entries that entry is in, so it never draws a line it does not hold.
fn said(
    app: &App,
    layouts: &mut Layouts,
    open: Option<&HashSet<EntryId>>,
    Source {
        list,
        items,
        shape,
        unsearched,
    }: Source<'_>,
    width: usize,
    height: u16,
) -> Entries {
    let filter = app.filter(Focus::Session);
    let (placed, total) = place(app, layouts, open, (items, shape.gaps), width);
    let filtered = !filter.is_empty();
    let asked = app.scroll_of(Focus::Session).seek;
    let seek = asked.and_then(|at| seek_line(&placed, shape, filtered, (at, total)));
    let Stand { top, lo, hi, tail } = stand(app, &placed, height, unsearched, seek);
    let anchor = if top == 0 {
        None
    } else {
        placed
            .iter()
            .rfind(|entry| entry.height > 0 && entry.start <= top)
            .map(|entry| Anchor {
                id: entry.id,
                line: i32::try_from(top - entry.start).unwrap_or(i32::MAX),
                moved: 0,
            })
    };
    let bottom = (top + usize::from(height)).min(hi);
    let mut lines = Vec::new();
    let mut rows = Rows::new();
    let shown: Vec<&Placed> = placed
        .iter()
        .filter(|entry| entry.start < bottom && entry.start + entry.height > top)
        .collect();
    for entry in &shown {
        let item = &items[entry.index];
        let laid = layouts.get(entry.id, entry.open, || lay_out(item, entry.open, width));
        let hovered = app.hover == Some(Target::Entry(entry.id));
        let from = top.saturating_sub(entry.start);
        let to = (bottom - entry.start).min(entry.height);
        for row in from..to {
            lines.push(entry_row(item, laid, row, entry.open, hovered));
            if laid.folds {
                rows.push((entry.start + row, None, Target::Entry(entry.id)));
            }
        }
    }
    if tail && bottom == hi {
        lines.push(Line::styled(UNSEARCHED, theme::dim()));
    }
    let lead = app.ahead.min(NEAR);
    let reach = shown
        .first()
        .zip(shown.last())
        .map(|(newest, oldest)| Reach {
            list,
            first: newest.id,
            last: oldest.id,
            below: hi.saturating_sub(top + usize::from(height)),
            above: top - lo,
            filtered,
            keep: within(&placed, (newest.id, oldest.id), lead),
        })
        // With no entry shown, the view stands at the oldest held entry, so a
        // filter that shows nothing still loads pages.
        .or_else(|| {
            let oldest = items.first()?.id;
            filtered.then_some(Reach {
                list,
                first: oldest,
                last: oldest,
                below: 0,
                above: 0,
                filtered,
                keep: (oldest, oldest),
            })
        });
    // A filter shows held entries only, so it counts none that are not held.
    let counted = if filtered { None } else { reach };
    let bar = bar(
        shape,
        counted,
        (top, total + usize::from(tail), placed.len()),
    );
    let seek = asked.map(|at| match seek {
        Some(_) => Seek::Held,
        None => Seek::Past(
            list,
            place_share(at, bar.1.saturating_sub(usize::from(height))),
        ),
    });
    Entries {
        lines,
        rows,
        total,
        top: u16::try_from(top).unwrap_or(u16::MAX),
        anchor,
        reach,
        bar,
        seek,
    }
}

/// The oldest and the newest of the `placed` entries that lie within `lead`
/// lines of the entries in view, `newest` to `oldest`, the entries in view too.
fn within(
    placed: &[Placed],
    (newest, oldest): (EntryId, EntryId),
    lead: usize,
) -> (EntryId, EntryId) {
    let at = |id: EntryId| placed.iter().position(|entry| entry.id == id).unwrap_or(0);
    // The entry that lies `lead` lines out along `entries`, else the last.
    let farthest = |entries: &mut dyn Iterator<Item = &Placed>, from: EntryId| {
        let mut lines = 0;
        let mut id = from;
        for entry in entries {
            if lines >= lead {
                break;
            }
            lines += entry.height;
            id = entry.id;
        }
        id
    };
    (
        farthest(&mut placed.iter().skip(at(oldest) + 1), oldest),
        farthest(&mut placed.iter().take(at(newest)).rev(), newest),
    )
}

/// The `items` that pass the filter, newest first, placed by the lines before
/// them, with how many lines they take. A gap is recorded after the newest
/// entry below it.
fn place(
    app: &App,
    layouts: &mut Layouts,
    open: Option<&HashSet<EntryId>>,
    (items, gaps): (&[Said], &Gaps),
    width: usize,
) -> (Vec<Placed>, usize) {
    let filter = app.filter(Focus::Session);
    let mut placed: Vec<Placed> = Vec::with_capacity(items.len());
    let mut total = 0;
    let mut run = 0;
    for (index, item) in items.iter().enumerate().rev() {
        if gaps.contains_key(&item.id) {
            run += 1;
        }
        if !filter.is_empty() && !state::passes(filter, &item.searched()) {
            continue;
        }
        let open = open.is_some_and(|open| open.contains(&item.id));
        let height = layouts
            .get(item.id, open, || lay_out(item, open, width))
            .lines
            .len();
        placed.push(Placed {
            index,
            id: item.id,
            open,
            run,
            start: total,
            height,
        });
        total += height;
    }
    (placed, total)
}

/// The run of held entries and the line of the `placed` ones that the
/// scrollbar's line `at` stands for, among `total` lines held: the inverse of
/// [`bar`]. `None` when no held entry covers it, because it falls in a gap or
/// among the older entries not held. A filter counts none that are not held.
fn seek_line(
    placed: &[Placed],
    shape: Shape<'_>,
    filtered: bool,
    (at, total): (usize, usize),
) -> Option<(usize, usize)> {
    placed.chunk_by(|a, b| a.run == b.run).find_map(|run| {
        let (first, last) = (run.first()?, run.last()?);
        let lines = last.start + last.height - first.start;
        // The gaps above the run's newest entry move it down the bar.
        let above: usize = if filtered {
            0
        } else {
            shape
                .gaps
                .range(first.id..)
                .map(|(_, missing)| *missing)
                .sum()
        };
        let from = first.start + tall(above, total, placed.len());
        (from..from + lines)
            .contains(&at)
            .then(|| (first.run, first.start + (at - from)))
    })
}

/// How far into the transcript's bytes the scrollbar's line `at` of
/// `scrollable` stands for, in millionths from its first byte. The list shows
/// the newest entries first, so the top of the bar is the end of the transcript
/// and its bottom the start.
fn place_share(at: usize, scrollable: usize) -> u32 {
    let far = u128::try_from(at.min(scrollable)).unwrap_or(0) * u128::from(MILLION)
        / u128::try_from(scrollable.max(1)).unwrap_or(1);
    MILLION - u32::try_from(far).unwrap_or(MILLION)
}

/// How many lines `entries` entries not held take: as many as those held take
/// on average, `held` entries taking `total` lines.
fn tall(entries: usize, total: usize, held: usize) -> usize {
    entries * total / held.max(1)
}

/// The scrollbar's position and length for a view of the held entries that
/// starts at line `top` of `total` lines, which `held` entries take. The
/// entries not held count as tall as those held are on average. `reach` is
/// `None` for a view that counts none.
fn bar(
    shape: Shape<'_>,
    reach: Option<Reach>,
    (top, total, held): (usize, usize, usize),
) -> (usize, usize) {
    let Some(reach) = reach else {
        return (top, total);
    };
    let not_held = shape.gaps.values().sum::<usize>() + shape.older;
    // The gaps above the view's first entry move the thumb down.
    let above: usize = shape
        .gaps
        .range(reach.first..)
        .map(|(_, missing)| *missing)
        .sum();
    (
        top + tall(above, total, held),
        total + tall(not_held, total, held),
    )
}

/// Row `row` of an entry laid out as `laid`, with what a frame draws over it:
/// the age and the fold marker before the first row, and before the rest a bar
/// when the pointer is on the entry. The marker is `▸` before a folded entry
/// that has more to show, and `▾` before an open one under the pointer.
fn entry_row(item: &Said, laid: &Laid, row: usize, open: bool, hovered: bool) -> Line<'static> {
    let mut spans = if row == 0 {
        let age = item.at.as_deref().map_or_default(when);
        let marker = match (laid.folds, open, hovered) {
            (true, false, false) => Span::styled("▸ ", theme::dim()),
            (true, _, true) => Span::raw(if open { "▾ " } else { "▸ " }),
            _ => Span::raw("  "),
        };
        vec![
            Span::styled(
                format!("{:>7} ", age.trim_end_matches(" ago")),
                theme::dim(),
            ),
            marker,
        ]
    } else if hovered {
        vec![
            Span::styled("▏", Style::new().fg(theme::ACCENT)),
            Span::raw(" ".repeat(laid.indent.saturating_sub(1))),
        ]
    } else {
        vec![Span::raw(" ".repeat(laid.indent))]
    };
    if let Some(line) = laid.lines.get(row) {
        spans.extend(line.spans.iter().cloned());
    }
    // What a folded entry leaves out shows plain under the pointer.
    if hovered
        && laid.tail == Some(row)
        && let Some(tail) = spans.last_mut()
    {
        tail.style = Style::new();
    }
    Line::from(spans)
}

/// An entry laid out for `width` columns, open or folded.
#[expect(
    clippy::too_many_lines,
    reason = "lays out one entry of a prompts or activity tab in one pass"
)]
fn lay_out(item: &Said, open: bool, width: usize) -> Laid {
    let (mut lead, close, style) = marks(&item.kind);
    // A call whose tool reported an error shows its name in red.
    if item.result.as_ref().is_some_and(|outcome| outcome.error)
        && let Some(name) = lead.first_mut()
    {
        name.style = name.style.fg(theme::ERROR);
    }
    let lead_width = lead.iter().map(Span::width).sum::<usize>();
    // Open, a message's text starts under its mark rather than after
    // its sender, so it keeps the width of the tab.
    let hang = open && lead_width > 6 && !matches!(item.kind, Kind::Tool { .. });
    let indent = if hang { AGE + 2 } else { AGE + lead_width };
    let room = width.saturating_sub(indent + close.chars().count()).max(8);
    let mut body = if hang {
        let mut body = vec![Line::default()];
        // A note whose text opens its longer text shows the longer one.
        let first = item.text.lines().next().unwrap_or_default();
        if !item
            .more
            .as_deref()
            .is_some_and(|more| more.trim_start().starts_with(first))
        {
            body.extend(body_lines(item, style, room));
        }
        body
    } else {
        body_lines(item, style, room)
    };
    let most = match item.kind {
        Kind::Prompt | Kind::Reply => 3,
        Kind::Ended { .. } => 2,
        Kind::Message { .. }
        | Kind::Idle { .. }
        | Kind::Relayed { .. }
        | Kind::Tool { .. }
        | Kind::Command
        | Kind::Shell
        | Kind::Memory
        | Kind::Interrupted => 1,
    };
    let mut more = item
        .more
        .as_deref()
        .map_or_default(|more| markup::markdown(more, width.saturating_sub(indent)));
    let mut result = item
        .result
        .as_ref()
        .map_or_default(|outcome| result_lines(outcome, width.saturating_sub(indent + 3)));
    let left = if open {
        0
    } else {
        // Folded, blank lines between paragraphs take no row, and a
        // one-line entry runs its lines together.
        body.retain(|line| line.width() > 0);
        let left = body.len().saturating_sub(most) + more.len() + result.len();
        if most == 1 && body.len() > 1 {
            let mut joined: Vec<Span<'static>> = Vec::new();
            for (index, line) in body.drain(..).enumerate() {
                if index > 0 {
                    joined.push(Span::raw(" "));
                }
                joined.extend(line.spans);
            }
            body.push(Line::from(joined));
        }
        body.truncate(most);
        more.clear();
        result.clear();
        left
    };
    // What a folded entry leaves out shows at its end, never cut off.
    let tail = if left > 0 {
        format!("  +{left}")
    } else {
        String::new()
    };
    let folds = left > 0 || open;
    let mut lines = Vec::new();
    let mut tail_row = None;
    let last = body.len().saturating_sub(1);
    for (index, part) in body.into_iter().enumerate() {
        let mut spans = mem::take(&mut lead);
        spans.extend(part.spans);
        // The age and the fold marker take the first row's first columns, and
        // the bar or blanks the others'.
        let room = width.saturating_sub(if index == 0 { AGE } else { indent });
        if index == last {
            let end = close.chars().count() + tail.chars().count();
            let mut line = markup::fit(&Line::from(spans), room.saturating_sub(end));
            line.spans.push(Span::styled(close, theme::dim()));
            line.spans.push(Span::styled(tail.clone(), theme::dim()));
            tail_row = Some(lines.len());
            lines.push(line);
        } else {
            lines.push(markup::fit(&Line::from(spans), room));
        }
    }
    lines.extend(more.into_iter().map(|part| Line::from(part.spans)));
    // What the tool gave back, under Claude Code's `⎿`.
    for (index, part) in result.into_iter().enumerate() {
        let mut spans = vec![Span::styled(
            if index == 0 { "⎿  " } else { "   " },
            theme::dim(),
        )];
        spans.extend(part.spans);
        lines.push(Line::from(spans));
    }
    Laid {
        lines,
        indent,
        folds,
        tail: tail_row,
    }
}

/// How many lines of a tool's result an open call shows.
const RESULT_LINES: usize = 12;

/// A tool's result as an open call shows it: its first lines, dim, or
/// red for an error, and how many it leaves out.
fn result_lines(outcome: &Outcome, width: usize) -> Vec<Line<'static>> {
    let style = if outcome.error {
        Style::new().fg(theme::ERROR)
    } else {
        Style::new().fg(theme::QUIET)
    };
    let all: Vec<Line<'static>> = outcome
        .text
        .lines()
        .flat_map(|line| markup::wrap(&[Span::styled(line.to_owned(), style)], width))
        .collect();
    let left = all.len().saturating_sub(RESULT_LINES);
    let mut shown: Vec<Line<'static>> = all.into_iter().take(RESULT_LINES).collect();
    if left > 0 {
        shown.push(Line::styled(format!("… +{left} lines"), theme::dim()));
    }
    shown
}

/// Which line of a tab, and which of its columns when not all of them,
/// stands for what a click there reaches.
type Rows = Vec<(usize, Option<(usize, usize)>, Target)>;

/// A file list's rows, each with its place in the full list.
type Listed<'files> = Vec<(usize, &'files (String, usize))>;

/// An entry's text as Claude Code draws it, wrapped to `room` columns:
/// shell commands in its code colours, prompts and messages as markdown.
fn body_lines(item: &Said, style: Style, room: usize) -> Vec<Line<'static>> {
    let plain = |text: &str| markup::wrap(&[Span::styled(text.to_owned(), style)], room);
    let code = |text: &str| -> Vec<Line<'static>> {
        markup::code(text, "bash")
            .into_iter()
            .flat_map(|line| markup::wrap(&line.spans, room))
            .collect()
    };
    match &item.kind {
        Kind::Tool { name } if name == "Bash" || name == "PowerShell" => code(&item.text),
        Kind::Shell => code(&item.text),
        Kind::Prompt
        | Kind::Reply
        | Kind::Message { .. }
        | Kind::Idle { .. }
        | Kind::Relayed { .. }
        | Kind::Ended { .. } => {
            let lines = markup::markdown(&item.text, room);
            if style == Style::new() {
                lines
            } else {
                lines
                    .into_iter()
                    .map(|line| line.patch_style(style))
                    .collect()
            }
        }
        Kind::Tool { .. } | Kind::Command | Kind::Memory | Kind::Interrupted => plain(&item.text),
    }
}

/// What Claude Code draws before and after an entry's text, and the
/// text's style.
fn marks(kind: &Kind) -> (Vec<Span<'static>>, &'static str, Style) {
    let name = |from: &str| Span::styled(format!("@{from} "), Style::new().fg(Color::Cyan));
    match kind {
        Kind::Prompt | Kind::Command => (vec![Span::styled("❯ ", theme::dim())], "", Style::new()),
        Kind::Shell => (
            vec![Span::styled("! ", Style::new().fg(theme::SHELL))],
            "",
            Style::new(),
        ),
        Kind::Memory => (
            vec![Span::styled("# ", Style::new().fg(theme::ACCENT))],
            "",
            Style::new(),
        ),
        Kind::Ended { status } => {
            let tone = match status.as_str() {
                "completed" => Style::new().fg(theme::WORKING),
                "failed" => Style::new().fg(theme::ERROR),
                "killed" => Style::new().fg(theme::WARNING),
                _ => Style::new(),
            };
            (
                vec![Span::styled("⏺ ", tone)],
                "",
                Style::new().fg(theme::QUIET),
            )
        }
        Kind::Message { from } => (vec![name(from)], "", Style::new()),
        Kind::Idle { from, how } => {
            let tone = match how.as_str() {
                "failed" => Style::new().fg(theme::ERROR),
                "was interrupted" => Style::new().fg(theme::WARNING),
                _ => Style::new().fg(theme::WORKING),
            };
            (
                vec![
                    Span::styled("⏺ ", tone),
                    name(from),
                    Span::styled(format!("{how} · "), theme::dim()),
                ],
                "",
                theme::dim(),
            )
        }
        Kind::Relayed { from } => (
            vec![
                Span::styled("← ", Style::new().fg(theme::ACCENT)),
                Span::styled(format!("{from}: "), theme::dim()),
            ],
            "",
            Style::new(),
        ),
        Kind::Interrupted => (Vec::new(), "", theme::dim()),
        Kind::Reply => (vec![Span::raw("⏺ ")], "", Style::new()),
        Kind::Tool { name } => (
            vec![
                Span::styled(name.clone(), Style::new().add_modifier(Modifier::BOLD)),
                Span::styled("(", theme::dim()),
            ],
            ")",
            Style::new(),
        ),
    }
}

/// The files the session changed, then under `Read` the ones it only
/// read, each list the most used first: each file's name, then its folder
/// shortened to fit, dim. A row of chips above narrows the lists to the
/// changed or the read files and to one file type; a click on a file
/// opens it, and on its folder narrows the tab to that folder.
#[expect(
    clippy::too_many_lines,
    reason = "draws the files tab's chips and both file lists in one pass"
)]
fn files(
    lines: &mut Vec<Line<'static>>,
    rows: &mut Rows,
    app: &App,
    facts: &Facts,
    cwd: &str,
    width: usize,
) {
    let filter = app.filter(Focus::Session);
    let changed = facts.files.len();
    // A file the session changed shows once, with the changes.
    let read: Vec<_> = facts
        .read
        .iter()
        .enumerate()
        .filter(|(_, (path, _))| !facts.files.iter().any(|(file, _)| file == path))
        .map(|(index, file)| (changed + index, file))
        .collect();
    // Files in temporary folders, such as a session's scratch logs, stay
    // out unless asked for, as scratch sessions do.
    let kept = |list| scratch_split(list, filter, app.show_scratch_files);
    let (changed_list, changed_scratch) = kept(facts.files.iter().enumerate().collect());
    let (read_list, read_scratch) = kept(read);
    let scratch = changed_scratch + read_scratch;
    // The chips: which list, then the file types the lists hold, the most
    // common first.
    let mut types: Vec<(String, usize)> = Vec::new();
    for (_, (path, _)) in changed_list
        .iter()
        .filter(|_| app.files_shown != state::Shown::Read)
        .chain(
            read_list
                .iter()
                .filter(|_| app.files_shown != state::Shown::Changed),
        )
    {
        let kind = file_type(path);
        match types.iter_mut().find(|(known, _)| *known == kind) {
            Some((_, count)) => *count += 1,
            None => types.push((kind, 1)),
        }
    }
    types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let chosen = app.file_type;
    let mut chips: Vec<(String, Target, bool)> = vec![
        (
            format!(" All {} ", changed_list.len() + read_list.len()),
            Target::FileKind(state::Shown::All),
            app.files_shown == state::Shown::All,
        ),
        (
            format!(" Changed {} ", changed_list.len()),
            Target::FileKind(state::Shown::Changed),
            app.files_shown == state::Shown::Changed,
        ),
        (
            format!(" Read {} ", read_list.len()),
            Target::FileKind(state::Shown::Read),
            app.files_shown == state::Shown::Read,
        ),
    ];
    for (kind, count) in types.iter().take(8) {
        let key = type_key(kind);
        chips.push((
            format!(" {kind} {count} "),
            Target::FileType(key),
            chosen == Some(key),
        ));
    }
    let mut spans = Vec::new();
    let mut x = 0;
    for (index, (text, target, on)) in chips.into_iter().enumerate() {
        let gap = if index == 3 { "   " } else { " " };
        if index > 0 {
            spans.push(Span::raw(gap));
            x += gap.len();
        }
        let size = text.chars().count();
        if x + size > width {
            break;
        }
        let hovered = app.hover == Some(target);
        let style = if on {
            theme::raised()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            theme::hover()
        } else {
            theme::dim()
        };
        rows.push((lines.len(), Some((x, x + size)), target));
        spans.push(Span::styled(text, style));
        x += size;
    }
    lines.push(Line::from(spans));
    lines.push(Line::default());
    let lists = [
        (None, changed_list, app.files_shown != state::Shown::Read),
        (
            Some("Read"),
            read_list,
            app.files_shown != state::Shown::Changed,
        ),
    ];
    for (title, mut list, shown) in lists {
        if !shown {
            continue;
        }
        if let Some(key) = chosen {
            list.retain(|(_, (path, _))| type_key(&file_type(path)) == key);
        }
        if list.is_empty() {
            continue;
        }
        if let Some(title) = title
            && app.files_shown == state::Shown::All
        {
            if lines.len() > 2 {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![
                Span::styled(title, theme::header()),
                Span::styled(format!("  {}", list.len()), theme::dim()),
            ]));
        }
        list.sort_by(|(_, a), (_, b)| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (index, (path, count)) in list {
            let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
            let count = format!("{count:>4}× ");
            let room = width.saturating_sub(count.chars().count() + name.chars().count() + 2);
            let shown_folder = short_folder(folder, cwd, room);
            let hovered = app.hover == Some(Target::File(index));
            let on_folder = app.hover == Some(Target::Folder(index));
            let start = count.chars().count() + name.chars().count() + 2;
            let end = start + shown_folder.chars().count();
            let line = Line::from(vec![
                Span::styled(count, theme::dim()),
                Span::styled(
                    name.to_owned(),
                    if hovered {
                        Style::new()
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    } else {
                        Style::new().add_modifier(Modifier::BOLD)
                    },
                ),
                Span::raw("  "),
                Span::styled(
                    shown_folder,
                    if on_folder {
                        Style::new()
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::UNDERLINED)
                    } else {
                        theme::dim()
                    },
                ),
            ]);
            rows.push((lines.len(), Some((start, end)), Target::Folder(index)));
            rows.push((lines.len(), Some((0, start)), Target::File(index)));
            lines.push(markup::fit(&line, width));
        }
    }
    if scratch > 0 {
        let hovered = app.hover == Some(Target::ScratchFiles);
        let text = if app.show_scratch_files {
            format!("  − hide {scratch} files in temporary folders")
        } else {
            format!("  + {scratch} files in temporary folders")
        };
        lines.push(Line::default());
        rows.push((lines.len(), None, Target::ScratchFiles));
        lines.push(Line::styled(
            text,
            if hovered {
                Style::new().fg(theme::ACCENT)
            } else {
                theme::dim()
            },
        ));
    }
}

/// The files of `list` that pass `filter`, without the ones in temporary
/// folders unless `scratch` asks for them, and how many are in those.
fn scratch_split<'files>(
    list: Listed<'files>,
    filter: &str,
    scratch: bool,
) -> (Listed<'files>, usize) {
    let list: Vec<_> = list
        .into_iter()
        .filter(|(_, (path, _))| state::passes(filter, path))
        .collect();
    let temporary = list
        .iter()
        .filter(|(_, (path, _))| state::in_temp(path))
        .count();
    let list = list
        .into_iter()
        .filter(|(_, (path, _))| scratch || !state::in_temp(path))
        .collect();
    (list, temporary)
}

/// A file's type as the chips name it: its extension with the dot, or its
/// whole name when it has none, such as `Makefile`.
fn file_type(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            format!(".{}", extension.to_lowercase())
        }
        _ => name.to_owned(),
    }
}

/// The key a file type's chip is chosen by.
pub(crate) fn type_key(kind: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    kind.hash(&mut hasher);
    hasher.finish()
}

/// `folder` in at most `room` columns: inside the session's folder from
/// there, under the home folder from `~`; then its middle folders cut to
/// their first letter, from the left, and last its start dropped.
fn short_folder(folder: &str, cwd: &str, room: usize) -> String {
    let home = env::var("HOME").unwrap_or_default();
    let shown = if folder == cwd {
        ".".to_owned()
    } else if let Some(inside) = folder
        .strip_prefix(cwd)
        .and_then(|rest| rest.strip_prefix('/'))
        && !cwd.is_empty()
    {
        inside.to_owned()
    } else if !home.is_empty()
        && let Some(rest) = folder.strip_prefix(&home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        format!("~{rest}")
    } else {
        folder.to_owned()
    };
    let mut parts: Vec<String> = shown.split('/').map(str::to_owned).collect();
    let fits = |parts: &[String]| parts.join("/").chars().count() <= room;
    // The last folder says the most; the ones before it shrink first.
    let keep = parts.len().saturating_sub(1);
    for index in 0..keep {
        if fits(&parts) {
            break;
        }
        let Some(part) = parts.get_mut(index) else {
            break;
        };
        let short: String = match part.strip_prefix('.') {
            Some(rest) => format!(".{}", rest.chars().next().map_or_default(String::from)),
            None => part.chars().next().map_or_default(String::from),
        };
        *part = short;
    }
    let mut shown = parts.join("/");
    let size = shown.chars().count();
    if size > room {
        let cut = size - room.saturating_sub(1);
        shown = format!("…{}", shown.chars().skip(cut).collect::<String>());
    }
    shown
}

/// `text` on lines of at most `width` characters, broken at spaces where
/// one is near, at most `most` lines, the last cut with `…`.
fn wrap(text: &str, width: usize, most: usize) -> Vec<String> {
    let width = width.max(8);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    let mut rest: &str = &flat;
    while !rest.is_empty() && out.len() < most {
        if rest.chars().count() <= width {
            out.push(rest.to_owned());
            break;
        }
        if out.len() + 1 == most {
            out.push(truncate(rest, width));
            break;
        }
        let end = rest
            .char_indices()
            .nth(width)
            .map_or(rest.len(), |(at, _)| at);
        let cut = rest[..end]
            .rfind(' ')
            .filter(|at| *at > end / 2)
            .unwrap_or(end);
        out.push(rest[..cut].to_owned());
        rest = rest[cut..].trim_start();
    }
    out
}

// The search over every session.

/// The search, over the columns: the results in session rows, each with
/// the line that matched.
fn search_overlay(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let rect = centred(
        area,
        area.width.saturating_sub(4).min(112),
        area.height.saturating_sub(1),
    );
    hits.cover(rect);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme::ACCENT))
        .title(" Search every session ")
        .title_bottom(Line::styled(" esc closes ", theme::dim()).right_aligned())
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    results(frame, app, inner, hits);
}

fn results(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let mut spans = if app.query.trim().is_empty() {
        vec![Span::styled(
            "Type to search prompts, titles, folders, and what Claude wrote",
            theme::dim(),
        )]
    } else {
        vec![
            Span::styled("Matches", theme::header()),
            Span::styled(format!("  {}", app.results.len()), theme::dim()),
        ]
    };
    if app.deep {
        let (read, total) = app.read;
        spans.push(Span::styled(
            format!("  · reading transcripts {read}/{total}…"),
            theme::dim(),
        ));
    }
    put(frame, row_at(area, area.y), Line::from(spans));
    let list = below(area, 2);
    let term = catalog::terms(&app.query).into_iter().next();
    let rows: Vec<Row<'_>> = app
        .results
        .iter()
        .enumerate()
        .map(|(index, hit)| Row {
            index,
            agent: &hit.agent,
            held: app.running(&hit.agent.session_id).is_some(),
            snippet: hit.snippet.as_deref(),
            prompts: Some(hit.prompts),
        })
        .collect();
    session_rows(
        frame,
        app,
        list,
        &rows,
        (Focus::Search, term.as_deref()),
        hits,
    );
}

/// The search box, drawn like Claude Code's prompt box: a rounded rule, a
/// `❯`, and the terminal's own cursor after the text. It holds the search,
/// or the focused column's filter.
fn search_box(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let column = app.at();
    let filter = app.filter(column);
    let searching = app.focus == Focus::Search;
    let typing = searching || app.filtering;
    let hovered = app.hover == Some(Target::Search);
    let border = if typing {
        Style::new().fg(theme::ACCENT)
    } else if hovered {
        Style::new().fg(Color::Indexed(244))
    } else {
        theme::line()
    };
    // What the filter narrows: the panes, the pane's sessions, or the
    // rows of the session's tab.
    let name = match column {
        Focus::Panes | Focus::Search => "panes",
        Focus::Sessions => "sessions",
        Focus::Session => "this tab",
    };
    let (mark, typed, title) = if searching {
        (
            "❯ ",
            app.query.as_str(),
            " search every session ".to_owned(),
        )
    } else if app.filtering || !filter.is_empty() {
        ("/ ", filter, format!(" filter {name} "))
    } else {
        ("", "", String::new())
    };
    let text = if typed.is_empty() && !typing {
        // A stand-in has no search to point to.
        let hint = if app.is_stand_in() {
            format!("filter {name}")
        } else {
            format!("filter {name}   ctrl+f search every session")
        };
        Line::from(vec![
            Span::styled("/ ", theme::dim()),
            Span::styled(hint, theme::dim()),
        ])
    } else {
        Line::from(vec![
            Span::styled(mark, Style::new().fg(theme::ACCENT)),
            Span::raw(typed.to_owned()),
        ])
    };
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        // The bottom border carries what a footer would: a notice, else
        // where the keys are, so no row goes to either.
        .title_bottom(match &app.notice {
            Some(notice) => Line::styled(
                format!(
                    " ▲ {} ",
                    truncate(notice, usize::from(area.width).saturating_sub(8))
                ),
                Style::new().fg(theme::WARNING),
            ),
            None => Line::styled(" ? keys · ctrl+l layout ", theme::dim()).right_aligned(),
        });
    if !title.is_empty() {
        block = block.title(Line::styled(title, theme::dim()));
    }
    frame.render_widget(Paragraph::new(text).block(block), area);
    if typing {
        let typed = u16::try_from(typed.chars().count()).unwrap_or(u16::MAX);
        hits.cursor = Some(Position::new(
            (area.x + 3 + typed).min(area.right().saturating_sub(2)),
            area.y + 1,
        ));
    }
    hits.add(area, Target::Search);
}

fn help(frame: &mut Frame<'_>, stand_in: bool) {
    let keys: &[(&str, &str)] = if stand_in {
        // The keys of the panes column and the search do nothing here.
        &[
            ("tab", "the pane's sessions, the session"),
            ("← → h l", "the session's tabs"),
            ("↑ ↓ j k", "move the selection, or scroll the session"),
            ("enter", "open the session; resume it"),
            ("r", "resume the session here"),
            ("[  ]", "the session's tabs"),
            ("/", "filter what has focus; esc clears it"),
            ("ctrl+l", "rails or one view at a time"),
            ("click", "select or focus; click again to act"),
            ("wheel", "scroll the list under the pointer"),
            ("esc q", "quit"),
        ]
    } else {
        &[
            ("1 2", "the panes, or the pane and its session"),
            ("tab", "panes, the pane's sessions, the session"),
            ("← → h l", "the session's tabs; past the first, the panes"),
            ("↑ ↓ j k", "move the selection, or scroll the session"),
            ("enter", "go to the pane; open the session; resume it"),
            ("g  r", "go to the pane; resume the session here"),
            ("[  ]", "the session's tabs"),
            ("/", "filter what has focus; esc clears it"),
            ("ctrl+f", "search every session; esc closes it"),
            ("ctrl+l", "rails or one view at a time"),
            ("click", "select or focus; click again to act"),
            ("wheel", "scroll the list under the pointer"),
            ("esc q", "back to the panes; quit from there"),
        ]
    };
    let lines: Vec<Line<'static>> = keys
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("  {key:<10}"), Style::new().fg(theme::ACCENT)),
                Span::raw((*what).to_owned()),
            ])
        })
        .collect();
    let area = centred(
        frame.area(),
        70,
        u16::try_from(lines.len()).unwrap_or(0) + 4,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .title(" Keys ")
                .title_bottom(Line::styled(" any key closes ", theme::dim()).centered())
                .border_style(Style::new().fg(theme::ACCENT))
                .padding(Padding::vertical(1)),
        ),
        area,
    );
}

/// The pull requests on one line, grouped by repository and named without
/// the owner: `rawr #9 #15  hakkiri #145`. Each number is a link that a
/// click opens.
fn pr_links(buf: &mut Buffer, area: Rect, prs: &[Pr]) {
    let mut repos: Vec<(&str, Vec<&Pr>)> = Vec::new();
    for pr in prs {
        let name = pr.repo.rsplit('/').next().unwrap_or(&pr.repo);
        match repos.iter_mut().find(|(repo, _)| *repo == name) {
            Some((_, list)) => list.push(pr),
            None => repos.push((name, vec![pr])),
        }
    }
    let end = area.right();
    let mut x = buf
        .set_stringn(
            area.x,
            area.y,
            "PRs  ",
            usize::from(area.width),
            theme::dim(),
        )
        .0;
    for (index, (repo, list)) in repos.iter().enumerate() {
        let gap = if index == 0 { "" } else { "   " };
        let label = format!("{gap}{repo}");
        if x + u16::try_from(label.len() + 3).unwrap_or(u16::MAX) > end {
            buf.set_string(x, area.y, "…", theme::dim());
            return;
        }
        x = buf
            .set_stringn(x, area.y, &label, usize::from(end - x), Style::new())
            .0;
        for pr in list {
            let text = format!("#{}", pr.number);
            let wide = u16::try_from(text.len()).unwrap_or(u16::MAX);
            if x + 1 + wide > end {
                buf.set_string(x.min(end.saturating_sub(1)), area.y, "…", theme::dim());
                return;
            }
            x += 1;
            hyperlink(
                buf,
                (x, area.y),
                &text,
                &pr.url,
                Style::new().fg(theme::ACCENT),
            );
            x += wide;
        }
    }
}

/// Writes `text` at `at` as an OSC 8 hyperlink to `url`. The sequence rides
/// in the first cell, forced to the text's width, and the cells it covers
/// are skipped, so the columns around it stay where they are. A URL with
/// anything but printable ASCII is written as plain text.
fn hyperlink(buf: &mut Buffer, (x, y): (u16, u16), text: &str, url: &str, style: Style) {
    let safe = url.starts_with("https://") && url.bytes().all(|byte| byte.is_ascii_graphic());
    let wide = u16::try_from(text.chars().count()).unwrap_or(0);
    let (true, Some(forced)) = (safe, NonZeroU16::new(wide)) else {
        buf.set_string(x, y, text, style);
        return;
    };
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_symbol(&format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\"))
            .set_style(style)
            .set_diff_option(CellDiffOption::ForcedWidth(forced));
    }
    for (offset, glyph) in (1..wide).zip(text.chars().skip(1)) {
        if let Some(cell) = buf.cell_mut((x + offset, y)) {
            cell.set_char(glyph)
                .set_style(style)
                .set_diff_option(CellDiffOption::Skip);
        }
    }
}

/// How long ago `stamp`, a UTC time such as `2026-11-13T13:40:12.000Z`,
/// was: `5m ago`. Relative, so no time zone is needed.
fn when(stamp: &str) -> String {
    let parse = |range: Range<usize>| stamp.get(range).and_then(|part| part.parse::<i64>().ok());
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute)) = (
        parse(0..4),
        parse(5..7),
        parse(8..10),
        parse(11..13),
        parse(14..16),
    ) else {
        return stamp.to_owned();
    };
    // Days from 1970-01-01 to the date, by the civil-calendar formula.
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let then = days * 86_400 + hour * 3_600 + minute * 60;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
    let ago = u64::try_from(now - then).unwrap_or(0);
    let age = short_age(Some(Duration::from_secs(ago)));
    if age == "now" {
        age
    } else {
        format!("{age} ago")
    }
}

/// `123456` as `123k`.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the count is shown to one decimal of a million, so a lost low bit changes nothing"
)]
fn thousands(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{}k", count / 1_000),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

/// One session in a list.
struct Row<'a> {
    /// Its place in the list it comes from: the pane's sessions, or the
    /// results.
    index: usize,
    agent: &'a Agent,
    /// Whether a live Claude holds it.
    held: bool,
    snippet: Option<&'a str>,
    prompts: Option<usize>,
}

/// What a session is doing, as its row says it.
enum Where {
    /// Its Claude runs in this pane, or in another pane of the sidebar.
    Pane(usize),
    /// A Claude holds it in a pane rawr has not recorded.
    Running,
    /// No Claude holds it; `r` resumes it.
    Resumable,
}

fn whereabouts(app: &App, row: &Row<'_>) -> Where {
    match app.running(&row.agent.session_id) {
        Some(index) => Where::Pane(index),
        None if row.held => Where::Running,
        None => Where::Resumable,
    }
}

/// Session rows: the status glyph, the title and its age; the line that
/// matched, with the search word lit; then the folder, where it runs, and
/// how many prompts it holds.
#[expect(
    clippy::too_many_lines,
    reason = "draws every line of a session row in one pass"
)]
fn session_rows(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    rows: &[Row<'_>],
    (list, term): (Focus, Option<&str>),
    hits: &mut Hits,
) {
    hits.lists.push((area, list));
    let (current, target): (usize, fn(usize) -> Target) = match list {
        Focus::Search => (app.result, Target::Result),
        Focus::Sessions | Focus::Panes | Focus::Session => (app.session, Target::Session),
    };
    let focused = app.focus == list;
    let height = row_height;
    let total: u16 = rows.iter().map(height).sum();
    let selection = rows.iter().position(|row| row.index == current).map(|at| {
        let start: u16 = rows.iter().take(at).map(height).sum();
        (start, start + rows.get(at).map_or(2, height))
    });
    let skip = top_of(app.scroll_of(list), total, area.height, selection);
    hits.tops.push((list, skip));
    draw_bar(frame, app, hits, list, area, skip, total);
    let area = Rect {
        width: area.width.saturating_sub(u16::from(total > area.height)),
        ..area
    };
    let mut y: u16 = 0;
    for row in rows {
        let tall = height(row);
        let top = y;
        y += tall;
        if top < skip {
            continue;
        }
        let at = area.y + (top - skip);
        if at + tall > area.bottom() {
            break;
        }
        let cell = Rect {
            y: at,
            height: tall,
            ..area
        };
        let selected = row.index == current;
        let hovered = app.hover == Some(target(row.index));
        let place = whereabouts(app, row);
        let (glyph, glyph_style, said) = match place {
            Where::Pane(pane) => {
                let status = app.panes.get(pane).map_or(Status::Idle, |pane| pane.status);
                let name = app.panes.get(pane).map_or_default(unique_name);
                let mine = list == Focus::Sessions && pane == app.pane;
                (
                    theme::glyph(status, app.frame),
                    theme::status(status),
                    if mine {
                        status.word().to_owned()
                    } else {
                        format!("{} in {name}", status.word())
                    },
                )
            }
            Where::Running => ("●", Style::new().fg(theme::WORKING), "running".to_owned()),
            Where::Resumable => ("○", theme::dim(), "resumable".to_owned()),
        };
        let age = short_age(row.agent.idle);
        let width = usize::from(cell.width);
        let room = width.saturating_sub(4 + age.chars().count() + 2);
        let title = truncate(&row.agent.title, room);
        let gap = room.saturating_sub(title.chars().count());
        let bar = Span::styled(
            if selected { "▌" } else { " " },
            Style::new().fg(theme::ACCENT),
        );
        let title_style = if selected {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let mut lines = vec![Line::from(vec![
            bar.clone(),
            Span::raw(" "),
            Span::styled(glyph, glyph_style),
            Span::raw(" "),
            Span::styled(title, title_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(format!(" {age} "), theme::dim()),
        ])];
        if let Some(snippet) = row.snippet {
            let mut spans = vec![bar.clone(), Span::raw("   ")];
            let room = width.saturating_sub(5);
            spans.extend(lit(&truncate(&centre_on(snippet, term, room), room), term));
            lines.push(Line::from(spans));
        }
        let mut facts = vec![row.agent.short_cwd(), said];
        if let Some(prompts) = row.prompts.filter(|count| *count > 0) {
            facts.push(format!("{prompts} prompts"));
        }
        lines.push(Line::from(vec![
            bar,
            Span::raw("   "),
            Span::styled(
                truncate(&facts.join(" · "), width.saturating_sub(5)),
                theme::dim(),
            ),
        ]));
        frame.render_widget(
            Paragraph::new(lines).style(theme::row(selected && focused, hovered)),
            cell,
        );
        hits.add(cell, target(row.index));
    }
}

/// How many lines a session row takes: a third for the line that matched.
fn row_height(row: &Row<'_>) -> u16 {
    if row.snippet.is_some() { 3 } else { 2 }
}

/// `text` with each place `term` occurs lit in the accent, the rest dim.
fn lit(text: &str, term: Option<&str>) -> Vec<Span<'static>> {
    let Some(term) = term.filter(|term| !term.is_empty()) else {
        return vec![Span::styled(text.to_owned(), theme::dim())];
    };
    let lower = text.to_lowercase();
    if lower.len() != text.len() {
        return vec![Span::styled(text.to_owned(), theme::dim())];
    }
    let mut spans = Vec::new();
    let mut from = 0;
    for (at, _) in lower.match_indices(term) {
        spans.push(Span::styled(text[from..at].to_owned(), theme::dim()));
        spans.push(Span::styled(
            text[at..at + term.len()].to_owned(),
            Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
        ));
        from = at + term.len();
    }
    spans.push(Span::styled(text[from..].to_owned(), theme::dim()));
    spans
}

/// The status as a pill on a tinted ground for the states that need the
/// user, and a glyph and word otherwise.
fn pill(status: Status, frame: usize) -> Span<'static> {
    let word = status.word();
    match status {
        Status::NeedsPermission | Status::NeedsAnswer | Status::NeedsLogin | Status::NeedsInput => {
            Span::styled(
                format!(" {} {word} ", theme::glyph(status, frame)),
                Style::new()
                    .fg(theme::WARNING)
                    .bg(Color::Indexed(58))
                    .add_modifier(Modifier::BOLD),
            )
        }
        Status::Failed => Span::styled(
            format!(" {word} "),
            Style::new()
                .fg(theme::ERROR)
                .bg(Color::Indexed(52))
                .add_modifier(Modifier::BOLD),
        ),
        Status::Done | Status::Working | Status::Idle | Status::Stopped => Span::styled(
            format!("{} {word}", theme::glyph(status, frame)),
            theme::status(status),
        ),
    }
}

/// An action button: the key, then what it does, on a ground that rises
/// when hovered.
fn action(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    (button, key, what): (Button, &str, &str),
    hits: &mut Hits,
) {
    let hovered = app.hover == Some(Target::Button(button));
    let (ground, text) = if hovered {
        (theme::raised(), Style::new())
    } else {
        (theme::hover(), theme::dim())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {key} "),
                Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{what} "), text),
        ]))
        .style(ground),
        area,
    );
    hits.add(area, Target::Button(button));
}

fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// An age as herdr writes it: `now`, `5m`, `3h`, `2d`.
fn short_age(idle: Option<Duration>) -> String {
    let Some(idle) = idle else {
        return String::new();
    };
    let minutes = idle.as_secs() / 60;
    match minutes {
        0 => "now".to_owned(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

/// `text` cut to `max` characters, with `…` when cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// The pane's name, with the start of its block ID when another listed pane
/// has the same name.
fn unique_name(pane: &Pane) -> String {
    format!("{} · {}", pane.name, pane.tab)
}

/// `snippet` with its start dropped so `term` sits a third of the way into
/// a line `room` characters wide.
fn centre_on(snippet: &str, term: Option<&str>, room: usize) -> String {
    let Some(term) = term else {
        return snippet.to_owned();
    };
    let lower = snippet.to_lowercase();
    let Some(byte) = lower.find(term) else {
        return snippet.to_owned();
    };
    let at = lower[..byte].chars().count();
    let drop = at.saturating_sub(room / 3);
    if drop == 0 {
        return snippet.to_owned();
    }
    let rest: String = snippet.chars().skip(drop + 1).collect();
    format!("…{rest}")
}

/// The first line to show of a list `total` lines tall in `height` rows:
/// where it was scrolled to, moved to show the `selection` lines when it
/// follows the selection, and never past its end.
fn top_of(scroll: Scroll, total: u16, height: u16, selection: Option<(u16, u16)>) -> u16 {
    let mut top = scroll.top;
    if scroll.follow
        && let Some((start, end)) = selection
    {
        if start < top {
            top = start;
        } else if end > top.saturating_add(height) {
            top = end.saturating_sub(height);
        }
    }
    top.min(total.saturating_sub(height))
}

/// The scrollbar of `list` down the last column of `area`, when a list `total`
/// lines tall does not fit in it, and where it takes the mouse. The column
/// stays reserved while the bar is hidden. The thumb stands where the pointer
/// holds it, else where the list stands.
fn draw_bar(
    frame: &mut Frame<'_>,
    app: &App,
    hits: &mut Hits,
    list: Focus,
    area: Rect,
    top: impl Into<usize>,
    total: impl Into<usize>,
) {
    let geometry = Geometry::new(top.into(), area.height, total.into());
    if !geometry.shown() {
        return;
    }
    let track = Rect {
        x: area.right().saturating_sub(1),
        width: 1,
        ..area
    };
    let (start, size) = geometry.thumb();
    scrollbar::draw(
        frame.buffer_mut(),
        track,
        (app.held_thumb(list).unwrap_or(start), size),
        app.bar_shade(list, app.now),
    );
    hits.bars.push(Track {
        list,
        area: track,
        geometry,
    });
    // Before the rows, so the bar's own column takes the press.
    hits.targets.insert(0, (track, Target::Bar(list)));
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::*;
    use crate::app::facts::Way;
    use crate::app::history::Ask;
    use crate::app::state::Jump;
    use crate::app::state::fixtures::pane;

    const OWN: &str = "block-own";

    /// The text of `app` drawn on a 100 by 40 screen, one line a row.
    fn screen(app: &mut App) -> String {
        let agent = app.current().cloned();
        let shown = agent.as_ref().map(|agent| (agent, None));
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("a test terminal");
        terminal
            .draw(|frame| {
                draw(frame, app, shown);
            })
            .expect("the app draws");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn panes() -> Vec<Pane> {
        vec![
            pane("block-a", "alpha/one", Status::Working, &["Fix login"]),
            pane(OWN, "work/shell", Status::Stopped, &["Fix checkout"]),
            pane("block-b", "beta/two", Status::Idle, &["Tune search"]),
        ]
    }

    #[test]
    fn a_stand_in_draws_its_own_pane_and_no_search() {
        let mut app = App::stand_in(OWN.to_owned());
        app.set_panes(panes());
        for arrangement in [Arrangement::Rails, Arrangement::Single] {
            app.arrangement = arrangement;
            let text = screen(&mut app);
            for shown in ["Fix checkout", "work/shell"] {
                assert!(
                    text.contains(shown),
                    "{arrangement:?} lacks {shown}:\n{text}"
                );
            }
            for hidden in [
                "alpha/one",
                "beta/two",
                "Fix login",
                "Tune search",
                "Panes",
                "ctrl+f",
            ] {
                assert!(
                    !text.contains(hidden),
                    "{arrangement:?} draws {hidden}:\n{text}"
                );
            }
        }
    }

    #[test]
    fn the_full_app_points_to_the_search() {
        let mut app = App::new(None);
        app.set_panes(panes());
        assert!(screen(&mut app).contains("ctrl+f search every session"));
    }

    /// A session's activity of `count` calls, oldest first, shown on a screen
    /// the tests resize.
    struct Rig {
        app: App,
        history: History,
        /// Where the last frame left the view.
        reach: Option<Reach>,
        /// Where the last frame put what takes a click.
        hits: Hits,
        terminal: Terminal<TestBackend>,
    }

    /// The tool call number `place`, one line folded.
    fn call(place: usize, text: &str) -> Said {
        Said {
            id: EntryId {
                line: u64::try_from(place).expect("a small number") * 100,
                index: 0,
            },
            at: None,
            text: text.to_owned(),
            kind: Kind::Tool {
                name: "Read".to_owned(),
            },
            more: None,
            call: None,
            result: None,
        }
    }

    /// The facts of a transcript read whole, which keep the calls `places`
    /// among `total`.
    fn calls(places: Range<usize>, total: usize) -> Facts {
        Facts {
            activity: places
                .map(|place| call(place, &format!("file-{place}.rs")))
                .collect(),
            activity_total: total,
            whole: true,
            ..Facts::default()
        }
    }

    impl Rig {
        fn new(count: usize) -> Self {
            Self::with(calls(0..count, count))
        }

        fn with(facts: Facts) -> Self {
            let mut app = App::new(None);
            app.set_panes(vec![pane(
                "block-a",
                "alpha/one",
                Status::Idle,
                &["Fix login"],
            )]);
            app.tab = Tab::Activity;
            app.cols = 120;
            let mut history = History::default();
            history.merge(facts);
            Self {
                app,
                history,
                reach: None,
                hits: Hits::default(),
                terminal: Terminal::new(TestBackend::new(120, 40)).expect("a test terminal"),
            }
        }

        /// Draws a frame, as the loop does, and returns what it shows.
        fn draw(&mut self) -> Vec<String> {
            self.frame();
            let buffer = self.terminal.backend().buffer();
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect()
        }

        /// Draws a frame and keeps where it scrolled to.
        fn frame(&mut self) {
            let agent = self.app.current().cloned();
            let shown = agent.as_ref().map(|agent| (agent, Some(&self.history)));
            let mut hits = None;
            self.terminal
                .draw(|frame| hits = Some(draw(frame, &mut self.app, shown)))
                .expect("the app draws");
            let hits = hits.expect("a frame was drawn");
            self.app.settle(&hits.tops, hits.anchor, &hits.bars);
            self.app.cols = hits.width;
            self.reach = hits.reach;
            self.app.found_seek(hits.seek);
            self.hits = hits;
        }

        /// The entries a frame shows, one text each.
        fn entries(&mut self) -> Vec<String> {
            self.draw()
                .into_iter()
                .filter(|line| line.contains("Read("))
                .map(|line| line.trim().to_owned())
                .collect()
        }

        /// How many entries have been laid out.
        fn laid(&self) -> usize {
            self.app.layouts.laid()
        }

        fn scroll(&mut self, lines: i64) {
            self.app.scroll_lines(Focus::Session, lines);
        }
    }

    #[test]
    fn a_tab_counts_the_whole_session_once_it_is_read() {
        let mut rig = Rig::new(300);
        rig.history.facts.activity_total = 2400;
        let rows = rig.draw();
        assert!(
            rows.iter().any(|row| row.contains("Activity 2400 ")),
            "{rows:?}"
        );
        // The newest 300 are read and the start is not.
        rig.history.facts.activity_total = 300;
        rig.history.facts.whole = false;
        let rows = rig.draw();
        assert!(
            rows.iter().any(|row| row.contains("Activity 300+ ")),
            "{rows:?}"
        );
    }

    /// What the daemon answers to the older page `ask`, from calls `places`.
    fn older_page(places: Range<usize>, texts: impl Fn(usize) -> String) -> Vec<Said> {
        places.map(|place| call(place, &texts(place))).collect()
    }

    #[test]
    fn the_view_waits_at_the_oldest_held_entry_for_its_page() {
        let mut rig = Rig::with(calls(1000..1300, 2400));
        rig.scroll(100_000);
        let shown = rig.entries();
        // The oldest held entry is the last row, with nothing drawn below it.
        assert!(
            shown.last().expect("rows").contains("file-1000.rs"),
            "{shown:?}"
        );
        let reach = rig.reach.expect("the view shows entries");
        assert_eq!(reach.below, 0);
        let ask = rig.history.settle(reach, 200).expect("a page is asked for");
        assert_eq!(
            (ask.way, ask.at),
            (Way::Older, rig.history.facts.activity[0].id)
        );
        // The reader scrolls on while the page is in flight, and nothing moves.
        rig.scroll(100_000);
        assert_eq!(rig.entries(), shown);
        // The page lands, and the view moves on.
        let page = older_page(900..1000, |place| format!("file-{place}.rs"));
        rig.history.land(ask, page, true);
        rig.scroll(100_000);
        let shown = rig.entries();
        assert!(
            shown.last().expect("rows").contains("file-900.rs"),
            "{shown:?}"
        );
    }

    #[test]
    fn the_view_does_not_draw_across_a_gap() {
        let mut rig = Rig::with(calls(1000..1300, 5000));
        // The facts of the newest 300 come, with the entries between not held.
        rig.history.merge(calls(4700..5000, 5000));
        assert_eq!(rig.history.shape(List::Activity).gaps.len(), 1);
        rig.scroll(100_000);
        let shown = rig.entries();
        assert!(
            shown.last().expect("rows").contains("file-4700.rs"),
            "{shown:?}"
        );
        assert!(shown.iter().all(|row| !row.contains("file-1299.rs")));
        let reach = rig.reach.expect("the view shows entries");
        assert_eq!(reach.below, 0);
        let ask = rig.history.settle(reach, 200).expect("a page is asked for");
        assert_eq!((ask.way, ask.at), (Way::Older, call(4700, "").id));
    }

    #[test]
    fn a_filter_shows_the_held_matches_and_pages_load_as_the_view_nears_the_last() {
        let text = |place: usize| {
            if place.is_multiple_of(10) {
                format!("cargo test {place}")
            } else {
                format!("ls {place}")
            }
        };
        let mut facts = calls(1000..1300, 2400);
        for said in &mut facts.activity {
            said.text = text(usize::try_from(said.id.line / 100).expect("a small number"));
        }
        let mut rig = Rig::with(facts);
        rig.app.filters[Focus::Session.index()] = "cargo".to_owned();
        let shown = rig.entries();
        assert!(
            shown.iter().all(|row| row.contains("cargo test")),
            "{shown:?}"
        );
        assert!(shown.len() > 5);
        rig.scroll(100_000);
        let shown = rig.entries();
        assert!(
            shown.last().expect("rows").contains("cargo test 1000"),
            "{shown:?}"
        );
        let reach = rig.reach.expect("the view shows entries");
        let ask = rig.history.settle(reach, 200).expect("a page is asked for");
        assert_eq!(ask.way, Way::Older);
        // Older matches appear as the reader scrolls on.
        rig.history.land(ask, older_page(900..1000, text), true);
        rig.scroll(100_000);
        let shown = rig.entries();
        assert!(
            shown.last().expect("rows").contains("cargo test 900"),
            "{shown:?}"
        );
    }

    /// Draws a frame and answers the page the view asks for from `all`, as the
    /// daemon does. Whether it asked for one.
    fn load_older(rig: &mut Rig, all: &[Said]) -> bool {
        rig.frame();
        let Some(reach) = rig.reach else {
            return false;
        };
        let Some(ask) = rig.history.settle(reach, 200) else {
            return false;
        };
        let end = all.partition_point(|said| said.id < ask.at);
        let from = end.saturating_sub(ask.count);
        rig.history.land(ask, all[from..end].to_vec(), from > 0);
        true
    }

    #[test]
    fn the_lead_keeps_entries_by_their_lines() {
        let id = |place: u64| EntryId {
            line: place,
            index: 0,
        };
        // Ten entries of five lines, newest first, the view on the sixth
        // newest.
        let placed: Vec<Placed> = (0..10_u64)
            .rev()
            .enumerate()
            .map(|(at, place)| Placed {
                index: usize::try_from(place).expect("a small number"),
                id: id(place),
                open: false,
                run: 0,
                start: at * 5,
                height: 5,
            })
            .collect();
        // Seven lines past the view take two entries each way, not seven.
        assert_eq!(within(&placed, (id(4), id(4)), 7), (id(2), id(6)));
        assert_eq!(within(&placed, (id(4), id(4)), 0), (id(4), id(4)));
        // The ends of the list stop it.
        assert_eq!(within(&placed, (id(1), id(0)), 100), (id(0), id(9)));
    }

    #[test]
    fn a_filter_without_a_match_pages_to_the_bound_and_then_says_so() {
        let all: Vec<Said> = (0..5000)
            .map(|place| call(place, &format!("file-{place}.rs")))
            .collect();
        let mut rig = Rig::with(calls(4700..5000, 5000));
        rig.app.filters[Focus::Session.index()] = "nothing".to_owned();
        let mut pages = 0;
        while load_older(&mut rig, &all) {
            pages += 1;
            assert!(rig.history.facts.activity.len() <= 3000);
            assert!(pages < 100, "pages keep coming");
        }
        // 2,700 entries more than the newest 300, a page of 100 at a time.
        assert_eq!(pages, 27);
        let rows = rig.draw();
        assert!(
            rows.iter().any(|row| row.contains("Nothing matches")),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.contains("ctrl+f searches the whole session")),
            "{rows:?}"
        );
    }

    #[test]
    fn a_filter_whose_matches_are_the_newest_stops_at_the_bound() {
        let all: Vec<Said> = (0..5000)
            .map(|place| call(place, &format!("file-{place}.rs")))
            .collect();
        let mut rig = Rig::with(calls(4700..5000, 5000));
        // The calls 4900 to 4999 match, and nothing older that is held.
        rig.app.filters[Focus::Session.index()] = "file-49".to_owned();
        let mut pages = 0;
        while load_older(&mut rig, &all) {
            pages += 1;
            assert!(rig.history.facts.activity.len() <= 3000);
            assert!(pages < 100, "pages keep coming");
        }
        assert_eq!(pages, 27);
        assert_eq!(rig.history.facts.activity.len(), 3000);
        rig.scroll(100_000);
        let rows = rig.draw();
        assert!(
            rows.iter()
                .any(|row| row.contains("ctrl+f searches the whole session")),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("file-4900.rs")),
            "{rows:?}"
        );
    }

    #[test]
    fn the_scrollbar_counts_the_entries_not_held_as_tall_as_those_held() {
        let mut rig = Rig::with(calls(1000..1300, 2400));
        let path = rig.app.current().expect("a session").transcript.clone();
        let mut layouts = Layouts::default();
        layouts.show(&path, 100);
        let bar = |rig: &Rig, layouts: &mut Layouts| {
            let source = Source {
                list: List::Activity,
                items: &rig.history.facts.activity,
                shape: rig.history.shape(List::Activity),
                unsearched: false,
            };
            said(&rig.app, layouts, None, source, 100, 20).bar
        };
        // 300 held, one line each, and 2,100 not held at the same height.
        assert_eq!(bar(&rig, &mut layouts), (0, 2400));
        rig.scroll(10);
        assert_eq!(bar(&rig, &mut layouts), (10, 2400));
        // A page lands: the thumb's length does not change.
        rig.scroll(100_000);
        rig.frame();
        let ask = rig
            .history
            .settle(rig.reach.expect("the view shows entries"), 200)
            .expect("a page is asked for");
        assert_eq!(ask.way, Way::Older);
        let page = older_page(900..1000, |place| format!("file-{place}.rs"));
        rig.history.land(ask, page, true);
        assert_eq!(bar(&rig, &mut layouts).1, 2400);
    }

    #[test]
    fn hovering_and_scrolling_lay_nothing_out_again() {
        let mut rig = Rig::new(3000);
        rig.draw();
        assert_eq!(rig.laid(), 3000);
        for place in [2999, 2000, 17] {
            rig.app.hover = Some(Target::Entry(rig.history.facts.activity[place].id));
            rig.draw();
        }
        rig.scroll(40);
        rig.draw();
        rig.scroll(-10);
        rig.draw();
        assert_eq!(rig.laid(), 3000);
    }

    #[test]
    fn opening_an_entry_lays_out_that_entry() {
        let mut rig = Rig::new(3000);
        rig.draw();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 20,
            row: 20,
            modifiers: KeyModifiers::NONE,
        };
        let under = Under {
            target: Some(Target::Entry(rig.history.facts.activity[2990].id)),
            ..Under::default()
        };
        rig.app.on_mouse(click, under);
        rig.draw();
        assert_eq!(rig.laid(), 3001);
        rig.draw();
        assert_eq!(rig.laid(), 3001);
    }

    #[test]
    fn a_width_change_lays_out_every_entry_again_once() {
        let mut rig = Rig::new(3000);
        rig.draw();
        rig.terminal.backend_mut().resize(100, 40);
        rig.app.cols = 100;
        rig.draw();
        assert_eq!(rig.laid(), 6000);
        rig.draw();
        assert_eq!(rig.laid(), 6000);
    }

    #[test]
    fn a_changed_entry_is_laid_out_again_and_a_kept_one_is_not() {
        let mut rig = Rig::new(300);
        rig.draw();
        // The daemon's facts, with the result of the newest call now read.
        let mut read = rig.history.facts.clone();
        read.activity[299].result = Some(Outcome {
            text: "ok".to_owned(),
            error: false,
        });
        let changed = read.activity[299].id;
        let stale = rig.history.merge(read);
        assert_eq!(stale, [changed]);
        let path = rig.app.current().expect("a session").transcript.clone();
        rig.app.layouts.forget(&path, &stale);
        rig.draw();
        assert_eq!(rig.laid(), 301);
    }

    #[test]
    fn a_new_entry_above_the_view_moves_nothing() {
        let mut still = Rig::new(300);
        let mut busy = Rig::new(300);
        for rig in [&mut still, &mut busy] {
            rig.scroll(40);
            rig.draw();
        }
        // The same wheel step, with and without a call arriving in its frame.
        still.scroll(2);
        busy.scroll(2);
        busy.history.facts.activity.push(call(300, "file-new.rs"));
        let shown = still.entries();
        assert!(shown[0].contains("file-257.rs"), "{shown:?}");
        assert_eq!(busy.entries(), shown);
    }

    #[test]
    fn a_view_whose_entry_is_gone_stays_at_the_nearest_newer_one() {
        let mut rig = Rig::new(300);
        rig.scroll(42);
        let shown = rig.entries();
        assert!(shown[0].contains("file-257.rs"), "{shown:?}");
        // The entry the view is anchored in goes, as when the daemon's facts
        // keep fewer of the older entries, or a filter hides it.
        rig.history.facts.activity.remove(257);
        let shown = rig.entries();
        assert!(shown[0].contains("file-258.rs"), "{shown:?}");
    }

    #[test]
    fn a_wheel_step_counts_when_the_anchor_entry_is_gone() {
        let mut rig = Rig::new(300);
        rig.scroll(42);
        let shown = rig.entries();
        assert!(shown[0].contains("file-257.rs"), "{shown:?}");
        // The entry goes in the frame the wheel steps 2 lines: the view stays
        // at the nearest newer entry, 41 lines down, and then moves the 2.
        rig.history.facts.activity.remove(257);
        rig.scroll(2);
        let shown = rig.entries();
        assert!(shown[0].contains("file-255.rs"), "{shown:?}");
    }

    #[test]
    fn an_unscrolled_list_follows_the_newest_entry() {
        let mut rig = Rig::new(300);
        assert!(rig.entries()[0].contains("file-299.rs"));
        rig.history.facts.activity.push(call(300, "file-new.rs"));
        assert!(rig.entries()[0].contains("file-new.rs"));
    }

    impl Rig {
        /// The pages the loop would ask for after the last frame: none while
        /// the view is held for a thumb.
        fn asks(&mut self) -> Vec<Ask> {
            if self.app.holds_view() {
                return Vec::new();
            }
            let reach = self.reach;
            reach
                .and_then(|reach| self.history.settle(reach, 200))
                .into_iter()
                .collect()
        }

        /// The pointer's report `kind` at `column`, `row`, over what the last
        /// frame drew there.
        fn pointer(&mut self, kind: MouseEventKind, column: u16, row: u16) {
            let under = self.hits.under(column, row);
            self.app.on_mouse(
                MouseEvent {
                    kind,
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                },
                under,
            );
        }

        /// The session list's scrollbar as the last frame drew it.
        fn track(&self) -> Track {
            self.hits
                .bars
                .iter()
                .find(|track| track.list == Focus::Session)
                .copied()
                .expect("the list does not fit, so a bar is drawn")
        }

        /// The glyph in each row of the scrollbar's column in the last frame.
        fn glyphs(&self) -> Vec<String> {
            let track = self.track();
            let buffer = self.terminal.backend().buffer();
            (track.area.y..track.area.bottom())
                .map(|y| buffer[(track.area.x, y)].symbol().to_owned())
                .collect()
        }
    }

    const LEFT: MouseButton = MouseButton::Left;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    /// The rows of `glyphs` that hold `glyph`.
    fn rows_of(glyphs: &[String], glyph: &str) -> Vec<usize> {
        (0..glyphs.len())
            .filter(|row| glyphs[*row] == glyph)
            .collect()
    }

    #[test]
    fn dragging_the_thumb_five_rows_down_moves_it_and_the_list_with_it() {
        let mut rig = Rig::new(300);
        rig.draw();
        let track = rig.track();
        let (top, size) = track.geometry.thumb();
        assert_eq!(top, 0);
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + 5);
        let shown = rig.entries();
        let size = usize::try_from(size).expect("a small number");
        assert_eq!(
            rows_of(&rig.glyphs(), "█"),
            (5..5 + size).collect::<Vec<_>>()
        );
        assert_eq!(rig.track().geometry.thumb().0, 5);
        let offset = usize::try_from(track.geometry.offset_for_top(5)).expect("a small number");
        assert!(
            shown[0].contains(&format!("file-{}.rs", 299 - offset)),
            "{shown:?}"
        );
    }

    #[test]
    fn a_drag_goes_on_when_the_pointer_leaves_the_bar_column() {
        let mut rig = Rig::new(300);
        rig.draw();
        let track = rig.track();
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        rig.pointer(MouseEventKind::Drag(LEFT), x - 30, y + 5);
        rig.draw();
        assert_eq!(rig.track().geometry.thumb().0, 5);
        rig.pointer(MouseEventKind::Up(LEFT), x - 30, y + 5);
        assert_eq!(rig.app.held_thumb(Focus::Session), None);
    }

    #[test]
    fn a_press_on_the_track_below_the_thumb_pages_down_by_the_lists_height() {
        let mut rig = Rig::new(300);
        rig.draw();
        let track = rig.track();
        let height = usize::from(track.area.height);
        rig.pointer(
            MouseEventKind::Down(LEFT),
            track.area.x,
            track.area.bottom() - 1,
        );
        let shown = rig.entries();
        assert!(
            shown[0].contains(&format!("file-{}.rs", 299 - height)),
            "{shown:?}"
        );
        // The wheel reports read before the press are stale.
        assert_eq!(rig.app.take_cleared(), [Focus::Session]);
        // A press above the thumb pages back to the top.
        let thumb = rig.track().geometry.thumb().0;
        assert!(thumb > 0);
        rig.pointer(MouseEventKind::Down(LEFT), track.area.x, track.area.y);
        assert!(rig.entries()[0].contains("file-299.rs"));
    }

    #[test]
    fn a_bar_is_drawn_only_when_the_list_does_not_fit() {
        let mut rig = Rig::new(5);
        rig.draw();
        assert_eq!(rig.hits.bars.len(), 0);
        let mut rig = Rig::new(300);
        rig.draw();
        assert_eq!(rig.hits.bars.len(), 1);
    }

    #[test]
    fn the_bar_column_takes_a_press_before_the_column_it_is_in() {
        let mut rig = Rig::new(300);
        rig.draw();
        let track = rig.track();
        for y in track.area.y..track.area.bottom() {
            assert_eq!(
                rig.hits.target(track.area.x, y),
                Some(Target::Bar(Focus::Session)),
                "row {y}"
            );
        }
    }

    #[test]
    fn the_bar_shows_while_the_list_scrolls_and_is_gone_a_second_after() {
        let mut rig = Rig::new(300);
        let t0 = Instant::now();
        rig.app.now = t0;
        rig.draw();
        assert!(rig.glyphs().iter().all(|glyph| glyph == " "));
        rig.scroll(10);
        let mut seen = Vec::new();
        let mut first_rows: Option<Vec<String>> = None;
        for ms in [0, 799, 800, 899, 999, 1000, 1500] {
            rig.app.now = at(t0, ms);
            let rows = rig.draw();
            let track = rig.track();
            let top = u16::try_from(track.geometry.thumb().0).expect("a small number");
            let cell = &rig.terminal.backend().buffer()[(track.area.x, track.area.y + top)];
            // A blank cell has no colour of its own to compare.
            let ink = (cell.symbol() != " ").then(|| cell.style().fg).flatten();
            seen.push((cell.symbol().to_owned(), ink));
            // The text keeps its columns whether the bar shows or not.
            let column = usize::from(track.area.x);
            let first = first_rows.get_or_insert_with(|| rows.clone());
            assert!(
                rows.iter()
                    .zip(first.iter())
                    .all(|(now, then)| now.chars().take(column).eq(then.chars().take(column))),
                "at {ms} ms the text moved"
            );
        }
        let want = |glyph: &str, style: Option<Style>| {
            (glyph.to_owned(), style.and_then(|style| style.fg))
        };
        assert_eq!(
            seen,
            [
                want("┃", Some(theme::line())),
                want("┃", Some(theme::line())),
                want("┃", Some(theme::fading(1))),
                want("┃", Some(theme::fading(2))),
                want("┃", Some(theme::fading(4))),
                want(" ", None),
                want(" ", None),
            ]
        );
    }

    #[test]
    fn hovering_the_last_column_shows_the_bar_with_the_thumb_a_full_cell() {
        let mut rig = Rig::new(300);
        let t0 = Instant::now();
        rig.app.now = t0;
        rig.draw();
        let track = rig.track();
        assert!(rig.glyphs().iter().all(|glyph| glyph == " "));
        rig.pointer(MouseEventKind::Moved, track.area.x, track.area.bottom() - 1);
        rig.draw();
        let (top, size) = track.geometry.thumb();
        let (top, size) = (
            usize::try_from(top).expect("a small number"),
            usize::try_from(size).expect("a small number"),
        );
        assert_eq!(
            rows_of(&rig.glyphs(), "█"),
            (top..top + size).collect::<Vec<_>>()
        );
        // The pointer going off it leaves the thumb a line again, for a while.
        rig.app.now = at(t0, 50);
        rig.pointer(MouseEventKind::Moved, 10, 10);
        rig.draw();
        assert_eq!(
            rows_of(&rig.glyphs(), "┃"),
            (top..top + size).collect::<Vec<_>>()
        );
        rig.app.now = at(t0, 1100);
        rig.draw();
        assert!(rig.glyphs().iter().all(|glyph| glyph == " "));
    }

    #[test]
    fn a_drag_past_the_held_entries_keeps_the_view_where_it_was() {
        // The newest 300 of 5,800 calls.
        let mut rig = Rig::with(calls(5500..5800, 5800));
        rig.draw();
        let track = rig.track();
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        // One row down stands for a place among the 300 held.
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + 1);
        // The rows, less the thumb at their end.
        let text = |rig: &mut Rig| -> Vec<String> {
            rig.entries()
                .iter()
                .map(|row| row.trim_end_matches(['█', '┃', ' ']).to_owned())
                .collect()
        };
        let held = text(&mut rig);
        assert!(!held[0].contains("file-5799.rs"), "{held:?}");
        for row in 2..track.area.height {
            rig.pointer(MouseEventKind::Drag(LEFT), x, y + row);
            assert_eq!(text(&mut rig), held, "row {row}");
        }
        // The thumb is at the bottom of the track all the same.
        assert_eq!(rig.glyphs().last().map(String::as_str), Some("█"));
    }

    #[test]
    fn dragging_to_the_bottom_of_a_long_session_asks_around_once_and_no_page_between() {
        // The newest 300 of 5,800 calls, as a window starts.
        let all: Vec<Said> = (0..5800)
            .map(|place| call(place, &format!("file-{place}.rs")))
            .collect();
        let mut rig = Rig::with(calls(5500..5800, 5800));
        let t0 = Instant::now();
        rig.app.now = t0;
        rig.draw();
        let mut asked = rig.asks();
        let track = rig.track();
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        for row in 1..track.area.height {
            rig.pointer(MouseEventKind::Drag(LEFT), x, y + row);
            rig.draw();
            asked.extend(rig.asks());
            // The thumb is moving, so nothing is asked for yet.
            assert_eq!(rig.app.take_jump(|_| 0), None, "row {row}");
        }
        rig.pointer(MouseEventKind::Up(LEFT), x, y + track.area.height - 1);
        rig.draw();
        asked.extend(rig.asks());
        let epoch = rig.history.epoch(List::Activity);
        let jump = rig.app.take_jump(|_| epoch).expect("a jump on letting go");
        // The bottom of the track is the start of the transcript.
        assert_eq!(
            jump,
            Jump {
                list: List::Activity,
                share: 0,
                ask: 1,
            }
        );
        assert_eq!(rig.app.take_jump(|_| epoch), None);
        assert_eq!(asked, [], "no page is asked for on the way");
        // The daemon's answer: the first 100 calls, and the start of the
        // transcript among them.
        assert!(rig.app.expects(List::Activity, jump.ask, epoch));
        assert!(!rig.app.expects(List::Activity, jump.ask + 1, epoch));
        rig.history
            .land_around(List::Activity, all[..100].to_vec(), false);
        rig.app.arrive(all[0].id);
        let shown = rig.entries();
        assert!(
            shown.last().is_some_and(|row| row.contains("file-0.rs")),
            "{shown:?}"
        );
        assert!(shown.iter().all(|row| !row.contains("file-5799.rs")));
        // The gaps the page left are filled as any gap is: from the newest
        // entry of the page, upward.
        let next = rig.asks();
        assert_eq!(next.len(), 1);
        assert_eq!((next[0].way, next[0].at), (Way::Newer, all[99].id));
    }

    #[test]
    fn a_thumb_that_rests_on_a_place_not_held_asks_for_it_once() {
        let mut rig = Rig::with(calls(5500..5800, 5800));
        let t0 = Instant::now();
        rig.app.now = t0;
        rig.draw();
        let track = rig.track();
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + track.area.height / 2);
        rig.draw();
        let epoch = rig.history.epoch(List::Activity);
        rig.app.now = at(t0, 99);
        assert_eq!(rig.app.take_jump(|_| epoch), None);
        assert_eq!(rig.app.jump_wake(), Some(at(t0, 100)));
        rig.app.now = at(t0, 100);
        let jump = rig.app.take_jump(|_| epoch).expect("a jump after the rest");
        // About half way: the middle of the transcript's bytes.
        assert!(jump.share.abs_diff(500_000) < 50_000, "{}", jump.share);
        assert_eq!(rig.app.take_jump(|_| epoch), None);
        assert_eq!(rig.app.jump_wake(), None);
        // The thumb moving on asks again, and the first answer is dropped.
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + track.area.height - 1);
        rig.draw();
        rig.app.now = at(t0, 300);
        let again = rig
            .app
            .take_jump(|_| epoch)
            .expect("a jump to the new place");
        assert_eq!((again.ask, again.share), (2, 0));
        assert!(!rig.app.expects(List::Activity, jump.ask, epoch));
        assert!(rig.app.expects(List::Activity, again.ask, epoch));
        // An answer read before the connection was lost is dropped.
        assert!(!rig.app.expects(List::Activity, again.ask, epoch + 1));
    }

    #[test]
    fn a_thumb_that_comes_back_to_held_entries_asks_for_nothing() {
        let mut rig = Rig::with(calls(5500..5800, 5800));
        rig.draw();
        let track = rig.track();
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + track.area.height - 1);
        rig.draw();
        assert!(rig.app.holds_view());
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + 1);
        rig.draw();
        assert!(rig.app.holds_view());
        rig.pointer(MouseEventKind::Up(LEFT), x, y + 1);
        rig.draw();
        assert!(!rig.app.holds_view());
        assert_eq!(rig.app.take_jump(|_| 0), None);
    }

    #[test]
    fn a_session_shown_instead_drops_the_place_the_thumb_was_going_to() {
        let mut rig = Rig::with(calls(5500..5800, 5800));
        rig.draw();
        let track = rig.track();
        let (x, y) = (track.area.x, track.area.y);
        rig.pointer(MouseEventKind::Down(LEFT), x, y);
        rig.pointer(MouseEventKind::Drag(LEFT), x, y + track.area.height - 1);
        rig.draw();
        rig.pointer(MouseEventKind::Up(LEFT), x, y);
        assert!(rig.app.holds_view());
        // A tab of the session clicked.
        let under = Under {
            target: Some(Target::Tab(Tab::Prompts)),
            ..Under::default()
        };
        rig.app.on_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(LEFT),
                column: 20,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            under,
        );
        assert!(!rig.app.holds_view());
    }

    #[test]
    fn a_seek_lands_on_the_run_that_covers_it_and_on_none_in_a_gap() {
        let placed = |run: usize, start: usize, id: u64| Placed {
            index: 0,
            id: EntryId { line: id, index: 0 },
            open: false,
            run,
            start,
            height: 1,
        };
        // Two runs of two lines, and 10 entries not held between them.
        let held = [
            placed(0, 0, 900),
            placed(0, 1, 800),
            placed(1, 2, 300),
            placed(1, 3, 200),
        ];
        let gaps = Gaps::from([(
            EntryId {
                line: 300,
                index: 0,
            },
            10,
        )]);
        let shape = Shape {
            gaps: &gaps,
            older: 0,
        };
        let seek = |at| seek_line(&held, shape, false, (at, 4));
        // 4 lines held by 4 entries: each entry not held takes a line.
        assert_eq!(seek(0), Some((0, 0)));
        assert_eq!(seek(1), Some((0, 1)));
        assert_eq!(seek(2), None);
        assert_eq!(seek(11), None);
        assert_eq!(seek(12), Some((1, 2)));
        assert_eq!(seek(13), Some((1, 3)));
        assert_eq!(seek(14), None);
        // A filter counts none that are not held.
        assert_eq!(seek_line(&held, shape, true, (2, 4)), Some((1, 2)));
    }

    /// The time of the frame at the 99th percentile of `times`.
    fn p99(times: &mut [Duration]) -> Duration {
        times.sort();
        times[times.len() * 99 / 100]
    }

    /// Frame times with 3,000 entries held, at 120 columns, in a release build:
    /// `cargo test --release -- --ignored frame_time --nocapture`. A frame that
    /// scrolls and a frame after the width changed, which lays every entry out
    /// again.
    #[test]
    #[ignore = "a timing run, in a release build"]
    fn frame_time_with_three_thousand_entries() {
        let mut rig = Rig::new(3000);
        // Some entries open to a longer text, and some have a result.
        for (place, call) in rig.history.facts.activity.iter_mut().enumerate() {
            if place % 3 == 0 {
                call.more = Some(format!(
                    "# Step {place}\n\nRead the file, then change the total\nand the tax code in it.\n\n- first\n- second"
                ));
            }
            if place % 5 == 0 {
                call.result = Some(Outcome {
                    text: format!("line one of {place}\nline two\nline three"),
                    error: place % 10 == 0,
                });
            }
        }
        rig.frame();
        let mut scrolling = Vec::new();
        for step in 0..2000 {
            rig.scroll(if step % 800 < 400 { 2 } else { -2 });
            rig.app.hover = Some(Target::Entry(rig.history.facts.activity[step % 3000].id));
            let began = Instant::now();
            rig.frame();
            scrolling.push(began.elapsed());
        }
        let mut resized = Vec::new();
        for step in 0..40_u16 {
            let wide = if step % 2 == 0 { 90 } else { 120 };
            rig.terminal.backend_mut().resize(wide, 40);
            rig.app.cols = wide;
            let began = Instant::now();
            rig.frame();
            resized.push(began.elapsed());
        }
        assert_eq!(
            rig.laid(),
            3000 * 41,
            "each width change lays out every entry"
        );
        let first = resized[0];
        let (scroll, resize) = (p99(&mut scrolling), p99(&mut resized));
        eprintln!(
            "scrolling: p99 {scroll:?}, max {:?}; after a width change: first {first:?}, p99 {resize:?}, max {:?}",
            scrolling.last().expect("frames"),
            resized.last().expect("frames"),
        );
        assert!(scroll < Duration::from_millis(2), "p99 {scroll:?}");
    }
}

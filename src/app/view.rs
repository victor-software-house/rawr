//! How the app looks: three columns, every pane, the selected pane's
//! sessions, and one session. The focused column takes the width it needs
//! and the others fold to rails, or, in the single arrangement, one column
//! fills the screen under a path back. Drawing reads the state and writes
//! cells, and records where each thing that takes a click landed, so a
//! click never disagrees with the screen.

use std::hash::{Hash, Hasher};
use std::num::NonZeroU16;
use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::facts::{Facts, Kind, Pr, Said};
use crate::app::feed::Pane;
use crate::app::state::{
    self, App, Arrangement, Button, FULL, Focus, Grip, Link, RAIL, Scroll, Tab, Target, Under,
};
use crate::app::{markup, theme};
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
    }
}

/// The session the session column shows, with its facts once read.
pub(crate) type Shown<'a> = Option<(&'a Agent, Option<&'a Facts>)>;

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App, shown: Shown<'_>) -> Hits {
    let mut hits = Hits {
        width: frame.area().width,
        height: frame.area().height,
        ..Hits::default()
    };
    let [body, search] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(frame.area());
    // One column and one row in from the outermost ring, which never hovers.
    let body = pad(body, 1, 1);
    match app.arrangement {
        Arrangement::Rails => rails(frame, app, body, shown, &mut hits),
        Arrangement::Single => single(frame, app, body, shown, &mut hits),
    }
    if app.focus == Focus::Search {
        search_overlay(frame, app, body, &mut hits);
    }
    search_box(frame, app, search, &mut hits);
    if app.help {
        hits.targets.clear();
        hits.rails.clear();
        hits.cursor = None;
        help(frame);
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
fn rails(frame: &mut Frame<'_>, app: &App, area: Rect, shown: Shown<'_>, hits: &mut Hits) {
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
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
    let grip = (first.width >= FULL).then(|| Grip {
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
            view(frame, app, column, below(rect, 2), shown, hits);
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
        app.dragging.is_some_and(|held| held.column == grip.column)
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
            if let Some((_, Some(facts))) = shown
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
    column: Focus,
    area: Rect,
    shown: Shown<'_>,
    hits: &mut Hits,
) {
    match column {
        Focus::Panes | Focus::Search => panes_list(frame, app, area, hits),
        Focus::Sessions | Focus::Session => pane_view(frame, app, area, shown, hits),
    }
}

// The single arrangement.

fn single(frame: &mut Frame<'_>, app: &App, area: Rect, shown: Shown<'_>, hits: &mut Hits) {
    if area.height < 4 {
        return;
    }
    crumbs(frame, app, row_at(area, area.y), hits);
    rule(frame, row_at(area, area.y + 1), theme::line());
    let body = below(area, 2);
    let at = app.at();
    view(frame, app, at, body, shown, hits);
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
        #[expect(clippy::cast_possible_truncation)]
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
    scrollbar(frame, list, skip, total);
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
        Link::Live | Link::Stale(_) => "No pane has a Claude session yet",
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
fn pane_view(frame: &mut Frame<'_>, app: &App, area: Rect, shown: Shown<'_>, hits: &mut Hits) {
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
                    .map(|percent| format!(" {percent}%"))
                    .unwrap_or_default(),
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
    let Some((agent, facts)) = shown else {
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
    if pane.is_some_and(|pane| !pane.closed) {
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
    tabs(frame, app, row_at(inner, y), facts, hits);
    rule(frame, row_at(area, y + 1), theme::line());
    let content = Rect {
        y: y + 2,
        height: bottom.saturating_sub(y + 2),
        ..inner
    };
    let Some(facts) = facts else {
        put(
            frame,
            row_at(content, content.y),
            Line::styled("reading the transcript…", theme::dim()),
        );
        return;
    };
    // The text leaves the scrollbar's columns free, so it keeps its width
    // as the tab grows past the screen.
    let text = Rect {
        width: content.width.saturating_sub(2),
        ..content
    };
    let (lines, prs_at, rows) = tab_lines(app, facts, &agent.cwd, usize::from(text.width));
    let total = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    hits.lists.push((content, Focus::Session));
    let skip = top_of(app.scroll_of(Focus::Session), total, content.height, None);
    hits.tops.push((Focus::Session, skip));
    for (at, target) in rows {
        if let Some(row) = u16::try_from(at)
            .ok()
            .and_then(|at| at.checked_sub(skip))
            .filter(|row| *row < text.height)
        {
            hits.add(row_at(text, text.y + row), target);
        }
    }
    scrollbar(frame, content, skip, total);
    frame.render_widget(Paragraph::new(lines).scroll((skip, 0)), text);
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
    scrollbar(frame, list, skip, total);
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

/// The tab bar: each tab's name and how many rows it holds.
fn tabs(frame: &mut Frame<'_>, app: &App, area: Rect, facts: Option<&Facts>, hits: &mut Hits) {
    let mut x = area.x;
    for tab in Tab::ALL {
        let count = facts.map(|facts| match tab {
            Tab::Overview => None,
            Tab::Prompts => Some(facts.log.len()),
            Tab::Activity => Some(facts.activity.len()),
            Tab::Files => Some(facts.files.len()),
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

/// The current tab's lines, which of them holds the pull requests, and
/// which lines take a click.
fn tab_lines(
    app: &App,
    facts: &Facts,
    cwd: &str,
    width: usize,
) -> (Vec<Line<'static>>, Option<usize>, Rows) {
    let filter = app.filter(Focus::Session);
    let mut lines = Vec::new();
    let mut rows = Rows::new();
    let mut prs_at = None;
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
                    lines.push(Line::raw(std::mem::take(&mut line)));
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
        Tab::Prompts => said(&mut lines, &mut rows, app, &facts.log, width),
        Tab::Activity => said(&mut lines, &mut rows, app, &facts.activity, width),
        Tab::Files => files(&mut lines, &mut rows, app, facts, cwd, width),
    }
    if lines.is_empty() {
        let text = if filter.is_empty() {
            "Nothing yet".to_owned()
        } else {
            format!("Nothing matches “{filter}”")
        };
        lines.push(Line::styled(text, theme::dim()));
    }
    (lines, prs_at, rows)
}

/// The width of the age column before every entry.
const AGE: usize = 10;

/// The entries of the prompts or the activity, newest first, as Claude
/// Code draws them. A folded entry shows its first lines with how many it
/// leaves out; a click opens it.
fn said(lines: &mut Vec<Line<'static>>, rows: &mut Rows, app: &App, items: &[Said], width: usize) {
    let filter = app.filter(Focus::Session);
    for item in items
        .iter()
        .rev()
        .filter(|item| state::passes(filter, &item.searched()))
    {
        let key = entry_key(item);
        let open = app.open.contains(&key);
        let age = item.at.as_deref().map(when).unwrap_or_default();
        let age = format!("{:>8}  ", age.trim_end_matches(" ago"));
        let (lead, close, style) = marks(&item.kind);
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
            Kind::Prompt => 3,
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
            .map(|more| markup::markdown(more, width.saturating_sub(indent)))
            .unwrap_or_default();
        let left = if open {
            0
        } else {
            // Folded, blank lines between paragraphs take no row, and a
            // one-line entry runs its lines together.
            body.retain(|line| line.width() > 0);
            let left = body.len().saturating_sub(most) + more.len();
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
            left
        };
        // What a folded entry leaves out shows at its end, never cut off.
        let tail = if left > 0 {
            format!("  +{left}")
        } else {
            String::new()
        };
        let start = lines.len();
        let last = body.len().saturating_sub(1);
        for (index, part) in body.into_iter().enumerate() {
            let mut spans = if index == 0 {
                let mut spans = vec![Span::styled(age.clone(), theme::dim())];
                spans.extend(lead.iter().cloned());
                spans
            } else {
                vec![Span::raw(" ".repeat(indent))]
            };
            spans.extend(part.spans);
            if index == last {
                let end = close.chars().count() + tail.chars().count();
                let mut line = markup::fit(&Line::from(spans), width.saturating_sub(end));
                line.spans.push(Span::styled(close, theme::dim()));
                line.spans
                    .push(Span::styled(tail.clone(), Style::new().fg(theme::ACCENT)));
                lines.push(line);
            } else {
                lines.push(markup::fit(&Line::from(spans), width));
            }
        }
        for part in more {
            let mut spans = vec![Span::raw(" ".repeat(indent))];
            spans.extend(part.spans);
            lines.push(Line::from(spans));
        }
        if left > 0 || open {
            let hovered = app.hover == Some(Target::Entry(key));
            for at in start..lines.len() {
                rows.push((at, Target::Entry(key)));
                if hovered && let Some(line) = lines.get_mut(at) {
                    line.style = theme::hover();
                }
            }
        }
    }
}

/// Which line of a tab stands for what a click there reaches.
type Rows = Vec<(usize, Target)>;

/// The key an entry is opened by: the same entry gives the same key on
/// every frame.
fn entry_key(item: &Said) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    item.at.hash(&mut hasher);
    item.text.hash(&mut hasher);
    hasher.finish()
}

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
            (vec![Span::styled("⏺ ", tone)], "", Style::new())
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

/// The files the session changed, the most changed first: each file's
/// name, then its folder shortened to fit, dim. A click opens the file.
fn files(
    lines: &mut Vec<Line<'static>>,
    rows: &mut Rows,
    app: &App,
    facts: &Facts,
    cwd: &str,
    width: usize,
) {
    let filter = app.filter(Focus::Session);
    let mut files: Vec<(usize, &(String, usize))> = facts
        .files
        .iter()
        .enumerate()
        .filter(|(_, (path, _))| state::passes(filter, path))
        .collect();
    files.sort_by(|(_, a), (_, b)| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (index, (path, count)) in files {
        let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
        let count = format!("{count:>4}× ");
        let room = width.saturating_sub(count.chars().count() + name.chars().count() + 2);
        let folder = short_folder(folder, cwd, room);
        let hovered = app.hover == Some(Target::File(index));
        let mut line = Line::from(vec![
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
            Span::styled(folder, theme::dim()),
        ]);
        if hovered {
            line.style = theme::hover();
        }
        rows.push((lines.len(), Target::File(index)));
        lines.push(markup::fit(&line, width));
    }
}

/// `folder` in at most `room` columns: inside the session's folder from
/// there, under the home folder from `~`; then its middle folders cut to
/// their first letter, from the left, and last its start dropped.
fn short_folder(folder: &str, cwd: &str, room: usize) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
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
            Some(rest) => format!(
                ".{}",
                rest.chars().next().map(String::from).unwrap_or_default()
            ),
            None => part.chars().next().map(String::from).unwrap_or_default(),
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
    let term = crate::catalog::terms(&app.query).into_iter().next();
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
        Line::from(vec![
            Span::styled("/ ", theme::dim()),
            Span::styled(
                format!("filter {name}   ctrl+f search every session"),
                theme::dim(),
            ),
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

fn help(frame: &mut Frame<'_>) {
    let keys = [
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
    ];
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
    let parse =
        |range: std::ops::Range<usize>| stamp.get(range).and_then(|part| part.parse::<i64>().ok());
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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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
    scrollbar(frame, area, skip, total);
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
                let name = app.panes.get(pane).map(unique_name).unwrap_or_default();
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

/// A thin scrollbar down the right edge of `area`, when a list `total` lines
/// tall does not fit in it.
fn scrollbar(frame: &mut Frame<'_>, area: Rect, top: u16, total: u16) {
    if total <= area.height {
        return;
    }
    let mut state = ScrollbarState::new(usize::from(total - area.height) + 1)
        .position(usize::from(top))
        .viewport_content_length(usize::from(area.height));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .thumb_symbol("┃")
            .thumb_style(theme::line()),
        area,
        &mut state,
    );
}

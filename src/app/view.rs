//! How the app looks: a sidebar of panes grouped by what they need, as
//! herdr's agents sidebar groups them, and the selected pane beside it.
//! Drawing reads the state and writes cells, and records where each thing
//! that takes a click landed, so a click never disagrees with the screen.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::app::feed::Pane;
use crate::app::state::{App, Button, Focus, Link, Target};
use crate::app::theme;
use crate::session::Agent;
use crate::status::Status;

/// The folded sidebar's width: a bar, a glyph and a margin.
const RAIL: u16 = 4;

/// Where the last frame put each thing that takes a click, and which list
/// each area scrolls.
#[derive(Debug, Default)]
pub(crate) struct Hits {
    targets: Vec<(Rect, Target)>,
    lists: Vec<(Rect, Focus)>,
    /// The screen's width, which bounds the sidebar.
    pub(crate) width: u16,
    /// Where the terminal's cursor goes: after the text in the search box.
    pub(crate) cursor: Option<Position>,
}

impl Hits {
    pub(crate) fn target(&self, column: u16, row: u16) -> Option<Target> {
        let at = Position::new(column, row);
        self.targets
            .iter()
            .find(|(area, _)| area.contains(at))
            .map(|(_, target)| *target)
    }

    pub(crate) fn list(&self, column: u16, row: u16) -> Option<Focus> {
        let at = Position::new(column, row);
        self.lists
            .iter()
            .find(|(area, _)| area.contains(at))
            .map(|(_, focus)| *focus)
    }

    fn add(&mut self, area: Rect, target: Target) {
        self.targets.push((area, target));
    }
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) -> Hits {
    let mut hits = Hits {
        width: frame.area().width,
        ..Hits::default()
    };
    let body = frame.area();
    let side = if app.folded { RAIL } else { app.sidebar };
    let [sidebar, edge, main] = Layout::horizontal([
        Constraint::Length(side),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(body);
    if app.folded {
        rail(frame, app, sidebar, &mut hits);
    } else {
        side_panel(frame, app, sidebar, &mut hits);
    }
    edge_line(frame, app, edge, &mut hits);
    let [content, search, footer] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(main);
    let content = pad(content, 1, 1);
    if app.focus == Focus::Search {
        results(frame, app, content, &mut hits);
    } else {
        detail(frame, app, content, &mut hits);
    }
    search_box(frame, app, search, &mut hits);
    footer_line(frame, app, footer, &mut hits);
    if app.help {
        hits.targets.clear();
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

/// The sidebar: the brand and the fold button, a summary, then one section
/// per status with its panes, two lines each, as Rex's own sidebar draws
/// its tabs: the title, then where the pane is and what its Claude says.
fn side_panel(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    if area.height < 3 {
        return;
    }
    let brand = row_at(area, area.y);
    put(
        frame,
        brand,
        Line::styled(" rawr", Style::new().add_modifier(Modifier::BOLD)),
    );
    let fold = Rect {
        x: brand.right().saturating_sub(4),
        width: 3,
        ..brand
    };
    button(frame, app, fold, Button::Sidebar, "«", hits);
    put(frame, row_at(area, area.y + 1), summary(app));

    let list = Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3),
        ..area
    };
    hits.lists.push((list, Focus::Panes));
    if app.panes.is_empty() {
        put(
            frame,
            row_at(list, list.y),
            Line::styled(format!(" {}", empty_text(app)), theme::dim()),
        );
        return;
    }
    // A header before each section, then its panes, and a blank line
    // between sections.
    let mut lines: Vec<Line_> = Vec::new();
    let mut previous = None;
    for (index, pane) in app.panes.iter().enumerate() {
        let section = theme::section(pane.status);
        if previous != Some(section) {
            if previous.is_some() {
                lines.push(Line_::Blank);
            }
            let count = app
                .panes
                .iter()
                .filter(|other| theme::section(other.status) == section)
                .count();
            lines.push(Line_::Header(section, count));
            previous = Some(section);
        }
        lines.push(Line_::Pane(index));
    }
    let tall = |line: &Line_| -> u16 {
        match line {
            Line_::Pane(_) => 2,
            Line_::Header(..) | Line_::Blank => 1,
        }
    };
    // Scroll so the selected pane's two lines show.
    let end_of = |at: usize| -> u16 { lines.iter().take(at + 1).map(tall).sum() };
    let selected = lines
        .iter()
        .position(|line| *line == Line_::Pane(app.pane))
        .unwrap_or(0);
    let skip = end_of(selected).saturating_sub(list.height);
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

/// One line of the sidebar's list.
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
    let mut spans = vec![Span::styled(
        format!(" {} panes", app.panes.len()),
        theme::dim(),
    )];
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

/// One sidebar row, two lines: the glyph, the newest session's title and
/// its age; then the Rex session and tab, and what Claude last said.
fn pane_row(frame: &mut Frame<'_>, app: &App, index: usize, cell: Rect) {
    let Some(pane) = app.panes.get(index) else {
        return;
    };
    let selected = index == app.pane;
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
    let under = match &pane.msg {
        Some(msg) if !msg.is_empty() => format!("{place} · {msg}"),
        _ => place,
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
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
            ]),
            Line::from(vec![
                bar,
                Span::raw("   "),
                Span::styled(truncate(&under, width.saturating_sub(5)), theme::dim()),
            ]),
        ])
        .style(theme::row(selected, hovered)),
        cell,
    );
}

/// The folded sidebar: the open button, then one glyph per pane.
fn rail(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let open = Rect {
        x: area.x,
        width: 3,
        ..row_at(area, area.y)
    };
    button(frame, app, open, Button::Sidebar, "»", hits);
    let list = Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3),
        ..area
    };
    hits.lists.push((list, Focus::Panes));
    let height = usize::from(list.height);
    let offset = app.pane.saturating_sub(height.saturating_sub(1));
    for (index, pane) in app.panes.iter().enumerate().skip(offset).take(height) {
        let cell = row_at(list, list.y + u16::try_from(index - offset).unwrap_or(0));
        let selected = index == app.pane;
        let hovered = app.hover == Some(Target::Pane(index));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if selected { "▌" } else { " " },
                    Style::new().fg(theme::ACCENT),
                ),
                Span::raw(" "),
                Span::styled(
                    theme::glyph(pane.status, app.frame),
                    theme::status(pane.status),
                ),
            ]))
            .style(theme::row(selected, hovered)),
            cell,
        );
        hits.add(cell, Target::Pane(index));
    }
}

/// The sidebar's edge: a thin rule that lights while hovered or dragged.
fn edge_line(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let lit = app.dragging || app.hover == Some(Target::Edge);
    let (glyph, style) = if lit {
        ("┃", Style::new().fg(theme::ACCENT))
    } else {
        ("│", theme::line())
    };
    let lines: Vec<Line<'static>> = (0..area.height)
        .map(|_| Line::styled(glyph, style))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
    // The edge's own column only: the columns beside it belong to rows.
    hits.targets.insert(0, (area, Target::Edge));
}

/// A small button: hovered, it rises and takes the accent.
fn button(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    button: Button,
    text: &str,
    hits: &mut Hits,
) {
    let hovered = app.hover == Some(Target::Button(button));
    let (ground, style) = if hovered {
        (theme::raised(), Style::new().fg(theme::ACCENT))
    } else {
        (Style::new(), theme::dim())
    };
    frame.render_widget(
        Paragraph::new(Line::styled(text.to_owned(), style))
            .centered()
            .style(ground),
        area,
    );
    hits.add(area, Target::Button(button));
}

/// The selected pane: its newest title, a status line, the actions, then
/// its sessions.
fn detail(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let Some(pane) = app.selected() else {
        put(
            frame,
            row_at(area, area.y),
            Line::styled(empty_text(app), theme::dim()),
        );
        return;
    };
    if area.height < 7 {
        return;
    }
    put(
        frame,
        row_at(area, area.y),
        Line::styled(
            truncate(pane.title(), usize::from(area.width)),
            Style::new().add_modifier(Modifier::BOLD),
        ),
    );
    let folder = pane
        .sessions
        .first()
        .map(|(agent, _)| agent.short_cwd())
        .unwrap_or_default();
    put(
        frame,
        row_at(area, area.y + 1),
        Line::from(vec![
            pill(pane.status, app.frame),
            Span::styled(
                pane.progress
                    .map(|percent| format!(" {percent}%"))
                    .unwrap_or_default(),
                theme::status(pane.status),
            ),
            Span::styled(format!("   {}   {folder}", unique_name(pane)), theme::dim()),
        ]),
    );
    if let Some(msg) = &pane.msg {
        put(
            frame,
            row_at(area, area.y + 2),
            Line::styled(truncate(msg, usize::from(area.width)), theme::dim()),
        );
    }
    let actions = row_at(area, area.y + 3);
    action(
        frame,
        app,
        Rect {
            width: 16,
            ..actions
        },
        (Button::GoTo, "↵", "Go to pane"),
        hits,
    );
    action(
        frame,
        app,
        Rect {
            x: actions.x + 18,
            width: 17,
            ..actions
        },
        (Button::Resume, "r", "Resume here"),
        hits,
    );
    put(
        frame,
        row_at(area, area.y + 5),
        Line::from(vec![
            Span::styled("Sessions", theme::header()),
            Span::styled(
                format!(
                    "  {}",
                    pane.sessions
                        .iter()
                        .filter(|(agent, _)| !is_scratch(agent))
                        .count()
                ),
                theme::dim(),
            ),
        ]),
    );
    let list = Rect {
        y: area.y + 6,
        height: area.height.saturating_sub(6),
        ..area
    };
    // Sessions with no title in a temporary folder are scripted runs, such
    // as `claude -p`: one count line stands for them.
    let (scratch, rows): (Vec<Row<'_>>, Vec<Row<'_>>) = pane
        .sessions
        .iter()
        .map(|(agent, held)| Row {
            agent,
            held: *held,
            snippet: None,
            prompts: None,
        })
        .partition(|row| is_scratch(row.agent));
    let list = if scratch.is_empty() {
        list
    } else {
        put(
            frame,
            row_at(list, list.bottom().saturating_sub(1)),
            Line::styled(
                format!(
                    "  + {} untitled sessions in temporary folders",
                    scratch.len()
                ),
                theme::dim(),
            ),
        );
        Rect {
            height: list.height.saturating_sub(2),
            ..list
        }
    };
    session_rows(frame, app, list, &rows, (Focus::Sessions, None), hits);
}

/// The search results, in the same rows as a pane's sessions, each with the
/// line that matched.
fn results(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let mut spans = if app.query.trim().is_empty() {
        vec![Span::styled(
            "Type to search every local session: prompts, titles, folders, and what Claude wrote",
            theme::dim(),
        )]
    } else {
        vec![
            Span::styled("Search", theme::header()),
            Span::styled(format!("  {} matches", app.results.len()), theme::dim()),
        ]
    };
    if app.deep {
        spans.push(Span::styled("  · reading transcripts…", theme::dim()));
    }
    put(frame, row_at(area, area.y), Line::from(spans));
    let list = Rect {
        y: area.y + 2,
        height: area.height.saturating_sub(2),
        ..area
    };
    let term = crate::catalog::terms(&app.query).into_iter().next();
    let rows: Vec<Row<'_>> = app
        .results
        .iter()
        .map(|hit| Row {
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

/// One session in a list.
struct Row<'a> {
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
        Focus::Sessions | Focus::Panes => (app.session, Target::Session),
    };
    let focused = app.focus == list;
    let height = |row: &Row<'_>| -> u16 { if row.snippet.is_some() { 3 } else { 2 } };
    let mut skip: u16 = 0;
    if focused {
        let end: u16 = rows.iter().take(current + 1).map(height).sum();
        skip = end.saturating_sub(area.height);
    }
    let mut y: u16 = 0;
    for (index, row) in rows.iter().enumerate() {
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
        let selected = focused && index == current;
        let hovered = app.hover == Some(target(index));
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
            Paragraph::new(lines).style(theme::row(selected, hovered)),
            cell,
        );
        hits.add(cell, target(index));
    }
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

/// The search box, drawn like Claude Code's prompt box: a rounded rule, a
/// `❯`, and the terminal's own cursor after the text.
fn search_box(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let focused = app.focus == Focus::Search;
    let hovered = app.hover == Some(Target::Search);
    let border = if focused {
        Style::new().fg(theme::ACCENT)
    } else if hovered {
        Style::new().fg(Color::Indexed(244))
    } else {
        theme::line()
    };
    let text = if focused || !app.query.is_empty() {
        Line::from(vec![
            Span::styled("❯ ", Style::new().fg(theme::ACCENT)),
            Span::raw(app.query.clone()),
        ])
    } else {
        Line::from(vec![
            Span::styled("❯ ", theme::dim()),
            Span::styled("Search every local session", theme::dim()),
        ])
    };
    frame.render_widget(
        Paragraph::new(text).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(border),
        ),
        area,
    );
    if focused {
        let typed = u16::try_from(app.query.chars().count()).unwrap_or(u16::MAX);
        hits.cursor = Some(Position::new(
            (area.x + 3 + typed).min(area.right().saturating_sub(2)),
            area.y + 1,
        ));
    }
    hits.add(area, Target::Search);
}

fn footer_line(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    if let Some(notice) = &app.notice {
        put(
            frame,
            area,
            Line::styled(format!("  ▲ {notice}"), Style::new().fg(theme::WARNING)),
        );
        return;
    }
    let keys: &[(Option<Button>, &str, &str)] = if app.focus == Focus::Search {
        &[
            (Some(Button::Resume), "enter", "resume"),
            (None, "↑↓", "move"),
            (None, "esc", "back"),
        ]
    } else {
        &[
            (Some(Button::GoTo), "enter", "go to pane"),
            (Some(Button::Resume), "r", "resume here"),
            (Some(Button::Search), "/", "search"),
            (Some(Button::Sidebar), "ctrl+b", "sidebar"),
            (Some(Button::Help), "?", "keys"),
            (Some(Button::Quit), "q", "quit"),
        ]
    };
    let mut x = area.x + 2;
    for (button, key, what) in keys {
        let text = format!("{key} {what}");
        let width = u16::try_from(text.chars().count()).unwrap_or(0);
        if x + width > area.right() {
            break;
        }
        let cell = Rect { x, width, ..area };
        let hovered = button.is_some_and(|button| app.hover == Some(Target::Button(button)));
        let (key_style, what_style) = if hovered {
            (Style::new().fg(theme::ACCENT), Style::new())
        } else {
            (theme::dim(), theme::dim())
        };
        put(
            frame,
            cell,
            Line::from(vec![
                Span::styled((*key).to_owned(), key_style),
                Span::styled(format!(" {what}"), what_style),
            ]),
        );
        if let Some(button) = button {
            hits.add(cell, Target::Button(*button));
        }
        x += width + 3;
    }
}

fn help(frame: &mut Frame<'_>) {
    let keys = [
        (
            "enter",
            "on a pane: show it in the Rex app; on a session: resume it here",
        ),
        ("r", "resume the selected session in this terminal"),
        ("tab ← →", "move between the sidebar and the sessions"),
        ("↑ ↓ j k", "move the selection"),
        ("/", "search every local session"),
        ("ctrl+b", "fold or open the sidebar"),
        ("< >", "narrow or widen the sidebar, or drag its edge"),
        ("click", "select; click the selected row to act on it"),
        ("wheel", "scroll the list under the pointer"),
        ("q esc", "quit"),
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
        76,
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

/// A session with no title that started in a temporary folder.
fn is_scratch(agent: &Agent) -> bool {
    let temp = std::env::temp_dir();
    agent.title == "Untitled session"
        && (["/tmp/", "/private/tmp/", "/var/folders/"]
            .iter()
            .any(|prefix| agent.cwd.starts_with(prefix))
            || std::path::Path::new(&agent.cwd).starts_with(&temp))
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

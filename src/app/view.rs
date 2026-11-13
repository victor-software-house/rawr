//! How the app looks, in each of the three layouts. Drawing reads the state
//! and writes widgets, and records where each thing that takes a click
//! landed, so a click never disagrees with the screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout as Split, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::feed::Pane;
use crate::app::state::{App, Button, Focus, Layout, Link, Target};
use crate::app::theme;
use crate::session::{Agent, ago};

/// Where the last frame put each thing that takes a click, and which list
/// each area scrolls.
#[derive(Debug, Default)]
pub(crate) struct Hits {
    /// The body between the header and the search box, for dragging the
    /// divider.
    pub(crate) body: Rect,
    targets: Vec<(Rect, Target)>,
    lists: Vec<(Rect, Focus)>,
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
    let mut hits = Hits::default();
    let tabs = app.layout == Layout::Tabs;
    let searching = app.focus == Focus::Search;
    let [header, bar, body, search, footer] = Split::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(tabs)),
        Constraint::Fill(1),
        Constraint::Length(if tabs && !searching { 0 } else { 3 }),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    hits.body = body;
    header_line(frame, app, header, &mut hits);
    if tabs {
        tab_bar(frame, app, bar, &mut hits);
    }
    match app.layout {
        Layout::Columns => columns(frame, app, body, &mut hits),
        Layout::Stacked => stacked(frame, app, body, &mut hits),
        Layout::Tabs => tab_body(frame, app, body, &mut hits),
    }
    if search.height > 0 {
        search_box(frame, app, search, &mut hits);
    }
    footer_line(frame, app, footer, &mut hits);
    if app.help {
        hits = Hits::default();
        help(frame);
    }
    hits
}

/// A row's style: hovered rows get the hover background.
fn row_style(app: &App, target: Target) -> Style {
    if app.hover == Some(target) {
        theme::hover()
    } else {
        Style::new()
    }
}

fn header_line(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let waiting = app
        .panes
        .iter()
        .filter(|pane| pane.status == crate::status::Status::NeedsInput)
        .count();
    let mut spans = vec![
        Span::styled(" rawr", Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {} panes", app.panes.len()), theme::dim()),
    ];
    if waiting > 0 {
        spans.push(Span::styled(
            format!(" · {waiting} need input"),
            Style::new().fg(theme::WARNING),
        ));
    }
    let link = match &app.link {
        Link::Connecting => Some(("reading Rex…".to_owned(), theme::dim())),
        Link::Live => None,
        Link::Offline(why) => Some((
            format!("Rex is not running here ({why})"),
            Style::new().fg(theme::ERROR),
        )),
        Link::Stale(why) => Some((format!("stale: {why}"), Style::new().fg(theme::WARNING))),
    };
    if let Some((text, style)) = link {
        spans.push(Span::styled(format!("  ▲ {text}"), style));
    }
    let label = format!(" layout: {} ▾ ", app.layout.name());
    let width = u16::try_from(label.chars().count()).unwrap_or(0);
    let button = Rect {
        x: area.right().saturating_sub(width),
        width: width.min(area.width),
        ..area
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let style = if app.hover == Some(Target::Layout) {
        theme::hover().fg(theme::ACCENT)
    } else {
        theme::dim()
    };
    frame.render_widget(Paragraph::new(Line::styled(label, style)), button);
    hits.add(button, Target::Layout);
}

fn footer_line(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    if let Some(notice) = &app.notice {
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!(" ▲ {notice}"),
                Style::new().fg(theme::WARNING),
            )),
            area,
        );
        return;
    }
    let buttons: &[(Button, &str, &str)] = match app.focus {
        Focus::Search => &[
            (Button::Resume, "enter", "resume"),
            (Button::Back, "esc", "back"),
        ],
        Focus::Panes | Focus::Sessions => &[
            (Button::GoTo, "enter", "go to pane"),
            (Button::Resume, "r", "resume here"),
            (Button::Switch, "tab", "switch list"),
            (Button::Search, "/", "search"),
            (Button::Help, "?", "keys"),
            (Button::Quit, "q", "quit"),
        ],
    };
    let mut x = area.x + 1;
    for (button, key, what) in buttons {
        let text = format!(" {key} {what} ");
        let width = u16::try_from(text.chars().count()).unwrap_or(0);
        if x + width > area.right() {
            break;
        }
        let cell = Rect { x, width, ..area };
        let hovered = app.hover == Some(Target::Button(*button));
        let line = Line::from(vec![
            Span::styled(
                format!(" {key}"),
                if hovered {
                    theme::hover()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(theme::ACCENT)
                },
            ),
            Span::styled(
                format!(" {what} "),
                if hovered {
                    theme::hover()
                } else {
                    theme::dim()
                },
            ),
        ]);
        frame.render_widget(Paragraph::new(line), cell);
        hits.add(cell, Target::Button(*button));
        x += width + 1;
    }
}

/// One pane row: glyph, name, status word, and the newest title when wide.
fn pane_item(app: &App, index: usize, pane: &Pane, width: u16, title: bool) -> ListItem<'static> {
    let mine = app.here.as_deref() == Some(pane.block_id.as_str());
    // Borders, the highlight symbol, the glyph and the status word.
    let room = usize::from(width).saturating_sub(2 + 2 + 2 + 13);
    let name_width = if title { room.min(32) } else { room };
    let name = unique_name(app, pane);
    let mut spans = vec![
        Span::styled(
            format!("{} ", theme::glyph(pane.status)),
            theme::status(pane.status),
        ),
        Span::raw(format!("{:<name_width$}", truncate(&name, name_width))),
        Span::styled(
            format!(" {:<12}", pane.status_word()),
            theme::status(pane.status),
        ),
    ];
    if title {
        let room = room.saturating_sub(name_width + 1);
        spans.push(Span::styled(truncate(pane.title(), room), theme::dim()));
    }
    if mine {
        spans.push(Span::styled(" (here)", theme::dim()));
    }
    ListItem::new(Line::from(spans)).style(row_style(app, Target::Pane(index)))
}

fn session_item(agent: &Agent, held: bool, width: u16, style: Style) -> ListItem<'static> {
    let age = ago(agent.idle);
    let room = usize::from(width).saturating_sub(age.len() + 6);
    let title = truncate(&agent.title, room);
    let pad = room.saturating_sub(title.chars().count());
    let folder = if held {
        format!("{} · running", agent.short_cwd())
    } else {
        agent.short_cwd()
    };
    ListItem::new(vec![
        Line::from(vec![
            Span::raw(title),
            Span::raw(" ".repeat(pad)),
            Span::styled(format!(" {age}"), theme::dim()),
        ]),
        Line::styled(truncate(&folder, room), theme::dim()),
    ])
    .style(style)
}

fn list<'a>(items: Vec<ListItem<'a>>, title: String, focused: bool) -> List<'a> {
    List::new(items)
        .block(
            Block::bordered()
                .title(Line::from(format!(" {title} ")))
                .border_style(theme::border(focused)),
        )
        .highlight_style(if focused {
            theme::selected()
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        })
        .highlight_symbol("❯ ")
}

/// Renders `list` in `area` and records one target per visible item, each
/// `height` rows tall.
fn render_list(
    frame: &mut Frame<'_>,
    area: Rect,
    list: List<'_>,
    selected: Option<usize>,
    (count, height): (usize, u16),
    (focus, target): (Focus, fn(usize) -> Target),
    hits: &mut Hits,
) {
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, area, &mut state);
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    hits.lists.push((inner, focus));
    let mut y = inner.y;
    for index in state.offset()..count {
        if y + height > inner.bottom() {
            break;
        }
        hits.add(
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height,
            },
            target(index),
        );
        y += height;
    }
}

fn panes_list(frame: &mut Frame<'_>, app: &App, area: Rect, title: bool, hits: &mut Hits) {
    if app.panes.is_empty() {
        empty(frame, app, area);
        return;
    }
    let items = app
        .panes
        .iter()
        .enumerate()
        .map(|(index, pane)| pane_item(app, index, pane, area.width, title))
        .collect();
    let focused = app.focus == Focus::Panes;
    render_list(
        frame,
        area,
        list(items, "Panes".to_owned(), focused),
        Some(app.pane),
        (app.panes.len(), 1),
        (Focus::Panes, Target::Pane),
        hits,
    );
}

fn empty(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let text = match app.link {
        Link::Connecting => "Reading Rex…",
        Link::Offline(_) => "Rex is not running here; / searches every local session",
        Link::Live | Link::Stale(_) => {
            "No pane has a Claude session yet; rawr setup claude records them"
        }
    };
    frame.render_widget(
        Paragraph::new(Line::styled(text, theme::dim()))
            .block(
                Block::bordered()
                    .title(" Panes ")
                    .border_style(theme::border(true)),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn sessions_list(frame: &mut Frame<'_>, app: &App, area: Rect, details: bool, hits: &mut Hits) {
    let Some(pane) = app.selected() else {
        frame.render_widget(Block::bordered().border_style(theme::dim()), area);
        return;
    };
    let [info, rest] = Split::vertical([
        Constraint::Length(if details { 3 } else { 0 }),
        Constraint::Fill(1),
    ])
    .areas(area);
    if details {
        let line = Line::from(vec![
            Span::styled(
                format!(" {} {}", theme::glyph(pane.status), pane.status_word()),
                theme::status(pane.status),
            ),
            Span::styled(
                format!(
                    " · {}",
                    pane.sessions
                        .first()
                        .map(|(agent, _)| agent.short_cwd())
                        .unwrap_or_default()
                ),
                theme::dim(),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(
                Block::bordered()
                    .title(format!(" {} ", unique_name(app, pane)))
                    .border_style(theme::dim()),
            ),
            info,
        );
    }
    let width = rest.width.saturating_sub(4);
    let items = pane
        .sessions
        .iter()
        .enumerate()
        .map(|(index, (agent, held))| {
            session_item(agent, *held, width, row_style(app, Target::Session(index)))
        })
        .collect();
    let focused = app.focus == Focus::Sessions;
    let title = if details {
        "Sessions in this pane".to_owned()
    } else {
        format!("{} · sessions", unique_name(app, pane))
    };
    render_list(
        frame,
        rest,
        list(items, title, focused),
        focused.then_some(app.session),
        (pane.sessions.len(), 2),
        (Focus::Sessions, Target::Session),
        hits,
    );
}

fn results_list(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let width = area.width.saturating_sub(4);
    let items: Vec<ListItem<'static>> = app
        .results
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            session_item(agent, false, width, row_style(app, Target::Result(index)))
        })
        .collect();
    let title = if app.query.trim().is_empty() {
        "Search · type to find a session".to_owned()
    } else {
        format!("Search · {} matches", app.results.len())
    };
    render_list(
        frame,
        area,
        list(items, title, true),
        Some(app.result),
        (app.results.len(), 2),
        (Focus::Search, Target::Result),
        hits,
    );
}

fn columns(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let [left, right] =
        Split::horizontal([Constraint::Percentage(app.split), Constraint::Fill(1)]).areas(area);
    panes_list(frame, app, left, false, hits);
    if app.focus == Focus::Search {
        results_list(frame, app, right, hits);
    } else {
        sessions_list(frame, app, right, true, hits);
    }
    divider(
        frame,
        app,
        Rect {
            x: left.right().saturating_sub(1),
            width: 2,
            ..area
        },
        true,
        hits,
    );
}

fn stacked(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    if app.focus == Focus::Search {
        results_list(frame, app, area, hits);
        return;
    }
    let [top, bottom] =
        Split::vertical([Constraint::Percentage(app.split), Constraint::Fill(1)]).areas(area);
    panes_list(frame, app, top, true, hits);
    sessions_list(frame, app, bottom, false, hits);
    divider(
        frame,
        app,
        Rect {
            y: top.bottom().saturating_sub(1),
            height: 2,
            ..area
        },
        false,
        hits,
    );
}

fn tab_bar(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let mut x = area.x + 1;
    for (focus, name) in [
        (Focus::Panes, "Panes"),
        (Focus::Sessions, "Sessions"),
        (Focus::Search, "Search"),
    ] {
        let text = format!(" {name} ");
        let width = u16::try_from(text.chars().count()).unwrap_or(0);
        let cell = Rect { x, width, ..area };
        let style = if app.focus == focus {
            theme::selected().add_modifier(Modifier::UNDERLINED)
        } else if app.hover == Some(Target::Tab(focus)) {
            theme::hover()
        } else {
            theme::dim()
        };
        frame.render_widget(Paragraph::new(Line::styled(text, style)), cell);
        hits.add(cell, Target::Tab(focus));
        x += width + 1;
    }
}

fn tab_body(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    match app.focus {
        Focus::Panes => panes_list(frame, app, area, true, hits),
        Focus::Sessions => sessions_list(frame, app, area, true, hits),
        Focus::Search => results_list(frame, app, area, hits),
    }
}

fn search_box(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let focused = app.focus == Focus::Search;
    let hovered = app.hover == Some(Target::Search);
    let text = if focused || !app.query.is_empty() {
        Line::from(vec![
            Span::raw(format!(" {}", app.query)),
            Span::styled("▏", theme::selected()),
        ])
    } else {
        Line::styled(" / search every local session", theme::dim())
    };
    let block = Block::bordered().border_style(if hovered && !focused {
        Style::new().fg(theme::ACCENT)
    } else {
        theme::border(focused)
    });
    frame.render_widget(
        Paragraph::new(text)
            .style(if hovered {
                theme::hover()
            } else {
                Style::new()
            })
            .block(block),
        area,
    );
    hits.add(area, Target::Search);
}

fn help(frame: &mut Frame<'_>) {
    let keys = [
        (
            "enter",
            "on a pane: show it in the Rex app; on a session: resume it here",
        ),
        ("r", "resume the selected session in this terminal"),
        ("tab ← →", "move between the panes and the sessions"),
        ("↑ ↓ j k", "move the selection"),
        ("/", "search every local session"),
        ("click", "select; click the selected row to act on it"),
        ("wheel", "scroll the list under the pointer"),
        ("ctrl+l", "next layout (preview only)"),
        ("q esc", "quit"),
    ];
    let lines: Vec<Line<'static>> = keys
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!(" {key:<10}"), theme::selected()),
                Span::raw((*what).to_owned()),
            ])
        })
        .collect();
    let area = centred(
        frame.area(),
        72,
        u16::try_from(lines.len()).unwrap_or(0) + 2,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(" Keys · any key closes ")
                .border_style(theme::border(true)),
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
fn unique_name(app: &App, pane: &Pane) -> String {
    let twins = app
        .panes
        .iter()
        .filter(|other| other.name == pane.name)
        .count();
    if twins < 2 {
        return pane.name.clone();
    }
    let bare = pane.block_id.rsplit('-').next().unwrap_or(&pane.block_id);
    format!(
        "{} #{}",
        pane.name,
        bare.chars().take(4).collect::<String>()
    )
}

/// Records the border between the two parts as a drag handle, and lights it
/// while it is hovered or dragged. `vertical` is a border that runs top to
/// bottom.
fn divider(frame: &mut Frame<'_>, app: &App, area: Rect, vertical: bool, hits: &mut Hits) {
    hits.targets.insert(0, (area, Target::Divider));
    if !(app.dragging || app.hover == Some(Target::Divider)) {
        return;
    }
    let style = Style::new().fg(theme::ACCENT).add_modifier(Modifier::BOLD);
    let buffer = frame.buffer_mut();
    for x in area.left()..area.right() {
        for y in area.top()..area.bottom() {
            if let Some(cell) = buffer.cell_mut(Position::new(x, y)) {
                let edge = if vertical {
                    y == area.top() || y + 1 == area.bottom()
                } else {
                    x == area.left() || x + 1 == area.right()
                };
                if !edge {
                    cell.set_style(style);
                }
            }
        }
    }
}

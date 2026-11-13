//! How the app looks, in each of the three layouts. Drawing reads the state
//! and writes widgets; it changes nothing.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout as Split, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::app::feed::Pane;
use crate::app::state::{App, Focus, Layout, Link};
use crate::app::theme;
use crate::session::{Agent, ago};

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App) {
    let [header, body, search, footer] = Split::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(if app.layout == Layout::Tabs { 0 } else { 3 }),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(Paragraph::new(header_line(app)), header);
    match app.layout {
        Layout::Columns => columns(frame, app, body),
        Layout::Stacked => stacked(frame, app, body),
        Layout::Tabs => tabs(frame, app, body),
    }
    if app.layout != Layout::Tabs {
        search_box(frame, app, search);
    }
    frame.render_widget(Paragraph::new(footer_line(app)), footer);
    if app.help {
        help(frame);
    }
}

fn header_line(app: &App) -> Line<'static> {
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
        Link::Connecting => Some(("connecting to Rex…".to_owned(), theme::dim())),
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
    spans.push(Span::styled(
        format!("   layout: {} (ctrl+l)", app.layout.name()),
        theme::dim(),
    ));
    Line::from(spans)
}

fn footer_line(app: &App) -> Line<'static> {
    if let Some(notice) = &app.notice {
        return Line::styled(format!(" ▲ {notice}"), Style::new().fg(theme::WARNING));
    }
    let keys = match app.focus {
        Focus::Search => "type to search · ↑↓ move · enter resume · esc back",
        Focus::Panes | Focus::Sessions => {
            "enter go to pane · r resume here · tab switch list · / search · ? keys · q quit"
        }
    };
    Line::styled(format!(" {keys}"), theme::dim())
}

/// One pane row: glyph, name, status word, and the newest title when wide.
fn pane_item(
    pane: &Pane,
    name: &str,
    here: Option<&str>,
    width: u16,
    title: bool,
) -> ListItem<'static> {
    let mine = here == Some(pane.block_id.as_str());
    // Borders, the highlight symbol, the glyph and the status word.
    let room = usize::from(width).saturating_sub(2 + 2 + 2 + 13);
    let name_width = if title { room.min(32) } else { room };
    let mut spans = vec![
        Span::styled(
            format!("{} ", theme::glyph(pane.status)),
            theme::status(pane.status),
        ),
        Span::raw(format!("{:<name_width$}", truncate(name, name_width))),
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
    ListItem::new(Line::from(spans))
}

fn session_item(agent: &Agent, held: bool, width: u16) -> ListItem<'static> {
    let age = ago(agent.idle);
    let room = usize::from(width).saturating_sub(age.len() + 6);
    let title = truncate(&agent.title, room);
    let pad = room.saturating_sub(title.chars().count());
    let mut lines = vec![Line::from(vec![
        Span::raw(title),
        Span::raw(" ".repeat(pad)),
        Span::styled(format!(" {age}"), theme::dim()),
    ])];
    let folder = if held {
        format!("{} · running", agent.short_cwd())
    } else {
        agent.short_cwd()
    };
    lines.push(Line::styled(truncate(&folder, room), theme::dim()));
    ListItem::new(lines)
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

fn panes_list(frame: &mut Frame<'_>, app: &App, area: Rect, title: bool) {
    if app.panes.is_empty() {
        empty(frame, app, area);
        return;
    }
    let items = app
        .panes
        .iter()
        .map(|pane| {
            pane_item(
                pane,
                &unique_name(app, pane),
                app.here.as_deref(),
                area.width,
                title,
            )
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.pane));
    frame.render_stateful_widget(
        list(items, "Panes".to_owned(), app.focus == Focus::Panes),
        area,
        &mut state,
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

fn sessions_list(frame: &mut Frame<'_>, app: &App, area: Rect, details: bool) {
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
                        .map(|(a, _)| a.short_cwd())
                        .unwrap_or_default()
                ),
                theme::dim(),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(vec![
                line,
                Line::styled(format!(" {}", pane.block_id), theme::dim()),
            ])
            .block(
                Block::bordered()
                    .title(format!(" {} ", pane.name))
                    .border_style(theme::dim()),
            ),
            info,
        );
    }
    let items = pane
        .sessions
        .iter()
        .map(|(agent, held)| session_item(agent, *held, rest.width.saturating_sub(4)))
        .collect();
    let focused = app.focus == Focus::Sessions;
    let mut state = ListState::default().with_selected(focused.then_some(app.session));
    let title = if details {
        "Sessions in this pane".to_owned()
    } else {
        format!("{} · sessions", pane.name)
    };
    frame.render_stateful_widget(list(items, title, focused), rest, &mut state);
}

fn results_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items: Vec<ListItem<'static>> = app
        .results
        .iter()
        .map(|agent| session_item(agent, false, area.width.saturating_sub(4)))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.result));
    let title = format!("Search · {} matches", app.results.len());
    frame.render_stateful_widget(list(items, title, true), area, &mut state);
}

fn columns(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let [left, right] =
        Split::horizontal([Constraint::Percentage(44), Constraint::Fill(1)]).areas(area);
    panes_list(frame, app, left, false);
    if app.focus == Focus::Search {
        results_list(frame, app, right);
    } else {
        sessions_list(frame, app, right, true);
    }
}

fn stacked(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.focus == Focus::Search {
        results_list(frame, app, area);
        return;
    }
    let rows = u16::try_from(app.panes.len().max(1))
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let [top, bottom] =
        Split::vertical([Constraint::Max(rows.max(4)), Constraint::Fill(1)]).areas(area);
    panes_list(frame, app, top, true);
    sessions_list(frame, app, bottom, false);
}

fn tabs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let [bar, body, search] = Split::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(if app.focus == Focus::Search { 3 } else { 0 }),
    ])
    .areas(area);
    let index = match app.focus {
        Focus::Panes => 0,
        Focus::Sessions => 1,
        Focus::Search => 2,
    };
    frame.render_widget(
        Tabs::new(["Panes", "Sessions", "Search"])
            .select(index)
            .highlight_style(theme::selected())
            .style(theme::dim())
            .padding(" ", " "),
        bar,
    );
    match app.focus {
        Focus::Panes => panes_list(frame, app, body, true),
        Focus::Sessions => sessions_list(frame, app, body, true),
        Focus::Search => {
            results_list(frame, app, body);
            search_box(frame, app, search);
        }
    }
}

fn search_box(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let focused = app.focus == Focus::Search;
    let text = if focused || !app.query.is_empty() {
        Line::from(vec![
            Span::raw(format!(" {}", app.query)),
            Span::styled("▏", theme::selected()),
        ])
    } else {
        Line::styled(" / search every local session", theme::dim())
    };
    frame.render_widget(
        Paragraph::new(text).block(Block::bordered().border_style(theme::border(focused))),
        area,
    );
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
                .title(" Keys ")
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

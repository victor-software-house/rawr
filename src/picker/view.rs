//! How the picker looks: one block per session on top, a scrollbar when the
//! blocks overflow, and at the bottom a search box drawn like Claude Code's
//! prompt box, with the key line under it. Drawing also reports which session
//! each screen row shows, for mouse hits.
//!
//! Body text is the terminal's default foreground and secondary text is
//! dimmed, so both read in light and dark colour themes. The selected session
//! is the one accent: its pointer and bold title in bright blue.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use textwrap::{Options, WordSplitter};

use crate::picker::state::State;
use crate::session::{Agent, ago, sanitize};

/// Bright blue, ANSI colour 12: the selection's pointer and title.
const ACCENT: Color = Color::LightBlue;
/// Columns before each session's text: a space, the pointer, a space.
const GUTTER: usize = 3;
/// The narrowest text column; a narrower pane wraps at this width anyway.
const MIN_TEXT: usize = 10;
const POINTER: &str = "❯";
/// The cursor after the text typed in the search box.
const CURSOR: &str = "▏";

/// What a drawn frame tells the loop.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Drawn {
    /// The session each screen row shows, by row; `None` for rows that show
    /// no session.
    pub(crate) rows: Vec<Option<usize>>,
    /// How many sessions one screen shows, for Page Up and Page Down.
    pub(crate) page: usize,
    /// The first list line shown, kept for the next frame.
    pub(crate) offset: usize,
}

/// How a session's block is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Selected,
    Hovered,
    Plain,
}

/// Wraps at spaces, never at a hyphen, so a folder stays whole when it fits;
/// a word wider than the row still breaks.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let options = Options::new(width.max(MIN_TEXT)).word_splitter(WordSplitter::NoHyphenation);
    textwrap::wrap(text, options)
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect()
}

/// One session's lines: its title, then its folder and age, each wrapped to
/// `width` columns after the gutter. The session ID is not drawn: search still
/// matches it, and the resume uses it.
fn block(agent: &Agent, look: Look, width: usize) -> Vec<Line<'static>> {
    let text_width = width.saturating_sub(GUTTER);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let (pointer, title, detail) = match look {
        Look::Selected => (
            Span::styled(POINTER, Style::new().fg(ACCENT)),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            Style::new(),
        ),
        Look::Hovered => (Span::styled(POINTER, dim), Style::new(), Style::new()),
        Look::Plain => (Span::raw(" "), Style::new(), dim),
    };
    let mut lines = Vec::new();
    for (n, part) in wrapped(&agent.title, text_width).into_iter().enumerate() {
        let lead = if n == 0 {
            pointer.clone()
        } else {
            Span::raw(" ")
        };
        lines.push(Line::from(vec![
            Span::raw(" "),
            lead,
            Span::raw(" "),
            Span::styled(part, title),
        ]));
    }
    let place = sanitize(&format!("{} · {}", agent.short_cwd(), ago(agent.idle)));
    for part in wrapped(&place, text_width) {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(GUTTER)),
            Span::styled(part, detail),
        ]));
    }
    lines
}

/// The first line to show: `previous`, moved only as far as it takes to show
/// the whole selected block (`start..end`), or its top when it is taller than
/// the screen.
fn follow(previous: usize, start: usize, end: usize, height: usize) -> usize {
    if start < previous || end.saturating_sub(start) > height {
        start
    } else if end > previous.saturating_add(height) {
        end.saturating_sub(height)
    } else {
        previous
    }
}

/// The key line: as many hints as fit, most useful first.
fn keys(width: usize, state: &State) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let expand = if state.expanded() {
        "show fewer"
    } else {
        "show all"
    };
    let mut hints = vec![("↑↓", "select"), ("⏎", "resume")];
    if state.can_expand() {
        hints.push(("ctrl+o", expand));
    }
    hints.push(if state.filtering() {
        ("esc", "clear")
    } else {
        ("esc", "shell")
    });
    hints.push(("click", "select, again resume"));
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    for (key, what) in hints {
        let hint_width = key.chars().count() + 1 + what.chars().count() + 2;
        if used + hint_width > width {
            break;
        }
        used += hint_width;
        spans.push(Span::raw(key));
        spans.push(Span::styled(format!(" {what}  "), dim));
    }
    Line::from(spans)
}

/// The box's top rule, with the title and the count of sessions or matches
/// in it.
fn title_rule(state: &State, count: usize, width: usize) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let count = match (count, state.filtering()) {
        (1, false) => "1 session".to_owned(),
        (n, false) => format!("{n} sessions"),
        (1, true) => "1 match".to_owned(),
        (n, true) => format!("{n} matches"),
    };
    let mut line = Line::from(vec![
        Span::styled("──", dim),
        Span::styled(
            " Resume a Claude Code session",
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" · {count} "), dim),
    ]);
    line.push_span(Span::styled(
        "─".repeat(width.saturating_sub(line.width())),
        dim,
    ));
    line
}

/// The box's one input line: the prompt, the text typed with a cursor after
/// it, or the placeholder while the box is empty. A text wider than the line
/// shows its end.
fn input_line(text: &str, width: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(format!("{POINTER} "))];
    if text.is_empty() {
        spans.push(Span::raw(CURSOR));
        spans.push(Span::styled(
            "type to search",
            Style::new().add_modifier(Modifier::DIM),
        ));
    } else {
        let text = sanitize(text);
        let room = width.saturating_sub(3);
        let hidden = text.chars().count().saturating_sub(room);
        spans.push(Span::raw(text.chars().skip(hidden).collect::<String>()));
        spans.push(Span::raw(CURSOR));
    }
    Line::from(spans)
}

/// The dim row that stands for the sessions `ctrl+o` would show.
fn earlier_row(count: usize) -> Line<'static> {
    Line::from(vec![
        Span::raw(" ".repeat(GUTTER)),
        Span::styled(
            format!("+{count} earlier · ctrl+o"),
            Style::new().add_modifier(Modifier::DIM),
        ),
    ])
}

/// Draws the picker for `state` over `agents` into `frame`, starting the list
/// at `offset` or wherever keeps the selection in view. The list is on top,
/// and the search box and the key line are at the bottom. The list shows the
/// first `state.visible()` sessions, then a row for the ones it hides.
pub(crate) fn draw(frame: &mut Frame<'_>, agents: &[Agent], state: &State, offset: usize) -> Drawn {
    let area = frame.area();
    let [list, top, input, bottom, footer] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let width = usize::from(area.width);
    let dim = Style::new().add_modifier(Modifier::DIM);
    frame.render_widget(Paragraph::new(title_rule(state, agents.len(), width)), top);
    frame.render_widget(Paragraph::new(input_line(state.text(), width)), input);
    frame.render_widget(Paragraph::new(Line::styled("─".repeat(width), dim)), bottom);
    frame.render_widget(Paragraph::new(keys(width, state)), footer);
    if agents.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled("   No session matches", dim)),
            list,
        );
        return Drawn {
            rows: vec![None; usize::from(area.height)],
            page: 1,
            offset: 0,
        };
    }

    // One column stays free for the scrollbar.
    let width = usize::from(list.width).saturating_sub(1);
    let height = usize::from(list.height);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut owners: Vec<Option<usize>> = Vec::new();
    let mut spans = Vec::with_capacity(agents.len());
    for (index, agent) in agents.iter().enumerate().take(state.visible()) {
        if index > 0 {
            lines.push(Line::default());
            owners.push(None);
        }
        let look = if index == state.selected() {
            Look::Selected
        } else if state.hover() == Some(index) {
            Look::Hovered
        } else {
            Look::Plain
        };
        let start = lines.len();
        let drawn = block(agent, look, width);
        owners.extend(std::iter::repeat_n(Some(index), drawn.len()));
        lines.extend(drawn);
        spans.push((start, lines.len()));
    }
    if state.earlier() > 0 {
        lines.push(Line::default());
        owners.push(None);
        lines.push(earlier_row(state.earlier()));
        owners.push(None);
    }
    let (start, end) = spans.get(state.selected()).copied().unwrap_or((0, 0));
    let offset = follow(offset.min(lines.len()), start, end, height);
    let shown: Vec<Line<'static>> = lines.iter().skip(offset).take(height).cloned().collect();
    frame.render_widget(Paragraph::new(shown), list);

    if lines.len() > height {
        let mut scroll = ScrollbarState::new(lines.len().saturating_sub(height))
            .position(offset)
            .viewport_content_length(height);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("┃")
                .track_style(Style::new().add_modifier(Modifier::DIM)),
            list,
            &mut scroll,
        );
    }

    let mut rows = vec![None; usize::from(area.height)];
    for (line, owner) in owners.iter().skip(offset).take(height).enumerate() {
        if let Some(row) = rows.get_mut(usize::from(list.y) + line) {
            *row = *owner;
        }
    }
    let visible = spans
        .iter()
        .filter(|&&(first, last)| first >= offset && last <= offset + height)
        .count();
    Drawn {
        rows,
        page: visible.max(1),
        offset,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

    use super::*;

    /// How many sessions show before `ctrl+o`, as the config defaults.
    const ROWS: usize = 3;
    /// The list's first row: the list is on top.
    const TOP: u16 = 0;

    fn agent(n: usize, title: &str) -> Agent {
        Agent {
            session_id: format!("6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d{n:02}"),
            cwd: "/srv/code/shop".to_owned(),
            title: title.to_owned(),
            idle: Some(Duration::from_hours(2)),
        }
    }

    fn agents(count: usize) -> Vec<Agent> {
        (0..count).map(|n| agent(n, &format!("Task {n}"))).collect()
    }

    fn render(agents: &[Agent], state: &State, width: u16, height: u16) -> (Buffer, Drawn) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut drawn = None;
        terminal
            .draw(|frame| drawn = Some(draw(frame, agents, state, 0)))
            .unwrap();
        (terminal.backend().buffer().clone(), drawn.unwrap())
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height).map(|y| row(buffer, y)).collect()
    }

    /// The row of the box's top rule, counted from the bottom of a buffer:
    /// the rule, the input line, the bottom rule and the key line.
    fn box_top(buffer: &Buffer) -> u16 {
        buffer.area.height - 4
    }

    fn ctrl_o() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)
    }

    #[test]
    fn the_selected_session_has_the_pointer_and_the_accent() {
        let agents = [
            agent(1, "Fix the login redirect"),
            agent(2, "Add CSV export"),
        ];
        let (buffer, drawn) = render(&agents, &State::new(2, ROWS), 60, 14);
        let top = TOP;
        assert_eq!(row(&buffer, top), " ❯ Fix the login redirect");
        assert_eq!(buffer[(3, top)].fg, ACCENT);
        assert!(buffer[(3, top)].modifier.contains(Modifier::BOLD));
        assert_eq!(row(&buffer, top + 1), "   /srv/code/shop · 2 h ago");
        assert_eq!(row(&buffer, top + 2), "");
        assert_eq!(row(&buffer, top + 3), "   Add CSV export");
        assert!(buffer[(3, top + 4)].modifier.contains(Modifier::DIM));
        assert_eq!(drawn.rows[usize::from(top)], Some(0));
        assert_eq!(drawn.rows[usize::from(top + 2)], None);
        assert_eq!(drawn.rows[usize::from(top + 3)], Some(1));
        assert!(
            !rows(&buffer).iter().any(|line| line.contains("6f1c2a90")),
            "{:?}",
            rows(&buffer)
        );
    }

    #[test]
    fn the_search_box_sits_under_the_list_with_the_key_line_last() {
        let agents = [
            agent(1, "Fix the login redirect"),
            agent(2, "Add CSV export"),
        ];
        let (buffer, _) = render(&agents, &State::new(2, ROWS), 60, 14);
        let rule = box_top(&buffer);
        let shown = rows(&buffer);
        assert!(
            shown[usize::from(rule)].starts_with("── Resume a Claude Code session · 2 sessions ─"),
            "{shown:?}"
        );
        assert!(shown[usize::from(rule)].ends_with('─'), "{shown:?}");
        assert_eq!(shown[usize::from(rule) + 1], "❯ ▏type to search");
        assert_eq!(shown[usize::from(rule) + 2], "─".repeat(60));
        assert!(shown[usize::from(rule) + 3].contains("resume"), "{shown:?}");
        assert!(
            buffer[(0, rule)].modifier.contains(Modifier::DIM),
            "the rule is dim"
        );
        assert!(
            buffer[(8, rule + 1)].modifier.contains(Modifier::DIM),
            "the placeholder is dim"
        );
    }

    #[test]
    fn typing_shows_in_the_box_and_the_title_counts_matches() {
        let agents = [agent(1, "Fix the shop login")];
        let mut state = State::new(2, ROWS);
        for c in "shop".chars() {
            state.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), 1);
        }
        state.set_len(1);
        let (buffer, _) = render(&agents, &state, 60, 14);
        let rule = box_top(&buffer);
        assert!(
            row(&buffer, rule).starts_with("── Resume a Claude Code session · 1 match ─"),
            "{:?}",
            rows(&buffer)
        );
        assert_eq!(row(&buffer, rule + 1), "❯ shop▏");
        assert_eq!(row(&buffer, TOP), " ❯ Fix the shop login");
        assert!(
            row(&buffer, rule + 3).contains("esc clear"),
            "{:?}",
            rows(&buffer)
        );
    }

    #[test]
    fn a_long_search_shows_its_end() {
        let state = State::searching("x".repeat(30) + "tail", ROWS);
        let (buffer, _) = render(&[], &state, 20, 10);
        let line = row(&buffer, box_top(&buffer) + 1);
        assert!(line.starts_with("❯ x"), "{line}");
        assert!(line.ends_with("tail▏"), "{line}");
        assert_eq!(line.chars().count(), 20);
    }

    #[test]
    fn an_escape_in_the_search_text_never_reaches_the_terminal() {
        let state = State::searching("a\u{1b}[2Jb".to_owned(), ROWS);
        let (buffer, _) = render(&[], &state, 40, 10);
        assert_eq!(row(&buffer, box_top(&buffer) + 1), "❯ a[2Jb▏");
    }

    #[test]
    fn an_escape_in_a_folder_never_reaches_the_terminal() {
        let mut shop = agent(1, "Fix the login redirect");
        shop.cwd = "/srv/code/\u{1b}[2Jshop".to_owned();
        let (buffer, _) = render(&[shop], &State::new(1, ROWS), 60, 10);
        assert_eq!(row(&buffer, TOP + 1), "   /srv/code/[2Jshop · 2 h ago");
    }

    #[test]
    fn hover_undims_a_session_without_selecting_it() {
        let agents = [
            agent(1, "Fix the login redirect"),
            agent(2, "Add CSV export"),
        ];
        let mut state = State::new(2, ROWS);
        let moved = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 4,
            row: 3,
            modifiers: KeyModifiers::NONE,
        };
        state.on_mouse(moved, Some(1));
        let (buffer, _) = render(&agents, &state, 60, 14);
        let top = TOP;
        assert_eq!(row(&buffer, top + 3), " ❯ Add CSV export");
        assert!(!buffer[(3, top + 4)].modifier.contains(Modifier::DIM));
        assert_ne!(buffer[(3, top + 3)].fg, ACCENT);
    }

    #[test]
    fn long_text_wraps_and_nothing_is_cut() {
        let title = "Fix the login redirect loop that sends users back to the sign-in page";
        let (buffer, _) = render(&[agent(1, title)], &State::new(1, ROWS), 40, 16);
        let shown = rows(&buffer);
        let words: Vec<&str> = shown
            .iter()
            .skip(usize::from(TOP))
            .take_while(|line| !line.contains("/srv"))
            .flat_map(|line| line.trim_start_matches(" ❯ ").split_whitespace())
            .collect();
        assert_eq!(words.join(" "), title);
        assert!(
            shown
                .iter()
                .any(|line| line.trim() == "/srv/code/shop · 2 h ago"),
            "{shown:?}"
        );
    }

    #[test]
    fn the_list_scrolls_to_keep_the_selection_whole() {
        let agents = agents(8);
        let mut state = State::new(8, 8);
        for _ in 0..7 {
            state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 1);
        }
        let (buffer, drawn) = render(&agents, &state, 50, 14);
        let shown = rows(&buffer);
        assert!(
            shown.iter().any(|line| line.starts_with(" ❯ Task 7")),
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|line| line.ends_with('┃')),
            "a scrollbar thumb: {shown:?}"
        );
        assert!(drawn.offset > 0);
        assert!(drawn.rows.contains(&Some(7)));
        assert!(!drawn.rows.contains(&Some(0)));
    }

    #[test]
    fn no_match_says_so_and_offers_no_row() {
        let state = State::searching("zzz".to_owned(), ROWS);
        let (buffer, drawn) = render(&[], &state, 60, 14);
        assert_eq!(row(&buffer, TOP), "   No session matches");
        assert!(drawn.rows.iter().all(Option::is_none));
    }

    #[test]
    fn seven_sessions_show_three_rows_then_the_earlier_row() {
        let agents = agents(7);
        let state = State::new(7, ROWS);
        let (buffer, drawn) = render(&agents, &state, 60, 24);
        let shown = rows(&buffer);
        assert_eq!(
            shown.iter().filter(|l| l.contains("Task")).count(),
            3,
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|line| line == "   +4 earlier · ctrl+o"),
            "{shown:?}"
        );
        assert_eq!(drawn.rows.iter().flatten().max(), Some(&2));
        let earlier = shown
            .iter()
            .position(|line| line.contains("earlier"))
            .unwrap();
        assert!(
            buffer[(3, u16::try_from(earlier).unwrap())]
                .modifier
                .contains(Modifier::DIM)
        );
        assert!(
            shown.iter().any(|line| line.contains("ctrl+o show all")),
            "{shown:?}"
        );
    }

    #[test]
    fn ctrl_o_shows_all_seven_and_again_hides_them() {
        let agents = agents(7);
        let mut state = State::new(7, ROWS);
        state.on_key(ctrl_o(), 1);
        let (buffer, drawn) = render(&agents, &state, 60, 30);
        let shown = rows(&buffer);
        assert_eq!(
            shown.iter().filter(|l| l.contains("Task")).count(),
            7,
            "{shown:?}"
        );
        assert!(!shown.iter().any(|l| l.contains("earlier")), "{shown:?}");
        assert!(
            shown.iter().any(|line| line.contains("ctrl+o show fewer")),
            "{shown:?}"
        );
        assert_eq!(drawn.rows.iter().flatten().max(), Some(&6));

        state.on_key(ctrl_o(), 1);
        let (buffer, _) = render(&agents, &state, 60, 30);
        let shown = rows(&buffer);
        assert_eq!(shown.iter().filter(|l| l.contains("Task")).count(), 3);
        assert!(shown.iter().any(|l| l.contains("+4 earlier")), "{shown:?}");
    }

    #[test]
    fn no_earlier_row_for_as_many_sessions_as_rows_or_fewer() {
        for count in [2, 3] {
            let agents = agents(count);
            let (buffer, drawn) = render(&agents, &State::new(count, ROWS), 60, 24);
            let shown = rows(&buffer);
            assert_eq!(
                shown.iter().filter(|l| l.contains("Task")).count(),
                count,
                "{shown:?}"
            );
            assert!(!shown.iter().any(|l| l.contains("earlier")), "{shown:?}");
            assert!(!shown.iter().any(|l| l.contains("ctrl+o")), "{shown:?}");
            assert_eq!(drawn.rows.iter().flatten().max(), Some(&(count - 1)));
        }
    }

    #[test]
    fn filtering_shows_every_match_and_no_earlier_row() {
        let agents = agents(5);
        let mut state = State::new(7, ROWS);
        state.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE), 1);
        state.set_len(5);
        let (buffer, _) = render(&agents, &state, 60, 30);
        let shown = rows(&buffer);
        assert_eq!(
            shown.iter().filter(|l| l.contains("Task")).count(),
            5,
            "{shown:?}"
        );
        assert!(!shown.iter().any(|l| l.contains("earlier")), "{shown:?}");
        assert!(shown.iter().any(|l| l.contains("· 5 matches")), "{shown:?}");
    }

    #[test]
    fn follow_moves_only_as_far_as_needed() {
        assert_eq!(follow(0, 0, 3, 10), 0);
        assert_eq!(follow(0, 8, 11, 10), 1);
        assert_eq!(follow(5, 2, 5, 10), 2);
        assert_eq!(follow(0, 4, 20, 10), 4);
    }
}

//! How the picker looks: a header, one block per session, a scrollbar when
//! the blocks overflow, and a key line. Drawing also reports which session
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
use crate::session::{Agent, ago};

/// Bright blue, ANSI colour 12: the selection's pointer and title.
const ACCENT: Color = Color::LightBlue;
/// Columns before each session's text: a space, the pointer, a space.
const GUTTER: usize = 3;
/// The narrowest text column; a narrower pane wraps at this width anyway.
const MIN_TEXT: usize = 10;
const POINTER: &str = "❯";

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

/// Wraps at spaces, never at a hyphen, so a session ID or a folder stays
/// whole when it fits; a word wider than the row still breaks.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let options = Options::new(width.max(MIN_TEXT)).word_splitter(WordSplitter::NoHyphenation);
    textwrap::wrap(text, options)
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect()
}

/// One session's lines: its title, its folder and age, then its session ID,
/// each wrapped to `width` columns after the gutter.
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
    let place = format!("{} · {}", agent.short_cwd(), ago(agent.idle));
    for text in [place.as_str(), agent.session_id.as_str()] {
        for part in wrapped(text, text_width) {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(GUTTER)),
                Span::styled(part, detail),
            ]));
        }
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
fn keys(width: usize) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let hints = [
        ("↑↓", "select"),
        ("⏎", "resume"),
        ("1-9", "resume nth"),
        ("double-click", "resume"),
        ("esc", "shell"),
    ];
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

/// Draws the picker for `state` over `agents` into `frame`, starting the list
/// at `offset` or wherever keeps the selection in view.
pub(crate) fn draw(frame: &mut Frame<'_>, agents: &[Agent], state: &State, offset: usize) -> Drawn {
    let area = frame.area();
    let [header, _, list, _, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let count = match agents.len() {
        1 => "1 session".to_owned(),
        n => format!("{n} sessions"),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ↻ ", Style::new().fg(ACCENT)),
            Span::styled(
                "Resume a Claude Code session",
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {count}"),
                Style::new().add_modifier(Modifier::DIM),
            ),
        ])),
        header,
    );
    frame.render_widget(Paragraph::new(keys(usize::from(area.width))), footer);

    // One column stays free for the scrollbar.
    let width = usize::from(list.width).saturating_sub(1);
    let height = usize::from(list.height);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut owners: Vec<Option<usize>> = Vec::new();
    let mut spans = Vec::with_capacity(agents.len());
    for (index, agent) in agents.iter().enumerate() {
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
    use std::time::{Duration, Instant};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

    use super::*;

    /// The list's first row: under the header and a blank row.
    const TOP: u16 = 2;

    fn agent(n: usize, title: &str) -> Agent {
        Agent {
            session_id: format!("6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d{n:02}"),
            cwd: "/srv/code/shop".to_owned(),
            title: title.to_owned(),
            idle: Some(Duration::from_hours(2)),
        }
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

    #[test]
    fn the_selected_session_has_the_pointer_and_the_accent() {
        let agents = [
            agent(1, "Fix the login redirect"),
            agent(2, "Add CSV export"),
        ];
        let (buffer, drawn) = render(&agents, &State::new(2), 60, 14);
        let top = TOP;
        assert_eq!(row(&buffer, top), " ❯ Fix the login redirect");
        assert_eq!(buffer[(3, top)].fg, ACCENT);
        assert!(buffer[(3, top)].modifier.contains(Modifier::BOLD));
        assert_eq!(row(&buffer, top + 1), "   /srv/code/shop · 2 h ago");
        assert_eq!(
            row(&buffer, top + 2),
            "   6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d01"
        );
        assert_eq!(row(&buffer, top + 4), "   Add CSV export");
        assert!(buffer[(3, top + 5)].modifier.contains(Modifier::DIM));
        assert_eq!(drawn.rows[usize::from(top)], Some(0));
        assert_eq!(drawn.rows[usize::from(top + 3)], None);
        assert_eq!(drawn.rows[usize::from(top + 4)], Some(1));
        assert!(
            row(&buffer, 0).contains("2 sessions"),
            "{:?}",
            rows(&buffer)
        );
        assert!(row(&buffer, 13).contains("resume"), "{:?}", rows(&buffer));
    }

    #[test]
    fn hover_undims_a_session_without_selecting_it() {
        let agents = [
            agent(1, "Fix the login redirect"),
            agent(2, "Add CSV export"),
        ];
        let mut state = State::new(2);
        let moved = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 4,
            row: 6,
            modifiers: KeyModifiers::NONE,
        };
        state.on_mouse(moved, Some(1), Instant::now());
        let (buffer, _) = render(&agents, &state, 60, 14);
        let top = TOP;
        assert_eq!(row(&buffer, top + 4), " ❯ Add CSV export");
        assert!(!buffer[(3, top + 5)].modifier.contains(Modifier::DIM));
        assert_ne!(buffer[(3, top + 4)].fg, ACCENT);
    }

    #[test]
    fn long_text_wraps_and_nothing_is_cut() {
        let title = "Fix the login redirect loop that sends users back to the sign-in page";
        let (buffer, _) = render(&[agent(1, title)], &State::new(1), 40, 16);
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
                .any(|line| line.trim() == "6f1c2a90-3b7e-4d21-9c55-0a8e4f7b1d01"),
            "{shown:?}"
        );
    }

    #[test]
    fn the_list_scrolls_to_keep_the_selection_whole() {
        let agents: Vec<Agent> = (0..8).map(|n| agent(n, &format!("Task {n}"))).collect();
        let mut state = State::new(8);
        for _ in 0..7 {
            state.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 1);
        }
        let (buffer, drawn) = render(&agents, &state, 50, 14);
        let shown = rows(&buffer);
        assert!(
            shown.iter().any(|line| line.starts_with(" ❯ Task 7")),
            "{shown:?}"
        );
        assert!(shown.iter().any(|line| line.ends_with("1d07")), "{shown:?}");
        assert!(drawn.offset > 0);
        assert!(drawn.rows.contains(&Some(7)));
        assert!(!drawn.rows.contains(&Some(0)));
    }

    #[test]
    fn follow_moves_only_as_far_as_needed() {
        assert_eq!(follow(0, 0, 3, 10), 0);
        assert_eq!(follow(0, 8, 11, 10), 1);
        assert_eq!(follow(5, 2, 5, 10), 2);
        assert_eq!(follow(0, 4, 20, 10), 4);
    }
}

//! Text as Claude Code draws it: markdown for prompts and messages, and
//! code in Claude Code's code theme.
//!
//! Drawn lines are kept by text and width, so a frame that draws the same
//! entries again costs a lookup.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::LazyLock;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use textwrap::core::display_width;

/// two-face's syntaxes: syntect's and the ones bat adds.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Claude Code's code theme under a dark colour theme, Monokai Extended
/// (`syntaxThemeName` in its `color-diff.ts`), from bat's themes.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    two_face::theme::extra()
        .get(two_face::theme::EmbeddedThemeName::MonokaiExtended)
        .clone()
});

/// Loads the syntaxes on a thread of their own, so the first frame that
/// draws code does not wait for them.
pub(crate) fn load() {
    std::thread::spawn(|| {
        LazyLock::force(&SYNTAXES);
        LazyLock::force(&THEME);
    });
}

/// How many drawings the cache keeps before it starts again.
const CACHE: usize = 4096;

thread_local! {
    static DRAWN: RefCell<HashMap<u64, Vec<Line<'static>>>> = RefCell::new(HashMap::new());
}

/// The lines `draw` gives for `key`, drawn once.
fn cached(key: impl Hash, draw: impl FnOnce() -> Vec<Line<'static>>) -> Vec<Line<'static>> {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let key = hasher.finish();
    if let Some(lines) = DRAWN.with_borrow(|drawn| drawn.get(&key).cloned()) {
        return lines;
    }
    let lines = draw();
    DRAWN.with_borrow_mut(|drawn| {
        if drawn.len() >= CACHE {
            drawn.clear();
        }
        drawn.insert(key, lines.clone());
    });
    lines
}

/// `text` in `language`'s colours, a line each, unwrapped; plain when no
/// syntax knows the language.
pub(crate) fn code(text: &str, language: &str) -> Vec<Line<'static>> {
    cached(("code", text, language), || {
        let syntaxes = &*SYNTAXES;
        let Some(syntax) = syntaxes
            .find_syntax_by_token(language)
            .or_else(|| syntaxes.find_syntax_by_extension(language))
        else {
            return text
                .lines()
                .map(|line| Line::raw(line.to_owned()))
                .collect();
        };
        let mut lines = HighlightLines::new(syntax, &THEME);
        LinesWithEndings::from(text)
            .map(|line| {
                let spans = lines
                    .highlight_line(line, syntaxes)
                    .map(|runs| {
                        runs.into_iter()
                            .map(|(style, run)| {
                                let run = run.trim_end_matches(['\n', '\r']).to_owned();
                                let colour = style.foreground;
                                let mut shown =
                                    Style::new().fg(Color::Rgb(colour.r, colour.g, colour.b));
                                if style.font_style.contains(FontStyle::BOLD) {
                                    shown = shown.add_modifier(Modifier::BOLD);
                                }
                                if style.font_style.contains(FontStyle::ITALIC) {
                                    shown = shown.add_modifier(Modifier::ITALIC);
                                }
                                if style.font_style.contains(FontStyle::UNDERLINE) {
                                    shown = shown.add_modifier(Modifier::UNDERLINED);
                                }
                                Span::styled(run, shown)
                            })
                            .collect()
                    })
                    .unwrap_or_else(|_| vec![Span::raw(line.trim_end().to_owned())]);
                Line::from(spans)
            })
            .collect()
    })
}

/// The style of inline code, Claude Code's `permission` blue.
fn inline_code() -> Style {
    Style::new().fg(Color::Blue)
}

/// `text` as markdown, wrapped to `width` columns.
pub(crate) fn markdown(text: &str, width: usize) -> Vec<Line<'static>> {
    cached(("markdown", text, width), || {
        let mut out = Markdown {
            width: width.max(8),
            ..Markdown::default()
        };
        let options =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
        for event in Parser::new_ext(text, options) {
            out.event(event);
        }
        out.flush();
        while out.lines.last().is_some_and(|line| line.width() == 0) {
            out.lines.pop();
        }
        out.lines
    })
}

#[derive(Default)]
struct Markdown {
    width: usize,
    lines: Vec<Line<'static>>,
    /// The runs of the block being read.
    runs: Vec<Span<'static>>,
    styles: Vec<Style>,
    /// Each open list's next number, `None` for a bullet list.
    lists: Vec<Option<u64>>,
    /// The mark the current list item's first line starts with.
    mark: Option<String>,
    quotes: usize,
    /// A code block being read: its language and its text.
    fence: Option<(String, String)>,
    /// Whether a blank line goes before the next block.
    gap: bool,
}

impl Markdown {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push(&mut self, style: impl FnOnce(Style) -> Style) {
        let next = style(self.style());
        self.styles.push(next);
    }

    /// The indent before every line of the current block.
    fn indent(&self) -> String {
        let mut indent = "▎ ".repeat(self.quotes);
        indent.push_str(&"  ".repeat(self.lists.len().saturating_sub(1)));
        indent
    }

    fn start_block(&mut self) {
        if self.gap && self.lists.is_empty() {
            self.lines.push(Line::default());
        }
        self.gap = false;
    }

    /// Wraps the runs read so far into lines.
    fn flush(&mut self) {
        if self.runs.is_empty() && self.mark.is_none() {
            return;
        }
        let indent = self.indent();
        let mark = self.mark.take().unwrap_or_default();
        let first = format!("{indent}{mark}");
        let rest = format!("{indent}{}", " ".repeat(display_width(&mark)));
        let runs = std::mem::take(&mut self.runs);
        let quoted = self.quotes > 0;
        for (index, mut line) in wrap(&runs, self.width.saturating_sub(display_width(&first)))
            .into_iter()
            .enumerate()
        {
            let lead = if index == 0 {
                first.clone()
            } else {
                rest.clone()
            };
            line.spans.insert(
                0,
                Span::styled(lead, if quoted { dim() } else { Style::new() }),
            );
            self.lines.push(line);
        }
    }

    fn event(&mut self, event: Event<'_>) {
        if let Some((_, body)) = self.fence.as_mut() {
            match event {
                Event::Text(text) => body.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    let (language, body) = self.fence.take().unwrap_or_default();
                    let indent = self.indent();
                    for mut line in code(&body, &language) {
                        line.spans.insert(0, Span::raw(format!("{indent}  ")));
                        self.lines.push(line);
                    }
                    self.gap = true;
                }
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self
                .runs
                .push(Span::styled(text.into_string(), self.style())),
            Event::Code(text) => self.runs.push(Span::styled(
                text.into_string(),
                self.style().patch(inline_code()),
            )),
            Event::InlineHtml(text)
            | Event::Html(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => {
                self.runs.push(Span::styled(text.into_string(), dim()));
            }
            Event::SoftBreak => self.runs.push(Span::raw(" ")),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.start_block();
                self.lines
                    .push(Line::styled("─".repeat(self.width.min(40)), dim()));
                self.gap = true;
            }
            Event::TaskListMarker(done) => {
                self.mark = Some(if done { "☒ " } else { "☐ " }.to_owned())
            }
            Event::FootnoteReference(name) => {
                self.runs.push(Span::styled(format!("[{name}]"), dim()))
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.start_block(),
            Tag::Heading { level, .. } => {
                self.start_block();
                self.push(|style| match level {
                    HeadingLevel::H1 => {
                        style.add_modifier(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED)
                    }
                    _ => style.add_modifier(Modifier::BOLD),
                });
            }
            Tag::BlockQuote(_) => {
                self.start_block();
                self.quotes += 1;
                self.push(|style| style.add_modifier(Modifier::ITALIC).patch(dim()));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.start_block();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.fence = Some((language, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                if self.lists.is_empty() {
                    self.start_block();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let mark = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let mark = format!("{number}. ");
                        *number += 1;
                        mark
                    }
                    _ => "- ".to_owned(),
                };
                self.mark = Some(mark);
            }
            Tag::Emphasis => self.push(|style| style.add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push(|style| style.add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push(|style| style.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.push(|style| style.add_modifier(Modifier::UNDERLINED)),
            Tag::Table(_) => self.start_block(),
            Tag::TableCell => {
                if !self.runs.is_empty() {
                    self.runs.push(Span::styled(" │ ", dim()));
                }
            }
            Tag::TableHead => self.push(|style| style.add_modifier(Modifier::BOLD)),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                self.gap = true;
            }
            TagEnd::Heading(_) | TagEnd::BlockQuote(_) => {
                self.flush();
                self.styles.pop();
                if matches!(tag, TagEnd::BlockQuote(_)) {
                    self.quotes = self.quotes.saturating_sub(1);
                }
                self.gap = true;
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                self.gap = true;
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::TableHead => {
                self.flush();
                self.styles.pop();
            }
            TagEnd::TableRow => self.flush(),
            TagEnd::Table => {
                self.flush();
                self.gap = true;
            }
            _ => {}
        }
    }
}

fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

/// `runs` broken into lines of at most `width` columns at spaces, a word
/// longer than a line cut where the line ends.
pub(crate) fn wrap(runs: &[Span<'static>], width: usize) -> Vec<Line<'static>> {
    let width = width.max(4);
    let mut lines = Vec::new();
    let mut line: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for run in runs {
        let mut rest: &str = &run.content;
        while !rest.is_empty() {
            // The next word with the spaces before it.
            let start = rest.len() - rest.trim_start().len();
            let end = rest[start..].find(' ').map_or(rest.len(), |at| start + at);
            let (piece, after) = rest.split_at(end);
            rest = after;
            let mut piece: &str = if used == 0 { piece.trim_start() } else { piece };
            while !piece.is_empty() {
                let room = width - used;
                let size = display_width(piece);
                if size <= room {
                    line.push(Span::styled(piece.to_owned(), run.style));
                    used += size;
                    break;
                }
                if used > 0 && display_width(piece.trim_start()) <= width {
                    lines.push(Line::from(std::mem::take(&mut line)));
                    used = 0;
                    piece = piece.trim_start();
                    continue;
                }
                // A word longer than the line: cut it where the line ends.
                let mut cut = 0;
                let mut taken = 0;
                for (at, ch) in piece.char_indices() {
                    let size = display_width(&ch.to_string());
                    if taken + size > room {
                        break;
                    }
                    taken += size;
                    cut = at + ch.len_utf8();
                }
                if cut == 0 {
                    lines.push(Line::from(std::mem::take(&mut line)));
                    used = 0;
                    continue;
                }
                line.push(Span::styled(piece[..cut].to_owned(), run.style));
                lines.push(Line::from(std::mem::take(&mut line)));
                used = 0;
                piece = &piece[cut..];
            }
        }
    }
    if !line.is_empty() {
        lines.push(Line::from(line));
    }
    lines
}

/// `line` cut to `width` columns, ending in `…` when it was longer.
pub(crate) fn fit(line: &Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line.clone();
    }
    let mut spans = Vec::new();
    let mut used = 0;
    let room = width.saturating_sub(1);
    'runs: for span in &line.spans {
        let mut kept = String::new();
        for ch in span.content.chars() {
            let size = display_width(&ch.to_string());
            if used + size > room {
                spans.push(Span::styled(kept, span.style));
                break 'runs;
            }
            used += size;
            kept.push(ch);
        }
        spans.push(Span::styled(kept, span.style));
    }
    spans.push(Span::styled("…", dim()));
    Line::from(spans)
}

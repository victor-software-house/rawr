//! Every colour the app uses, by role. Text roles are ANSI slots, so the
//! terminal's own palette shows through; the two surface greys sit just
//! above a dark background, as herdr's raised and hover surfaces do.

use ratatui::style::{Color, Modifier, Style};

use crate::status::Status;

pub(crate) const ACCENT: Color = Color::LightBlue;
pub(crate) const WARNING: Color = Color::Yellow;
pub(crate) const ERROR: Color = Color::Red;
pub(crate) const WORKING: Color = Color::Green;
/// The row under the pointer: one step above the background.
const HOVER: Color = Color::Indexed(234);
/// The selected row: two steps above the background.
const RAISED: Color = Color::Indexed(236);
/// Rules and borders.
const LINE: Color = Color::Indexed(238);

pub(crate) fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

pub(crate) fn line() -> Style {
    Style::new().fg(LINE)
}

/// A section header in the sidebar, such as `Needs you`.
pub(crate) fn header() -> Style {
    Style::new().add_modifier(Modifier::DIM | Modifier::BOLD)
}

pub(crate) fn hover() -> Style {
    Style::new().bg(HOVER)
}

pub(crate) fn raised() -> Style {
    Style::new().bg(RAISED)
}

/// The background of a row: raised when selected, a step up when hovered.
pub(crate) fn row(selected: bool, hovered: bool) -> Style {
    if selected {
        raised()
    } else if hovered {
        hover()
    } else {
        Style::new()
    }
}

/// The braille spinner herdr's working state turns, one frame per tick.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(crate) fn glyph(status: Status, frame: usize) -> &'static str {
    match status {
        Status::NeedsPermission | Status::NeedsInput => "▲",
        Status::NeedsAnswer => "?",
        Status::NeedsLogin => "◆",
        Status::Failed => "✘",
        Status::Done => "✔",
        Status::Working => SPINNER[frame % SPINNER.len()],
        Status::Idle => "○",
        Status::Stopped => "·",
    }
}

pub(crate) fn status(status: Status) -> Style {
    match status {
        Status::NeedsPermission | Status::NeedsAnswer | Status::NeedsLogin | Status::NeedsInput => {
            Style::new().fg(WARNING).add_modifier(Modifier::BOLD)
        }
        Status::Failed => Style::new().fg(ERROR),
        Status::Done => Style::new().fg(ACCENT),
        Status::Working => Style::new().fg(WORKING),
        Status::Idle | Status::Stopped => dim(),
    }
}

/// The section a status sits under in the sidebar.
pub(crate) const fn section(status: Status) -> &'static str {
    match status {
        Status::NeedsPermission | Status::NeedsAnswer | Status::NeedsLogin | Status::NeedsInput => {
            "Needs you"
        }
        Status::Failed => "Failed",
        Status::Done => "Done",
        Status::Working => "Working",
        Status::Idle => "Idle",
        Status::Stopped => "Stopped",
    }
}

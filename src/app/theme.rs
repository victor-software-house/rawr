//! Every colour the app uses, by role. Each role is an ANSI slot, so the
//! terminal's own palette shows through in light and dark themes.

use ratatui::style::{Color, Modifier, Style};

use crate::status::Status;

pub(crate) const ACCENT: Color = Color::LightBlue;
pub(crate) const WARNING: Color = Color::Yellow;
pub(crate) const ERROR: Color = Color::Red;
pub(crate) const WORKING: Color = Color::Green;

pub(crate) fn dim() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

pub(crate) fn border(focused: bool) -> Style {
    if focused {
        Style::new().fg(ACCENT)
    } else {
        dim()
    }
}

pub(crate) fn selected() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub(crate) const fn glyph(status: Status) -> &'static str {
    match status {
        Status::NeedsInput => "◐",
        Status::Failed => "✘",
        Status::Working => "●",
        Status::Idle => "○",
        Status::Stopped => "·",
    }
}

pub(crate) fn status(status: Status) -> Style {
    match status {
        Status::NeedsInput => Style::new().fg(WARNING).add_modifier(Modifier::BOLD),
        Status::Failed => Style::new().fg(ERROR),
        Status::Working => Style::new().fg(WORKING),
        Status::Idle | Status::Stopped => dim(),
    }
}

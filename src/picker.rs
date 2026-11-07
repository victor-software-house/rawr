//! The full-screen picker: it reads keys and mouse reports, draws the
//! sessions, and returns the one the user resumes. The state and the view
//! are pure; this loop only moves events between them and the terminal.

mod state;
mod terminal;
mod view;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event};

use crate::picker::state::{State, Step};
use crate::picker::terminal::{Pointer, Screen};
use crate::session::Agent;

/// Shows `agents` and returns the index of the one the user resumes, or
/// `None` when the user dismisses the picker.
///
/// # Errors
///
/// Fails when the terminal cannot be taken, drawn on or read; the terminal is
/// given back first.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "focus and paste events, whose set depends on crossterm's features, change nothing"
)]
pub(crate) fn choose(agents: &[Agent]) -> Result<Option<usize>> {
    let mut screen = Screen::enter()?;
    let mut state = State::new(agents.len());
    let mut offset = 0;
    let mut drawn = None;
    loop {
        let frame = if let Some(frame) = drawn.take() {
            frame
        } else {
            let mut result = None;
            screen
                .terminal()
                .draw(|frame| result = Some(view::draw(frame, agents, &state, offset)))
                .context("draw the picker")?;
            result.context("the picker drew no frame")?
        };
        offset = frame.offset;
        let step = match event::read().context("read the terminal")? {
            Event::Key(key) => state.on_key(key, frame.page),
            Event::Mouse(mouse) => {
                let hit = frame.rows.get(usize::from(mouse.row)).copied().flatten();
                let step = state.on_mouse(mouse, hit);
                let pointer = if state.hover().is_some() {
                    Pointer::Hand
                } else {
                    Pointer::Default
                };
                screen.set_pointer(pointer)?;
                step
            }
            Event::Resize(..) => Step::Redraw,
            _ => Step::Ignore,
        };
        match step {
            Step::Resume(index) => return Ok(Some(index)),
            Step::Dismiss => return Ok(None),
            Step::Redraw => {}
            Step::Ignore => drawn = Some(frame),
        }
    }
}

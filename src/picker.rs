//! The full-screen picker: it reads keys and mouse reports, draws the
//! sessions, and returns the one the user resumes. The state and the view
//! are pure; this loop only moves events between them and the terminal.

mod state;
pub(crate) mod terminal;
mod view;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, Event};

use crate::catalog::{self, Found};
use crate::picker::state::{State, Step};
use crate::picker::terminal::{Pointer, Screen};
use crate::session::Agent;

/// Where the picker opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Start {
    /// On the pane's own sessions, with the search box empty.
    Pane,
    /// On every local session, with this text typed in the search box.
    Search(String),
}

/// Reads every local session once, the first time the list needs them.
struct Catalog<L> {
    load: Option<L>,
    found: Vec<Found>,
}

impl<L: FnOnce() -> Result<Vec<Found>>> Catalog<L> {
    fn found(&mut self) -> Result<&[Found]> {
        if let Some(load) = self.load.take() {
            self.found = load()?;
        }
        Ok(&self.found)
    }

    /// The rows to list: the pane's sessions, or, once the user searches,
    /// the matches for `query`.
    fn rows(&mut self, pane: &[Agent], query: Option<&str>) -> Result<Vec<Agent>> {
        match query {
            None => Ok(pane.to_vec()),
            Some(query) => Ok(catalog::search(pane, self.found()?, &catalog::terms(query))),
        }
    }
}

/// Shows `pane`'s sessions, or the search results while the user searches,
/// and returns the session the user resumes, or `None` when the user
/// dismisses the picker. `limit` is how many sessions show before `ctrl+o`.
/// `load` reads every local session, and runs only once the user types.
///
/// # Errors
///
/// Fails when the terminal cannot be taken, drawn on or read, or when `load`
/// fails; the terminal is given back first.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "focus and paste events, whose set depends on crossterm's features, change nothing"
)]
pub(crate) fn choose(
    pane: &[Agent],
    start: Start,
    limit: usize,
    load: impl FnOnce() -> Result<Vec<Found>>,
) -> Result<Option<Agent>> {
    let mut catalog = Catalog {
        load: Some(load),
        found: Vec::new(),
    };
    let mut state = match start {
        Start::Pane => State::new(pane.len(), limit),
        Start::Search(query) => State::searching(query, limit),
    };
    let mut rows = catalog.rows(pane, state.query())?;
    state.set_len(rows.len());
    let mut screen = Screen::enter()?;
    let mut offset = 0;
    let mut drawn = None;
    loop {
        let frame = if let Some(frame) = drawn.take() {
            frame
        } else {
            let mut result = None;
            screen
                .draw(|frame| result = Some(view::draw(frame, &rows, &state, offset)))
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
            Step::Resume(index) => return Ok(rows.get(index).cloned()),
            Step::Dismiss => return Ok(None),
            Step::Filter => {
                rows = catalog.rows(pane, state.query())?;
                state.set_len(rows.len());
                offset = 0;
            }
            Step::Redraw => {}
            Step::Ignore => drawn = Some(frame),
        }
    }
}

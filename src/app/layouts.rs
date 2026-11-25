//! The entries of the prompts and the activity, laid out once.
//!
//! An entry is wrapped, and its text drawn as markdown or code, for a width
//! and a fold state. The result is kept, so a frame costs a lookup for each
//! entry, and only the entries in view are built into rows. What changes
//! between frames, the age, the fold marker and the bar under the pointer,
//! is not kept: the view draws it over the kept rows.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ratatui::text::Line;

use crate::app::facts::{EntryId, Facts, Said};

/// One entry laid out for a width and a fold state.
#[derive(Debug)]
pub(crate) struct Laid {
    /// The entry's rows, without the columns the view draws over: the age and
    /// the fold marker before the first row, the bar or blanks before the rest.
    pub(crate) lines: Vec<Line<'static>>,
    /// How many columns those take on every row but the first.
    pub(crate) indent: usize,
    /// Whether a click on the entry opens or folds it.
    pub(crate) folds: bool,
    /// The row that ends with what a folded entry leaves out, which draws
    /// plain under the pointer.
    pub(crate) tail: Option<usize>,
}

/// The laid out entries of the transcript shown, at one width.
#[derive(Debug, Default)]
pub(crate) struct Layouts {
    transcript: Option<PathBuf>,
    width: usize,
    entries: HashMap<(EntryId, bool), Laid>,
    /// How many entries were laid out, for the tests.
    #[cfg(test)]
    laid: usize,
}

impl Layouts {
    /// Keeps what is laid out for `transcript` at `width`, and forgets it all
    /// when either changes, so what is held stays within one transcript's
    /// entries.
    pub(crate) fn show(&mut self, transcript: &Path, width: usize) {
        if self.width != width || self.transcript.as_deref() != Some(transcript) {
            self.entries.clear();
            self.width = width;
            transcript.clone_into(self.transcript.get_or_insert_default());
        }
    }

    /// The entry `id`, open or folded, laid out by `lay` unless it is held.
    pub(crate) fn get(&mut self, id: EntryId, open: bool, lay: impl FnOnce() -> Laid) -> &Laid {
        self.entries.entry((id, open)).or_insert_with(|| {
            #[cfg(test)]
            {
                self.laid += 1;
            }
            lay()
        })
    }

    /// Facts for `transcript` came, `now` after `before`: forgets the entries
    /// that are gone or differ from what they were, so each is laid out again
    /// as it is now.
    pub(crate) fn refresh(&mut self, transcript: &Path, before: &Facts, now: &Facts) {
        if self.transcript.as_deref() != Some(transcript) {
            return;
        }
        let held: HashMap<EntryId, &Said> = before
            .log
            .iter()
            .chain(&before.activity)
            .map(|said| (said.id, said))
            .collect();
        let same: HashSet<EntryId> = now
            .log
            .iter()
            .chain(&now.activity)
            .filter(|said| held.get(&said.id).is_some_and(|held| *held == *said))
            .map(|said| said.id)
            .collect();
        self.entries.retain(|(id, _), _| same.contains(id));
    }

    /// How many entries were laid out so far.
    #[cfg(test)]
    pub(crate) fn laid(&self) -> usize {
        self.laid
    }
}

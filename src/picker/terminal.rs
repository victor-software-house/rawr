//! The terminal while the picker owns it: raw mode, the alternate screen,
//! mouse reporting and a hidden cursor, all on stderr, since stdout carries
//! only the chosen command. One guard takes the terminal and gives it back.

use std::io::{self, Stderr, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// How long input is read and dropped after mouse reporting stops, so reports
/// already on their way are not typed into the shell.
const DRAIN: Duration = Duration::from_millis(50);

/// Whether the picker holds the terminal now. It is one per process, as the
/// terminal is, and lets the panic hook and the guard give it back once.
static HELD: AtomicBool = AtomicBool::new(false);

/// The pointer shapes the picker asks for with OSC 22.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pointer {
    /// The hand, over a row that takes a click.
    Hand,
    /// The terminal's own pointer.
    Default,
}

impl Pointer {
    /// The OSC 22 sequence that sets this shape; terminals that do not know
    /// OSC 22 ignore it.
    fn sequence(self) -> &'static [u8] {
        match self {
            Self::Hand => b"\x1b]22;pointer\x1b\\",
            Self::Default => b"\x1b]22;default\x1b\\",
        }
    }
}

/// The terminal, taken over for the picker; dropping it gives it back.
pub(crate) struct Screen {
    terminal: Terminal<CrosstermBackend<Stderr>>,
    pointer: Pointer,
}

impl std::fmt::Debug for Screen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Screen")
            .field("pointer", &self.pointer)
            .finish_non_exhaustive()
    }
}

impl Screen {
    /// Takes the terminal: raw mode, the alternate screen, mouse reporting
    /// with motion for hover, and a hidden cursor. A panic gives it back
    /// first, since the release build aborts and runs no destructor.
    ///
    /// # Errors
    ///
    /// Fails when the terminal refuses raw mode or a write to stderr fails;
    /// whatever was already set is undone.
    pub(crate) fn enter() -> Result<Self> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave();
            previous(info);
        }));
        enable_raw_mode().context("enter raw mode")?;
        HELD.store(true, Ordering::Release);
        let taken = execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture, Hide)
            .context("take the screen")
            .and_then(|()| {
                Terminal::new(CrosstermBackend::new(io::stderr())).context("start drawing")
            });
        match taken {
            Ok(terminal) => Ok(Self {
                terminal,
                pointer: Pointer::Default,
            }),
            Err(error) => {
                leave();
                Err(error)
            }
        }
    }

    pub(crate) fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stderr>> {
        &mut self.terminal
    }

    /// Sets the pointer shape, writing only when it changes.
    ///
    /// # Errors
    ///
    /// Fails when the write to stderr fails.
    pub(crate) fn set_pointer(&mut self, pointer: Pointer) -> Result<()> {
        if pointer == self.pointer {
            return Ok(());
        }
        let mut err = io::stderr();
        err.write_all(pointer.sequence())
            .and_then(|()| err.flush())
            .context("set the pointer shape")?;
        self.pointer = pointer;
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        leave();
    }
}

/// Gives the terminal back as it was found: the default pointer, no mouse
/// reporting, the cursor, the main screen and cooked mode. It runs once
/// however many paths call it, and never fails: each step is best effort, so
/// one refusal does not stop the rest.
fn leave() {
    if !HELD.swap(false, Ordering::AcqRel) {
        return;
    }
    let mut err = io::stderr();
    let _ = err.write_all(Pointer::Default.sequence());
    let _ = execute!(err, DisableMouseCapture, Show, LeaveAlternateScreen);
    let deadline = Instant::now() + DRAIN;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match event::poll(left) {
            Ok(true) => {
                let _ = event::read();
            }
            Ok(false) | Err(_) => break,
        }
    }
    let _ = disable_raw_mode();
}

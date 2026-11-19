//! The terminal while the app owns it: raw mode, the alternate screen,
//! mouse reporting and a hidden cursor, all on stderr, since stdout carries
//! only the chosen command. One guard takes the terminal and gives it back.

use std::io::{self, BufWriter, Stderr, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{fmt, panic};

use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, SetCursorStyle, Show};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture,
};
use ratatui::crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui::crossterm::{execute, queue};

/// How long input is read and dropped after mouse reporting stops, so reports
/// already on their way are not typed into the shell.
const DRAIN: Duration = Duration::from_millis(50);

/// How much of a frame the screen holds before writing it: a full frame of
/// a large terminal, so each frame goes out in one write.
const FRAME_BYTES: usize = 1 << 18;

/// The terminal's writer: stderr, buffered so a frame is written whole.
type Out = CrosstermBackend<BufWriter<Stderr>>;

/// Whether the app holds the terminal now. It is one per process, as the
/// terminal is, and lets the panic hook and the guard give it back once.
static HELD: AtomicBool = AtomicBool::new(false);

/// Clears the Program Status Protocol (OSC 7501) record the app reports
/// under `id=rawr`, so the pane shows no status once the app is gone.
const STATUS_CLEAR: &[u8] = b"\x1b]7501;state=clear:id=rawr\x1b\\";

/// The pointer shapes the app asks for with OSC 22.
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

/// The terminal, taken over for the app; dropping it gives it back.
pub(crate) struct Screen {
    terminal: Terminal<Out>,
    pointer: Pointer,
}

impl fmt::Debug for Screen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            leave();
            previous(info);
        }));
        enable_raw_mode().context("enter raw mode")?;
        HELD.store(true, Ordering::Release);
        let mut err = io::stderr();
        let taken = execute!(
            err,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableFocusChange,
            EnableBracketedPaste,
            Hide
        )
        .context("take the screen")
        .and_then(|()| {
            Terminal::new(CrosstermBackend::new(BufWriter::with_capacity(
                FRAME_BYTES,
                io::stderr(),
            )))
            .context("start drawing")
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

    /// Draws one frame and writes it whole, inside a synchronized update,
    /// so the terminal never shows half of one.
    ///
    /// # Errors
    ///
    /// Fails when a write to stderr fails.
    pub(crate) fn draw(&mut self, render: impl FnOnce(&mut ratatui::Frame<'_>)) -> Result<()> {
        queue!(self.terminal.backend_mut(), BeginSynchronizedUpdate).context("start the frame")?;
        self.terminal.draw(render).context("draw the frame")?;
        let out = self.terminal.backend_mut();
        queue!(out, EndSynchronizedUpdate).context("end the frame")?;
        out.flush().context("write the frame")?;
        Ok(())
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
    let _ = err.write_all(STATUS_CLEAR);
    let _ = execute!(
        err,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture,
        SetCursorStyle::DefaultUserShape,
        Show,
        LeaveAlternateScreen
    );
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

#[cfg(test)]
mod tests {
    use super::STATUS_CLEAR;

    #[test]
    fn the_clear_record_matches_the_protocol() {
        assert_eq!(STATUS_CLEAR, b"\x1b]7501;state=clear:id=rawr\x1b\\");
    }
}

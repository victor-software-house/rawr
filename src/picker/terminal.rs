//! The terminal while the picker owns it: raw mode, the alternate screen,
//! mouse reporting and a hidden cursor, all on stderr, since stdout carries
//! only the chosen command. One guard takes the terminal and gives it back.

use std::io::{self, Stderr, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{fmt, panic};

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

/// The Program Status Protocol (OSC 7501) record that marks the pane as
/// waiting for the user while the picker is open; Rex shows it as a
/// needs-attention dot on the pane's tab. The message is "Resume a Claude Code
/// session" in base64. Terminals that do not know OSC 7501 ignore it.
const STATUS_WAITING: &[u8] = b"\x1b]7501;state=blocked:kind=question:app=rawr:id=rawr:msg=UmVzdW1lIGEgQ2xhdWRlIENvZGUgc2Vzc2lvbg==\x1b\\";

/// Clears the record [`STATUS_WAITING`] set.
const STATUS_CLEAR: &[u8] = b"\x1b]7501;state=clear:id=rawr\x1b\\";

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
        let taken = err
            .write_all(STATUS_WAITING)
            .map_err(anyhow::Error::from)
            .and_then(|()| {
                execute!(err, EnterAlternateScreen, EnableMouseCapture, Hide).map_err(Into::into)
            })
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
    let _ = err.write_all(STATUS_CLEAR);
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

#[cfg(test)]
mod tests {
    use super::{STATUS_CLEAR, STATUS_WAITING};

    #[test]
    fn status_records_match_the_protocol() {
        let waiting = str::from_utf8(STATUS_WAITING).unwrap();
        let fields = waiting
            .strip_prefix("\x1b]7501;")
            .and_then(|rest| rest.strip_suffix("\x1b\\"))
            .unwrap();
        let pairs: Vec<&str> = fields.split(':').collect();
        assert_eq!(
            pairs[..4],
            ["state=blocked", "kind=question", "app=rawr", "id=rawr"]
        );
        let message = pairs[4].strip_prefix("msg=").unwrap();
        assert_eq!(message, "UmVzdW1lIGEgQ2xhdWRlIENvZGUgc2Vzc2lvbg==");
        assert_eq!(STATUS_CLEAR, b"\x1b]7501;state=clear:id=rawr\x1b\\");
    }
}

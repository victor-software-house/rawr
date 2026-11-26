//! What a rawr window tells Rex about itself, through the Program Status
//! Protocol (OSC 7501): one record, ID `rawr`, so the shell's and Claude's own
//! records are never touched. The screen guard clears it when the window gives
//! the terminal back.
//!
//! Each report replaces the record whole, so every report carries `app` and
//! `title`. Free text is base64 of one line of UTF-8.

use std::io::{self, Write};

use anyhow::{Context, Result};

/// The protocol's longest decoded `msg`.
const MSG_BYTES: usize = 2048;

/// The protocol's longest decoded `title`.
pub(super) const TITLE_BYTES: usize = 192;

/// Whether the window waits for the user or is idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Waiting for the user's next key.
    Idle,
    /// An agent needs the user.
    Blocked,
}

impl State {
    /// The word the protocol names the state by.
    const fn word(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Blocked => "blocked",
        }
    }
}

/// What the window's record says: its state, the title Rex lists it under,
/// and one line of detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) state: State,
    pub(crate) title: String,
    pub(crate) msg: String,
}

impl Report {
    fn sequence(&self) -> Vec<u8> {
        let pairs = [
            format!("state={}", self.state.word()),
            "id=rawr".to_owned(),
            "app=rawr".to_owned(),
            format!("title={}", base64(&one_line(&self.title, TITLE_BYTES))),
            format!("msg={}", base64(&one_line(&self.msg, MSG_BYTES))),
        ];
        format!("\x1b]7501;{}\x1b\\", pairs.join(":")).into_bytes()
    }
}

/// Decides which reports go out: each only when it differs from the last.
#[derive(Debug, Default)]
pub(crate) struct Reporter {
    last: Option<Report>,
}

impl Reporter {
    /// The bytes that report `report`, or none when it is what the last
    /// report said.
    pub(crate) fn next(&mut self, report: Report) -> Option<Vec<u8>> {
        if self.last.as_ref() == Some(&report) {
            return None;
        }
        let bytes = report.sequence();
        self.last = Some(report);
        Some(bytes)
    }

    /// Writes what [`Reporter::next`] returns to stderr.
    ///
    /// # Errors
    ///
    /// Fails when the write to stderr fails.
    pub(crate) fn send(&mut self, report: Report) -> Result<()> {
        let Some(bytes) = self.next(report) else {
            return Ok(());
        };
        let mut err = io::stderr();
        err.write_all(&bytes)
            .and_then(|()| err.flush())
            .context("report the window's status")
    }
}

/// `text` with control characters turned into spaces, cut to `most` bytes on
/// a character boundary.
pub(super) fn one_line(text: &str, most: usize) -> String {
    let mut line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    while line.len() > most {
        line.pop();
    }
    line
}

/// Standard base64 with padding.
fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        let digits = chunk.len() + 1;
        for i in 0..4 {
            if i < digits {
                #[expect(
                    clippy::as_conversions,
                    reason = "the index is masked to six bits, so it fits a usize"
                )]
                let index = (n >> (18 - 6 * i) & 63) as usize;
                out.push(char::from(ALPHABET[index]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(state: State, title: &str, msg: &str) -> Report {
        Report {
            state,
            title: title.to_owned(),
            msg: msg.to_owned(),
        }
    }

    #[test]
    fn an_idle_report_matches_the_protocol() {
        let report = report(State::Idle, "rawr", "Every pane and session");
        assert_eq!(
            report.sequence(),
            b"\x1b]7501;state=idle:id=rawr:app=rawr:title=cmF3cg==:msg=RXZlcnkgcGFuZSBhbmQgc2Vzc2lvbg==\x1b\\"
        );
    }

    #[test]
    fn a_blocked_report_has_no_kind() {
        let report = report(State::Blocked, "rawr", "An agent needs you");
        assert_eq!(
            report.sequence(),
            b"\x1b]7501;state=blocked:id=rawr:app=rawr:title=cmF3cg==:msg=QW4gYWdlbnQgbmVlZHMgeW91\x1b\\"
        );
    }

    #[test]
    fn a_report_is_sent_only_when_it_changes() {
        let mut reporter = Reporter::default();
        let idle = report(State::Idle, "rawr", "Every pane and session");
        assert!(reporter.next(idle.clone()).is_some());
        assert_eq!(reporter.next(idle), None);
        let blocked = report(State::Blocked, "rawr", "An agent needs you");
        assert!(reporter.next(blocked).is_some());
    }

    #[test]
    fn a_title_is_cut_on_a_character_boundary() {
        assert_eq!(
            one_line(&"€".repeat(65), TITLE_BYTES),
            "€".repeat(64),
            "65 characters of 3 bytes are 195 bytes, past the 192 the protocol allows"
        );
        assert_eq!(
            one_line("fix\u{1b}[2J\tbuild", TITLE_BYTES),
            "fix [2J build"
        );
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64("rawr"), "cmF3cg==");
        assert_eq!(
            base64("Resume a Claude Code session"),
            "UmVzdW1lIGEgQ2xhdWRlIENvZGUgc2Vzc2lvbg=="
        );
        assert_eq!(base64("ab"), "YWI=");
        assert_eq!(base64("abc"), "YWJj");
    }
}

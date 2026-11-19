//! What the app tells Rex about itself, through the Program Status Protocol
//! (OSC 7501): one record, ID `rawr`, so the shell's and Claude's own records
//! are never touched. The screen guard clears it when the app gives the
//! terminal back.
//!
//! Each report replaces the record whole, so every report carries `app` and
//! `title`. Free text is base64 of one line of UTF-8.

use std::io::{self, Write};

use anyhow::{Context, Result};

/// The state the record holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Report {
    /// Waiting for the user's next key; the text sums up the panes.
    Idle(String),
    /// Busy, with no percentage: reading Rex or every local session.
    Working(String),
}

impl Report {
    fn sequence(&self) -> Vec<u8> {
        let (state, msg) = match self {
            Self::Idle(msg) => ("idle", msg),
            Self::Working(msg) => ("working", msg),
        };
        let pairs = [
            format!("state={state}"),
            "id=rawr".to_owned(),
            "app=rawr".to_owned(),
            format!("title={}", base64("rawr")),
            format!("msg={}", base64(&one_line(msg))),
        ];
        format!("\x1b]7501;{}\x1b\\", pairs.join(":")).into_bytes()
    }
}

/// Sends reports, each only when it differs from the last.
#[derive(Debug, Default)]
pub(crate) struct Reporter {
    last: Option<Report>,
}

impl Reporter {
    /// # Errors
    ///
    /// Fails when the write to stderr fails.
    pub(crate) fn send(&mut self, report: Report) -> Result<()> {
        if self.last.as_ref() == Some(&report) {
            return Ok(());
        }
        let mut err = io::stderr();
        err.write_all(&report.sequence())
            .and_then(|()| err.flush())
            .context("report the app's status")?;
        self.last = Some(report);
        Ok(())
    }
}

/// `text` with control characters turned into spaces, cut to the protocol's
/// 2048 decoded bytes.
fn one_line(text: &str) -> String {
    let mut line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    while line.len() > 2048 {
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

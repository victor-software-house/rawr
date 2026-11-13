//! rawr's config: `$XDG_CONFIG_HOME/rawr/config.yaml`, or
//! `~/.config/rawr/config.yaml`.
//!
//! ```yaml
//! history:
//!   per_pane: 5
//! picker:
//!   rows: 3
//! restart:
//!   wait: 5m
//! drain:
//!   interrupt: 2m
//!   grace: 1m
//! ```
//!
//! Every key is optional, and an absent file holds the defaults. ctl-core's
//! `input` loader reads the file, so a bad value is reported with the file's
//! name and the line it sits on.

// The restart and drain keys, and the picker's rows, are read by later
// slices of the supervisor change. Once every key is read, the compiler
// reports this expectation as unfulfilled, which is the cue to remove it.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "a later slice of the supervisor reads these keys")
)]

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use ctl_core::input::Input;
use garde::Validate;
use serde::{Deserialize, Deserializer, de};

/// The seconds in each unit a duration may be written in.
const UNITS: [(char, u64); 3] = [('s', 1), ('m', 60), ('h', 3600)];

/// The whole config file.
#[derive(Debug, Clone, Default, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    #[garde(dive)]
    pub(crate) history: History,
    #[garde(dive)]
    pub(crate) picker: Picker,
    #[garde(skip)]
    pub(crate) restart: Restart,
    #[garde(skip)]
    pub(crate) drain: Drain,
}

/// How many sessions each pane remembers.
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct History {
    /// The most sessions one pane's index holds.
    #[garde(custom(at_least_one))]
    pub(crate) per_pane: usize,
}

/// How the picker shows the sessions.
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Picker {
    /// The most sessions the picker shows before `ctrl+o` expands the list.
    #[garde(custom(at_least_one))]
    pub(crate) rows: usize,
}

/// How long a restart waits.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Restart {
    /// How long a restart waits for the panes to settle.
    #[serde(deserialize_with = "positive_duration")]
    pub(crate) wait: Duration,
}

/// How long a drain waits on a session.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Drain {
    /// How long a busy session runs before its turn is cancelled.
    #[serde(deserialize_with = "positive_duration")]
    pub(crate) interrupt: Duration,
    /// How long a session with no rawr mod reads idle before it counts as
    /// settled.
    #[serde(deserialize_with = "positive_duration")]
    pub(crate) grace: Duration,
}

impl Default for History {
    fn default() -> Self {
        Self { per_pane: 5 }
    }
}

impl Default for Picker {
    fn default() -> Self {
        Self { rows: 3 }
    }
}

impl Default for Restart {
    fn default() -> Self {
        Self {
            wait: minutes(5),
        }
    }
}

impl Default for Drain {
    fn default() -> Self {
        Self {
            interrupt: minutes(2),
            grace: minutes(1),
        }
    }
}

/// A rule for a count that must leave room for one.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "garde passes the value and the validation context by reference"
)]
fn at_least_one(count: &usize, _: &()) -> garde::Result {
    if *count == 0 {
        return Err(garde::Error::new("must be at least 1"));
    }
    Ok(())
}

const fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

impl Config {
    /// Reads the config file in force, or holds the defaults when there is
    /// none.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read or holds a bad value; the message
    /// names the file and the line.
    pub(crate) fn load() -> Result<Self> {
        Self::load_from(&path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        match Input::open(path) {
            Ok(input) => Ok(input.load::<Self>()?),
            Err(error) if error.io_failure() == Some(ErrorKind::NotFound) => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

/// `$XDG_CONFIG_HOME/rawr/config.yaml`, or `~/.config/rawr/config.yaml`.
fn path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::home_dir()
            .context("no home directory")?
            .join(".config"),
    };
    Ok(base.join("rawr/config.yaml"))
}

/// Reads a length of time written as whole seconds, minutes or hours, such as
/// `90s`, `5m` or `2h`, and refuses zero and negative ones.
fn positive_duration<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_duration(&text).map_err(de::Error::custom)
}

fn parse_duration(text: &str) -> Result<Duration, String> {
    let unit = UNITS
        .iter()
        .find(|(suffix, _)| text.ends_with(*suffix))
        .ok_or_else(|| format!("`{text}` needs a unit: write it as 90s, 5m or 2h"))?;
    let digits = &text[..text.len() - 1];
    if digits.starts_with('-') {
        return Err(format!("`{text}` must be longer than zero"));
    }
    let count: u64 = digits
        .parse()
        .map_err(|error| format!("`{text}` is not a whole number of units: {error}"))?;
    let seconds = count
        .checked_mul(unit.1)
        .ok_or_else(|| format!("`{text}` is too long"))?;
    if seconds == 0 {
        return Err(format!("`{text}` must be longer than zero"));
    }
    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use ctl_core::indoc;

    use super::*;

    /// Loads `text` as the config file `config.yaml` in a folder of its own.
    fn load(name: &str, text: &str) -> Result<Config> {
        let dir = std::env::temp_dir().join(format!("rawr-config-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        std::fs::write(&path, text).unwrap();
        let loaded = Config::load_from(&path);
        std::fs::remove_dir_all(&dir).unwrap();
        loaded
    }

    fn seconds(count: u64) -> Duration {
        Duration::from_secs(count)
    }

    #[test]
    fn an_absent_file_holds_the_defaults() {
        let dir = std::env::temp_dir().join(format!("rawr-config-absent-{}", std::process::id()));
        let config = Config::load_from(&dir.join("config.yaml")).unwrap();
        assert_eq!(config.history.per_pane, 5);
        assert_eq!(config.picker.rows, 3);
        assert_eq!(config.restart.wait, seconds(300));
        assert_eq!(config.drain.interrupt, seconds(120));
        assert_eq!(config.drain.grace, seconds(60));
    }

    #[test]
    fn a_file_sets_only_the_keys_it_names() {
        let config = load(
            "partial",
            indoc! {"
                history:
                  per_pane: 8
                drain:
                  grace: 90s
            "},
        )
        .unwrap();
        assert_eq!(config.history.per_pane, 8);
        assert_eq!(config.picker.rows, 3);
        assert_eq!(config.restart.wait, seconds(300));
        assert_eq!(config.drain.interrupt, seconds(120));
        assert_eq!(config.drain.grace, seconds(90));
    }

    #[test]
    fn durations_take_seconds_minutes_and_hours() {
        let config = load(
            "units",
            indoc! {"
                restart:
                  wait: 2h
                drain:
                  interrupt: 45s
                  grace: 3m
            "},
        )
        .unwrap();
        assert_eq!(config.restart.wait, seconds(7200));
        assert_eq!(config.drain.interrupt, seconds(45));
        assert_eq!(config.drain.grace, seconds(180));
    }

    #[test]
    fn a_negative_per_pane_names_the_file_and_line() {
        let error = load(
            "negative",
            indoc! {"
                history:
                  per_pane: -1
            "},
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("config.yaml:2:"), "{error}");
    }

    #[test]
    fn a_zero_bound_names_the_file_and_line() {
        let error = load(
            "zero-per-pane",
            indoc! {"
                history:
                  per_pane: 0
            "},
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("config.yaml:2:"), "{error}");
        assert!(error.contains("history.per_pane"), "{error}");

        let error = load(
            "zero-rows",
            indoc! {"
                picker:
                  rows: 0
            "},
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("config.yaml:2:"), "{error}");
        assert!(error.contains("picker.rows"), "{error}");
    }

    #[test]
    fn a_bad_duration_names_the_file_and_line() {
        for (name, value) in [("none", "0s"), ("minus", "-5m"), ("unit", "5"), ("word", "soon")] {
            let error = load(
                name,
                &format!("restart:\n  wait: {value}\n"),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("config.yaml:2:"), "{value}: {error}");
        }
    }

    #[test]
    fn an_unknown_key_names_the_file_and_line() {
        let error = load(
            "unknown",
            indoc! {"
                history:
                  per_pane: 5
                  depth: 2
            "},
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("config.yaml:3:"), "{error}");
    }
}

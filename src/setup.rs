//! `setup claude`: add the `SessionStart` hook that runs `rawr record` to
//! Claude Code's user settings, once, and only when asked.

use std::fs::{self, OpenOptions, Permissions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};

/// The command the hook runs. Any hook command that contains it counts as
/// the hook, so a guarded form such as `command -v rawr && rawr record` is
/// not added twice.
pub(crate) const RECORD: &str = "rawr record";

/// What `setup claude` did to the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Change {
    /// The hook was added.
    Added,
    /// The hook is missing; a dry run left it so.
    WouldAdd,
    /// A hook already runs `rawr record`.
    Unchanged,
}

/// Whether [`claude`] writes the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Write,
    DryRun,
}

/// Claude Code's user settings file inside its configuration folder.
pub(crate) fn settings_path(claude: &Path) -> PathBuf {
    claude.join("settings.json")
}

/// Adds the hook to the settings at `path` unless one already runs
/// `rawr record`, keeping every other setting and its order. A missing file
/// is created.
///
/// # Errors
///
/// Fails when the file cannot be read or written, is not a JSON object, or
/// holds `hooks` or `hooks.SessionStart` in a shape Claude Code does not use.
pub(crate) fn claude(path: &Path, mode: Mode) -> Result<Change> {
    let mut settings = match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .with_context(|| format!("parse {}", path.display()))?,
        Err(error) if error.kind() == ErrorKind::NotFound => Value::Object(Map::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let Value::Object(root) = &mut settings else {
        bail!("{} does not hold a JSON object", path.display());
    };
    let starts = session_start(root).with_context(|| format!("read {}", path.display()))?;
    if starts.iter().any(runs_record) {
        return Ok(Change::Unchanged);
    }
    if mode == Mode::DryRun {
        return Ok(Change::WouldAdd);
    }
    starts.push(json!({
        "hooks": [{ "type": "command", "command": RECORD, "timeout": 10 }]
    }));
    write(path, &settings)?;
    Ok(Change::Added)
}

/// The `hooks.SessionStart` list, created empty when missing.
fn session_start(root: &mut Map<String, Value>) -> Result<&mut Vec<Value>> {
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(hooks) = hooks else {
        bail!("`hooks` is not an object");
    };
    let starts = hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Value::Array(starts) = starts else {
        bail!("`hooks.SessionStart` is not a list");
    };
    Ok(starts)
}

/// Whether a `SessionStart` entry holds a command hook that runs `rawr
/// record`.
fn runs_record(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(RECORD))
            })
        })
}

/// Writes `settings` beside the file `path` resolves to and renames it over
/// that file, so a reader never sees half a file. A symlink at `path`, as
/// dotfile managers make, stays a symlink, and the file keeps its
/// permissions.
fn write(path: &Path, settings: &Value) -> Result<()> {
    let target = resolve(path)?;
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let mut text = serde_json::to_string_pretty(settings).context("encode the settings")?;
    text.push('\n');
    let mode = fs::metadata(&target).map_or(0o600, |meta| meta.permissions().mode() & 0o7777);
    let staged = target.with_extension(format!("json.rawr-{}", process::id()));
    let written = stage(&staged, &text, mode).and_then(|()| {
        fs::rename(&staged, &target).with_context(|| format!("replace {}", target.display()))
    });
    if written.is_err() {
        fs::remove_file(&staged).ok();
    }
    written
}

/// The file the settings live in: `path`, or what a symlink at `path` points
/// to, even when that file does not exist yet.
fn resolve(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(target) => Ok(target),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let dir = path.parent().unwrap_or_else(|| Path::new(""));
            Ok(fs::read_link(path).map_or_else(|_| path.to_owned(), |link| dir.join(link)))
        }
        Err(error) => Err(error).with_context(|| format!("resolve {}", path.display())),
    }
}

/// Writes `text` to a new file at `staged` that only `mode` lets anyone read,
/// from its first byte: the settings can hold API keys.
fn stage(staged: &Path, text: &str, mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(staged)
        .with_context(|| format!("create {}", staged.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("write {}", staged.display()))?;
    file.set_permissions(Permissions::from_mode(mode))
        .with_context(|| format!("set the permissions of {}", staged.display()))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::os::unix::fs::symlink;

    use indoc::indoc;

    use super::*;

    fn fixture(name: &str, text: Option<&str>) -> PathBuf {
        let dir = env::temp_dir().join(format!("rawr-setup-{name}-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = settings_path(&dir);
        if let Some(text) = text {
            fs::write(&path, text).unwrap();
        }
        path
    }

    const SETTINGS: &str = indoc! {r#"
        {
          "theme": "dark",
          "hooks": {
            "SessionStart": [
              { "hooks": [{ "type": "command", "command": "echo hi" }] }
            ],
            "Stop": []
          },
          "model": "opus"
        }
    "#};

    #[test]
    fn the_hook_is_added_once_and_the_rest_is_kept_in_order() {
        let path = fixture("twice", Some(SETTINGS));
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Added);
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Unchanged);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches(RECORD).count(), 1);
        let theme = text.find("\"theme\"").unwrap();
        let hooks = text.find("\"hooks\"").unwrap();
        let model = text.find("\"model\"").unwrap();
        assert!(theme < hooks && hooks < model, "keys moved:\n{text}");
        let settings: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            settings["hooks"]["SessionStart"].as_array().unwrap().len(),
            2
        );
        assert_eq!(settings["hooks"]["Stop"], json!([]));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let path = fixture("dry", Some(SETTINGS));
        assert_eq!(claude(&path, Mode::DryRun).unwrap(), Change::WouldAdd);
        assert_eq!(fs::read_to_string(&path).unwrap(), SETTINGS);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_file_is_created_and_a_guarded_hook_counts() {
        let path = fixture("missing", None);
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Added);
        let guarded = indoc! {r#"
            {"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"command -v rawr >/dev/null && exec rawr record"}]}]}}
        "#};
        fs::write(&path, guarded).unwrap();
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Unchanged);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_symlinked_file_stays_a_symlink() {
        let path = fixture("link", None);
        let real = path.with_file_name("real.json");
        fs::write(&real, SETTINGS).unwrap();
        symlink(&real, &path).unwrap();
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Added);
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains(RECORD));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_dangling_symlink_stays_a_symlink() {
        let path = fixture("dangling", None);
        let real = path.with_file_name("not-yet.json");
        symlink("not-yet.json", &path).unwrap();
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Added);
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains(RECORD));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_private_file_stays_private_and_leaves_no_copy() {
        let path = fixture("mode", Some(SETTINGS));
        fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
        assert_eq!(claude(&path, Mode::Write).unwrap(), Change::Added);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["settings.json"]);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_hooks_value_claude_code_does_not_use_is_refused() {
        let path = fixture("shape", Some(r#"{"hooks": []}"#));
        assert!(claude(&path, Mode::Write).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"hooks": []}"#);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

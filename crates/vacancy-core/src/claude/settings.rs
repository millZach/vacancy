//! The user's Claude choices, kept in the app's data folder (the fixture's in fixture mode): whether Claude is on
//! (off until the user turns it on), whether privacy mode is on (on by default), and the model and reasoning new
//! questions ask for (Claude Code's own defaults until the user picks). Turning Claude off and on again keeps the
//! other choices.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The file the choices are kept in, inside the app's data folder.
pub const FILE: &str = "claude.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, rename = "ClaudeSettings")]
pub struct Settings {
    /// Claude is on. Off until the user turns it on, after the privacy notice.
    pub enabled: bool,
    /// Privacy mode: Claude gets names, sizes, counts and dates only. On by default.
    pub privacy: bool,
    /// The model new questions ask for: the account's default until the user picks one.
    pub model: Model,
    /// The reasoning new questions ask for: Claude Code's default until the user picks one.
    pub effort: Effort,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            privacy: true,
            model: Model::Default,
            effort: Effort::Default,
        }
    }
}

/// The model a call asks Claude Code for. Only these reach the command line: Claude Code's aliases, each the newest
/// model of its line, or none at all, so the account's default applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeModel")]
pub enum Model {
    /// No `--model`: the account's default.
    #[default]
    Default,
    Opus,
    Sonnet,
    Haiku,
    Fable,
}

impl Model {
    /// What `--model` is given, or `None` for the account's default (then there is no `--model`).
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Model::Default => None,
            Model::Opus => Some("opus"),
            Model::Sonnet => Some("sonnet"),
            Model::Haiku => Some("haiku"),
            Model::Fable => Some("fable"),
        }
    }
}

/// How hard Claude thinks for a call (Claude Code's `--effort`). Only these reach the command line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeEffort")]
pub enum Effort {
    /// No `--effort`: Claude Code's default.
    #[default]
    Default,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// What `--effort` is given, or `None` for Claude Code's default (then there is no `--effort`).
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Effort::Default => None,
            Effort::Low => Some("low"),
            Effort::Medium => Some("medium"),
            Effort::High => Some("high"),
            Effort::Xhigh => Some("xhigh"),
            Effort::Max => Some("max"),
        }
    }
}

/// Where the choices are kept for the app data folder `app_data`.
pub fn path(app_data: &Path) -> PathBuf {
    app_data.join(FILE)
}

/// The kept choices. A missing or damaged file reads as the defaults: Claude off, privacy mode on.
pub fn load(app_data: &Path) -> Settings {
    std::fs::read(path(app_data))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Keeps the choices, replacing the file whole (written beside it, then renamed over it).
pub fn save(app_data: &Path, settings: Settings) -> io::Result<()> {
    std::fs::create_dir_all(app_data)?;
    let target = path(app_data);
    let temp = app_data.join(format!("{FILE}.tmp"));
    let json = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, &target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_off_with_privacy_on_and_remembers_choices() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(dir.path()),
            Settings {
                enabled: false,
                privacy: true,
                model: Model::Default,
                effort: Effort::Default,
            }
        );
        std::fs::write(path(dir.path()), b"not json").unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        let chosen = Settings {
            enabled: true,
            privacy: false,
            model: Model::Sonnet,
            effort: Effort::Xhigh,
        };
        save(dir.path(), chosen).unwrap();
        assert_eq!(load(dir.path()), chosen);
        // Turning Claude off keeps the privacy choice.
        save(
            dir.path(),
            Settings {
                enabled: false,
                ..chosen
            },
        )
        .unwrap();
        assert!(!load(dir.path()).privacy);
        assert_eq!(load(dir.path()).model, Model::Sonnet);
        assert_eq!(load(dir.path()).effort, Effort::Xhigh);
        // A file from an older or newer Vacancy with fewer or other fields still reads.
        std::fs::write(path(dir.path()), br#"{"enabled":true}"#).unwrap();
        assert_eq!(
            load(dir.path()),
            Settings {
                enabled: true,
                privacy: true,
                ..Settings::default()
            }
        );
        // A model this Vacancy doesn't know is never passed on: the file reads as the defaults.
        std::fs::write(
            path(dir.path()),
            br#"{"enabled":true,"model":"opus --dangerously-skip-permissions"}"#,
        )
        .unwrap();
        assert_eq!(load(dir.path()), Settings::default());
    }

    #[test]
    fn only_fixed_values_reach_the_command_line() {
        assert_eq!(Model::Default.flag(), None);
        assert_eq!(
            [Model::Opus, Model::Sonnet, Model::Haiku, Model::Fable].map(Model::flag),
            [Some("opus"), Some("sonnet"), Some("haiku"), Some("fable")]
        );
        assert_eq!(Effort::Default.flag(), None);
        assert_eq!(
            [
                Effort::Low,
                Effort::Medium,
                Effort::High,
                Effort::Xhigh,
                Effort::Max
            ]
            .map(Effort::flag),
            [
                Some("low"),
                Some("medium"),
                Some("high"),
                Some("xhigh"),
                Some("max")
            ]
        );
        assert_eq!(serde_json::to_string(&Effort::Xhigh).unwrap(), r#""xhigh""#);
    }
}

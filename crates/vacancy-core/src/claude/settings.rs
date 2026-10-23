//! The user's two Claude choices, kept in the app's data folder (the fixture's in fixture mode): whether Claude is
//! on (off until the user turns it on) and whether privacy mode is on (on by default). Turning Claude off and on
//! again keeps the privacy choice.

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            privacy: true,
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
                privacy: true
            }
        );
        std::fs::write(path(dir.path()), b"not json").unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        let chosen = Settings {
            enabled: true,
            privacy: false,
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
        // A file from an older or newer Vacancy with fewer or other fields still reads.
        std::fs::write(path(dir.path()), br#"{"enabled":true}"#).unwrap();
        assert_eq!(
            load(dir.path()),
            Settings {
                enabled: true,
                privacy: true
            }
        );
    }
}

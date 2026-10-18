//! Read-only status returned to the app. Executable paths stay in the internal discovery result and never cross
//! the bridge. A detected file is not a verified provider, version, authentication state or isolation capability.

use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

use super::{AdvisorAvailability, AdvisorProvider, detect_in};
use crate::Platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AdvisorDiscovery {
    /// Inherited PATH and a few documented standard locations only. No login shell or configuration is read.
    Passive,
    /// Synthetic fixture data; makes no claim about what is installed on the host.
    Fixture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AdvisorProviderStatus {
    pub provider: AdvisorProvider,
    pub availability: AdvisorAvailability,
}

/// A read-only report, not a setting. The backend always returns disabled while no live isolation strategy is
/// supported. There are deliberately no executable paths, versions, credentials or account details here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AdvisorStatus {
    pub enabled: bool,
    pub discovery: AdvisorDiscovery,
    pub providers: Vec<AdvisorProviderStatus>,
}

impl AdvisorStatus {
    /// Accept synthetic statuses without consulting environment variables, filesystem or authentication.
    pub fn fixture(providers: Vec<AdvisorProviderStatus>) -> Self {
        Self {
            enabled: false,
            discovery: AdvisorDiscovery::Fixture,
            providers,
        }
    }

    /// Native fixture and refused-fixture modes never search the host, including when the fixture is invalid.
    pub fn fixture_disabled() -> Self {
        Self::fixture(
            [AdvisorProvider::ClaudeCode, AdvisorProvider::Codex]
                .into_iter()
                .map(|provider| AdvisorProviderStatus {
                    provider,
                    availability: AdvisorAvailability::NotChecked,
                })
                .collect(),
        )
    }
}

/// Only metadata checks on explicitly supplied directories. This has no process launch or authentication path.
/// Absence means not detected in this limited search, never a definitive assertion that nothing is installed.
pub fn passive_status_in(directories: &[PathBuf]) -> AdvisorStatus {
    AdvisorStatus {
        enabled: false,
        discovery: AdvisorDiscovery::Passive,
        providers: detect_in(directories)
            .into_iter()
            .map(|found| AdvisorProviderStatus {
                provider: found.provider,
                availability: found.availability,
            })
            .collect(),
    }
}

/// Inherited PATH and documented defaults, checked without a shell, `--version`, user configuration or account
/// lookup. Nonstandard shell-installed locations may remain undiscovered. Only real mode calls this function.
pub fn passive_status() -> AdvisorStatus {
    let path = std::env::var_os("PATH");
    let paths = path
        .as_deref()
        .map(std::env::split_paths)
        .into_iter()
        .flatten();
    let platform = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::MacOs
    };
    let app_data = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        None
    };
    passive_status_in(&search_directories(
        platform,
        paths,
        std::env::home_dir().as_deref(),
        app_data.as_deref(),
    ))
}

fn search_directories(
    platform: Platform,
    inherited: impl Iterator<Item = PathBuf>,
    home: Option<&Path>,
    app_data: Option<&Path>,
) -> Vec<PathBuf> {
    // Claude native: https://support.claude.com/en/articles/14554922-claude-code-user-faq
    // npm Windows prefix: https://docs.npmjs.com/configuring-npm/folders.html/
    // Homebrew prefixes: https://docs.brew.sh/Installation and https://docs.brew.sh/Manpage
    let mut locations: Vec<_> = inherited.filter(|p| p.is_absolute()).collect();
    if let Some(home) = home.filter(|p| p.is_absolute()) {
        locations.push(home.join(".local").join("bin"));
    }
    match platform {
        Platform::Windows => {
            if let Some(app_data) = app_data.filter(|p| p.is_absolute()) {
                locations.push(app_data.join("npm"));
            }
        }
        Platform::MacOs => {
            locations.push(PathBuf::from("/opt/homebrew/bin"));
            locations.push(PathBuf::from("/usr/local/bin"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    locations.retain(|p| seen.insert(p.clone()));
    locations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_locations_are_explicit_and_exclude_relative_path_entries() {
        let home = PathBuf::from(if cfg!(windows) {
            r"D:\Fixture\Users\alex"
        } else {
            "/fixture/Users/alex"
        });
        let app_data = home.join("Roaming");
        let inherited = home.join("tools");
        for platform in [Platform::Windows, Platform::MacOs] {
            let result = search_directories(
                platform,
                [PathBuf::from("."), inherited.clone(), inherited.clone()].into_iter(),
                Some(&home),
                Some(&app_data),
            );
            assert!(!result.contains(&PathBuf::from(".")));
            assert_eq!(result.iter().filter(|p| *p == &inherited).count(), 1);
            assert!(result.contains(&home.join(".local").join("bin")));
            match platform {
                Platform::Windows => assert!(result.contains(&app_data.join("npm"))),
                Platform::MacOs => {
                    assert!(result.contains(&PathBuf::from("/opt/homebrew/bin")));
                    assert!(result.contains(&PathBuf::from("/usr/local/bin")));
                }
            }
        }
    }
}

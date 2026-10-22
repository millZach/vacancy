//! Finding the user's Claude Code, including where a GUI app's PATH doesn't reach. Only an executable file is
//! taken; on Windows only `claude.exe`, never a `.cmd` or `.bat` shim (those run through `cmd.exe`).
//!
//! - Windows: `%USERPROFILE%\.local\bin\claude.exe` (the native installer's), then the PATH.
//! - macOS: `~/.local/bin/claude` (native), Homebrew's `/opt/homebrew/bin` and `/usr/local/bin` (also npm's default
//!   global prefix), npm's common user prefix `~/.npm-global/bin`, the app's PATH, and last the login shell's
//!   PATH (`$SHELL -lc 'command -v claude'`), since an app opened from the Finder doesn't get the shell's PATH.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::CancelToken;
use crate::Platform;
use crate::tools::{Limits, Status, run_contained};

/// The places to look, in order, before any login shell.
pub fn candidates(platform: Platform, home: Option<&Path>, path: Option<&OsStr>) -> Vec<PathBuf> {
    let name = match platform {
        Platform::Windows => "claude.exe",
        Platform::MacOs => "claude",
    };
    let home = home.filter(|h| h.is_absolute());
    let mut out = Vec::new();
    if let Some(home) = home {
        out.push(home.join(".local").join("bin").join(name));
    }
    if platform == Platform::MacOs {
        out.push(PathBuf::from("/opt/homebrew/bin/claude"));
        out.push(PathBuf::from("/usr/local/bin/claude"));
        if let Some(home) = home {
            out.push(home.join(".npm-global").join("bin").join(name));
        }
    }
    if let Some(path) = path {
        out.extend(
            std::env::split_paths(path)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join(name)),
        );
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// Whether `path` is a file this platform would run as a program (on Windows, an `.exe`).
fn runnable(path: &Path) -> bool {
    if cfg!(windows)
        && !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The first runnable candidate, else what `login_shell` finds (macOS only), if that is runnable and absolute.
pub fn locate_in(
    platform: Platform,
    home: Option<&Path>,
    path: Option<&OsStr>,
    login_shell: &dyn Fn() -> Option<PathBuf>,
) -> Option<PathBuf> {
    candidates(platform, home, path)
        .into_iter()
        .find(|p| runnable(p))
        .or_else(|| {
            if platform != Platform::MacOs {
                return None;
            }
            login_shell().filter(|p| p.is_absolute() && runnable(p))
        })
}

/// Where this computer's Claude Code is, if it can be found.
pub fn locate() -> Option<PathBuf> {
    let platform = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::MacOs
    };
    let home = std::env::home_dir();
    let path = std::env::var_os("PATH");
    locate_in(platform, home.as_deref(), path.as_deref(), &login_shell)
}

/// What the user's login shell says `claude` is: `$SHELL -lc 'command -v claude'`, bounded to a few seconds,
/// with no input. Only an absolute path on the first line counts.
fn login_shell() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|s| s.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/bin/zsh"));
    let mut command = Command::new(shell);
    command
        .args(["-lc", "command -v claude"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_contained(
        command,
        Limits {
            timeout: Duration::from_secs(5),
            output_bytes: 16 * 1024,
        },
        &CancelToken::new(),
        None,
        &mut |_| {},
    );
    if output.status != Status::Success {
        return None;
    }
    let line = output
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    Some(PathBuf::from(line)).filter(|p| p.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_in_the_native_install_then_the_path_and_never_at_cmd_shims() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let tools = dir.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let path = std::env::join_paths([PathBuf::from("relative"), tools.clone()]).unwrap();
        let win = candidates(Platform::Windows, Some(&home), Some(&path));
        assert_eq!(
            win,
            vec![
                home.join(".local").join("bin").join("claude.exe"),
                tools.join("claude.exe")
            ],
            "relative PATH entries are skipped"
        );
        let mac = candidates(Platform::MacOs, Some(&home), Some(&path));
        assert_eq!(
            mac,
            vec![
                home.join(".local").join("bin").join("claude"),
                PathBuf::from("/opt/homebrew/bin/claude"),
                PathBuf::from("/usr/local/bin/claude"),
                home.join(".npm-global").join("bin").join("claude"),
                tools.join("claude"),
            ]
        );
        // Only a .cmd shim on the PATH: not found, and the login shell is never asked on Windows.
        std::fs::write(tools.join("claude.cmd"), b"@echo off").unwrap();
        assert_eq!(
            locate_in(Platform::Windows, Some(&home), Some(&path), &|| panic!(
                "no login shell on Windows"
            )),
            None
        );
    }

    #[test]
    fn finds_an_executable_and_falls_back_to_the_login_shell_on_macos() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bin = home.join(".local").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        let exe = bin.join(name);
        std::fs::write(&exe, b"fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let platform = if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::MacOs
        };
        assert_eq!(
            locate_in(platform, Some(&home), None, &|| None),
            Some(exe.clone())
        );
        std::fs::remove_file(&exe).unwrap();
        let elsewhere = dir.path().join(name);
        std::fs::write(&elsewhere, b"fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let shell = elsewhere.clone();
        let found = locate_in(Platform::MacOs, Some(&home), None, &move || {
            Some(shell.clone())
        });
        assert_eq!(found, Some(elsewhere));
        assert_eq!(
            locate_in(Platform::MacOs, Some(&home), None, &|| Some(PathBuf::from(
                "relative/claude"
            ))),
            None
        );
    }
}

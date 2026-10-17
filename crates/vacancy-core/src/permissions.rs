//! macOS Full Disk Access. Without it, macOS hides Mail, Messages, Safari and other apps' data from Vacancy: the
//! scan still runs, those folders count as unreadable, and their space shows as the part Vacancy can't see. There
//! is no API to ask whether an app has it, so Vacancy tries to list folders that are always protected.
//! Windows has no such setting.

use std::io;
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

/// Whether Vacancy may see everything on the disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FullDiskAccess {
    /// It has Full Disk Access.
    Granted,
    /// macOS refuses it the protected folders: Full Disk Access is off.
    Missing,
    /// None of the protected folders exist, so Vacancy can't tell.
    Unknown,
    /// This platform has no such permission.
    NotApplicable,
}

/// Folders under the home folder that macOS only lets apps with Full Disk Access list.
const PROTECTED: [&str; 4] = [
    "Library/Application Support/com.apple.TCC",
    "Library/Safari",
    "Library/Mail",
    "Library/Messages",
];

/// Whether Vacancy has Full Disk Access. Read-only: it only tries to list folders.
pub fn full_disk_access() -> FullDiskAccess {
    if !cfg!(target_os = "macos") {
        return FullDiskAccess::NotApplicable;
    }
    match std::env::var_os("HOME") {
        Some(home) => probe(Path::new(&home)),
        None => FullDiskAccess::Unknown,
    }
}

/// The check behind [`full_disk_access`], against a given home folder: the first protected folder that exists
/// decides.
pub fn probe(home: &Path) -> FullDiskAccess {
    for rel in PROTECTED {
        match std::fs::read_dir(
            rel.split('/')
                .fold(home.to_path_buf(), |p, part| p.join(part)),
        ) {
            Ok(_) => return FullDiskAccess::Granted,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                return FullDiskAccess::Missing;
            }
            Err(_) => continue,
        }
    }
    FullDiskAccess::Unknown
}

/// The System Settings pane where people turn Full Disk Access on.
pub const FULL_DISK_ACCESS_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// Opens System Settings at the Full Disk Access list. macOS only.
pub fn open_full_disk_access_settings() -> io::Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Full Disk Access is a macOS setting",
        ));
    }
    let status = std::process::Command::new("/usr/bin/open")
        .arg(FULL_DISK_ACCESS_SETTINGS)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("open exited with {status}")))
    }
}

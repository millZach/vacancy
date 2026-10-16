//! Volumes: which drive a path lives on, what the OS calls it, and how full it is.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

#[cfg(target_os = "macos")]
#[path = "volume/macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "volume/windows.rs"]
mod imp;
#[cfg(not(any(windows, target_os = "macos")))]
#[path = "volume/unsupported.rs"]
mod imp;

/// A mounted volume, as the scan screen's plate shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VolumeInfo {
    /// The name the OS shows for the volume: `Local Disk (C:)` on Windows, `Macintosh HD` on macOS.
    pub name: String,
    /// Where the volume is mounted: `C:\` on Windows, `/` on macOS.
    pub mount_point: String,
    /// Capacity in bytes.
    #[ts(type = "number")]
    pub total_bytes: u64,
    /// Bytes in use: capacity minus free space.
    #[ts(type = "number")]
    pub used_bytes: u64,
}

/// Why volume information could not be read.
#[derive(Debug, thiserror::Error)]
pub enum VolumeError {
    /// The OS refused, or the path does not exist.
    #[error("could not read the volume for {path}: {source}")]
    Io {
        /// The path that was asked about.
        path: PathBuf,
        /// What the OS said.
        #[source]
        source: io::Error,
    },
    /// Vacancy supports Windows and macOS only.
    #[error("volume information is only available on Windows and macOS")]
    Unsupported,
}

/// The volume that `path` lives on. Read-only: it asks the OS about the volume and never lists or opens files.
///
/// `path` must exist.
pub fn volume_info(path: &Path) -> Result<VolumeInfo, VolumeError> {
    // Each OS answers for some missing paths and not others; refuse them all the same way.
    std::fs::symlink_metadata(path).map_err(|source| VolumeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    imp::volume_info(path)
}

/// The volume the OS runs from: the Windows drive (usually `C:`), or `/` on macOS.
pub fn system_volume() -> Result<VolumeInfo, VolumeError> {
    imp::system_volume()
}

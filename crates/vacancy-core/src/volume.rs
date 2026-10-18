//! Volumes: which drives Vacancy offers to scan, which drive a path lives on, what the OS calls it, what kind of
//! drive it is, and how full it is.
//!
//! Each OS's module only gathers what the OS says about its volumes. Which of them to offer and how to describe
//! them is decided in [`offer`], from plain data, so both platforms' rules are unit-tested on both CI runners.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod offer;

#[cfg(target_os = "macos")]
#[path = "volume/macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "volume/windows.rs"]
mod imp;
#[cfg(not(any(windows, target_os = "macos")))]
#[path = "volume/unsupported.rs"]
mod imp;

/// A drive Vacancy can scan, as the drive picker and the map show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VolumeInfo {
    /// The name the OS shows for the volume: `Local Disk (C:)` on Windows, `Macintosh HD` on macOS.
    pub name: String,
    /// Where the volume is mounted: `C:\` on Windows, `/` on macOS.
    pub mount_point: String,
    /// Tells this volume apart from another mounted at the same place later (a USB stick swapped for another): the
    /// volume's serial number on Windows, its file system id on macOS. Empty when the OS could not supply it.
    pub id: String,
    /// Built in, on USB, or another external drive.
    pub kind: DriveKind,
    /// The OS runs from this drive.
    pub system: bool,
    /// Capacity in bytes.
    #[ts(type = "number")]
    pub total_bytes: u64,
    /// Bytes in use: capacity minus free space.
    #[ts(type = "number")]
    pub used_bytes: u64,
    /// Bytes free.
    #[ts(type = "number")]
    pub free_bytes: u64,
}

/// How a drive is attached to the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DriveKind {
    /// Built into the computer.
    Internal,
    /// A USB stick, card reader or USB disk (Windows tells these apart).
    Usb,
    /// Any other drive that isn't built in: on macOS every external volume; on Windows SD cards and FireWire.
    External,
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
    /// The OS would not list its volumes.
    #[error("could not list the drives: {0}")]
    List(#[source] io::Error),
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

/// The drives to offer for scanning: the system drive first, then the others by mount point.
///
/// Internal drives plus USB and external ones. Network shares, optical and RAM drives, cloud services' virtual
/// drives, read-only volumes, and hidden, system-only or recovery volumes are left out (Windows volumes without a
/// drive letter; macOS's Preboot, VM, Recovery and Update volumes, and every volume under `/System/Volumes`, whose
/// Data volume is scanned through `/`). On macOS disk images are left out too; on Windows a mounted ISO is a CD
/// drive, and a mounted VHD (a Dev Drive is one) counts as internal. The system drive is always listed.
///
/// Fast and read-only, like [`volume_info`]: it asks the OS about each volume and never lists or opens files.
pub fn list_volumes() -> Result<Vec<VolumeInfo>, VolumeError> {
    let drives = imp::list_volumes()?;
    if drives.first().is_some_and(|d| d.system) {
        return Ok(drives);
    }
    // The OS should always list the drive it runs from; if it didn't, ask for that drive directly.
    match system_volume() {
        Ok(system) => Ok(offer::system_first(system, drives)),
        Err(e) if drives.is_empty() => Err(e),
        Err(_) => Ok(drives),
    }
}

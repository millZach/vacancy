//! Platforms other than Windows and macOS. Vacancy does not support them (Linux is out of scope for v1).

use std::path::Path;

use super::{VolumeError, VolumeInfo};

pub(super) fn volume_info(_path: &Path) -> Result<VolumeInfo, VolumeError> {
    Err(VolumeError::Unsupported)
}

pub(super) fn file_system_of(_path: &Path) -> String {
    String::new()
}

pub(super) fn system_volume() -> Result<VolumeInfo, VolumeError> {
    Err(VolumeError::Unsupported)
}

pub(super) fn list_volumes() -> Result<Vec<VolumeInfo>, VolumeError> {
    Err(VolumeError::Unsupported)
}

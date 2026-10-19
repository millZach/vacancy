//! Which drives Vacancy offers to scan, and how it describes each: name, kind, whether the OS runs from it, sizes.
//!
//! These are pure functions over what the OS reported about its volumes, gathered by `volume/windows.rs` and
//! `volume/macos.rs`. Both platforms' rules compile everywhere, so their tests (made-up volumes) run on both CI
//! runners.

use super::VolumeInfo;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod macos;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod windows;

/// The system drive first, then the rest by mount point, each mount point once.
fn ordered(mut drives: Vec<VolumeInfo>) -> Vec<VolumeInfo> {
    drives.sort_by_cached_key(|d| (!d.system, d.mount_point.to_lowercase()));
    drives.dedup_by(|a, b| a.mount_point == b.mount_point);
    drives
}

/// `system` first, then `others` without it, by mount point.
pub(crate) fn system_first(system: VolumeInfo, others: Vec<VolumeInfo>) -> Vec<VolumeInfo> {
    let mut drives: Vec<VolumeInfo> = others
        .into_iter()
        .filter(|d| d.mount_point != system.mount_point)
        .map(|d| VolumeInfo { system: false, ..d })
        .collect();
    drives.push(system);
    ordered(drives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::DriveKind;

    fn drive(mount_point: &str, system: bool) -> VolumeInfo {
        VolumeInfo {
            name: mount_point.to_owned(),
            mount_point: mount_point.to_owned(),
            id: mount_point.to_owned(),
            kind: DriveKind::Internal,
            system,
            total_bytes: 100,
            used_bytes: 40,
            free_bytes: 60,
            file_system: String::new(),
        }
    }

    #[test]
    fn puts_the_system_drive_first_and_the_rest_in_order() {
        let drives = ordered(vec![
            drive("E:\\", false),
            drive("d:\\", false),
            drive("C:\\", true),
            drive("E:\\", false),
        ]);
        let mounts: Vec<_> = drives.iter().map(|d| d.mount_point.as_str()).collect();
        assert_eq!(mounts, ["C:\\", "d:\\", "E:\\"]);
    }

    #[test]
    fn adds_a_system_drive_the_listing_missed() {
        let drives = system_first(
            drive("C:\\", true),
            vec![drive("D:\\", false), drive("C:\\", false)],
        );
        let mounts: Vec<_> = drives
            .iter()
            .map(|d| (d.mount_point.as_str(), d.system))
            .collect();
        assert_eq!(mounts, [("C:\\", true), ("D:\\", false)]);
    }
}

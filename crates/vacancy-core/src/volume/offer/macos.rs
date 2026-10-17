//! macOS's rules: which mounted volumes to offer, and what kind of drive each is.

use crate::volume::{DriveKind, VolumeInfo};

/// What macOS says about one mounted file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacMount {
    /// Where it is mounted: `/`, `/Volumes/Samsung T7`.
    pub(crate) mount_point: String,
    /// The volume's name as Finder shows it.
    pub(crate) name: String,
    /// The file system id `statfs` reports, which a volume keeps while it stays mounted.
    pub(crate) fsid: [i32; 2],
    /// The file system's type: `apfs`, `exfat`, `smbfs`.
    pub(crate) file_system: String,
    /// Mounted from a local device (`MNT_LOCAL`), not over the network.
    pub(crate) local: bool,
    /// Finder may show it (no `MNT_DONTBROWSE`). The system's own volumes under `/System/Volumes` are hidden.
    pub(crate) browsable: bool,
    /// Mounted read-only (`MNT_RDONLY`): disk images, NTFS drives, the sealed system volume at `/`.
    pub(crate) read_only: bool,
    /// Disk Arbitration's word on whether the device is built in, when it gives one.
    pub(crate) internal: Option<bool>,
    /// Disk Arbitration's word on how the device is attached ("USB", "Thunderbolt", "Disk Image"), when it gives one.
    pub(crate) protocol: Option<String>,
    /// The APFS volume's role ("Preboot", "Recovery", "VM", "Data"), from its IOMedia's `Role` property. `None` for a
    /// volume without one (most volumes people make) or that isn't APFS.
    pub(crate) role: Option<String>,
    /// Capacity in bytes.
    pub(crate) total_bytes: u64,
    /// Free bytes.
    pub(crate) free_bytes: u64,
}

/// File systems of local disks. Anything else (network shares, optical discs, cloud services' FUSE mounts) is not
/// offered.
const LOCAL_FILE_SYSTEMS: [&str; 6] = ["apfs", "hfs", "exfat", "msdos", "ntfs", "ufs"];
/// Where Time Machine mounts its snapshots and backups.
const TIME_MACHINE: [&str; 2] = [
    "/Volumes/com.apple.TimeMachine.localsnapshots",
    "/Volumes/.timemachine",
];
/// APFS roles of volumes that hold people's files. Every other role (Preboot, Recovery, VM, Update, System, Hardware,
/// xART, Backup and any newer one) marks a volume macOS keeps for itself.
const USER_ROLES: [&str; 2] = ["Data", "User"];

/// Whether a mount might be offered, so is worth asking more about: local, with a disk file system. Asking a stale
/// network share anything can block for a long time.
pub(crate) fn worth_asking(local: bool, file_system: &str) -> bool {
    local && LOCAL_FILE_SYSTEMS.contains(&file_system)
}

/// The drives to offer from the mounted file systems: `/` (the startup disk) first, then the rest by mount point.
pub(crate) fn drives(found: &[MacMount]) -> Vec<VolumeInfo> {
    super::ordered(found.iter().filter(|m| offers(m)).map(describe).collect())
}

/// Whether to offer a volume: the startup disk at `/`, or a local, writable volume Finder shows, with a disk file
/// system, that isn't a disk image, a Time Machine mount or one of the system's own volumes.
///
/// The system's own volumes are told by what they are, never by name (an external drive may well be called "VM"):
/// they are mounted under `/System/Volumes`, or hidden from Finder (`MNT_DONTBROWSE`), or carry an APFS role such as
/// Preboot or Recovery, which also catches one mounted by hand under `/Volumes`.
///
/// `/` stands for the startup disk as a whole: the scanner reaches the Data volume (mounted at
/// `/System/Volumes/Data`) through it, so that mount and every other one under `/System/Volumes` is left out.
fn offers(m: &MacMount) -> bool {
    if m.mount_point == "/" {
        return true;
    }
    if !worth_asking(m.local, &m.file_system) || !m.browsable || m.read_only {
        return false;
    }
    if m.mount_point.starts_with("/System/Volumes/")
        || TIME_MACHINE
            .iter()
            .any(|tm| m.mount_point == *tm || m.mount_point.starts_with(&format!("{tm}/")))
    {
        return false;
    }
    if m.role.as_deref().is_some_and(|r| !USER_ROLES.contains(&r)) {
        return false;
    }
    m.protocol.as_deref() != Some("Disk Image")
}

/// How Vacancy shows a volume: its name, its kind, and its sizes. `/` is the system drive and always internal;
/// anything Disk Arbitration doesn't call internal is external, as in Finder.
pub(crate) fn describe(m: &MacMount) -> VolumeInfo {
    let system = m.mount_point == "/";
    let free = m.free_bytes.min(m.total_bytes);
    VolumeInfo {
        name: m.name.clone(),
        mount_point: m.mount_point.clone(),
        id: format!("{:x}-{:x}", m.fsid[0], m.fsid[1]),
        kind: if system || m.internal == Some(true) {
            DriveKind::Internal
        } else {
            DriveKind::External
        },
        system,
        total_bytes: m.total_bytes,
        used_bytes: m.total_bytes - free,
        free_bytes: free,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    /// An external APFS volume Finder shows; each test changes what it is about.
    fn mounted(mount_point: &str, name: &str) -> MacMount {
        MacMount {
            mount_point: mount_point.into(),
            name: name.into(),
            fsid: [mount_point.len() as i32, 0x1a],
            file_system: "apfs".into(),
            local: true,
            browsable: true,
            read_only: false,
            internal: Some(false),
            protocol: Some("USB".into()),
            role: None,
            total_bytes: 1000 * GB,
            free_bytes: 620 * GB,
        }
    }

    /// One of macOS's own volumes on the internal disk, with its APFS role, as mounted at startup: hidden from Finder.
    fn system(mount_point: &str, name: &str, role: &str) -> MacMount {
        MacMount {
            browsable: false,
            internal: Some(true),
            protocol: Some("Apple Fabric".into()),
            role: Some(role.into()),
            ..mounted(mount_point, name)
        }
    }

    /// A MacBook with an external SSD, plus the mounts macOS has that Vacancy must leave out.
    fn a_busy_mac() -> Vec<MacMount> {
        vec![
            MacMount {
                browsable: true,
                read_only: true,
                total_bytes: 494 * GB,
                free_bytes: 34 * GB,
                ..system("/", "Macintosh HD", "System")
            },
            mounted("/Volumes/Samsung T7", "Samsung T7"),
            MacMount {
                file_system: "exfat".into(),
                internal: None,
                protocol: None,
                ..mounted("/Volumes/CAMERA", "CAMERA")
            },
            // A second volume someone added to the internal disk.
            MacMount {
                internal: Some(true),
                protocol: Some("Apple Fabric".into()),
                ..mounted("/Volumes/Work", "Work")
            },
            // An external drive someone named "VM".
            mounted("/Volumes/VM", "VM"),
            // Left out from here on.
            system("/System/Volumes/Data", "Macintosh HD - Data", "Data"),
            system("/System/Volumes/VM", "VM", "VM"),
            system("/System/Volumes/Preboot", "Preboot", "Preboot"),
            system("/System/Volumes/Update", "Update", "Update"),
            system("/System/Volumes/xarts", "xarts", "xART"),
            system("/System/Volumes/iSCPreboot", "iSCPreboot", "Preboot"),
            system("/System/Volumes/Hardware", "Hardware", "Hardware"),
            // The Recovery volume, mounted by hand, shows in Finder.
            MacMount {
                browsable: true,
                ..system("/Volumes/Recovery", "Recovery", "Recovery")
            },
            MacMount {
                browsable: true,
                ..system("/System/Volumes/Update/mnt1", "Macintosh HD", "System")
            },
            MacMount {
                file_system: "devfs".into(),
                role: None,
                ..system("/dev", "devfs", "")
            },
            MacMount {
                local: false,
                file_system: "smbfs".into(),
                internal: None,
                protocol: None,
                ..mounted("/Volumes/Projects", "Projects")
            },
            MacMount {
                file_system: "cd9660".into(),
                read_only: true,
                ..mounted("/Volumes/Holiday DVD", "Holiday DVD")
            },
            MacMount {
                file_system: "hfs".into(),
                read_only: true,
                protocol: Some("Disk Image".into()),
                ..mounted("/Volumes/Firefox", "Firefox")
            },
            MacMount {
                protocol: Some("Disk Image".into()),
                ..mounted("/Volumes/Scratch", "Scratch")
            },
            MacMount {
                file_system: "ntfs".into(),
                read_only: true,
                ..mounted("/Volumes/WINDOWS", "WINDOWS")
            },
            MacMount {
                file_system: "macfuse".into(),
                ..mounted("/Volumes/GoogleDrive", "Google Drive")
            },
            mounted(
                "/Volumes/com.apple.TimeMachine.localsnapshots/Backups.backupdb/Mac/2026-10-16-101010/Data",
                "Data",
            ),
            mounted("/Volumes/.timemachine/ABCD/2026-10-16.backup", "Backups"),
        ]
    }

    #[test]
    fn offers_the_startup_disk_first_then_internal_and_external_volumes() {
        let drives = drives(&a_busy_mac());
        let offered: Vec<_> = drives
            .iter()
            .map(|d| (d.mount_point.as_str(), d.name.as_str(), d.kind, d.system))
            .collect();
        assert_eq!(
            offered,
            [
                ("/", "Macintosh HD", DriveKind::Internal, true),
                ("/Volumes/CAMERA", "CAMERA", DriveKind::External, false),
                (
                    "/Volumes/Samsung T7",
                    "Samsung T7",
                    DriveKind::External,
                    false
                ),
                ("/Volumes/VM", "VM", DriveKind::External, false),
                ("/Volumes/Work", "Work", DriveKind::Internal, false),
            ]
        );
    }

    #[test]
    fn reports_capacity_used_and_free() {
        let drives = drives(&a_busy_mac());
        let hd = &drives[0];
        assert_eq!(hd.id, "1-1a", "the file system id");
        assert_eq!(
            (hd.total_bytes, hd.used_bytes, hd.free_bytes),
            (494 * GB, 460 * GB, 34 * GB)
        );
        let t7 = &drives[2];
        assert_eq!(
            (t7.total_bytes, t7.used_bytes, t7.free_bytes),
            (1000 * GB, 380 * GB, 620 * GB)
        );
    }

    /// Each differs from an offered external volume in one way only, so each reason is tested on its own.
    #[test]
    fn leaves_out_system_recovery_network_optical_images_read_only_and_time_machine() {
        let base = mounted("/Volumes/T7", "T7");
        assert!(offers(&base));
        let left_out = [
            MacMount {
                browsable: false,
                ..base.clone()
            },
            mounted("/System/Volumes/Data", "Macintosh HD - Data"),
            MacMount {
                role: Some("Recovery".into()),
                ..base.clone()
            },
            MacMount {
                role: Some("Backup".into()),
                ..base.clone()
            },
            MacMount {
                local: false,
                ..base.clone()
            },
            MacMount {
                file_system: "cd9660".into(),
                ..base.clone()
            },
            MacMount {
                file_system: "smbfs".into(),
                ..base.clone()
            },
            MacMount {
                protocol: Some("Disk Image".into()),
                ..base.clone()
            },
            MacMount {
                read_only: true,
                ..base.clone()
            },
            mounted("/Volumes/com.apple.TimeMachine.localsnapshots", "Snapshots"),
            mounted("/Volumes/.timemachine/ABCD/2026-10-16.backup", "Backups"),
        ];
        for m in left_out {
            assert!(!offers(&m), "{m:?} is left out");
        }
    }

    /// A volume is macOS's own because of where it is mounted, how, or its role; never because of its name.
    #[test]
    fn tells_the_systems_own_volumes_by_what_they_are_not_their_names() {
        for name in ["VM", "Preboot", "Recovery", "Update"] {
            let drive = mounted(&format!("/Volumes/{name}"), name);
            assert!(offers(&drive), "an external drive named {name} is offered");
        }

        // The real ones, as macOS mounts them at startup, and the Recovery volume mounted by hand.
        let real = [
            system("/System/Volumes/Preboot", "Preboot", "Preboot"),
            system("/System/Volumes/VM", "VM", "VM"),
            system("/System/Volumes/Update", "Update", "Update"),
            MacMount {
                browsable: true,
                ..system("/Volumes/Recovery", "Recovery", "Recovery")
            },
        ];
        for m in &real {
            assert!(!offers(m), "{m:?} is left out");
        }
        // Each sign is enough on its own.
        let only_mounted_there = MacMount {
            browsable: true,
            role: None,
            ..system("/System/Volumes/VM", "VM", "VM")
        };
        let only_hidden = MacMount {
            role: None,
            ..system("/Volumes/Preboot", "Preboot", "Preboot")
        };
        let only_its_role = MacMount {
            browsable: true,
            ..system("/Volumes/Update", "Update", "Update")
        };
        for m in [only_mounted_there, only_hidden, only_its_role] {
            assert!(!offers(&m), "{m:?} is left out");
        }

        // Another Mac's Data volume, plugged in, holds someone's files.
        let other_mac = MacMount {
            role: Some("Data".into()),
            ..mounted("/Volumes/Old Mac - Data", "Old Mac - Data")
        };
        assert!(offers(&other_mac));
    }

    #[test]
    fn an_external_volume_disk_arbitration_says_nothing_about_is_external() {
        let unknown = MacMount {
            internal: None,
            protocol: None,
            ..mounted("/Volumes/USB", "USB")
        };
        assert_eq!(describe(&unknown).kind, DriveKind::External);
    }
}

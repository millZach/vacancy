//! Windows' rules: which drive letters to offer, and how Explorer would name them.

use crate::volume::{DriveKind, VolumeInfo};

/// `GetDriveTypeW`'s answers that matter here (`windows-sys`'s `DRIVE_*`; `volume/windows.rs` checks they match).
pub(crate) mod drive_type {
    /// A disk with removable media: USB sticks and card readers.
    pub(crate) const REMOVABLE: u32 = 2;
    /// A disk that stays put: internal drives, and most USB hard drives and SSDs.
    pub(crate) const FIXED: u32 = 3;
    /// A network share.
    pub(crate) const REMOTE: u32 = 4;
    /// A CD, DVD or Blu-ray drive.
    pub(crate) const CDROM: u32 = 5;
    /// A RAM disk.
    pub(crate) const RAMDISK: u32 = 6;
}

/// How a disk is attached (`STORAGE_BUS_TYPE`), for the values that matter here.
pub(crate) mod bus {
    /// FireWire.
    pub(crate) const IEEE1394: u32 = 0x4;
    /// USB, including USB hard drives that Windows calls fixed.
    pub(crate) const USB: u32 = 0x7;
    /// An SD card.
    pub(crate) const SD: u32 = 0xC;
    /// An MMC card.
    pub(crate) const MMC: u32 = 0xD;
}

/// `FILE_READ_ONLY_VOLUME`, from the file system flags `GetVolumeInformationW` reports.
pub(crate) const READ_ONLY_VOLUME: u32 = 0x0008_0000;

/// What Windows says about one drive letter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowsDrive {
    /// The drive's root, with a trailing backslash: `E:\`.
    pub(crate) root: String,
    /// `GetDriveTypeW`: see [`drive_type`].
    pub(crate) drive_type: u32,
    /// The letter is a `subst` alias for a folder on another drive.
    pub(crate) subst: bool,
    /// The volume's label, empty when it has none.
    pub(crate) label: String,
    /// The volume's serial number when Windows supplies it. A successful zero is distinct from query failure.
    pub(crate) serial: Option<u32>,
    /// The file system's name ("NTFS"), empty when Windows won't say (a card reader with no card in it).
    pub(crate) file_system: String,
    /// The file system is mounted read-only.
    pub(crate) read_only: bool,
    /// How the disk is attached (see [`bus`]), when Windows says.
    pub(crate) bus: Option<u32>,
    /// What the partition is for, when Windows says.
    pub(crate) partition: Option<Partition>,
    /// Capacity in bytes, as this user sees it (disk quotas included).
    pub(crate) total_bytes: u64,
    /// Free bytes, from the same point of view.
    pub(crate) free_bytes: u64,
    /// Windows runs from this drive.
    pub(crate) system: bool,
}

/// What a partition is for: its GPT type and attribute bits, or its MBR type byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Partition {
    /// A partition on a GPT disk.
    Gpt {
        /// The partition type.
        type_id: Guid,
        /// `GPT_ATTRIBUTE_*` and `GPT_BASIC_DATA_ATTRIBUTE_*` bits.
        attributes: u64,
        /// Windows marks it as a service partition (a recovery or OEM tools partition).
        service: bool,
    },
    /// A partition on an MBR disk.
    Mbr {
        /// The partition type byte.
        kind: u8,
        /// Windows marks it as a service partition.
        service: bool,
    },
}

/// A GUID, laid out the way Windows stores one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Guid {
    pub(crate) data1: u32,
    pub(crate) data2: u16,
    pub(crate) data3: u16,
    pub(crate) data4: [u8; 8],
}

/// The EFI system partition: `C12A7328-F81F-11D2-BA4B-00A0C93EC93B`.
pub(crate) const EFI_SYSTEM: Guid = Guid {
    data1: 0xC12A_7328,
    data2: 0xF81F,
    data3: 0x11D2,
    data4: [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B],
};
/// The Microsoft reserved partition: `E3C9E316-0B5C-4DB8-817D-F92DF00215AE`.
pub(crate) const MICROSOFT_RESERVED: Guid = Guid {
    data1: 0xE3C9_E316,
    data2: 0x0B5C,
    data3: 0x4DB8,
    data4: [0x81, 0x7D, 0xF9, 0x2D, 0xF0, 0x02, 0x15, 0xAE],
};
/// The Windows recovery partition: `DE94BBA4-06D1-4D40-A16A-BFD50179D6AC`.
pub(crate) const WINDOWS_RECOVERY: Guid = Guid {
    data1: 0xDE94_BBA4,
    data2: 0x06D1,
    data3: 0x4D40,
    data4: [0xA1, 0x6A, 0xBF, 0xD5, 0x01, 0x79, 0xD6, 0xAC],
};
/// `GPT_ATTRIBUTE_PLATFORM_REQUIRED`: the computer needs this partition to work.
pub(crate) const GPT_REQUIRED: u64 = 0x1;
/// `GPT_BASIC_DATA_ATTRIBUTE_HIDDEN`: Windows hides this partition.
pub(crate) const GPT_HIDDEN: u64 = 0x4000_0000_0000_0000;
/// MBR partition types Windows and PC makers keep for themselves: OEM tools (0x12), hidden NTFS (0x17), the Windows
/// recovery environment (0x27) and EFI (0xEF).
const MBR_SYSTEM: [u8; 4] = [0x12, 0x17, 0x27, 0xEF];

/// File systems of local disks. A volume with any other (a cloud drive's virtual file system) is not offered.
const LOCAL_FILE_SYSTEMS: [&str; 5] = ["NTFS", "ReFS", "exFAT", "FAT32", "FAT"];

/// The drives to offer from what Windows reported about each drive letter: the system drive first, then by
/// letter. Windows lists only volumes with a letter, so volumes without one never get here.
pub(crate) fn drives(found: &[WindowsDrive]) -> Vec<VolumeInfo> {
    super::ordered(
        found
            .iter()
            .filter(|d| d.system || offers(d))
            .map(describe)
            .collect(),
    )
}

/// Whether to offer a drive: a fixed or removable disk with a disk file system on an ordinary, writable data
/// partition. Network shares, CD and RAM drives, `subst` aliases, cloud services' virtual drives, read-only volumes
/// and EFI, reserved, recovery, OEM or hidden partitions that were given a letter are left out.
fn offers(d: &WindowsDrive) -> bool {
    if d.drive_type != drive_type::FIXED && d.drive_type != drive_type::REMOVABLE {
        return false;
    }
    if d.subst || d.read_only {
        return false;
    }
    let local = LOCAL_FILE_SYSTEMS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(&d.file_system));
    // Google Drive's virtual drive says it is a FAT32 disk; its label gives it away.
    if !local || d.label.trim().eq_ignore_ascii_case("Google Drive") {
        return false;
    }
    !d.partition.is_some_and(|p| p.is_system())
}

impl Partition {
    /// A partition Windows or the PC maker keeps for itself, not one for the user's files.
    fn is_system(self) -> bool {
        match self {
            Partition::Gpt {
                type_id,
                attributes,
                service,
            } => {
                service
                    || [EFI_SYSTEM, MICROSOFT_RESERVED, WINDOWS_RECOVERY].contains(&type_id)
                    || attributes & (GPT_REQUIRED | GPT_HIDDEN) != 0
            }
            Partition::Mbr { kind, service } => service || MBR_SYSTEM.contains(&kind),
        }
    }
}

/// How Vacancy shows a drive: Explorer's name, its kind, and its sizes.
pub(crate) fn describe(d: &WindowsDrive) -> VolumeInfo {
    let free = d.free_bytes.min(d.total_bytes);
    VolumeInfo {
        name: display_name(&d.label, d.drive_type, &d.root),
        mount_point: d.root.clone(),
        id: d
            .serial
            .map(|serial| format!("{serial:08X}"))
            .unwrap_or_default(),
        kind: kind(d.drive_type, d.bus),
        system: d.system,
        total_bytes: d.total_bytes,
        used_bytes: d.total_bytes - free,
        free_bytes: free,
        file_system: d.file_system.clone(),
    }
}

/// USB when the disk is on USB (Windows calls USB hard drives fixed, so the bus decides), external for SD cards
/// and FireWire, and internal otherwise. A removable disk Windows won't say more about is taken to be on USB.
fn kind(drive_type: u32, bus: Option<u32>) -> DriveKind {
    match bus {
        Some(bus::USB) => DriveKind::Usb,
        Some(bus::IEEE1394 | bus::SD | bus::MMC) => DriveKind::External,
        _ if drive_type == drive_type::REMOVABLE => DriveKind::Usb,
        _ => DriveKind::Internal,
    }
}

/// Explorer's name for a drive: its label (or a word for its kind when it has none), then the drive in brackets.
pub(crate) fn display_name(label: &str, drive_type: u32, root: &str) -> String {
    let label = match label.trim() {
        "" => match drive_type {
            drive_type::FIXED => "Local Disk",
            drive_type::REMOVABLE => "USB Drive",
            drive_type::REMOTE => "Network Drive",
            drive_type::CDROM => "CD Drive",
            drive_type::RAMDISK => "RAM Disk",
            _ => "Drive",
        },
        label => label,
    };
    let drive = root.strip_suffix('\\').unwrap_or(root);
    format!("{label} ({drive})")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;
    /// An ordinary data partition: `EBD0A0A2-B9E5-4433-87C0-68B6B72699C7`.
    const BASIC_DATA: Guid = Guid {
        data1: 0xEBD0_A0A2,
        data2: 0xB9E5,
        data3: 0x4433,
        data4: [0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7],
    };

    /// An internal NTFS data drive; each test changes what it is about.
    fn found(letter: char) -> WindowsDrive {
        WindowsDrive {
            root: format!("{letter}:\\"),
            drive_type: drive_type::FIXED,
            subst: false,
            label: String::new(),
            serial: Some(u32::from(letter)),
            file_system: "NTFS".into(),
            read_only: false,
            bus: Some(0x11), // NVMe
            partition: Some(Partition::Gpt {
                type_id: BASIC_DATA,
                attributes: 0,
                service: false,
            }),
            total_bytes: 1000 * GB,
            free_bytes: 400 * GB,
            system: false,
        }
    }

    /// Zach's laptop, plus everything Windows might list that Vacancy must leave out.
    fn a_busy_pc() -> Vec<WindowsDrive> {
        vec![
            WindowsDrive {
                label: "SanDisk".into(),
                drive_type: drive_type::REMOVABLE,
                file_system: "exFAT".into(),
                bus: Some(bus::USB),
                partition: Some(Partition::Mbr {
                    kind: 0x07,
                    service: false,
                }),
                total_bytes: 64 * GB,
                free_bytes: 23 * GB,
                ..found('E')
            },
            WindowsDrive {
                label: "Windows-SSD".into(),
                system: true,
                free_bytes: 7 * GB,
                ..found('C')
            },
            WindowsDrive {
                label: "Data".into(),
                bus: Some(0x0B), // SATA
                total_bytes: 2000 * GB,
                free_bytes: 1300 * GB,
                ..found('D')
            },
            // A USB hard drive: Windows calls it fixed.
            WindowsDrive {
                label: "Backup Plus".into(),
                bus: Some(bus::USB),
                ..found('F')
            },
            WindowsDrive {
                label: "Camera".into(),
                drive_type: drive_type::REMOVABLE,
                file_system: "FAT32".into(),
                bus: Some(bus::SD),
                ..found('S')
            },
            // Left out from here on.
            WindowsDrive {
                label: "Projects".into(),
                drive_type: drive_type::REMOTE,
                bus: None,
                partition: None,
                ..found('N')
            },
            WindowsDrive {
                label: "DVD".into(),
                drive_type: drive_type::CDROM,
                file_system: "UDF".into(),
                bus: Some(0x2),
                partition: None,
                ..found('O')
            },
            WindowsDrive {
                drive_type: drive_type::RAMDISK,
                ..found('R')
            },
            WindowsDrive {
                subst: true,
                ..found('W')
            },
            WindowsDrive {
                label: "Google Drive".into(),
                file_system: "FAT32".into(),
                bus: None,
                partition: None,
                ..found('G')
            },
            // A card reader with no card in it.
            WindowsDrive {
                drive_type: drive_type::REMOVABLE,
                file_system: String::new(),
                total_bytes: 0,
                free_bytes: 0,
                ..found('H')
            },
            WindowsDrive {
                label: "Recovery".into(),
                partition: Some(Partition::Gpt {
                    type_id: WINDOWS_RECOVERY,
                    attributes: 0x8000_0000_0000_0001,
                    service: true,
                }),
                ..found('K')
            },
            WindowsDrive {
                label: "SYSTEM".into(),
                file_system: "FAT32".into(),
                partition: Some(Partition::Gpt {
                    type_id: EFI_SYSTEM,
                    attributes: 0,
                    service: false,
                }),
                ..found('Z')
            },
            WindowsDrive {
                label: "Tools".into(),
                partition: Some(Partition::Gpt {
                    type_id: BASIC_DATA,
                    attributes: GPT_HIDDEN,
                    service: false,
                }),
                ..found('T')
            },
            WindowsDrive {
                label: "RECOVERY".into(),
                partition: Some(Partition::Mbr {
                    kind: 0x27,
                    service: false,
                }),
                ..found('Q')
            },
            WindowsDrive {
                label: "Locked SD".into(),
                read_only: true,
                ..found('L')
            },
        ]
    }

    #[test]
    fn unavailable_serial_is_not_a_successful_zero_and_does_not_hide_the_system_drive() {
        let failed = WindowsDrive {
            system: true,
            serial: None,
            label: String::new(),
            file_system: String::new(),
            ..found('C')
        };
        let offered = drives(std::slice::from_ref(&failed));
        assert_eq!(offered.len(), 1, "read-only offering policy is unchanged");
        assert!(
            offered[0].id.is_empty(),
            "failed OS identity cannot authorize a launcher"
        );
        let legitimate_zero = WindowsDrive {
            serial: Some(0),
            ..failed
        };
        assert_eq!(describe(&legitimate_zero).id, "00000000");
    }

    #[test]
    fn offers_internal_usb_and_external_drives_with_the_system_drive_first() {
        let drives = drives(&a_busy_pc());
        let offered: Vec<_> = drives
            .iter()
            .map(|d| (d.name.as_str(), d.kind, d.system))
            .collect();
        assert_eq!(
            offered,
            [
                ("Windows-SSD (C:)", DriveKind::Internal, true),
                ("Data (D:)", DriveKind::Internal, false),
                ("SanDisk (E:)", DriveKind::Usb, false),
                ("Backup Plus (F:)", DriveKind::Usb, false),
                ("Camera (S:)", DriveKind::External, false),
            ]
        );
    }

    #[test]
    fn reports_capacity_used_and_free() {
        let drives = drives(&a_busy_pc());
        let c = &drives[0];
        assert_eq!(c.mount_point, "C:\\");
        assert_eq!(c.id, "00000043", "the serial number, in hex");
        assert_eq!(
            (c.total_bytes, c.used_bytes, c.free_bytes),
            (1000 * GB, 993 * GB, 7 * GB)
        );
        let e = &drives[2];
        assert_eq!(
            (e.total_bytes, e.used_bytes, e.free_bytes),
            (64 * GB, 41 * GB, 23 * GB)
        );
    }

    #[test]
    fn leaves_out_network_optical_ram_alias_cloud_empty_read_only_and_system_partitions() {
        let offered: Vec<String> = drives(&a_busy_pc())
            .into_iter()
            .map(|d| d.mount_point)
            .collect();
        for left_out in ["N", "O", "R", "W", "G", "H", "K", "Z", "T", "Q", "L"] {
            assert!(
                !offered.contains(&format!("{left_out}:\\")),
                "{left_out}: is not offered: {offered:?}"
            );
        }
    }

    #[test]
    fn always_offers_the_drive_windows_runs_from() {
        let odd_system = WindowsDrive {
            system: true,
            partition: Some(Partition::Gpt {
                type_id: BASIC_DATA,
                attributes: 0,
                service: true,
            }),
            ..found('C')
        };
        assert_eq!(drives(&[odd_system]).len(), 1);
    }

    #[test]
    fn takes_a_removable_disk_without_a_bus_to_be_usb() {
        let stick = WindowsDrive {
            drive_type: drive_type::REMOVABLE,
            bus: None,
            partition: None,
            ..found('E')
        };
        assert_eq!(describe(&stick).kind, DriveKind::Usb);
        let disk = WindowsDrive {
            bus: None,
            partition: None,
            ..found('D')
        };
        assert_eq!(describe(&disk).kind, DriveKind::Internal);
    }

    #[test]
    fn never_reports_more_free_than_there_is() {
        let odd = WindowsDrive {
            total_bytes: 10,
            free_bytes: 12,
            ..found('D')
        };
        let d = describe(&odd);
        assert_eq!((d.used_bytes, d.free_bytes), (0, 10));
    }

    #[test]
    fn names_drives_the_way_explorer_does() {
        assert_eq!(
            display_name("", drive_type::FIXED, "C:\\"),
            "Local Disk (C:)"
        );
        assert_eq!(
            display_name("  Games ", drive_type::FIXED, "D:\\"),
            "Games (D:)"
        );
        assert_eq!(
            display_name("", drive_type::REMOVABLE, "E:\\"),
            "USB Drive (E:)"
        );
        assert_eq!(
            display_name("Data", drive_type::FIXED, "C:\\Mount\\Data\\"),
            "Data (C:\\Mount\\Data)"
        );
    }
}

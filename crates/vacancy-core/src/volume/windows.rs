//! Windows: the volume root from `GetVolumePathNameW`, sizes from `GetDiskFreeSpaceExW`, and the label from
//! `GetVolumeInformationW`, named the way Explorer names drives ("Local Disk (C:)").

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::{io, ptr};

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    GetVolumePathNameW, QueryDosDeviceW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
};

use super::{VolumeError, VolumeInfo};

/// The longest path Windows accepts, in UTF-16 units.
const LONGEST_PATH: u32 = 32_768;
/// Volume labels are at most 32 characters; the API asks for `MAX_PATH + 1`.
const LABEL_LEN: u32 = 261;
/// File systems of local disks. A volume with any other (a cloud drive's virtual file system) is not offered.
const LOCAL_FILE_SYSTEMS: [&str; 5] = ["NTFS", "ReFS", "exFAT", "FAT32", "FAT"];

/// Local drives: fixed and removable disks with a disk file system. Network shares, CD and RAM drives, `subst`
/// aliases of folders and cloud services' virtual drives are left out.
pub(super) fn list_volumes() -> Vec<VolumeInfo> {
    let mut buf = vec![0u16; 1024];
    // SAFETY: `buf` holds `buf.len()` writable units.
    let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    if n == 0 || n > buf.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for root in buf[..n].split(|&c| c == 0).filter(|r| !r.is_empty()) {
        let root_w: Vec<u16> = root.iter().copied().chain(Some(0)).collect();
        // SAFETY: `root_w` is a NUL-terminated drive root.
        let kind = unsafe { GetDriveTypeW(root_w.as_ptr()) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        if is_subst(root) {
            continue;
        }
        let (label, fs) = label_and_file_system(&root_w);
        let local = LOCAL_FILE_SYSTEMS
            .iter()
            .any(|f| f.eq_ignore_ascii_case(&fs));
        if !local || label.trim().eq_ignore_ascii_case("Google Drive") {
            continue;
        }
        if let Ok(v) = volume_info(Path::new(&OsString::from_wide(root))) {
            out.push(v);
        }
    }
    out
}

/// Whether a drive letter is a `subst` alias for a folder on another drive (its DOS device names a path).
fn is_subst(root: &[u16]) -> bool {
    let device: Vec<u16> = root.iter().copied().take(2).chain(Some(0)).collect();
    let mut target = vec![0u16; 1024];
    // SAFETY: `device` is a NUL-terminated "X:"; `target` holds `target.len()` writable units.
    let n = unsafe { QueryDosDeviceW(device.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
    let prefix: Vec<u16> = r"\??\".encode_utf16().collect();
    n > 0 && target.starts_with(&prefix)
}

pub(super) fn volume_info(path: &Path) -> Result<VolumeInfo, VolumeError> {
    let fail = |source| VolumeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let root = volume_root(path).map_err(fail)?;
    let root_w = to_wide(OsStr::new(&root));

    // Total and free from the same point of view: what this user can use. Without disk quotas that is the whole
    // volume; with quotas both are the user's share, as in Explorer. The volume-wide free figure (the last out
    // parameter) ignores quotas, so subtracting it from a quota-limited total would misreport the used space.
    let mut total = 0u64;
    let mut free = 0u64;
    // SAFETY: `root_w` is NUL-terminated and outlives the call; the two out pointers are valid `u64`s and the
    // volume-wide free pointer may be null.
    let ok =
        unsafe { GetDiskFreeSpaceExW(root_w.as_ptr(), &mut free, &mut total, ptr::null_mut()) };
    if ok == 0 {
        return Err(fail(io::Error::last_os_error()));
    }
    // SAFETY: `root_w` is a NUL-terminated volume root.
    let kind = unsafe { GetDriveTypeW(root_w.as_ptr()) };
    let name = display_name(&volume_label(&root_w), kind, &root);
    Ok(VolumeInfo {
        name,
        mount_point: root,
        total_bytes: total,
        used_bytes: total.saturating_sub(free),
    })
}

pub(super) fn system_volume() -> Result<VolumeInfo, VolumeError> {
    let mut buf = vec![0u16; LONGEST_PATH as usize];
    // SAFETY: `buf` holds `LONGEST_PATH` writable UTF-16 units.
    let n = unsafe { GetSystemWindowsDirectoryW(buf.as_mut_ptr(), LONGEST_PATH) } as usize;
    if n == 0 || n >= buf.len() {
        let source = io::Error::last_os_error();
        return Err(VolumeError::Io {
            path: "%SystemRoot%".into(),
            source,
        });
    }
    let windows_dir = OsString::from_wide(&buf[..n]);
    super::volume_info(Path::new(&windows_dir))
}

/// The root of the volume holding `path`, with a trailing backslash: `C:\`, or a mounted folder such as `D:\Mount\`.
fn volume_root(path: &Path) -> io::Result<String> {
    let path_w = to_wide(path.as_os_str());
    let mut buf = vec![0u16; LONGEST_PATH as usize];
    // SAFETY: `path_w` is NUL-terminated; `buf` holds `LONGEST_PATH` writable UTF-16 units.
    let ok = unsafe { GetVolumePathNameW(path_w.as_ptr(), buf.as_mut_ptr(), LONGEST_PATH) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(from_wide(&buf))
}

/// The volume's label, or an empty string when it has none or Windows won't say.
fn volume_label(root_w: &[u16]) -> String {
    label_and_file_system(root_w).0
}

/// The volume's label and its file system's name ("NTFS"), each empty when Windows won't say.
fn label_and_file_system(root_w: &[u16]) -> (String, String) {
    let mut label = [0u16; LABEL_LEN as usize];
    let mut fs = [0u16; LABEL_LEN as usize];
    // SAFETY: `root_w` is a NUL-terminated volume root; both buffers hold `LABEL_LEN` writable units; the other
    // out parameters may be null.
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            label.as_mut_ptr(),
            LABEL_LEN,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            fs.as_mut_ptr(),
            LABEL_LEN,
        )
    };
    if ok == 0 {
        (String::new(), String::new())
    } else {
        (from_wide(&label), from_wide(&fs))
    }
}

/// Explorer's name for a drive: its label (or a word for its kind when it has none), then the drive in brackets.
fn display_name(label: &str, kind: u32, root: &str) -> String {
    let label = match label.trim() {
        "" => match kind {
            DRIVE_FIXED => "Local Disk",
            DRIVE_REMOVABLE => "USB Drive",
            DRIVE_REMOTE => "Network Drive",
            DRIVE_CDROM => "CD Drive",
            DRIVE_RAMDISK => "RAM Disk",
            _ => "Drive",
        },
        label => label,
    };
    let drive = root.strip_suffix('\\').unwrap_or(root);
    format!("{label} ({drive})")
}

fn to_wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_drives_the_way_explorer_does() {
        assert_eq!(display_name("", DRIVE_FIXED, "C:\\"), "Local Disk (C:)");
        assert_eq!(display_name("  Games ", DRIVE_FIXED, "D:\\"), "Games (D:)");
        assert_eq!(display_name("", DRIVE_REMOVABLE, "E:\\"), "USB Drive (E:)");
        assert_eq!(
            display_name("Data", DRIVE_FIXED, "C:\\Mount\\Data\\"),
            "Data (C:\\Mount\\Data)"
        );
    }
}

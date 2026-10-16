//! Windows: the volume root from `GetVolumePathNameW`, sizes from `GetDiskFreeSpaceExW`, and the label from
//! `GetVolumeInformationW`, named the way Explorer names drives ("Local Disk (C:)").

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::{io, ptr};

use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW,
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

pub(super) fn volume_info(path: &Path) -> Result<VolumeInfo, VolumeError> {
    let fail = |source| VolumeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let root = volume_root(path).map_err(fail)?;
    let root_w = to_wide(OsStr::new(&root));

    let mut total = 0u64;
    let mut free = 0u64;
    // SAFETY: `root_w` is NUL-terminated and outlives the call; the two out pointers are valid `u64`s and the
    // per-user quota pointer may be null.
    let ok =
        unsafe { GetDiskFreeSpaceExW(root_w.as_ptr(), ptr::null_mut(), &mut total, &mut free) };
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
    let mut buf = [0u16; LABEL_LEN as usize];
    // SAFETY: `root_w` is a NUL-terminated volume root; `buf` holds `LABEL_LEN` writable units; every other out
    // parameter may be null.
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            buf.as_mut_ptr(),
            LABEL_LEN,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        String::new()
    } else {
        from_wide(&buf)
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

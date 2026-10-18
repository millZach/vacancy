//! Windows: the drive letters from `GetLogicalDriveStringsW`, each volume's type, label, file system and sizes, and
//! from the volume's device how its disk is attached (USB or not) and what its partition is for.
//! [`super::offer::windows`] decides what to offer, and names drives the way Explorer does ("Local Disk (C:)").

use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::Path;
use std::{io, mem, ptr};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDiskFreeSpaceExW, GetDriveTypeW,
    GetLogicalDriveStringsW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, OPEN_EXISTING, QueryDosDeviceW,
};
use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetThreadErrorMode};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    IOCTL_DISK_GET_PARTITION_INFO_EX, IOCTL_STORAGE_QUERY_PROPERTY, PARTITION_INFORMATION_EX,
    PARTITION_INFORMATION_GPT, PARTITION_INFORMATION_MBR, PARTITION_STYLE_GPT, PARTITION_STYLE_MBR,
    PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    StorageDeviceProperty,
};
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows_sys::Win32::System::WindowsProgramming as wp;

use super::offer::windows::{self as rules, Guid, Partition, WindowsDrive, drive_type};
use super::{VolumeError, VolumeInfo};

// The rules use plain numbers for drive types so they compile everywhere; they must be Windows' own.
const _: () = assert!(
    drive_type::REMOVABLE == wp::DRIVE_REMOVABLE
        && drive_type::FIXED == wp::DRIVE_FIXED
        && drive_type::REMOTE == wp::DRIVE_REMOTE
        && drive_type::CDROM == wp::DRIVE_CDROM
        && drive_type::RAMDISK == wp::DRIVE_RAMDISK
);

/// The longest path Windows accepts, in UTF-16 units.
const LONGEST_PATH: u32 = 32_768;
/// Volume labels are at most 32 characters; the API asks for `MAX_PATH + 1`.
const LABEL_LEN: u32 = 261;
/// `\\?\Volume{GUID}\` and its NUL fit in 50 units.
const VOLUME_NAME_LEN: u32 = 50;

/// While alive, Windows reports a drive with no disk in it (a card reader with no card) as an error instead of
/// asking the user to insert one. The thread's previous setting comes back when it is dropped.
struct QuietErrors(u32);

impl QuietErrors {
    fn new() -> Self {
        let mut old = 0u32;
        // SAFETY: `old` is a valid `u32` for the previous mode.
        unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS, &mut old) };
        Self(old)
    }
}

impl Drop for QuietErrors {
    fn drop(&mut self) {
        // SAFETY: restores the mode saved in `new`; the old-mode pointer may be null.
        unsafe { SetThreadErrorMode(self.0, ptr::null_mut()) };
    }
}

pub(super) fn list_volumes() -> Result<Vec<VolumeInfo>, VolumeError> {
    let _quiet = QuietErrors::new();
    let mut buf = vec![0u16; 1024];
    // SAFETY: `buf` holds `buf.len()` writable units.
    let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    if n == 0 || n > buf.len() {
        return Err(VolumeError::List(io::Error::last_os_error()));
    }
    let system = system_root().ok();
    let mut found = Vec::new();
    for root in buf[..n].split(|&c| c == 0).filter(|r| !r.is_empty()) {
        let root = OsString::from_wide(root).to_string_lossy().into_owned();
        let root_w = to_wide(OsStr::new(&root));
        // SAFETY: `root_w` is a NUL-terminated drive root.
        let kind = unsafe { GetDriveTypeW(root_w.as_ptr()) };
        // Never ask a network share, CD or RAM drive anything more: a disconnected share can take seconds to answer.
        if kind != drive_type::FIXED && kind != drive_type::REMOVABLE {
            continue;
        }
        if let Ok(drive) = probe(&root, kind, system.as_deref()) {
            found.push(drive);
        }
    }
    Ok(rules::drives(&found))
}

pub(super) fn volume_info(path: &Path) -> Result<VolumeInfo, VolumeError> {
    let fail = |source| VolumeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let _quiet = QuietErrors::new();
    let root = volume_root(path).map_err(fail)?;
    let root_w = to_wide(OsStr::new(&root));
    // SAFETY: `root_w` is a NUL-terminated volume root.
    let kind = unsafe { GetDriveTypeW(root_w.as_ptr()) };
    let system = system_root().ok();
    let drive = probe(&root, kind, system.as_deref()).map_err(fail)?;
    Ok(rules::describe(&drive))
}

pub(super) fn system_volume() -> Result<VolumeInfo, VolumeError> {
    let windows_dir = system_dir().map_err(|source| VolumeError::Io {
        path: "%SystemRoot%".into(),
        source,
    })?;
    super::volume_info(Path::new(&windows_dir))
}

/// The Windows folder, usually `C:\Windows`.
fn system_dir() -> io::Result<OsString> {
    let mut buf = vec![0u16; LONGEST_PATH as usize];
    // SAFETY: `buf` holds `LONGEST_PATH` writable UTF-16 units.
    let n = unsafe { GetSystemWindowsDirectoryW(buf.as_mut_ptr(), LONGEST_PATH) } as usize;
    if n == 0 || n >= buf.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(OsString::from_wide(&buf[..n]))
}

/// The root of the volume Windows runs from: `C:\`.
fn system_root() -> io::Result<String> {
    volume_root(Path::new(&system_dir()?))
}

/// Everything Windows says about the volume at `root` that the rules need. Fails only if its sizes can't be read
/// (a card reader with no card in it); the rest is best effort.
fn probe(root: &str, kind: u32, system: Option<&str>) -> io::Result<WindowsDrive> {
    let root_w = to_wide(OsStr::new(root));

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
        return Err(io::Error::last_os_error());
    }
    let (label, serial, file_system, fs_flags) = label_and_file_system(&root_w);
    let (bus, partition) = match open_volume(&root_w) {
        Some(volume) => (bus_type(&volume), partition(&volume)),
        None => (None, None),
    };
    Ok(WindowsDrive {
        root: root.to_owned(),
        drive_type: kind,
        subst: is_subst(&root_w),
        label,
        serial,
        file_system,
        read_only: fs_flags & rules::READ_ONLY_VOLUME != 0,
        bus,
        partition,
        total_bytes: total,
        free_bytes: free,
        system: system.is_some_and(|s| s.eq_ignore_ascii_case(root)),
    })
}

/// Whether a drive letter is a `subst` alias for a folder on another drive (its DOS device names a path).
fn is_subst(root_w: &[u16]) -> bool {
    // Only a bare drive root ("X:\") can be one.
    if root_w.len() != 4 {
        return false;
    }
    let device = [root_w[0], root_w[1], 0];
    let mut target = vec![0u16; 1024];
    // SAFETY: `device` is a NUL-terminated "X:"; `target` holds `target.len()` writable units.
    let n = unsafe { QueryDosDeviceW(device.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
    let prefix: Vec<u16> = r"\??\".encode_utf16().collect();
    n > 0 && target.starts_with(&prefix)
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

/// The volume's label, optional serial number, file system's name ("NTFS") and file system flags. A failed query
/// has no serial, distinct from a successful query that reports the legitimate numeric value zero.
fn label_and_file_system(root_w: &[u16]) -> (String, Option<u32>, String, u32) {
    let mut label = [0u16; LABEL_LEN as usize];
    let mut fs = [0u16; LABEL_LEN as usize];
    let mut serial = 0u32;
    let mut flags = 0u32;
    // SAFETY: `root_w` is a NUL-terminated volume root; both buffers hold `LABEL_LEN` writable units; `serial` and
    // `flags` are valid `u32`s; the longest-name out parameter may be null.
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            label.as_mut_ptr(),
            LABEL_LEN,
            &mut serial,
            ptr::null_mut(),
            &mut flags,
            fs.as_mut_ptr(),
            LABEL_LEN,
        )
    };
    if ok == 0 {
        (String::new(), None, String::new(), 0)
    } else {
        (from_wide(&label), Some(serial), from_wide(&fs), flags)
    }
}

/// The volume's device, opened with no access rights: enough to ask it about itself, and nothing else. No admin
/// needed. `None` if Windows won't open it.
fn open_volume(root_w: &[u16]) -> Option<OwnedHandle> {
    let mut name = [0u16; VOLUME_NAME_LEN as usize];
    // SAFETY: `root_w` is a NUL-terminated volume root ending in a backslash; `name` holds `VOLUME_NAME_LEN` units.
    let ok = unsafe {
        GetVolumeNameForVolumeMountPointW(root_w.as_ptr(), name.as_mut_ptr(), VOLUME_NAME_LEN)
    };
    if ok == 0 {
        return None;
    }
    // `\\?\Volume{GUID}\` names the volume's root folder; without the backslash it names the volume itself.
    let end = name.iter().position(|&c| c == 0)?;
    if end == 0 || name[end - 1] != u16::from(b'\\') {
        return None;
    }
    name[end - 1] = 0;
    // SAFETY: `name` is NUL-terminated; no access rights are asked for, so nothing can be read or changed through
    // the handle, and the security attributes and template may be null.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return None;
    }
    // SAFETY: `handle` is a live handle we own; `OwnedHandle` closes it once.
    Some(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// What a query wrote. 8-aligned like the structures Windows writes into it.
#[repr(C, align(8))]
struct Out<const N: usize>([u8; N]);

/// How the volume's disk is attached (`STORAGE_BUS_TYPE`), from `IOCTL_STORAGE_QUERY_PROPERTY`.
///
/// The answers are read field by field from the bytes Windows wrote, never through the `windows-sys` structures,
/// whose `BOOLEAN` members are Rust `bool`s: a driver writing anything but 0 or 1 there must not be undefined
/// behaviour.
fn bus_type(volume: &OwnedHandle) -> Option<u32> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    // The descriptor is followed by its strings; 1 KiB holds them all.
    let mut out = Out([0u8; 1024]);
    let got = ioctl(
        volume,
        IOCTL_STORAGE_QUERY_PROPERTY,
        (&raw const query).cast(),
        mem::size_of::<STORAGE_PROPERTY_QUERY>(),
        (&raw mut out).cast(),
        mem::size_of_val(&out),
    )?;
    let written = out.0.get(..got)?;
    let bus = i32::from_ne_bytes(field(
        written,
        mem::offset_of!(STORAGE_DEVICE_DESCRIPTOR, BusType),
    )?);
    u32::try_from(bus).ok()
}

/// What the volume's partition is for, from `IOCTL_DISK_GET_PARTITION_INFO_EX`. `None` for a disk with no partition
/// table (a "superfloppy" USB stick) or when Windows won't say. Read field by field, like [`bus_type`].
fn partition(volume: &OwnedHandle) -> Option<Partition> {
    let mut out = Out([0u8; mem::size_of::<PARTITION_INFORMATION_EX>()]);
    let got = ioctl(
        volume,
        IOCTL_DISK_GET_PARTITION_INFO_EX,
        ptr::null(),
        0,
        (&raw mut out).cast(),
        mem::size_of_val(&out),
    )?;
    let info = out.0.get(..got)?;
    let style = i32::from_ne_bytes(field(
        info,
        mem::offset_of!(PARTITION_INFORMATION_EX, PartitionStyle),
    )?);
    let service = field::<1>(
        info,
        mem::offset_of!(PARTITION_INFORMATION_EX, IsServicePartition),
    )?[0]
        != 0;
    // The style says which member of the union Windows filled.
    let union = mem::offset_of!(PARTITION_INFORMATION_EX, Anonymous);
    match style {
        PARTITION_STYLE_GPT => Some(Partition::Gpt {
            type_id: guid(
                info,
                union + mem::offset_of!(PARTITION_INFORMATION_GPT, PartitionType),
            )?,
            attributes: u64::from_ne_bytes(field(
                info,
                union + mem::offset_of!(PARTITION_INFORMATION_GPT, Attributes),
            )?),
            service,
        }),
        PARTITION_STYLE_MBR => Some(Partition::Mbr {
            kind: field::<1>(
                info,
                union + mem::offset_of!(PARTITION_INFORMATION_MBR, PartitionType),
            )?[0],
            service,
        }),
        _ => None,
    }
}

/// `N` bytes at `at` in what Windows wrote, if it wrote that far.
fn field<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// A GUID at `at`, laid out as Windows stores one.
fn guid(bytes: &[u8], at: usize) -> Option<Guid> {
    Some(Guid {
        data1: u32::from_ne_bytes(field(bytes, at)?),
        data2: u16::from_ne_bytes(field(bytes, at + 4)?),
        data3: u16::from_ne_bytes(field(bytes, at + 6)?),
        data4: field(bytes, at + 8)?,
    })
}

/// One `DeviceIoControl` call; the number of bytes written, or `None` if it failed.
fn ioctl(
    volume: &OwnedHandle,
    code: u32,
    input: *const c_void,
    input_len: usize,
    output: *mut c_void,
    output_len: usize,
) -> Option<usize> {
    use std::os::windows::io::AsRawHandle;
    let mut got = 0u32;
    // SAFETY: the handle is live; `input` and `output` point to buffers of the given lengths (the input may be
    // null with length 0), and the call is synchronous, so no OVERLAPPED is needed.
    let ok = unsafe {
        DeviceIoControl(
            volume.as_raw_handle(),
            code,
            input,
            input_len as u32,
            output,
            output_len as u32,
            &mut got,
            ptr::null_mut(),
        )
    };
    (ok != 0).then_some(got as usize)
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

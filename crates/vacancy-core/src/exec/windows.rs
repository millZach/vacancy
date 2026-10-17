//! Windows: every open is relative to an open folder handle (`NtOpenFile` with `RootDirectory`), by the exact name
//! listed, with `FILE_OPEN_REPARSE_POINT`, so no link is ever followed and no path is resolved twice. Folders are
//! listed with `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` like the scanner does, which gives each
//! entry's file id, attributes, reparse tag, allocation and times. Files are opened exclusively (no sharing), so a
//! file any app has open is skipped as in use, and nobody can open it while it is checked and deleted. Deletes go
//! through that handle: `FileDispositionInfoEx` with POSIX semantics (the name goes at once) and read-only ignored,
//! or, where the file system doesn't support that, the read-only attribute cleared on that file alone and the
//! classic disposition.
//!
//! Folders being deleted are opened without `FILE_SHARE_DELETE`, so no one can rename or move them while Vacancy
//! works inside them; with folders open below, Windows won't rename the folders above either.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, Prefix};
use std::{mem, ptr};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::NtOpenFile;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_DIR_NOT_EMPTY, ERROR_FILE_NOT_FOUND,
    ERROR_INVALID_FUNCTION, ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
    ERROR_NOT_SUPPORTED, ERROR_SHARING_VIOLATION, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SPARSE_FILE, FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_STANDARD_INFO, FILE_WRITE_ATTRIBUTES, FileAttributeTagInfo,
    FileBasicInfo, FileDispositionInfo, FileDispositionInfoEx, FileIdExtdDirectoryInfo,
    FileIdExtdDirectoryRestartInfo, FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx,
    OPEN_EXISTING, SYNCHRONIZE, SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};
use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;

use super::{Entry, Fail, Kind};
use crate::fsid::{Identity, Step};

/// Reparse points that name somewhere else (symlinks, junctions, mount points) have this bit.
const NAME_SURROGATE: u32 = 0x2000_0000;
/// Junctions and volume mount points.
const TAG_MOUNT_POINT: u32 = 0xA000_0003;
/// Files the Windows Overlay Filter compressed (CompactOS, `compact /exe`) and files deduplicated by Data
/// Deduplication: ordinary files for Vacancy, deleted like any other.
const TAG_WOF: u32 = 0x8000_0017;
const TAG_DEDUP: u32 = 0x8000_0013;
/// A Unix domain socket.
const TAG_AF_UNIX: u32 = 0x8000_0023;
/// The cloud files family (`IO_REPARSE_TAG_CLOUD_*`) and old OneDrive placeholders.
const CLOUD_MASK: u32 = 0xFFFF_0FFF;
const CLOUD_TAG: u32 = 0x9000_001A;
const ONEDRIVE_TAG: u32 = 0x8000_0021;
/// Attributes under which an empty allocation is the truth, not a listing quirk (see the scanner).
const SPARSE_LIKE: u32 = FILE_ATTRIBUTE_SPARSE_FILE
    | FILE_ATTRIBUTE_COMPRESSED
    | FILE_ATTRIBUTE_OFFLINE
    | FILE_ATTRIBUTE_RECALL_ON_OPEN
    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
const FILE_NON_DIRECTORY_FILE: u32 = 0x0000_0040;
const FILE_OPEN_FOR_BACKUP_INTENT: u32 = 0x0000_4000;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

const STATUS_ACCESS_DENIED: i32 = 0xC000_0022_u32 as i32;
const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034_u32 as i32;
const STATUS_OBJECT_PATH_NOT_FOUND: i32 = 0xC000_003A_u32 as i32;
const STATUS_SHARING_VIOLATION: i32 = 0xC000_0043_u32 as i32;
const STATUS_DELETE_PENDING: i32 = 0xC000_0056_u32 as i32;
const STATUS_FILE_IS_A_DIRECTORY: i32 = 0xC000_00BA_u32 as i32;
const STATUS_NOT_A_DIRECTORY: i32 = 0xC000_0103_u32 as i32;
const STATUS_CANNOT_DELETE: i32 = 0xC000_0121_u32 as i32;

/// Bytes per listing call, as the scanner uses.
const BUF_BYTES: usize = 64 * 1024;

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned here.
        unsafe { CloseHandle(self.0) };
    }
}

/// An open folder.
pub(crate) struct Dir {
    handle: Handle,
}

/// The folders from a path's volume root down to its parent, all open, and their steps.
pub(crate) struct Parent {
    /// The parent folder.
    pub(crate) dir: Dir,
    /// Its chain: the volume root and each folder down to it.
    pub(crate) steps: Vec<Step>,
    /// The folders above it, kept open while the action runs: Windows won't rename a folder while something inside
    /// it is open.
    _above: Vec<Dir>,
}

/// Nothing to look up on Windows: an app that has a file open makes the exclusive open fail.
pub(crate) struct Busy;

impl Busy {
    pub(crate) fn now() -> Self {
        Busy
    }
}

/// Walks to `path`'s parent from its volume root, each folder opened by name inside the one before, never through a
/// link, and keeps them open.
pub(crate) fn walk_to_parent(path: &Path) -> Result<Parent, Fail> {
    let mut parts = path.components();
    let letter = match parts.next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return Err(Fail::Unsupported),
        },
        _ => return Err(Fail::Failed("the path has no drive".into())),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err(Fail::Failed("the path is not absolute".into()));
    }
    let names: Vec<&OsStr> = parts
        .map(|c| match c {
            Component::Normal(n) => Ok(n),
            _ => Err(Fail::Failed("the path has . or ..".into())),
        })
        .collect::<Result<_, _>>()?;
    let Some((_, folders)) = names.split_last() else {
        return Err(Fail::Failed("the path is a drive".into()));
    };
    let root_path = format!(r"\\?\{}:\", char::from(letter));
    let wide: Vec<u16> = OsStr::new(&root_path).encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(Fail::Failed("the drive could not be opened".into()));
    }
    let root = Handle(raw);
    let (volume, file) = identity(&root).ok_or(Fail::Unsupported)?;
    let mut steps = vec![Step {
        name: root_path.into(),
        id: Identity::new(volume, file),
        mount: None,
    }];
    let mut dir = Dir { handle: root };
    let mut above = Vec::new();
    for name in folders {
        let (next, step) = open_dir(&dir, name, None, false)?;
        steps.push(step);
        above.push(mem::replace(&mut dir, next));
    }
    Ok(Parent {
        dir,
        steps,
        _above: above,
    })
}

/// The listing's entry for `name` in `parent`, matched exactly.
pub(crate) fn entry(parent: &Dir, name: &OsStr) -> Result<Entry, Fail> {
    list(parent)?
        .into_iter()
        .find(|e| e.name == name)
        .ok_or(Fail::Gone)
}

/// Opens the folder `name` in `parent` without following it: for listing, and with `delete` for deleting it too
/// (then without letting anyone else delete or rename it while it is open). It must not be a link, and if
/// `expect` says so it must be the folder that listing entry described (same file id). Gives its step.
pub(crate) fn open_dir(
    parent: &Dir,
    name: &OsStr,
    expect: Option<&Entry>,
    delete: bool,
) -> Result<(Dir, Step), Fail> {
    let wide = name_of(name)?;
    let (access, share) = if delete {
        (
            DELETE
                | FILE_LIST_DIRECTORY
                | FILE_READ_ATTRIBUTES
                | FILE_WRITE_ATTRIBUTES
                | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
    } else {
        (
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
    };
    let handle =
        nt_open(&parent.handle, &wide, access, share, FILE_DIRECTORY_FILE).map_err(|status| {
            match status {
                STATUS_NOT_A_DIRECTORY => Fail::Changed,
                s => open_fail(s),
            }
        })?;
    let (attributes, tag) = attribute_tag(&handle).ok_or(Fail::Failed("no attributes".into()))?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 && tag & NAME_SURROGATE != 0 {
        return Err(Fail::Link);
    }
    let (volume, file) = identity(&handle).ok_or(Fail::Unsupported)?;
    if let Some(e) = expect
        && (e.id != file || e.kind != Kind::Dir)
    {
        return Err(Fail::Changed);
    }
    let step = Step {
        name: name.to_owned(),
        id: Identity::new(volume, file),
        mount: None,
    };
    Ok((Dir { handle }, step))
}

/// The later of an open folder's last-write and change times, in nanoseconds since the Unix epoch.
pub(crate) fn changed(dir: &Dir) -> Result<i128, Fail> {
    let basic = basic(&dir.handle).ok_or(Fail::Failed("no times".into()))?;
    Ok(nanos(basic.LastWriteTime.max(basic.ChangeTime)))
}

/// Every entry of an open folder, from the start. A listing that fails part way fails whole: nothing is decided
/// on half a listing.
pub(crate) fn list(dir: &Dir) -> Result<Vec<Entry>, Fail> {
    let mut buf = vec![0u64; BUF_BYTES / 8];
    let mut out = Vec::new();
    let mut class = FileIdExtdDirectoryRestartInfo;
    loop {
        // SAFETY: `buf` is BUF_BYTES writable, 8-byte aligned bytes; the handle is an open folder.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.handle.0,
                class,
                buf.as_mut_ptr().cast(),
                BUF_BYTES as u32,
            )
        };
        class = FileIdExtdDirectoryInfo;
        if ok == 0 {
            // SAFETY: reads this thread's last error.
            return match unsafe { GetLastError() } {
                ERROR_NO_MORE_FILES => Ok(out),
                // FAT and exFAT have no file ids: nothing there can be confirmed.
                ERROR_INVALID_PARAMETER | ERROR_INVALID_LEVEL | ERROR_NOT_SUPPORTED => {
                    Err(Fail::Unsupported)
                }
                ERROR_ACCESS_DENIED => Err(Fail::Refused),
                code => Err(Fail::Failed(format!("listing failed with error {code}"))),
            };
        }
        let mut offset = 0usize;
        loop {
            // SAFETY: Windows filled `buf` with a chain of FILE_ID_EXTD_DIR_INFO entries, each 8-byte aligned,
            // linked by NextEntryOffset and ending with a zero offset, all inside the buffer.
            let (e, name, next) = unsafe { read_entry(&buf, offset) };
            if name != [u16::from(b'.')] && name != [u16::from(b'.'), u16::from(b'.')] {
                out.push(to_entry(dir, &e, name));
            }
            if next == 0 {
                break;
            }
            offset += next as usize;
        }
    }
}

/// Deletes the file `e` in `dir`. It is opened exclusively, without following it, and must be the file listed
/// (same id), not a link, and unchanged since `threshold`; then it goes through that handle. Gives the bytes freed
/// if this was its last name, `None` if other names keep it.
pub(crate) fn delete_file(
    dir: &Dir,
    e: &Entry,
    threshold: i128,
    _busy: &Busy,
    _inside_target: bool,
) -> Result<Option<u64>, Fail> {
    let wide = name_of(&e.name)?;
    let handle = nt_open(
        &dir.handle,
        &wide,
        DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        0,
        FILE_NON_DIRECTORY_FILE,
    )
    .map_err(|status| match status {
        STATUS_FILE_IS_A_DIRECTORY => Fail::Changed,
        s => open_fail(s),
    })?;
    let (_, file) = identity(&handle).ok_or(Fail::Unsupported)?;
    if file != e.id {
        return Err(Fail::Changed);
    }
    let (attributes, tag) = attribute_tag(&handle).ok_or(Fail::Failed("no attributes".into()))?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && (tag & NAME_SURROGATE != 0 || (tag != TAG_WOF && tag != TAG_DEDUP))
    {
        return Err(Fail::Changed);
    }
    let times = basic(&handle).ok_or(Fail::Failed("no times".into()))?;
    if nanos(times.LastWriteTime.max(times.ChangeTime)) >= threshold {
        return Err(Fail::Changed);
    }
    let links = standard(&handle).map_or(1, |s| s.NumberOfLinks);
    dispose(&handle, times.FileAttributes)?;
    drop(handle);
    Ok((links <= 1).then_some(e.alloc))
}

/// Deletes the link `e` in `dir` as itself: opened without following it, it must be the link listed (same id)
/// and still a link, made before the scan.
pub(crate) fn delete_link(dir: &Dir, e: &Entry, threshold: i128) -> Result<(), Fail> {
    let wide = name_of(&e.name)?;
    let handle = nt_open(
        &dir.handle,
        &wide,
        DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        0,
    )
    .map_err(open_fail)?;
    let (_, file) = identity(&handle).ok_or(Fail::Unsupported)?;
    let (attributes, tag) = attribute_tag(&handle).ok_or(Fail::Failed("no attributes".into()))?;
    if file != e.id || attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 || tag & NAME_SURROGATE == 0 {
        return Err(Fail::Changed);
    }
    let times = basic(&handle).ok_or(Fail::Failed("no times".into()))?;
    if nanos(times.LastWriteTime.max(times.ChangeTime)) >= threshold {
        return Err(Fail::Changed);
    }
    // A volume mounted in a folder is left mounted: deleting the mount point would take the volume away from
    // wherever it is used.
    if tag == TAG_MOUNT_POINT && is_volume(&handle) {
        return Err(Fail::OtherVolume);
    }
    dispose(&handle, times.FileAttributes)
}

/// Whether the mount-point reparse point `e` in `dir` is where a volume is mounted, rather than a junction to a
/// folder. Read from the reparse point itself, opened without following it.
pub(crate) fn mounts_volume(dir: &Dir, e: &Entry) -> Result<bool, Fail> {
    if !e.junction {
        return Ok(false);
    }
    let wide = name_of(&e.name)?;
    let handle = nt_open(
        &dir.handle,
        &wide,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        0,
    )
    .map_err(open_fail)?;
    match identity(&handle) {
        Some((_, file)) if file == e.id => Ok(is_volume(&handle)),
        _ => Err(Fail::Changed),
    }
}

/// Whether an open mount-point reparse point names a volume (`\??\Volume{...}\`): its substitute name, from
/// `FSCTL_GET_REPARSE_POINT`. One that can't be read counts as a volume, which leaves it alone.
fn is_volume(handle: &Handle) -> bool {
    const MAX: usize = 16 * 1024;
    let mut buf = vec![0u8; MAX];
    let mut got = 0u32;
    // SAFETY: `buf` is MAX writable bytes; the handle is open; there is no input buffer.
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            FSCTL_GET_REPARSE_POINT,
            ptr::null(),
            0,
            buf.as_mut_ptr().cast(),
            MAX as u32,
            &mut got,
            ptr::null_mut(),
        )
    };
    if ok == 0 || (got as usize) < 16 {
        return true;
    }
    let tag = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if tag != TAG_MOUNT_POINT {
        return false;
    }
    // MountPointReparseBuffer: SubstituteNameOffset and SubstituteNameLength at bytes 8 and 10, the path buffer
    // from byte 16.
    let u16_at = |at: usize| usize::from(u16::from_le_bytes([buf[at], buf[at + 1]]));
    let start = 16 + u16_at(8);
    let Some(bytes) = buf.get(start..start + u16_at(10)) else {
        return true;
    };
    let name: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&name)
        .to_ascii_lowercase()
        .starts_with(r"\??\volume{")
}

/// Removes an emptied folder through the handle it was opened with for deleting.
pub(crate) fn remove_dir(
    _parent: &Dir,
    dir: Dir,
    _step: &Step,
    _inside_target: bool,
) -> Result<(), Fail> {
    let attributes = basic(&dir.handle).map_or(0, |b| b.FileAttributes);
    dispose(&dir.handle, attributes)
}

/// Marks an open file or folder for deletion. POSIX semantics take the name away at once, even while other
/// handles (which shared delete) stay open; read-only is ignored for this file alone. Where the file system can't
/// (FAT, older Windows), the read-only attribute is cleared on this handle's file and the classic disposition set,
/// and put back if that fails.
fn dispose(handle: &Handle, attributes: u32) -> Result<(), Fail> {
    let ex = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    // SAFETY: `ex` is a valid FILE_DISPOSITION_INFO_EX of the size passed; the handle is open with DELETE.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileDispositionInfoEx,
            (&raw const ex).cast(),
            mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    // SAFETY: reads this thread's last error.
    let code = unsafe { GetLastError() };
    if !matches!(
        code,
        ERROR_INVALID_PARAMETER
            | ERROR_NOT_SUPPORTED
            | ERROR_INVALID_FUNCTION
            | ERROR_INVALID_LEVEL
    ) {
        return Err(set_fail(code));
    }
    let readonly = attributes & FILE_ATTRIBUTE_READONLY != 0;
    if readonly {
        set_attributes(handle, attributes & !FILE_ATTRIBUTE_READONLY)?;
    }
    let classic = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `classic` is a valid FILE_DISPOSITION_INFO of the size passed; the handle is open with DELETE.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileDispositionInfo,
            (&raw const classic).cast(),
            mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok != 0 {
        return Ok(());
    }
    // SAFETY: reads this thread's last error.
    let code = unsafe { GetLastError() };
    if readonly {
        let _ = set_attributes(handle, attributes);
    }
    Err(set_fail(code))
}

/// Sets an open file's attributes, leaving its times alone (zero means "unchanged").
fn set_attributes(handle: &Handle, attributes: u32) -> Result<(), Fail> {
    let info = FILE_BASIC_INFO {
        CreationTime: 0,
        LastAccessTime: 0,
        LastWriteTime: 0,
        ChangeTime: 0,
        FileAttributes: if attributes == 0 {
            FILE_ATTRIBUTE_NORMAL
        } else {
            attributes
        },
    };
    // SAFETY: `info` is a valid FILE_BASIC_INFO of the size passed; the handle is open with FILE_WRITE_ATTRIBUTES.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileBasicInfo,
            (&raw const info).cast(),
            mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        // SAFETY: reads this thread's last error.
        Err(set_fail(unsafe { GetLastError() }))
    }
}

/// Opens `name` inside the open folder `dir` by name, never by path, without following it if it is a link, and
/// without `OBJ_CASE_INSENSITIVE`: the exact name, as the file system matches names. `options` add to the
/// synchronous, backup-intent, no-follow opening every call uses.
fn nt_open(
    dir: &Handle,
    name: &[u16],
    access: u32,
    share: u32,
    options: u32,
) -> Result<Handle, i32> {
    let bytes = u16::try_from(name.len() * 2).map_err(|_| STATUS_OBJECT_NAME_NOT_FOUND)?;
    let object_name = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: name.as_ptr().cast_mut(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: dir.0,
        ObjectName: &object_name,
        Attributes: 0,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    // SAFETY: an all-zero IO_STATUS_BLOCK is valid.
    let mut status_block: IO_STATUS_BLOCK = unsafe { mem::zeroed() };
    let mut file: HANDLE = ptr::null_mut();
    // SAFETY: every pointer is valid for the call: `object_name` borrows `name`, which outlives it, and `dir` is an
    // open folder handle.
    let status = unsafe {
        NtOpenFile(
            &mut file,
            access,
            &attributes,
            &mut status_block,
            share,
            options
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_FOR_BACKUP_INTENT
                | FILE_OPEN_REPARSE_POINT,
        )
    };
    if status >= 0 && !file.is_null() {
        Ok(Handle(file))
    } else {
        Err(status)
    }
}

/// A name as UTF-16 for `nt_open`: one name, never a path.
fn name_of(name: &OsStr) -> Result<Vec<u16>, Fail> {
    let wide: Vec<u16> = name.encode_wide().collect();
    if wide.is_empty() || wide.contains(&u16::from(b'\\')) || wide.contains(&u16::from(b'/')) {
        return Err(Fail::Failed("not a single name".into()));
    }
    Ok(wide)
}

fn open_fail(status: i32) -> Fail {
    match status {
        STATUS_SHARING_VIOLATION => Fail::InUse(None),
        STATUS_OBJECT_NAME_NOT_FOUND | STATUS_OBJECT_PATH_NOT_FOUND | STATUS_DELETE_PENDING => {
            Fail::Gone
        }
        STATUS_ACCESS_DENIED | STATUS_CANNOT_DELETE => Fail::Refused,
        s => Fail::Failed(format!("status {:#010x}", s as u32)),
    }
}

/// What a failed `SetFileInformationByHandle` means.
fn set_fail(code: u32) -> Fail {
    match code {
        ERROR_DIR_NOT_EMPTY => Fail::NotEmpty,
        ERROR_SHARING_VIOLATION => Fail::InUse(None),
        // A running program's file, or one Windows protects.
        ERROR_ACCESS_DENIED => Fail::Refused,
        ERROR_FILE_NOT_FOUND => Fail::Gone,
        c => Fail::Failed(format!("error {c}")),
    }
}

/// An open file's volume serial number and 128-bit file id.
fn identity(handle: &Handle) -> Option<(u64, u128)> {
    // SAFETY: an all-zero FILE_ID_INFO is valid.
    let mut info: FILE_ID_INFO = unsafe { mem::zeroed() };
    // SAFETY: `info` is writable and sized as passed; the handle is open.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileIdInfo,
            (&raw mut info).cast(),
            mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    (ok != 0).then(|| {
        (
            info.VolumeSerialNumber,
            u128::from_le_bytes(info.FileId.Identifier),
        )
    })
}

fn attribute_tag(handle: &Handle) -> Option<(u32, u32)> {
    // SAFETY: an all-zero FILE_ATTRIBUTE_TAG_INFO is valid.
    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { mem::zeroed() };
    // SAFETY: `info` is writable and sized as passed; the handle is open.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    (ok != 0).then_some((info.FileAttributes, info.ReparseTag))
}

fn basic(handle: &Handle) -> Option<FILE_BASIC_INFO> {
    let mut info = FILE_BASIC_INFO::default();
    // SAFETY: `info` is writable and sized as passed; the handle is open.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileBasicInfo,
            (&raw mut info).cast(),
            mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    (ok != 0).then_some(info)
}

fn standard(handle: &Handle) -> Option<FILE_STANDARD_INFO> {
    let mut info = FILE_STANDARD_INFO::default();
    // SAFETY: `info` is writable and sized as passed; the handle is open.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileStandardInfo,
            (&raw mut info).cast(),
            mem::size_of::<FILE_STANDARD_INFO>() as u32,
        )
    };
    (ok != 0).then_some(info)
}

/// A FILETIME (100 ns ticks since 1601) in nanoseconds since the Unix epoch.
fn nanos(filetime: i64) -> i128 {
    const EPOCH_GAP_TICKS: i128 = 116_444_736_000_000_000;
    (i128::from(filetime) - EPOCH_GAP_TICKS) * 100
}

/// Reads the entry at byte `offset` of a listing buffer: its fixed fields, its name, and the offset of the next.
///
/// # Safety
/// `buf` was filled by `GetFileInformationByHandleEx` with FILE_ID_EXTD_DIR_INFO entries, and `offset` is where one
/// of them starts.
unsafe fn read_entry(buf: &[u64], offset: usize) -> (FILE_ID_EXTD_DIR_INFO, &[u16], u32) {
    // SAFETY: the caller guarantees `offset` is the start of an entry inside `buf`.
    let at = unsafe { buf.as_ptr().cast::<u8>().add(offset) };
    // SAFETY: the caller guarantees a whole FILE_ID_EXTD_DIR_INFO at `at`.
    let e = unsafe { ptr::read_unaligned(at.cast::<FILE_ID_EXTD_DIR_INFO>()) };
    let name_at = mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
    // SAFETY: the name follows the fixed fields and is FileNameLength bytes long, inside the entry.
    let name = unsafe {
        std::slice::from_raw_parts(at.add(name_at).cast::<u16>(), e.FileNameLength as usize / 2)
    };
    (e, name, e.NextEntryOffset)
}

fn to_entry(dir: &Dir, e: &FILE_ID_EXTD_DIR_INFO, name: &[u16]) -> Entry {
    let is_dir = e.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let reparse = e.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    let tag = if reparse { e.ReparsePointTag } else { 0 };
    let cloud = tag & CLOUD_MASK == CLOUD_TAG || tag == ONEDRIVE_TAG;
    let kind = match (is_dir, reparse) {
        (_, true) if tag & NAME_SURROGATE != 0 => Kind::Link,
        (true, false) => Kind::Dir,
        (true, true) if cloud => Kind::Special("a cloud-sync placeholder folder"),
        (true, true) => Kind::Special("a special folder"),
        (false, true) if tag == TAG_WOF || tag == TAG_DEDUP => Kind::File,
        (false, true) if cloud => Kind::Special("a cloud-sync placeholder"),
        (false, true) if tag == TAG_AF_UNIX => Kind::Special("a socket"),
        (false, true) => Kind::Special("a special file"),
        (false, false) => Kind::File,
    };
    let mut alloc = u64::try_from(e.AllocationSize).unwrap_or(0);
    // As the scanner does: a file the Windows Overlay Filter compressed lists with nothing allocated, so it is
    // asked again, relative to the folder's handle.
    if kind == Kind::File
        && alloc == 0
        && e.EndOfFile > 0
        && e.FileAttributes & SPARSE_LIKE == 0
        && let Ok(file) = nt_open(
            &dir.handle,
            name,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_NON_DIRECTORY_FILE,
        )
        && let Some(std) = standard(&file)
    {
        alloc = u64::try_from(std.AllocationSize).unwrap_or(0);
    }
    Entry {
        name: OsString::from_wide(name),
        kind,
        id: u128::from_le_bytes(e.FileId.Identifier),
        dev: 0,
        alloc,
        changed: nanos(e.LastWriteTime.max(e.ChangeTime)),
        links: None,
        // As the scanner counts: everything that isn't a folder or a link.
        counts: !is_dir && kind != Kind::Link,
        junction: tag == TAG_MOUNT_POINT,
    }
}

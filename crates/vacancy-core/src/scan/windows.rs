//! Windows: one directory handle per folder, listed with `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)`,
//! which returns each entry's attributes, reparse tag, allocated size, times and file id in bulk, so no file is
//! opened. Long paths go through the `\\?\` prefix.
//!
//! Sizes come from the listing, which already reflects NTFS compression, sparse files and cloud placeholders (an
//! online-only OneDrive file has nothing allocated). One case it gets wrong: a file compressed by the Windows
//! Overlay Filter (CompactOS, `compact /exe`) lists as nothing allocated, so files that list with a length but no
//! allocation and none of the sparse, compressed or cloud attributes are asked again, by handle.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf, Prefix};
use std::{io, mem, ptr};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_CANT_ACCESS_FILE, ERROR_DIRECTORY,
    ERROR_FILE_NOT_FOUND, ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
    ERROR_NOT_SUPPORTED, ERROR_PATH_NOT_FOUND, ERROR_PRIVILEGE_NOT_HELD, ERROR_SHARING_VIOLATION,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_OFFLINE,
    FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SPARSE_FILE, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FULL_DIR_INFO, FILE_ID_EXTD_DIR_INFO,
    FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileFullDirectoryInfo,
    FileIdExtdDirectoryInfo, FileStandardInfo, GetFileInformationByHandleEx, GetVolumeInformationW,
    GetVolumePathNameW, OPEN_EXISTING, SYNCHRONIZE,
};

use super::{Collector, Entry, EntryKind, LinkKey, ListError, RootSpec};
use crate::snapshot::{RootKind, UnixTime};

/// Bytes per listing call. Bigger means fewer calls; 64 KiB is what the OS hands back comfortably.
const BUF_BYTES: usize = 64 * 1024;
const LONGEST_PATH: usize = 32_768;
/// Folder-shaped reparse points that name somewhere else (junctions, symlinks, mount points) have this bit.
const NAME_SURROGATE: u32 = 0x2000_0000;
/// The cloud files family: `IO_REPARSE_TAG_CLOUD` and `IO_REPARSE_TAG_CLOUD_1` to `_F`, `0x9000_X01A`.
const CLOUD_MASK: u32 = 0xFFFF_0FFF;
const CLOUD_TAG: u32 = 0x9000_001A;
/// Older OneDrive placeholders.
const ONEDRIVE_TAG: u32 = 0x8000_0021;
/// Attributes under which an empty allocation is the truth, not a listing quirk.
const SPARSE_LIKE: u32 = FILE_ATTRIBUTE_SPARSE_FILE
    | FILE_ATTRIBUTE_COMPRESSED
    | FILE_ATTRIBUTE_OFFLINE
    | FILE_ATTRIBUTE_RECALL_ON_OPEN
    | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

/// What the scanner knows about the volume being walked.
#[derive(Clone, Debug)]
pub(crate) struct Volume {
    /// NTFS: file ids are record numbers, so hard links are tracked in a bitmap.
    ntfs: bool,
}

pub(crate) fn roots(path: &Path) -> io::Result<Vec<RootSpec>> {
    let real = plain(path);
    if !std::fs::symlink_metadata(&real)?.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a folder"));
    }
    let ntfs = file_system(&real).is_some_and(|fs| fs.eq_ignore_ascii_case("NTFS"));
    Ok(vec![RootSpec {
        shown: real.clone(),
        real,
        kind: RootKind::Volume,
        volume: Volume { ntfs },
    }])
}

/// The path without a `\\?\` prefix, as people write it.
fn plain(path: &Path) -> PathBuf {
    let mut parts = path.components();
    if let Some(Component::Prefix(prefix)) = parts.next()
        && let Prefix::VerbatimDisk(letter) = prefix.kind()
    {
        let mut out = PathBuf::from(format!("{}:\\", char::from(letter)));
        for part in parts {
            if let Component::Normal(name) = part {
                out.push(name);
            }
        }
        return out;
    }
    path.to_path_buf()
}

/// The file system's name ("NTFS", "ReFS", "exFAT") for the volume holding `path`.
fn file_system(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = vec![0u16; LONGEST_PATH];
    // SAFETY: `wide` is NUL-terminated; `root` holds LONGEST_PATH writable units.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), LONGEST_PATH as u32) } == 0 {
        return None;
    }
    let mut name = [0u16; 64];
    // SAFETY: `root` is a NUL-terminated volume root; `name` holds 64 writable units; other outputs may be null.
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..end]))
}

/// A scan worker's buffers, reused from folder to folder.
pub(crate) struct Lister {
    buf: Vec<u64>,
    path: Vec<u16>,
    scratch: Scratch,
    /// Some volumes (FAT, exFAT) have no file ids; listings there fall back to `FileFullDirectoryInfo`.
    full_only: bool,
}

/// The parts of a lister an entry is built in, apart from the listing buffer the entry is read from.
#[derive(Default)]
struct Scratch {
    file: Vec<u16>,
    name: String,
}

impl Default for Lister {
    fn default() -> Self {
        Self {
            buf: vec![0u64; BUF_BYTES / 8],
            path: Vec::new(),
            scratch: Scratch::default(),
            full_only: false,
        }
    }
}

/// Closes a handle when dropped.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

impl Lister {
    pub(crate) fn list(
        &mut self,
        dir: &Path,
        volume: &Volume,
        out: &mut Collector<'_>,
    ) -> Result<(), ListError> {
        verbatim(dir, &mut self.path);
        self.path.push(0);
        // SAFETY: `self.path` is NUL-terminated. Opening with FILE_FLAG_OPEN_REPARSE_POINT never follows a link:
        // if the folder was swapped for a junction, this opens the junction itself, which lists empty.
        let handle = unsafe {
            CreateFileW(
                self.path.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        self.path.pop();
        if handle == INVALID_HANDLE_VALUE {
            return Err(open_error());
        }
        let handle = Handle(handle);
        let mut first = true;
        loop {
            let class = if self.full_only {
                FileFullDirectoryInfo
            } else {
                FileIdExtdDirectoryInfo
            };
            // SAFETY: `buf` is BUF_BYTES writable, 8-byte aligned bytes; the handle is an open directory.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    handle.0,
                    class,
                    self.buf.as_mut_ptr().cast(),
                    BUF_BYTES as u32,
                )
            };
            if ok == 0 {
                // SAFETY: reads this thread's last error.
                let code = unsafe { GetLastError() };
                match code {
                    ERROR_NO_MORE_FILES => return Ok(()),
                    ERROR_INVALID_PARAMETER | ERROR_INVALID_LEVEL | ERROR_NOT_SUPPORTED
                        if first && !self.full_only =>
                    {
                        self.full_only = true;
                        continue;
                    }
                    ERROR_ACCESS_DENIED if first => return Err(ListError::Denied),
                    _ if first => return Err(ListError::Failed),
                    _ => {
                        out.mark_incomplete();
                        return Ok(());
                    }
                }
            }
            first = false;
            read_batch(&self.buf, class, dir, volume, &mut self.scratch, out);
        }
    }
}

/// Walks one buffer of entries.
fn read_batch(
    buf: &[u64],
    class: FILE_INFO_BY_HANDLE_CLASS,
    dir: &Path,
    volume: &Volume,
    scratch: &mut Scratch,
    out: &mut Collector<'_>,
) {
    let mut offset = 0usize;
    loop {
        // SAFETY: Windows filled `buf` with a chain of entries of the requested class, each 8-byte aligned, linked
        // by NextEntryOffset and ending with a zero offset; every offset stays inside the buffer it was given.
        let raw = unsafe { read_entry(buf, offset, class) };
        if scratch.decode_name(raw.name) {
            if scratch.name != "." && scratch.name != ".." {
                let entry = scratch.entry(&raw, dir, volume);
                out.add(&entry);
            }
        } else {
            // A name that is not valid UTF-16 (an unpaired surrogate): rare, so it gets its own allocation.
            let owned = OsString::from_wide(raw.name);
            let mut entry = scratch.entry(&raw, dir, volume);
            entry.name = &owned;
            out.add(&entry);
        }
        if raw.next == 0 {
            break;
        }
        offset += raw.next as usize;
    }
}

impl Scratch {
    fn entry<'n>(&'n mut self, raw: &RawEntry<'_>, dir: &Path, volume: &Volume) -> Entry<'n> {
        let is_dir = raw.attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
        let reparse = raw.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        let tag = if reparse { raw.reparse_tag } else { 0 };
        let kind = match (is_dir, reparse) {
            (true, false) => EntryKind::Dir,
            (true, true) if is_cloud(tag) => EntryKind::CloudDir,
            (true, true) => EntryKind::DirLink,
            (false, true) if tag & NAME_SURROGATE != 0 => EntryKind::FileLink,
            (false, _) => EntryKind::File,
        };
        let mut allocated = u64::try_from(raw.allocation).unwrap_or(0);
        if kind == EntryKind::File
            && allocated == 0
            && raw.end_of_file > 0
            && raw.attributes & SPARSE_LIKE == 0
            && let Some(real) = self.allocation_by_handle(dir)
        {
            allocated = real;
        }
        let link_key = match (kind, raw.file_id) {
            (EntryKind::File, Some(id)) if volume.ntfs => {
                // An NTFS file id is a 48-bit record number and a 16-bit sequence number.
                Some(LinkKey::Index((id as u64) & 0x0000_FFFF_FFFF_FFFF))
            }
            (EntryKind::File, Some(id)) => Some(LinkKey::Id(id)),
            _ => None,
        };
        Entry {
            name: OsStr::new(self.name.as_str()),
            kind,
            allocated,
            modified: unix_time(raw.modified),
            accessed: unix_time(raw.accessed),
            link_key,
        }
    }

    /// The allocation the OS reports for `dir\{self.name}` through a handle, for files whose listing says none.
    /// Opens for attributes only, which never reads or recalls the file's data.
    fn allocation_by_handle(&mut self, dir: &Path) -> Option<u64> {
        verbatim(&dir.join(&self.name), &mut self.file);
        self.file.push(0);
        // SAFETY: `self.file` is NUL-terminated; attribute-only access with OPEN_REPARSE_POINT.
        let handle = unsafe {
            CreateFileW(
                self.file.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let handle = Handle(handle);
        // SAFETY: an all-zero FILE_STANDARD_INFO is valid.
        let mut info: FILE_STANDARD_INFO = unsafe { mem::zeroed() };
        // SAFETY: `info` is writable and sized as passed; the handle is open.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileStandardInfo,
                (&raw mut info).cast(),
                mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        };
        (ok != 0).then(|| u64::try_from(info.AllocationSize).unwrap_or(0))
    }

    /// Decodes a UTF-16 name into `self.name`. False if it is not valid UTF-16.
    fn decode_name(&mut self, name: &[u16]) -> bool {
        self.name.clear();
        for c in char::decode_utf16(name.iter().copied()) {
            match c {
                Ok(c) => self.name.push(c),
                Err(_) => return false,
            }
        }
        true
    }
}

/// The fields the scanner reads from either listing class.
struct RawEntry<'b> {
    next: u32,
    attributes: u32,
    reparse_tag: u32,
    end_of_file: i64,
    allocation: i64,
    modified: i64,
    accessed: i64,
    file_id: Option<u128>,
    name: &'b [u16],
}

/// Reads the entry at byte `offset` of a listing buffer.
///
/// # Safety
/// `buf` was filled by `GetFileInformationByHandleEx` with entries of `class`, and `offset` is where one of them
/// starts: zero, or the sum of the NextEntryOffsets before it.
unsafe fn read_entry(buf: &[u64], offset: usize, class: FILE_INFO_BY_HANDLE_CLASS) -> RawEntry<'_> {
    // SAFETY: the caller guarantees `offset` is the start of an entry inside `buf`.
    let at = unsafe { buf.as_ptr().cast::<u8>().add(offset) };
    if class == FileIdExtdDirectoryInfo {
        // SAFETY: the caller guarantees a whole FILE_ID_EXTD_DIR_INFO at `at`.
        let e = unsafe { ptr::read_unaligned(at.cast::<FILE_ID_EXTD_DIR_INFO>()) };
        let name_at = mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
        // SAFETY: the name follows the fixed fields and is FileNameLength bytes long, inside the entry.
        let name = unsafe {
            std::slice::from_raw_parts(at.add(name_at).cast::<u16>(), e.FileNameLength as usize / 2)
        };
        RawEntry {
            next: e.NextEntryOffset,
            attributes: e.FileAttributes,
            reparse_tag: e.ReparsePointTag,
            end_of_file: e.EndOfFile,
            allocation: e.AllocationSize,
            modified: e.LastWriteTime,
            accessed: e.LastAccessTime,
            file_id: Some(u128::from_le_bytes(e.FileId.Identifier)),
            name,
        }
    } else {
        // SAFETY: the caller guarantees a whole FILE_FULL_DIR_INFO at `at`.
        let e = unsafe { ptr::read_unaligned(at.cast::<FILE_FULL_DIR_INFO>()) };
        let name_at = mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
        // SAFETY: as above.
        let name = unsafe {
            std::slice::from_raw_parts(at.add(name_at).cast::<u16>(), e.FileNameLength as usize / 2)
        };
        RawEntry {
            next: e.NextEntryOffset,
            attributes: e.FileAttributes,
            // For reparse points this class puts the tag in EaSize.
            reparse_tag: e.EaSize,
            end_of_file: e.EndOfFile,
            allocation: e.AllocationSize,
            modified: e.LastWriteTime,
            accessed: e.LastAccessTime,
            file_id: None,
            name,
        }
    }
}

fn is_cloud(tag: u32) -> bool {
    tag & CLOUD_MASK == CLOUD_TAG || tag == ONEDRIVE_TAG
}

/// Why a folder would not open.
fn open_error() -> ListError {
    // SAFETY: reads this thread's last error.
    let code = unsafe { GetLastError() };
    match code {
        ERROR_ACCESS_DENIED
        | ERROR_PRIVILEGE_NOT_HELD
        | ERROR_SHARING_VIOLATION
        | ERROR_CANT_ACCESS_FILE => ListError::Denied,
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_DIRECTORY => ListError::Gone,
        _ => ListError::Failed,
    }
}

/// `path` as a `\\?\` path in UTF-16, without the terminating NUL, so it may be longer than MAX_PATH.
fn verbatim(path: &Path, out: &mut Vec<u16>) {
    out.clear();
    let wide = path.as_os_str().encode_wide();
    if path.as_os_str().as_encoded_bytes().starts_with(br"\\?\") {
        out.extend(wide);
    } else {
        out.extend(r"\\?\".encode_utf16());
        out.extend(wide.map(|c| {
            if c == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                c
            }
        }));
    }
}

/// A FILETIME (100 ns ticks since 1601) as Unix seconds; zero when unset.
fn unix_time(filetime: i64) -> UnixTime {
    const TICKS: i64 = 10_000_000;
    const EPOCH_GAP: i64 = 11_644_473_600;
    if filetime <= 0 {
        0
    } else {
        (filetime / TICKS - EPOCH_GAP).max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_filetimes() {
        assert_eq!(unix_time(0), 0);
        // 2024-01-01T00:00:00Z
        assert_eq!(unix_time(133_485_408_000_000_000), 1_704_067_200);
    }

    #[test]
    fn tells_cloud_tags_from_links() {
        assert!(is_cloud(0x9000_F01A), "Dropbox's sync root");
        assert!(is_cloud(0x9000_001A));
        assert!(!is_cloud(0xA000_0003), "a junction");
        assert!(!is_cloud(0xA000_000C), "a symlink");
    }

    #[test]
    fn writes_verbatim_paths() {
        let mut out = Vec::new();
        verbatim(Path::new(r"C:\Users\a"), &mut out);
        assert_eq!(String::from_utf16_lossy(&out), r"\\?\C:\Users\a");
        verbatim(Path::new(r"\\?\C:\x"), &mut out);
        assert_eq!(String::from_utf16_lossy(&out), r"\\?\C:\x");
        assert_eq!(plain(Path::new(r"\\?\C:\x\y")), PathBuf::from(r"C:\x\y"));
    }
}

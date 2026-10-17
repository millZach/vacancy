//! macOS: sizes and the mount point from `statfs`, the volume's name ("Macintosh HD") from `getattrlist`.
//!
//! On APFS the sizes are the container's, which is what Finder and System Settings show.

use std::ffi::{CStr, CString};
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{VolumeError, VolumeInfo};

pub(super) fn volume_info(path: &Path) -> Result<VolumeInfo, VolumeError> {
    let fail = |source| VolumeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        fail(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path contains a NUL byte",
        ))
    })?;

    let mut st = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c_path` is NUL-terminated; `st` is writable memory for one `statfs`.
    if unsafe { libc::statfs(c_path.as_ptr(), st.as_mut_ptr()) } != 0 {
        return Err(fail(io::Error::last_os_error()));
    }
    // SAFETY: `statfs` returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };

    let block = u64::from(st.f_bsize);
    let total = st.f_blocks.saturating_mul(block);
    let free = st.f_bfree.saturating_mul(block);
    // SAFETY: the kernel NUL-terminates `f_mntonname` within its 1024 bytes.
    let mount = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) };
    let mount_point = mount.to_string_lossy().into_owned();
    let name = volume_name(mount).unwrap_or_else(|| fallback_name(&mount_point));
    Ok(VolumeInfo {
        name,
        mount_point,
        total_bytes: total,
        used_bytes: total.saturating_sub(free),
    })
}

pub(super) fn system_volume() -> Result<VolumeInfo, VolumeError> {
    super::volume_info(Path::new("/"))
}

/// File systems of local disks. Anything else (network shares, cloud services' FUSE mounts) is not offered.
const LOCAL_FILE_SYSTEMS: [&str; 6] = ["apfs", "hfs", "exfat", "msdos", "ntfs", "ufs"];

/// Local volumes Finder shows: mounted locally, browsable (which leaves out the system's own volumes under
/// `/System/Volumes`), with a disk file system.
pub(super) fn list_volumes() -> Vec<VolumeInfo> {
    // SAFETY: a null buffer asks only for the number of mounts.
    let n = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if n <= 0 {
        return Vec::new();
    }
    let cap = n as usize + 8;
    let mut buf: Vec<libc::statfs> = Vec::with_capacity(cap);
    let bytes = (cap * mem::size_of::<libc::statfs>()) as libc::c_int;
    // SAFETY: `buf` has room for `cap` statfs records, `bytes` long; getfsstat writes at most that many.
    let got = unsafe { libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    if got <= 0 {
        return Vec::new();
    }
    // SAFETY: getfsstat filled the first `got` records.
    unsafe { buf.set_len((got as usize).min(cap)) };
    let mut out = Vec::new();
    for st in &buf {
        let flags = st.f_flags as u64;
        if flags & libc::MNT_LOCAL as u64 == 0 || flags & libc::MNT_DONTBROWSE as u64 != 0 {
            continue;
        }
        // SAFETY: the kernel NUL-terminates both names within their arrays.
        let fs = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }.to_string_lossy();
        if !LOCAL_FILE_SYSTEMS.contains(&fs.as_ref()) {
            continue;
        }
        // SAFETY: as above.
        let mount = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        if let Ok(v) = super::volume_info(Path::new(&mount)) {
            out.push(v);
        }
    }
    out
}

const NAME_BUF_LEN: usize = 4 + 8 + 1024;

/// The buffer `getattrlist` fills: a `u32` total length, an `attrreference_t` (offset, length), then the name.
#[repr(C, align(8))]
struct NameBuf([u8; NAME_BUF_LEN]);

/// The volume's name as Finder shows it, from `getattrlist(ATTR_VOL_NAME)` on its mount point.
fn volume_name(mount: &CStr) -> Option<String> {
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_NAME,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buf = NameBuf([0; NAME_BUF_LEN]);
    // SAFETY: `mount` is NUL-terminated; `list` and `buf` live for the whole call and `buf` is
    // `size_of::<NameBuf>()` writable bytes.
    let rc = unsafe {
        libc::getattrlist(
            mount.as_ptr(),
            (&raw mut list).cast(),
            (&raw mut buf).cast(),
            mem::size_of::<NameBuf>(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let bytes = &buf.0;
    let word = |at: usize| bytes.get(at..at + 4).map(|b| [b[0], b[1], b[2], b[3]]);
    // The data offset counts from the attrreference_t itself, which starts after the leading length.
    let offset = usize::try_from(i32::from_ne_bytes(word(4)?)).ok()?;
    let length = u32::from_ne_bytes(word(8)?) as usize;
    let start = 4usize.checked_add(offset)?;
    let raw = bytes.get(start..start.checked_add(length)?)?;
    let raw = raw.split(|&b| b == 0).next()?;
    let name = String::from_utf8_lossy(raw).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// Used when the OS won't name the volume: the last folder of the mount point.
fn fallback_name(mount_point: &str) -> String {
    match Path::new(mount_point).file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => mount_point.to_owned(),
    }
}

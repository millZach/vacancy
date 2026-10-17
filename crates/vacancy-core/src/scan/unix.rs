//! macOS (and other Unix): each folder is opened with `O_NOFOLLOW | O_DIRECTORY`, checked to be on the root's file
//! system, then listed with `readdir` and `fstatat(AT_SYMLINK_NOFOLLOW)`, which gives each entry's type, allocated
//! blocks, times, inode and link count without opening it.
//!
//! The macOS startup disk is two volumes in one APFS volume group: the sealed system volume at `/` and the Data
//! volume at `/System/Volumes/Data`, which firmlinks show inside `/` (`/Users`, `/Applications`, `/Library`).
//! Following the firmlinks would cross file systems, and walking both `/Users` and `/System/Volumes/Data/Users`
//! would count everything twice. So the scan walks each volume from its own root, never leaving its file system,
//! and shows the Data volume's paths the way Finder does (`/Users/alex`, not `/System/Volumes/Data/Users/alex`).
//! Other volumes in the container (swap, Preboot, Recovery, snapshots) are not walked; their space shows as the
//! part of the disk Vacancy can't see.
//!
//! The two volumes of a volume group report the same device number (`st_dev`), so on macOS "the same file system"
//! also means the same mount: every opened folder's `f_mntonname` is compared with its root's.

use std::ffi::{CStr, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::{Collector, Entry, EntryKind, LinkKey, ListError, RootSpec};
use crate::snapshot::{RootKind, UnixTime};

/// The file system a walk stays on: its device and, on macOS, where it is mounted.
#[derive(Clone, Debug)]
pub(crate) struct Volume {
    dev: u64,
    /// The root's mount point (`f_mntonname`), on macOS. See the module docs.
    #[cfg(target_os = "macos")]
    mount: std::sync::Arc<[u8]>,
}

impl Volume {
    fn of(path: &Path) -> io::Result<Self> {
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a folder"));
        }
        Ok(Self {
            dev: std::os::unix::fs::MetadataExt::dev(&meta),
            #[cfg(target_os = "macos")]
            mount: mount_of(path)?.into(),
        })
    }
}

/// The macOS startup disk's Data volume, where the system volume's firmlinks lead.
#[cfg(target_os = "macos")]
const DATA: &str = "/System/Volumes/Data";

pub(crate) fn roots(path: &Path) -> io::Result<Vec<RootSpec>> {
    let volume = Volume::of(path)?;
    #[cfg(target_os = "macos")]
    if path == Path::new("/")
        && let Ok(data) = Volume::of(Path::new(DATA))
        && *data.mount == *DATA.as_bytes()
        && data.mount != volume.mount
    {
        return Ok(vec![
            RootSpec {
                real: PathBuf::from("/"),
                shown: PathBuf::from("/"),
                kind: RootKind::SealedSystem,
                volume,
            },
            RootSpec {
                real: PathBuf::from(DATA),
                shown: PathBuf::from("/"),
                kind: RootKind::Data,
                volume: data,
            },
        ]);
    }
    Ok(vec![RootSpec {
        real: path.to_path_buf(),
        shown: path.to_path_buf(),
        kind: RootKind::Volume,
        volume,
    }])
}

/// Where the file system holding `path` is mounted.
#[cfg(target_os = "macos")]
fn mount_of(path: &Path) -> io::Result<Vec<u8>> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut st = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c_path` is NUL-terminated; `st` is writable memory for one statfs.
    if unsafe { libc::statfs(c_path.as_ptr(), st.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statfs returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };
    Ok(mount_name(&st).to_vec())
}

/// A statfs's mount point, without its NUL.
#[cfg(target_os = "macos")]
fn mount_name(st: &libc::statfs) -> &[u8] {
    // SAFETY: the kernel NUL-terminates f_mntonname within its array.
    unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) }.to_bytes()
}

/// Whether an open folder is on the walk's mount (macOS; elsewhere the device check is enough).
#[cfg(target_os = "macos")]
fn on_mount(fd: libc::c_int, volume: &Volume) -> bool {
    let mut st = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `fd` is an open descriptor; `st` is writable memory for one statfs.
    if unsafe { libc::fstatfs(fd, st.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: fstatfs returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };
    *mount_name(&st) == *volume.mount
}

#[cfg(not(target_os = "macos"))]
fn on_mount(_fd: libc::c_int, _volume: &Volume) -> bool {
    true
}

/// Closes a directory stream when dropped.
struct Dir(*mut libc::DIR);

impl Drop for Dir {
    fn drop(&mut self) {
        // SAFETY: the stream came from fdopendir and is closed only here; closedir also closes its descriptor.
        unsafe { libc::closedir(self.0) };
    }
}

/// A scan worker's state. Nothing to reuse on Unix beyond the path buffer.
#[derive(Default)]
pub(crate) struct Lister {
    path: Vec<u8>,
}

impl Lister {
    /// Lists the folder at `dir`. `expect` is the device and inode its parent's listing gave it: the folder
    /// opened must have them, or it is not listed (`Changed`). That is what keeps a folder above it that was
    /// swapped for a symlink after being listed from redirecting the walk: the path would lead elsewhere, and
    /// elsewhere is another inode.
    pub(crate) fn list(
        &mut self,
        dir: &Path,
        expect: Option<u128>,
        volume: &Volume,
        out: &mut Collector<'_>,
    ) -> Result<(), ListError> {
        self.path.clear();
        self.path.extend_from_slice(dir.as_os_str().as_bytes());
        self.path.push(0);
        let c_path = CStr::from_bytes_with_nul(&self.path).map_err(|_| ListError::Failed)?;
        // SAFETY: `c_path` is NUL-terminated. O_NOFOLLOW refuses a folder swapped for a symlink.
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(open_error(io::Error::last_os_error()));
        }
        let mut st = MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `fd` is an open descriptor; `st` is writable memory for one stat.
        if unsafe { libc::fstat(fd, st.as_mut_ptr()) } != 0 {
            // SAFETY: `fd` is open and not used again.
            unsafe { libc::close(fd) };
            return Err(ListError::Failed);
        }
        // SAFETY: fstat returned 0, so it filled `st`.
        let st = unsafe { st.assume_init() };
        if st.st_dev as u64 != volume.dev || !on_mount(fd, volume) {
            // SAFETY: `fd` is open and not used again.
            unsafe { libc::close(fd) };
            return Err(ListError::OtherVolume);
        }
        if expect.is_some_and(|expect| expect != identity(&st)) {
            // SAFETY: `fd` is open and not used again.
            unsafe { libc::close(fd) };
            return Err(ListError::Changed);
        }
        out.opened(dir);
        // SAFETY: `fd` is an open directory; on success the stream owns it.
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() {
            // SAFETY: fdopendir failed, so `fd` is still ours.
            unsafe { libc::close(fd) };
            return Err(ListError::Failed);
        }
        let stream = Dir(stream);
        // SAFETY: `stream` is an open directory stream.
        let dir_fd = unsafe { libc::dirfd(stream.0) };
        let mut seen = 0u32;
        loop {
            seen = seen.wrapping_add(1);
            if seen.is_multiple_of(256) && out.cancelled() {
                return Err(ListError::Cancelled);
            }
            set_errno(0);
            // SAFETY: `stream` is open and only this thread uses it.
            let ent = unsafe { libc::readdir(stream.0) };
            if ent.is_null() {
                if io::Error::last_os_error().raw_os_error().unwrap_or(0) != 0 {
                    out.mark_incomplete();
                }
                return Ok(());
            }
            // SAFETY: readdir returned an entry whose d_name is NUL-terminated.
            let name = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
            let bytes = name.to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let mut st = MaybeUninit::<libc::stat>::uninit();
            // SAFETY: `dir_fd` is the stream's descriptor and `name` a NUL-terminated entry name in it.
            let rc = unsafe {
                libc::fstatat(
                    dir_fd,
                    name.as_ptr(),
                    st.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
            if rc != 0 {
                // Vanished since the listing: nothing to count. Anything else (no search permission on the
                // folder) means the folder can't be fully read.
                if io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
                    out.mark_entry_unreadable();
                }
                continue;
            }
            // SAFETY: fstatat returned 0, so it filled `st`.
            let st = unsafe { st.assume_init() };
            let entry = entry(OsStr::from_bytes(bytes), &st, volume);
            out.add(&entry);
        }
    }
}

fn entry<'n>(name: &'n OsStr, st: &libc::stat, volume: &Volume) -> Entry<'n> {
    let kind = match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR if st.st_dev as u64 != volume.dev => EntryKind::DirOtherVolume,
        libc::S_IFDIR => EntryKind::Dir,
        libc::S_IFREG => EntryKind::File,
        libc::S_IFLNK => EntryKind::FileLink,
        _ => EntryKind::Other,
    };
    let identity_of = identity;
    let identity = matches!(kind, EntryKind::Dir | EntryKind::File).then(|| identity_of(st));
    let link_key = (kind == EntryKind::File && st.st_nlink > 1)
        .then(|| LinkKey::Id((u128::from(st.st_dev as u64) << 64) | u128::from(st.st_ino)));
    Entry {
        name,
        kind,
        // st_blocks counts 512-byte units whatever the file system's block size. A dataless iCloud file has none.
        allocated: (st.st_blocks.max(0) as u64).saturating_mul(512),
        modified: st.st_mtime as UnixTime,
        accessed: st.st_atime as UnixTime,
        link_key,
        identity,
        key: identity_of(st),
    }
}

/// A folder's device and inode, as one number.
fn identity(st: &libc::stat) -> u128 {
    (u128::from(st.st_dev as u64) << 64) | u128::from(st.st_ino)
}

fn open_error(err: io::Error) -> ListError {
    match err.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => ListError::Denied,
        Some(libc::ENOENT | libc::ELOOP | libc::ENOTDIR) => ListError::Gone,
        _ => ListError::Failed,
    }
}

#[cfg(target_os = "macos")]
fn set_errno(value: i32) {
    // SAFETY: __error returns this thread's errno.
    unsafe { *libc::__error() = value };
}

#[cfg(not(target_os = "macos"))]
fn set_errno(value: i32) {
    // SAFETY: __errno_location returns this thread's errno.
    unsafe { *libc::__errno_location() = value };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS 10.15 and later split the startup disk; the scan walks both halves from their own roots. The two
    /// report the same device number, so only their mounts tell them apart.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_startup_disk_is_its_system_and_data_volumes() {
        let roots = roots(Path::new("/")).expect("roots of /");
        let layout = || {
            let mount = std::process::Command::new("/sbin/mount").output();
            let stat = std::process::Command::new("/usr/bin/stat")
                .args(["-f", "%d %N", "/", "/System/Volumes/Data", "/Users"])
                .output();
            format!(
                "{roots:?}\nmount:\n{}\nstat:\n{}",
                mount
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default(),
                stat.map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default()
            )
        };
        if Path::new("/System/Volumes/Data").exists() {
            assert_eq!(roots.len(), 2, "{}", layout());
            assert_eq!(roots[0].kind, RootKind::SealedSystem);
            assert_eq!(roots[0].real, Path::new("/"));
            assert_eq!(roots[1].kind, RootKind::Data);
            assert_eq!(roots[1].real, Path::new("/System/Volumes/Data"));
            assert_eq!(
                roots[1].shown,
                Path::new("/"),
                "shown the way Finder shows it"
            );
            assert_ne!(
                roots[0].volume.mount, roots[1].volume.mount,
                "two file systems"
            );
        }
    }

    /// Lists `dir` as part of a walk over `volume`.
    #[cfg(target_os = "macos")]
    fn list(dir: &str, volume: &Volume) -> Result<(), ListError> {
        let mut listing = crate::snapshot::Listing::default();
        let links = super::super::LinkSet::new();
        let cancel = super::super::CancelToken::new();
        let mut collector = Collector {
            listing: &mut listing,
            links: &links,
            keep_from: u64::MAX,
            now: i64::MAX,
            cancel: &cancel,
            after_open: None,
        };
        Lister::default().list(Path::new(dir), None, volume, &mut collector)
    }

    /// The firmlinks from the system volume lead into the Data volume, which has the same device number: the walk
    /// of `/` must stop there, and the Data volume's own walk must get in.
    #[cfg(target_os = "macos")]
    #[test]
    fn never_walks_from_the_system_volume_into_the_data_volume() {
        let roots = roots(Path::new("/")).expect("roots of /");
        let [system, data] = roots.as_slice() else {
            return; // a Mac without a separate Data volume (before 10.15)
        };
        assert!(list("/System", &system.volume).is_ok());
        for crossing in ["/Users", "/Applications", "/System/Volumes/Data"] {
            assert!(
                matches!(list(crossing, &system.volume), Err(ListError::OtherVolume)),
                "{crossing} is on the Data volume"
            );
        }
        assert!(list("/System/Volumes/Data/Users", &data.volume).is_ok());
        assert!(list("/System/Volumes/Data", &data.volume).is_ok());
    }

    #[test]
    fn any_other_folder_is_one_root() {
        let dir = std::env::temp_dir();
        let roots = roots(&dir).expect("roots of the temp dir");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].kind, RootKind::Volume);
        assert_eq!(roots[0].shown, dir);
    }
}

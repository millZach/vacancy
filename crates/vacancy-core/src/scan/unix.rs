//! macOS (and other Unix): each folder is opened with `O_NOFOLLOW | O_DIRECTORY`, checked to be on the root's
//! device, then listed with `readdir` and `fstatat(AT_SYMLINK_NOFOLLOW)`, which gives each entry's type, allocated
//! blocks, times, inode and link count without opening it.
//!
//! The macOS startup disk is two volumes in one APFS container: the sealed system volume at `/` and the Data volume
//! at `/System/Volumes/Data`, which firmlinks show inside `/` (`/Users`, `/Applications`, `/Library`). Following
//! the firmlinks would cross file systems, and walking both `/Users` and `/System/Volumes/Data/Users` would count
//! everything twice. So the scan walks each volume from its own root, never leaving its device, and shows the Data
//! volume's paths the way Finder does (`/Users/alex`, not `/System/Volumes/Data/Users/alex`). Other volumes in the
//! container (swap, Preboot, Recovery, snapshots) are not walked; their space shows as the part of the disk Vacancy
//! can't see.

use std::ffi::{CStr, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::{Collector, Entry, EntryKind, LinkKey, ListError, RootSpec};
use crate::snapshot::{RootKind, UnixTime};

/// The device a walk stays on.
#[derive(Clone, Debug)]
pub(crate) struct Volume {
    dev: u64,
}

pub(crate) fn roots(path: &Path) -> io::Result<Vec<RootSpec>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotADirectory, "not a folder"));
    }
    let dev = std::os::unix::fs::MetadataExt::dev(&meta);
    if cfg!(target_os = "macos") && path == Path::new("/") {
        let data = Path::new("/System/Volumes/Data");
        if let Ok(data_meta) = std::fs::symlink_metadata(data) {
            let data_dev = std::os::unix::fs::MetadataExt::dev(&data_meta);
            if data_meta.is_dir() && data_dev != dev {
                return Ok(vec![
                    RootSpec {
                        real: PathBuf::from("/"),
                        shown: PathBuf::from("/"),
                        kind: RootKind::SealedSystem,
                        volume: Volume { dev },
                    },
                    RootSpec {
                        real: data.to_path_buf(),
                        shown: PathBuf::from("/"),
                        kind: RootKind::Data,
                        volume: Volume { dev: data_dev },
                    },
                ]);
            }
        }
    }
    Ok(vec![RootSpec {
        real: path.to_path_buf(),
        shown: path.to_path_buf(),
        kind: RootKind::Volume,
        volume: Volume { dev },
    }])
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
    pub(crate) fn list(
        &mut self,
        dir: &Path,
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
        if st.st_dev as u64 != volume.dev {
            // SAFETY: `fd` is open and not used again.
            unsafe { libc::close(fd) };
            return Err(ListError::OtherVolume);
        }
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
        loop {
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
                continue; // vanished since the listing, or not ours to stat
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
    }
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

    /// macOS 10.15 and later split the startup disk; the scan walks both halves from their own roots.
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
            assert_ne!(roots[0].volume.dev, roots[1].volume.dev, "two file systems");
        }
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

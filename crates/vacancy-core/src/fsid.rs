//! File-system identity: which file or folder a path names, as the file system answers it, never as the path's
//! spelling suggests. The Guard authorizes by identity (ADR 0002). Two spellings that fold to the same string
//! (`İpek` and `i` + U+0307 + `pek`, or `alex` and `Alex` in a case-sensitive folder) are two places when the file
//! system keeps them apart, and one place when it doesn't.
//!
//! A path's *chain* is its volume root and every folder below it down to the path itself, each opened by name
//! inside the one before, the way the file system resolves the name, and never through a link: a symlink,
//! junction or mount point anywhere on the way makes the path one that can't be vouched for.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// A file or folder's identity: on Windows the volume's serial number and the 128-bit file id
/// (`GetFileInformationByHandleEx(FileIdInfo)`), on macOS the device and inode numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Identity {
    volume: u64,
    file: u128,
}

/// One step of a path's chain: the name it was reached by (the volume root's is the root's path) and what it is.
#[derive(Clone, Debug)]
pub(crate) struct Step {
    pub(crate) name: OsString,
    pub(crate) id: Identity,
}

/// The chain of `path`: its volume root, then each folder below down to `path` itself. `None` when the path isn't
/// absolute or has `..`, or when a step is missing, can't be opened, or is a link.
pub(crate) fn chain(path: &Path) -> Option<Vec<Step>> {
    Walker::default().chain(path)
}

/// Walks chains, keeping the last few folders it opened, so that a path whose parent it has open is reached with
/// one more open: still by name inside the real parent, never through a link. For many paths in a few folders
/// (the contents of a cache folder), in one short run.
#[derive(Default)]
pub(crate) struct Walker {
    /// Open folders with their paths and chains, the most recently used last.
    open: Vec<(PathBuf, sys::Dir, Vec<Step>)>,
}

/// How many folders a [`Walker`] keeps open.
const KEEP_OPEN: usize = 16;

impl Walker {
    /// [`chain`], reusing the parent folder if it is still open.
    pub(crate) fn chain(&mut self, path: &Path) -> Option<Vec<Step>> {
        if let (Some(parent), Some(Component::Normal(name))) =
            (path.parent(), path.components().next_back())
            && let Some(at) = self.open.iter().position(|(p, ..)| p == parent)
        {
            let entry = self.open.remove(at);
            let found = sys::step(&entry.1, name, false).map(|(step, _)| {
                let mut chain = entry.2.clone();
                chain.push(step);
                chain
            });
            self.open.push(entry);
            return found;
        }
        let (mut dir, root, rest) = sys::root(path)?;
        let mut chain = vec![root];
        let mut at = PathBuf::new();
        for part in path.components().take(path.components().count() - rest) {
            at.push(part);
        }
        let names: Vec<&std::ffi::OsStr> = path
            .components()
            .skip(path.components().count() - rest)
            .filter_map(|p| match p {
                Component::Normal(name) => Some(Some(name)),
                Component::CurDir => None,
                _ => Some(None),
            })
            .collect::<Option<_>>()?;
        for (i, name) in names.iter().enumerate() {
            let last = i + 1 == names.len();
            if last {
                // Keep the parent open for its other contents.
                if self.open.len() == KEEP_OPEN {
                    self.open.remove(0);
                }
                let (step, _) = sys::step(&dir, name, false)?;
                self.open.push((at, dir, chain.clone()));
                chain.push(step);
                return Some(chain);
            }
            let (step, next) = sys::step(&dir, name, true)?;
            chain.push(step);
            at.push(name);
            dir = next?;
        }
        Some(chain)
    }
}

/// The chain of the place `path` leads to with links followed: where a configured place (a known folder, a cloud
/// root, a temp folder) really is. `None` if it isn't there or can't be opened.
pub(crate) fn resolve(path: &Path) -> Option<Vec<Step>> {
    let real = std::fs::canonicalize(path).ok()?;
    chain(&real)
}

/// The identity of the place `path` leads to (see [`resolve`]).
pub(crate) fn identity(path: &Path) -> Option<Identity> {
    resolve(path)?.last().map(|s| s.id)
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Path, Prefix};
    use std::{mem, ptr};

    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::NtOpenFile;
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileAttributeTagInfo, FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        OPEN_EXISTING, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    use super::{Identity, Step};

    /// Reparse points that name somewhere else (symlinks, junctions, mount points) have this bit.
    const NAME_SURROGATE: u32 = 0x2000_0000;

    pub(super) struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned here.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// An open folder.
    pub(super) type Dir = Handle;

    /// The volume root `path` starts at, open, with its step and how many of the path's parts come after it.
    pub(super) fn root(path: &Path) -> Option<(Dir, Step, usize)> {
        let mut parts = path.components();
        let Some(Component::Prefix(prefix)) = parts.next() else {
            return None;
        };
        if parts.next() != Some(Component::RootDir) {
            return None;
        }
        let root_path = root_of(prefix.kind())?;
        let dir = open_root(&root_path)?;
        let step = Step {
            name: root_path.into(),
            id: id_of(&dir)?,
        };
        Some((dir, step, parts.count()))
    }

    /// `name` inside the open folder `dir`, opened by name without following it, as a step, and open if `open`
    /// says so. `None` if it isn't there, can't be opened, or is a link.
    pub(super) fn step(dir: &Dir, name: &OsStr, open: bool) -> Option<(Step, Option<Dir>)> {
        let next = open_in(dir, name)?;
        if is_link(&next)? {
            return None;
        }
        let step = Step {
            name: name.to_owned(),
            id: id_of(&next)?,
        };
        Some((step, open.then_some(next)))
    }

    /// The root of the volume a path starts with, as a `\\?\` path.
    fn root_of(prefix: Prefix<'_>) -> Option<String> {
        let text = |s: &OsStr| s.to_str().map(str::to_owned);
        Some(match prefix {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                format!(r"\\?\{}:\", char::from(letter))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                format!(r"\\?\UNC\{}\{}\", text(server)?, text(share)?)
            }
            Prefix::DeviceNS(device) => format!(r"\\.\{}\", text(device)?),
            Prefix::Verbatim(rest) => format!(r"\\?\{}\", text(rest)?),
        })
    }

    fn open_root(root: &str) -> Option<Handle> {
        let wide: Vec<u16> = OsStr::new(root).encode_wide().chain([0]).collect();
        // SAFETY: `wide` is NUL-terminated.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        (handle != INVALID_HANDLE_VALUE).then_some(Handle(handle))
    }

    /// Opens `name` inside the open folder `dir`, by name and never by path, without following it if it is a link.
    /// No `OBJ_CASE_INSENSITIVE`: the file system matches the name as it matches any name, so a case-sensitive
    /// folder tells `alex` from `Alex` here as it does everywhere else.
    fn open_in(dir: &Handle, name: &OsStr) -> Option<Handle> {
        const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
        const FILE_OPEN_FOR_BACKUP_INTENT: u32 = 0x0000_4000;
        const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let wide: Vec<u16> = name.encode_wide().collect();
        if wide.is_empty() || wide.contains(&u16::from(b'\\')) || wide.contains(&u16::from(b'/')) {
            return None;
        }
        let bytes = u16::try_from(wide.len() * 2).ok()?;
        let object_name = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: wide.as_ptr().cast_mut(),
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
        // SAFETY: every pointer is valid for the call: `object_name` borrows `wide`, which outlives it, and `dir`
        // is an open handle.
        let status = unsafe {
            NtOpenFile(
                &mut file,
                FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                &attributes,
                &mut status_block,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                FILE_SYNCHRONOUS_IO_NONALERT
                    | FILE_OPEN_FOR_BACKUP_INTENT
                    | FILE_OPEN_REPARSE_POINT,
            )
        };
        (status >= 0 && !file.is_null()).then_some(Handle(file))
    }

    /// Whether an open file or folder is a link (a symlink, junction or mount point). `None` if it can't say.
    fn is_link(handle: &Handle) -> Option<bool> {
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
        (ok != 0).then_some(
            info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                && info.ReparseTag & NAME_SURROGATE != 0,
        )
    }

    /// An open file's identity: its 128-bit file id, or on file systems without one (FAT) its file index.
    fn id_of(handle: &Handle) -> Option<Identity> {
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
        if ok != 0 {
            return Some(Identity {
                volume: info.VolumeSerialNumber,
                file: u128::from_le_bytes(info.FileId.Identifier),
            });
        }
        // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is valid.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
        // SAFETY: `info` is writable; the handle is open.
        let ok = unsafe { GetFileInformationByHandle(handle.0, &mut info) };
        (ok != 0).then(|| Identity {
            volume: u64::from(info.dwVolumeSerialNumber),
            file: (u128::from(info.nFileIndexHigh) << 32) | u128::from(info.nFileIndexLow),
        })
    }
}

#[cfg(unix)]
mod sys {
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Component, Path};

    use super::{Identity, Step};

    pub(super) struct Fd(libc::c_int);

    impl Drop for Fd {
        fn drop(&mut self) {
            // SAFETY: the descriptor is open and owned here.
            unsafe { libc::close(self.0) };
        }
    }

    #[allow(clippy::unnecessary_cast)] // dev_t and ino_t differ in width between platforms
    fn id(st: &libc::stat) -> Identity {
        Identity {
            volume: st.st_dev as u64,
            file: st.st_ino as u128,
        }
    }

    /// An open folder.
    pub(super) type Dir = Fd;

    /// The root `path` starts at, open, with its step and how many of the path's parts come after it.
    pub(super) fn root(path: &Path) -> Option<(Dir, Step, usize)> {
        let mut parts = path.components();
        if parts.next() != Some(Component::RootDir) {
            return None;
        }
        // SAFETY: the path is a NUL-terminated literal.
        let fd = unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }
        let dir = Fd(fd);
        let step = Step {
            name: "/".into(),
            id: id(&fstat(&dir)?),
        };
        Some((dir, step, parts.count()))
    }

    /// `name` inside the open folder `dir`, looked at without following it, as a step, and opened as a folder if
    /// `open` says so. `None` if it isn't there, can't be opened, or is a link.
    pub(super) fn step(dir: &Dir, name: &OsStr, open: bool) -> Option<(Step, Option<Dir>)> {
        let c_name = CString::new(name.as_bytes()).ok()?;
        // SAFETY: an all-zero stat is a valid value of a plain C struct.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `c_name` is NUL-terminated, `dir` is open and `st` is writable.
        let found =
            unsafe { libc::fstatat(dir.0, c_name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
        if found != 0 || st.st_mode & libc::S_IFMT == libc::S_IFLNK {
            return None;
        }
        let step = Step {
            name: name.to_owned(),
            id: id(&st),
        };
        if !open {
            return Some((step, None));
        }
        // SAFETY: `c_name` is NUL-terminated and `dir` is open.
        let fd = unsafe {
            libc::openat(
                dir.0,
                c_name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return None;
        }
        let next = Fd(fd);
        // The folder opened must be the one just looked at, not something swapped in since.
        if id(&fstat(&next)?) != step.id {
            return None;
        }
        Some((step, Some(next)))
    }

    fn fstat(fd: &Fd) -> Option<libc::stat> {
        // SAFETY: an all-zero stat is a valid value of a plain C struct.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is open and `st` is writable.
        let found = unsafe { libc::fstat(fd.0, &mut st) };
        (found == 0).then_some(st)
    }
}

#[cfg(not(any(windows, unix)))]
mod sys {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::Step;

    pub(super) struct Dir;

    pub(super) fn root(_path: &Path) -> Option<(Dir, Step, usize)> {
        None
    }

    pub(super) fn step(_dir: &Dir, _name: &OsStr, _open: bool) -> Option<(Step, Option<Dir>)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp folder at its real path: a Mac's is reached through the `/var` link.
    fn real_tempdir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = if cfg!(windows) {
            dir.path().to_path_buf()
        } else {
            dir.path().canonicalize().unwrap()
        };
        (dir, path)
    }

    #[test]
    fn a_chain_runs_from_the_volume_root_to_the_place() {
        let (_fx, root) = real_tempdir();
        let deep = root.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("f.txt"), b"x").unwrap();
        let steps = chain(&deep.join("f.txt")).unwrap();
        assert_eq!(steps.last().unwrap().name, "f.txt");
        assert_eq!(steps[steps.len() - 2].name, "b");
        assert_eq!(
            steps[steps.len() - 2].id,
            identity(&deep).unwrap(),
            "the folder's identity, however it is reached"
        );
        assert_ne!(steps[steps.len() - 2].id, steps[steps.len() - 3].id);
        assert!(chain(&root.join("missing")).is_none());
        assert!(chain(Path::new("relative")).is_none());
        assert!(chain(&deep.join("..").join("b")).is_none());
    }

    /// A walker reaches a folder's other contents through the folder it keeps open, with the same answers as a
    /// fresh walk, and a link swapped in for one of them is still refused.
    #[test]
    fn a_walker_agrees_with_a_fresh_walk() {
        let (_fx, root) = real_tempdir();
        let dir = root.join("cache");
        for name in ["a", "b", "c"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        let mut walker = Walker::default();
        for name in ["a", "b", "c", "missing"] {
            let fresh = chain(&dir.join(name)).map(|c| c.iter().map(|s| s.id).collect::<Vec<_>>());
            let walked = walker
                .chain(&dir.join(name))
                .map(|c| c.iter().map(|s| s.id).collect::<Vec<_>>());
            assert_eq!(walked, fresh, "{name}");
        }
        std::fs::remove_dir(dir.join("c")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("a"), dir.join("c")).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(dir.join("c"))
                .arg(dir.join("a"))
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(walker.chain(&dir.join("c")).is_none(), "now a link");
    }

    #[test]
    fn a_chain_never_goes_through_a_link() {
        let (_fx, root) = real_tempdir();
        let real = root.join("real");
        std::fs::create_dir_all(real.join("inside")).unwrap();
        let link = root.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&real)
                .output()
                .unwrap();
            assert!(made.status.success(), "mklink /J");
        }
        assert!(chain(&real.join("inside")).is_some());
        assert!(chain(&link.join("inside")).is_none(), "through the link");
        assert!(chain(&link).is_none(), "the link itself");
        assert_eq!(
            identity(&link.join("inside")),
            identity(&real.join("inside")),
            "resolving a configured place follows it"
        );
    }
}

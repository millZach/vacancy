//! macOS (and other Unix): every folder is opened with `openat(..., O_NOFOLLOW | O_DIRECTORY)` relative to the one
//! above, its device and inode compared with what the listing said, and on macOS its mount (`fstatfs`) with the
//! target's. Entries are looked at with `fstatat(AT_SYMLINK_NOFOLLOW)` and removed with `unlinkat` relative to the
//! verified folder, so no link is followed and no path is resolved twice. A symlink inside a target is unlinked as
//! itself.
//!
//! POSIX has no "unlink this inode": `unlinkat` removes whatever has the name at that moment. So each entry is
//! looked at again with `fstatat` right before it goes, and must still be the one listed (same device and inode,
//! unchanged since the scan). The window between the two calls is the narrowest POSIX allows, and it is inside a
//! folder already verified to be part of the target: nothing outside the target can be reached through it.
//!
//! A file is in use when a process has it open: the open files of every process Vacancy may look at are read once
//! per step (`proc_pidinfo`, `proc_pidfdinfo`), and a file one of them holds is skipped, with the app's name.

use std::collections::HashMap;
use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path};

use super::{Entry, Fail, Kind, Open};
use crate::fsid::{Identity, Step};

struct Fd(libc::c_int);

impl Drop for Fd {
    fn drop(&mut self) {
        // SAFETY: the descriptor is open and owned here.
        unsafe { libc::close(self.0) };
    }
}

/// An open folder.
pub(crate) struct Dir {
    fd: Fd,
}

/// The folders from a path's volume root down to its parent, all open, and their steps.
pub(crate) struct Parent {
    /// The parent folder.
    pub(crate) dir: Dir,
    /// Its chain: the volume root and each folder down to it.
    pub(crate) steps: Vec<Step>,
    _above: Vec<Dir>,
}

/// Files some process has open, by device and inode, and which process.
pub(crate) struct Busy {
    open: HashMap<(u64, u64), libc::c_int>,
    read: std::time::Instant,
}

impl Busy {
    /// The files every process Vacancy may look at has open now.
    pub(crate) fn now() -> Self {
        Self {
            open: open_files(),
            read: std::time::Instant::now(),
        }
    }

    /// Reads again which files are open, if the last read is `every` old or older. Whether it did.
    pub(crate) fn refresh(&mut self, every: std::time::Duration) -> bool {
        if self.read.elapsed() < every {
            return false;
        }
        *self = Self::now();
        true
    }

    /// The app holding the file open (its name, when it can be read), if one does.
    pub(crate) fn holder(&self, dev: u64, ino: u128) -> Option<Option<String>> {
        let ino = u64::try_from(ino).ok()?;
        self.open.get(&(dev, ino)).map(|&pid| process_name(pid))
    }
}

fn c_name(name: &OsStr) -> Result<CString, Fail> {
    if name.is_empty() || name.as_bytes().contains(&b'/') {
        return Err(Fail::Failed("not a single name".into()));
    }
    CString::new(name.as_bytes()).map_err(|_| Fail::Failed("a name with a NUL".into()))
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn fstatat(dir: &Dir, name: &CString) -> Result<libc::stat, Fail> {
    // SAFETY: an all-zero stat is a valid value of a plain C struct.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is NUL-terminated, `dir` is open and `st` is writable.
    let rc = unsafe { libc::fstatat(dir.fd.0, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
    if rc == 0 {
        Ok(st)
    } else {
        Err(match errno() {
            libc::ENOENT | libc::ENOTDIR => Fail::Gone,
            libc::EACCES | libc::EPERM => Fail::Refused,
            e => Fail::Failed(format!("error {e}")),
        })
    }
}

fn fstat(fd: &Fd) -> Result<libc::stat, Fail> {
    // SAFETY: an all-zero stat is a valid value of a plain C struct.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is open and `st` is writable.
    if unsafe { libc::fstat(fd.0, &mut st) } == 0 {
        Ok(st)
    } else {
        Err(Fail::Failed(format!("error {}", errno())))
    }
}

#[allow(clippy::unnecessary_cast)] // dev_t and ino_t differ in width between platforms
fn dev_ino(st: &libc::stat) -> (u64, u64) {
    (st.st_dev as u64, st.st_ino as u64)
}

/// The later of when an entry was modified and when it changed, in nanoseconds since the Unix epoch.
#[allow(clippy::unnecessary_cast)]
fn changed_of(st: &libc::stat) -> i128 {
    let m = i128::from(st.st_mtime as i64) * 1_000_000_000 + i128::from(st.st_mtime_nsec as i64);
    let c = i128::from(st.st_ctime as i64) * 1_000_000_000 + i128::from(st.st_ctime_nsec as i64);
    m.max(c)
}

/// Where the volume an open folder is on is mounted (macOS; `None` elsewhere). A mount macOS can't report fails the
/// walk: a folder whose volume can't be compared is never vouched for.
fn mount_of(fd: &Fd) -> Result<Option<OsString>, Fail> {
    #[cfg(target_os = "macos")]
    {
        if crate::fsid::test_hooks::fail_mount() {
            return Err(Fail::Failed("which volume it is on can't be read".into()));
        }
        // SAFETY: an all-zero statfs is a valid value of a plain C struct.
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is open and `fs` is writable.
        if unsafe { libc::fstatfs(fd.0, &mut fs) } != 0 {
            return Err(Fail::Failed("which volume it is on can't be read".into()));
        }
        let name: Vec<u8> = fs
            .f_mntonname
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        Ok(Some(OsString::from_vec(name)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = fd;
        Ok(None)
    }
}

/// Walks to `path`'s parent from `/`, each folder opened by name inside the one before, never through a link, and
/// keeps them open.
///
/// `pin` is Windows' (holding folders so they can't be renamed); POSIX has no such hold, so the executor walks the
/// chain again before each unlink instead.
pub(crate) fn walk_to_parent(path: &Path, _pin: bool) -> Result<Parent, Fail> {
    let mut parts = path.components();
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
        return Err(Fail::Failed("the path is the root".into()));
    };
    // SAFETY: the path is a NUL-terminated literal.
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(Fail::Failed("/ could not be opened".into()));
    }
    let root = Fd(fd);
    let st = fstat(&root)?;
    let (dev, ino) = dev_ino(&st);
    let mut steps = vec![Step {
        name: "/".into(),
        id: Identity::new(dev, u128::from(ino)),
        mount: mount_of(&root)?,
    }];
    let mut dir = Dir { fd: root };
    let mut above = Vec::new();
    for name in folders {
        let (next, step) = open_dir(&dir, name, None, Open::Look)?;
        steps.push(step);
        above.push(std::mem::replace(&mut dir, next));
    }
    Ok(Parent {
        dir,
        steps,
        _above: above,
    })
}

fn to_entry(name: &OsStr, st: &libc::stat) -> Entry {
    let kind = match st.st_mode & libc::S_IFMT {
        libc::S_IFREG => Kind::File,
        libc::S_IFDIR => Kind::Dir,
        libc::S_IFLNK => Kind::Link,
        libc::S_IFSOCK => Kind::Special("a socket"),
        libc::S_IFIFO => Kind::Special("a pipe"),
        _ => Kind::Special("a device"),
    };
    let (dev, ino) = dev_ino(st);
    Entry {
        name: name.to_owned(),
        kind,
        id: u128::from(ino),
        dev,
        // As the scanner measures: st_blocks counts 512-byte units whatever the block size.
        alloc: (st.st_blocks.max(0) as u64).saturating_mul(512),
        logical: st.st_size.max(0) as u64,
        changed: changed_of(st),
        #[allow(clippy::unnecessary_cast)]
        links: Some(st.st_nlink as u64),
        // As the scanner counts: regular files.
        counts: kind == Kind::File,
        junction: false,
    }
}

/// The entry `name` in `parent`, looked at without following it.
pub(crate) fn entry(parent: &Dir, name: &OsStr) -> Result<Entry, Fail> {
    let c = c_name(name)?;
    Ok(to_entry(name, &fstatat(parent, &c)?))
}

/// Opens the folder `name` in `parent` without following it. It must be a folder, not a link, the one
/// `expect` describes if given (same device and inode), and the one looked at just before opening. Gives its step,
/// with its mount on macOS.
pub(crate) fn open_dir(
    parent: &Dir,
    name: &OsStr,
    expect: Option<&Entry>,
    _mode: Open,
) -> Result<(Dir, Step), Fail> {
    let c = c_name(name)?;
    let st = fstatat(parent, &c)?;
    match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => {}
        libc::S_IFLNK => return Err(Fail::Link),
        _ => return Err(Fail::Changed),
    }
    let looked = dev_ino(&st);
    if let Some(e) = expect
        && (e.dev, e.id) != (looked.0, u128::from(looked.1))
    {
        return Err(Fail::Changed);
    }
    // SAFETY: `c` is NUL-terminated and `parent` is open.
    let fd = unsafe {
        libc::openat(
            parent.fd.0,
            c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(match errno() {
            libc::ENOENT => Fail::Gone,
            libc::ELOOP | libc::ENOTDIR => Fail::Link,
            libc::EACCES | libc::EPERM => Fail::Refused,
            e => Fail::Failed(format!("error {e}")),
        });
    }
    let fd = Fd(fd);
    // The folder opened must be the one just looked at, not something swapped in since.
    if dev_ino(&fstat(&fd)?) != looked {
        return Err(Fail::Changed);
    }
    let step = Step {
        name: name.to_owned(),
        id: Identity::new(looked.0, u128::from(looked.1)),
        mount: mount_of(&fd)?,
    };
    Ok((Dir { fd }, step))
}

/// The later of an open folder's modified and changed times.
pub(crate) fn changed(dir: &Dir) -> Result<i128, Fail> {
    Ok(changed_of(&fstat(&dir.fd)?))
}

/// Every entry of an open folder. A listing that fails part way fails whole.
pub(crate) fn list(dir: &Dir) -> Result<Vec<Entry>, Fail> {
    // The stream gets its own descriptor: closing the stream closes it, and `dir` stays open.
    // SAFETY: `dir` is open.
    let dup = unsafe { libc::fcntl(dir.fd.0, libc::F_DUPFD_CLOEXEC, 0) };
    if dup < 0 {
        return Err(Fail::Failed(format!("error {}", errno())));
    }
    // SAFETY: `dup` is an open folder descriptor; on success the stream owns it.
    let stream = unsafe { libc::fdopendir(dup) };
    if stream.is_null() {
        // SAFETY: fdopendir failed, so `dup` is still ours.
        unsafe { libc::close(dup) };
        return Err(Fail::Failed(format!("error {}", errno())));
    }
    struct Stream(*mut libc::DIR);
    impl Drop for Stream {
        fn drop(&mut self) {
            // SAFETY: the stream is open and closed only here.
            unsafe { libc::closedir(self.0) };
        }
    }
    let stream = Stream(stream);
    // SAFETY: the stream is open; a duplicated descriptor shares its offset, so start from the beginning.
    unsafe { libc::rewinddir(stream.0) };
    let mut out = Vec::new();
    loop {
        set_errno(0);
        // SAFETY: `stream` is open and only this thread uses it.
        let ent = unsafe { libc::readdir(stream.0) };
        if ent.is_null() {
            return match errno() {
                0 => Ok(out),
                e => Err(Fail::Failed(format!("listing failed with error {e}"))),
            };
        }
        // SAFETY: readdir returned an entry whose d_name is NUL-terminated.
        let name = unsafe { std::ffi::CStr::from_ptr((*ent).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let name = OsStr::from_bytes(bytes);
        let st = match fstatat(dir, &c_name(name)?) {
            Ok(st) => st,
            Err(Fail::Gone) => continue,
            Err(fail) => return Err(fail),
        };
        out.push(to_entry(name, &st));
    }
}

/// Looks at `e` again right before it goes: still the entry listed (same device and inode), of the kind listed,
/// and unchanged since `threshold`.
fn recheck(dir: &Dir, e: &Entry, c: &CString, threshold: i128) -> Result<libc::stat, Fail> {
    let st = fstatat(dir, c)?;
    let now = to_entry(&e.name, &st);
    if (now.dev, now.id) != (e.dev, e.id) || now.kind != e.kind {
        return Err(Fail::Changed);
    }
    // Rewritten to another size since it was listed.
    if now.kind == Kind::File && (now.logical, now.alloc) != (e.logical, e.alloc) {
        return Err(Fail::Changed);
    }
    if now.changed >= threshold {
        return Err(Fail::Changed);
    }
    Ok(st)
}

/// Deletes the file `e` in `dir`, if it is still the file listed, unchanged since `threshold`, not locked, and no
/// process holds it open. Gives the bytes freed if this was its last name, `None` if other names keep it. With `inside_target`,
/// `dir` is being deleted too, and is made writable if it isn't (Go's module cache makes its folders read-only).
pub(crate) fn delete_file(
    dir: &Dir,
    e: &Entry,
    threshold: i128,
    busy: &Busy,
    inside_target: bool,
) -> Result<Option<u64>, Fail> {
    let c = c_name(&e.name)?;
    let st = recheck(dir, e, &c, threshold)?;
    if locked(&st) {
        return Err(Fail::Locked);
    }
    let (dev, ino) = dev_ino(&st);
    if let Some(app) = busy.holder(dev, u128::from(ino)) {
        return Err(Fail::InUse(app));
    }
    unlink(dir, &c, 0, inside_target)?;
    #[allow(clippy::unnecessary_cast)]
    let last = st.st_nlink as u64 <= 1;
    Ok(last.then_some(e.alloc))
}

/// Deletes the symlink `e` in `dir` as itself, if it is still the link listed, made before the scan.
pub(crate) fn delete_link(dir: &Dir, e: &Entry, threshold: i128) -> Result<(), Fail> {
    let c = c_name(&e.name)?;
    recheck(dir, e, &c, threshold)?;
    unlink(dir, &c, 0, true)
}

/// A mount on macOS shows as another device, which the folder checks catch; there are no junctions.
pub(crate) fn mounts_volume(_dir: &Dir, _e: &Entry) -> Result<bool, Fail> {
    Ok(false)
}

/// Removes the emptied folder `step` names in `parent`, if the name still leads to it. With `inside_target`,
/// `parent` is being deleted too and may be made writable for this.
pub(crate) fn remove_dir(
    parent: &Dir,
    dir: Dir,
    step: &Step,
    inside_target: bool,
) -> Result<(), Fail> {
    let c = c_name(&step.name)?;
    let st = fstatat(parent, &c)?;
    let (dev, ino) = dev_ino(&st);
    if Identity::new(dev, u128::from(ino)) != step.id || st.st_mode & libc::S_IFMT != libc::S_IFDIR
    {
        return Err(Fail::Changed);
    }
    drop(dir);
    unlink(parent, &c, libc::AT_REMOVEDIR, inside_target)
}

/// `unlinkat`, and once more after making `dir` writable when it refused for want of that and `may_chmod` says the
/// folder is being deleted anyway.
fn unlink(dir: &Dir, c: &CString, flags: libc::c_int, may_chmod: bool) -> Result<(), Fail> {
    for attempt in 0..2 {
        // SAFETY: `c` is NUL-terminated and `dir` is open.
        if unsafe { libc::unlinkat(dir.fd.0, c.as_ptr(), flags) } == 0 {
            return Ok(());
        }
        let e = errno();
        if e == libc::EACCES && attempt == 0 && may_chmod {
            let st = fstat(&dir.fd)?;
            let mode = (st.st_mode | libc::S_IWUSR | libc::S_IXUSR) & 0o7777;
            // SAFETY: `dir` is open.
            if unsafe { libc::fchmod(dir.fd.0, mode) } == 0 {
                continue;
            }
        }
        return Err(match e {
            libc::ENOENT => Fail::Gone,
            libc::EBUSY => Fail::InUse(None),
            libc::ENOTEMPTY | libc::EEXIST => Fail::NotEmpty,
            // Also what Full Disk Access and System Integrity Protection answer: a lock is told by its flags,
            // which `delete_file` looks at first.
            libc::EPERM | libc::EACCES => Fail::Refused,
            e => Fail::Failed(format!("error {e}")),
        });
    }
    Err(Fail::Refused)
}

/// The file is locked (Finder's Locked, `chflags uchg`) or append-only: macOS won't delete it, and Vacancy doesn't
/// unlock what someone locked.
fn locked(st: &libc::stat) -> bool {
    #[cfg(target_os = "macos")]
    {
        const LOCKS: u32 =
            libc::UF_IMMUTABLE | libc::UF_APPEND | libc::SF_IMMUTABLE | libc::SF_APPEND;
        st.st_flags & LOCKS != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = st;
        false
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

/// The files every process Vacancy may look at holds open, by device and inode, with the process.
#[cfg(target_os = "macos")]
fn open_files() -> HashMap<(u64, u64), libc::c_int> {
    use std::mem::size_of;

    /// `struct proc_fileinfo` from `sys/proc_info.h`.
    #[repr(C)]
    struct ProcFileInfo {
        fi_openflags: u32,
        fi_status: u32,
        fi_offset: libc::off_t,
        fi_type: i32,
        fi_guardflags: u32,
    }
    /// `struct vnode_fdinfo`.
    #[repr(C)]
    struct VnodeFdInfo {
        pfi: ProcFileInfo,
        pvi: libc::vnode_info,
    }
    const PROC_PIDFDVNODEINFO: libc::c_int = 1;

    let mut out = HashMap::new();
    // SAFETY: a null buffer asks for the number of processes.
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if n <= 0 {
        return out;
    }
    let mut pids = vec![0 as libc::pid_t; n as usize + 64];
    // SAFETY: `pids` holds that many pids' worth of writable bytes.
    let n = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    pids.truncate(n.max(0) as usize);
    for pid in pids {
        // SAFETY: a null buffer asks for the size of the process's descriptor list.
        let bytes =
            unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if bytes <= 0 {
            continue; // not ours to look at, or gone
        }
        let count = bytes as usize / size_of::<libc::proc_fdinfo>() + 16;
        // SAFETY: an all-zero proc_fdinfo is a valid value of a plain C struct.
        let mut fds: Vec<libc::proc_fdinfo> = vec![unsafe { std::mem::zeroed() }; count];
        // SAFETY: `fds` holds `count` writable entries.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                (count * size_of::<libc::proc_fdinfo>()) as libc::c_int,
            )
        };
        let got = got.max(0) as usize / size_of::<libc::proc_fdinfo>();
        for fd in &fds[..got.min(count)] {
            if fd.proc_fdtype as libc::c_int != libc::PROX_FDTYPE_VNODE {
                continue;
            }
            // SAFETY: an all-zero VnodeFdInfo is a valid value of plain C structs.
            let mut info: VnodeFdInfo = unsafe { std::mem::zeroed() };
            // SAFETY: `info` is writable and sized as passed.
            let rc = unsafe {
                libc::proc_pidfdinfo(
                    pid,
                    fd.proc_fd,
                    PROC_PIDFDVNODEINFO,
                    (&raw mut info).cast(),
                    size_of::<VnodeFdInfo>() as libc::c_int,
                )
            };
            if rc as usize == size_of::<VnodeFdInfo>() {
                let st = &info.pvi.vi_stat;
                out.insert((u64::from(st.vst_dev), st.vst_ino), pid);
            }
        }
    }
    out
}

#[cfg(not(target_os = "macos"))]
fn open_files() -> HashMap<(u64, u64), libc::c_int> {
    HashMap::new()
}

/// A process's name, for the reason a file was skipped.
fn process_name(pid: libc::c_int) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` holds that many writable bytes.
        let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

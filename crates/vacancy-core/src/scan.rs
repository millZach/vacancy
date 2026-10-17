//! The scanner: walks a volume in parallel and returns a [`Snapshot`]. Read-only: it lists folders and reads their
//! entries' attributes, and never opens a file's contents.
//!
//! Safety rules it keeps on both platforms:
//! - It never follows a link. Symlinks, junctions, mount points and every other reparse point that names somewhere
//!   else are recorded and skipped, judged from the listing itself before descending. Folders are opened without
//!   following links (`FILE_FLAG_OPEN_REPARSE_POINT`, `O_NOFOLLOW`), so a folder swapped for a link mid-scan is not
//!   followed either.
//! - It never crosses into another file system: on Windows only reparse points lead elsewhere; on macOS every
//!   folder's device and mount point are compared with its root's, which also stops at firmlinks into the Data
//!   volume (it has the same device number as the system volume).
//! - It counts allocated bytes (what files take on disk), each hard-linked file once.
//! - A folder it may not read is counted and flagged, never fatal.

use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::snapshot::{Builder, DirFlags, DirId, Listing, Markers, RootKind, Snapshot, UnixTime};

mod links;
#[cfg(unix)]
#[path = "scan/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "scan/windows.rs"]
mod platform;
#[cfg(not(any(unix, windows)))]
#[path = "scan/unsupported.rs"]
mod platform;

use links::{LinkKey, LinkSet};

/// How to scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    /// Files at or above this many allocated bytes are kept individually in the snapshot; smaller ones are only
    /// added to their folder's totals. Default: 1 MiB.
    pub keep_files_from: u64,
    /// Worker threads. Default: two per core, from 8 to 32.
    pub threads: Option<NonZeroUsize>,
    /// How often the progress callback runs while the scan is going. Default: every 100 ms.
    pub progress_every: Duration,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            keep_files_from: 1 << 20,
            threads: None,
            progress_every: Duration::from_millis(100),
        }
    }
}

/// Stops a scan. Clone it and call [`CancelToken::cancel`] from anywhere; the scan returns
/// [`ScanError::Cancelled`] soon after.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

/// Two tokens are equal when they are clones of each other: cancelling one cancels the other.
impl PartialEq for CancelToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CancelToken {}

impl CancelToken {
    /// A token that has not been cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the scan to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether [`CancelToken::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// How far a running scan has got. Passed to the progress callback every [`ScanOptions::progress_every`] while the
/// scan runs, and once more when it has finished.
#[derive(Debug)]
pub struct ScanProgress<'a> {
    /// Files counted so far, each hard-linked file once.
    pub files: u64,
    /// Folders found so far.
    pub folders: u64,
    /// Allocated bytes counted so far.
    pub bytes: u64,
    /// Folders the OS refused to list so far.
    pub unreadable: u64,
    /// A folder being listed right now.
    pub current: &'a Path,
    /// Time since the scan started.
    pub elapsed: Duration,
    builder: &'a Mutex<Builder>,
}

impl ScanProgress<'_> {
    /// Lends `f` the snapshot so far: every folder found, with the files of the folders listed so far, and totals
    /// that are consistent with that. The scan's workers wait while `f` runs, so keep it quick (categorizing a
    /// few hundred thousand folders takes a few milliseconds).
    pub fn preview<R>(&self, f: impl FnOnce(&Snapshot) -> R) -> R {
        let builder = lock(self.builder);
        f(builder.snapshot())
    }
}

/// Why a scan stopped without a snapshot.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// [`CancelToken::cancel`] was called.
    #[error("the scan was cancelled")]
    Cancelled,
    /// The folder to scan could not be opened.
    #[error("could not scan {path}: {source}")]
    Root {
        /// The folder.
        path: PathBuf,
        /// What the OS said.
        #[source]
        source: io::Error,
    },
}

/// Scans `volume` (a volume's mount point, or any folder on one) and everything below it on the same file system.
///
/// `progress` is called on the calling thread every [`ScanOptions::progress_every`] while the scan runs, and once
/// more with the final counts. On macOS, scanning the startup disk (`/`) covers both its sealed system volume and
/// its Data volume, each once.
pub fn scan(
    volume: &Path,
    options: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress<'_>),
    cancel: &CancelToken,
) -> Result<Snapshot, ScanError> {
    scan_inner(volume, options, progress, cancel, None)
}

/// [`scan`], with a hook the tests use to change the tree just before a folder is opened.
pub(crate) fn scan_inner(
    volume: &Path,
    options: &ScanOptions,
    progress: &mut dyn FnMut(&ScanProgress<'_>),
    cancel: &CancelToken,
    before_enter: Option<&(dyn Fn(&Path) + Sync)>,
) -> Result<Snapshot, ScanError> {
    let started = Instant::now();
    let fail = |source| ScanError::Root {
        path: volume.to_path_buf(),
        source,
    };
    let volume = std::path::absolute(volume).map_err(fail)?;
    let roots = platform::roots(&volume).map_err(fail)?;
    let threads = options
        .threads
        .map(NonZeroUsize::get)
        .unwrap_or_else(default_threads);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("vacancy-scan-{i}"))
        .build()
        .map_err(|e| fail(io::Error::other(e)))?;

    let mut builder = Builder::new(options.keep_files_from);
    let tasks: Vec<Task> = roots
        .into_iter()
        .map(|root| Task {
            dir: builder.add_root(root.real.clone(), root.shown, root.kind),
            path: root.real,
            expect: None,
            volume: root.volume,
        })
        .collect();
    let shared = Shared {
        builder: Mutex::new(builder),
        links: LinkSet::new(),
        cancel,
        keep_from: options.keep_files_from,
        now: unix_now(),
        files: AtomicU64::new(0),
        folders: AtomicU64::new(tasks.len() as u64),
        bytes: AtomicU64::new(0),
        unreadable: AtomicU64::new(0),
        current: Mutex::new(tasks.first().map(|t| t.path.clone()).unwrap_or_default()),
        before_enter,
    };

    std::thread::scope(|threads| {
        let walker = threads.spawn(|| {
            pool.scope(|s| {
                for task in tasks {
                    let shared = &shared;
                    s.spawn(move |s| walk(s, shared, task));
                }
            });
        });
        let mut current = PathBuf::new();
        let mut report = |current: &mut PathBuf| {
            current.clone_from(&lock(&shared.current));
            progress(&ScanProgress {
                files: shared.files.load(Ordering::Relaxed),
                folders: shared.folders.load(Ordering::Relaxed),
                bytes: shared.bytes.load(Ordering::Relaxed),
                unreadable: shared.unreadable.load(Ordering::Relaxed),
                current,
                elapsed: started.elapsed(),
                builder: &shared.builder,
            });
        };
        let tick = options.progress_every.max(Duration::from_millis(1));
        let mut next = Instant::now() + tick;
        while !walker.is_finished() {
            // Short naps, so a finished scan is noticed quickly however long the progress interval is.
            std::thread::sleep(tick.min(Duration::from_millis(20)));
            if Instant::now() >= next && !walker.is_finished() {
                report(&mut current);
                next = Instant::now() + tick;
            }
        }
        if let Err(panic) = walker.join() {
            std::panic::resume_unwind(panic);
        }
        if !cancel.is_cancelled() {
            report(&mut current);
        }
    });

    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    let builder = shared
        .builder
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(builder.finish(started.elapsed()))
}

/// Two per core, from 8 to 32. A first scan mostly waits on the disk (each folder's records are read once), so
/// more requests in flight help; with everything cached, more than about 16 stop helping but cost little.
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(4)
        .saturating_mul(2)
        .clamp(8, 32)
}

/// A top-level folder to walk.
#[derive(Debug)]
pub(crate) struct RootSpec {
    pub(crate) real: PathBuf,
    pub(crate) shown: PathBuf,
    pub(crate) kind: RootKind,
    pub(crate) volume: platform::Volume,
}

#[derive(Debug)]
struct Task {
    dir: DirId,
    path: PathBuf,
    /// The identity its parent's listing recorded; the folder opened at `path` must have it.
    expect: Option<u128>,
    volume: platform::Volume,
}

struct Shared<'a> {
    builder: Mutex<Builder>,
    links: LinkSet,
    cancel: &'a CancelToken,
    keep_from: u64,
    now: UnixTime,
    files: AtomicU64,
    folders: AtomicU64,
    bytes: AtomicU64,
    unreadable: AtomicU64,
    current: Mutex<PathBuf>,
    before_enter: Option<&'a (dyn Fn(&Path) + Sync)>,
}

/// Why a folder could not be listed.
#[derive(Debug)]
pub(crate) enum ListError {
    /// The OS refused: no permission, or privacy protection.
    Denied,
    /// It is on another file system.
    #[cfg_attr(
        windows,
        allow(
            dead_code,
            reason = "on Windows only reparse points lead to other volumes"
        )
    )]
    OtherVolume,
    /// It disappeared, or became a link, since its parent was listed. Nothing to count.
    Gone,
    /// Its path now leads to a different folder than its parent listed: it or a folder above it was replaced or
    /// moved, perhaps by a link. Not entered.
    Changed,
    /// The scan was cancelled mid-listing. Nothing is recorded.
    Cancelled,
    /// Anything else. Counted like a refusal.
    Failed,
}

/// One entry of a folder listing, as a platform lister reports it.
#[derive(Debug)]
pub(crate) struct Entry<'n> {
    pub(crate) name: &'n std::ffi::OsStr,
    pub(crate) kind: EntryKind,
    /// Allocated bytes, files only.
    pub(crate) allocated: u64,
    pub(crate) modified: UnixTime,
    pub(crate) accessed: UnixTime,
    /// Identifies the file across its hard links, when it may have more than one name.
    pub(crate) link_key: Option<LinkKey>,
    /// For folders: what identifies this folder (Windows file id; Unix device and inode), checked when it is
    /// opened.
    pub(crate) identity: Option<u128>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Dir,
    /// A cloud-sync placeholder folder: entered.
    CloudDir,
    /// A folder-shaped link (junction, folder symlink, mount point): recorded, not entered. Unix listings can't
    /// tell a folder symlink from a file one without looking through it, so there every symlink is a `FileLink`.
    #[cfg_attr(
        unix,
        allow(dead_code, reason = "Unix reports every symlink as a FileLink")
    )]
    DirLink,
    /// A folder where another file system is mounted: recorded, not entered.
    #[cfg_attr(
        windows,
        allow(
            dead_code,
            reason = "on Windows only reparse points lead to other volumes"
        )
    )]
    DirOtherVolume,
    /// A file-shaped link (a symlink; on Unix any symlink): counted as a link, takes no space, never followed.
    FileLink,
    /// Devices, sockets, pipes.
    #[cfg_attr(
        windows,
        allow(dead_code, reason = "Windows listings hold only files and folders")
    )]
    Other,
}

/// Turns entries into a folder's [`Listing`]: hard links merged, small files folded into the folder's totals,
/// big ones kept, markers noted.
pub(crate) struct Collector<'a> {
    listing: &'a mut Listing,
    links: &'a LinkSet,
    keep_from: u64,
    now: UnixTime,
    cancel: &'a CancelToken,
}

impl Collector<'_> {
    pub(crate) fn add(&mut self, e: &Entry<'_>) {
        let listing = &mut *self.listing;
        match e.kind {
            EntryKind::File => {
                if let Some(key) = e.link_key
                    && !self.links.first_sighting(key)
                {
                    listing.hard_links += 1;
                    return;
                }
                let used = e.modified.max(e.accessed).min(self.now);
                listing.own_bytes += e.allocated;
                listing.own_files += 1;
                listing.own_last_used = listing.own_last_used.max(used);
                if e.allocated >= self.keep_from {
                    listing.push_kept(e.name, e.allocated, used);
                }
                let len = e.name.len();
                if Markers::may_mark(len)
                    && let Some(name) = e.name.to_str()
                {
                    listing.markers = listing.markers.with(Markers::for_file(name));
                }
            }
            EntryKind::Dir | EntryKind::CloudDir => {
                let flags = if e.kind == EntryKind::CloudDir {
                    DirFlags::CLOUD
                } else {
                    DirFlags::default()
                };
                listing.push_dir(e.name, flags, e.modified.min(self.now), e.identity);
                if let Some(name) = e.name.to_str() {
                    listing.markers = listing.markers.with(Markers::for_dir(name));
                }
            }
            EntryKind::DirLink => listing.push_dir(e.name, DirFlags::LINK, 0, None),
            EntryKind::DirOtherVolume => {
                listing.push_dir(e.name, DirFlags::OTHER_VOLUME, 0, None);
            }
            EntryKind::FileLink => listing.file_links += 1,
            EntryKind::Other => {}
        }
    }

    pub(crate) fn mark_incomplete(&mut self) {
        self.listing.flags = self.listing.flags.with(DirFlags::INCOMPLETE);
    }

    /// An entry the OS listed but would not describe (no search permission on the folder): the folder counts as
    /// unreadable, with what could be read.
    #[cfg_attr(
        windows,
        allow(dead_code, reason = "Windows listings describe every entry")
    )]
    pub(crate) fn mark_entry_unreadable(&mut self) {
        self.listing.flags = self
            .listing
            .flags
            .with(DirFlags::UNREADABLE)
            .with(DirFlags::INCOMPLETE);
    }

    /// Whether the scan was cancelled. Listers check it between batches of entries.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

std::thread_local! {
    static WORKER: std::cell::RefCell<Worker> = std::cell::RefCell::new(Worker::default());
}

#[derive(Default)]
struct Worker {
    lister: platform::Lister,
    listing: Listing,
    listed: u32,
}

/// Lists one folder, records it, and queues its subfolders.
fn walk<'s>(s: &rayon::Scope<'s>, shared: &'s Shared<'s>, task: Task) {
    if shared.cancel.is_cancelled() {
        return;
    }
    let children = WORKER.with_borrow_mut(|w| {
        w.listed = w.listed.wrapping_add(1);
        if w.listed % 32 == 1
            && let Ok(mut current) = shared.current.try_lock()
        {
            current.clone_from(&task.path);
        }
        w.listing.clear();
        if let Some(hook) = shared.before_enter {
            hook(&task.path);
        }
        let mut collector = Collector {
            listing: &mut w.listing,
            links: &shared.links,
            keep_from: shared.keep_from,
            now: shared.now,
            cancel: shared.cancel,
        };
        let listed = w
            .lister
            .list(&task.path, task.expect, &task.volume, &mut collector);
        let flag = match listed {
            Ok(()) => None,
            Err(ListError::Gone | ListError::Cancelled) => return Vec::new(),
            Err(ListError::OtherVolume) => Some(DirFlags::OTHER_VOLUME),
            Err(ListError::Changed) => Some(DirFlags::CHANGED),
            Err(ListError::Denied | ListError::Failed) => Some(DirFlags::UNREADABLE),
        };
        if shared.cancel.is_cancelled() {
            return Vec::new();
        }
        if let Some(flag) = flag {
            if flag == DirFlags::UNREADABLE {
                shared.unreadable.fetch_add(1, Ordering::Relaxed);
            }
            lock(&shared.builder).flag(task.dir, flag);
            return Vec::new();
        }
        if w.listing.flags.contains(DirFlags::UNREADABLE) {
            shared.unreadable.fetch_add(1, Ordering::Relaxed);
        }
        let ids = lock(&shared.builder).commit(task.dir, &w.listing);
        shared
            .files
            .fetch_add(u64::from(w.listing.own_files), Ordering::Relaxed);
        shared
            .bytes
            .fetch_add(w.listing.own_bytes, Ordering::Relaxed);
        shared
            .folders
            .fetch_add(w.listing.subdirs.len() as u64, Ordering::Relaxed);
        w.listing
            .subdirs
            .iter()
            .zip(ids)
            .filter(|(sub, _)| sub.flags.entered())
            .map(|(sub, id)| Task {
                dir: DirId(id),
                path: task.path.join(w.listing.dir_name(sub)),
                expect: sub.identity,
                volume: task.volume.clone(),
            })
            .collect::<Vec<_>>()
    });
    for child in children {
        if shared.cancel.is_cancelled() {
            break;
        }
        s.spawn(move |s| walk(s, shared, child));
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unix_now() -> UnixTime {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;

    use super::*;

    const MIB: u64 = 1 << 20;

    fn write(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; len]).unwrap();
    }

    /// A folder link, the way an attacker or a sync tool would make one: a junction on Windows (no privilege
    /// needed), a symlink elsewhere.
    fn link_dir(link: &Path, target: &Path) {
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .expect("run mklink");
            assert!(
                out.status.success(),
                "mklink /J: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).expect("symlink");
    }

    /// A folder is listed, then (before its subfolder is opened) swapped for a link to a tree outside the scan.
    /// Opening the subfolder by path would walk through the link; the identity check refuses it.
    #[test]
    fn never_follows_a_folder_swapped_for_a_link_after_it_was_listed() {
        let fx = tempfile::tempdir().unwrap();
        let root = fx.path().join("scanned");
        let outside = fx.path().join("outside");
        write(
            &root.join("cache").join("inner").join("blob.bin"),
            64 * 1024,
        );
        write(&outside.join("inner").join("secret.bin"), 4 * MIB as usize);
        let swapped = AtomicBool::new(false);
        let hook = |path: &Path| {
            if path.ends_with(Path::new("cache").join("inner"))
                && !swapped.swap(true, Ordering::SeqCst)
            {
                fs::rename(root.join("cache"), root.join("cache-moved"))
                    .expect("move the listed folder");
                link_dir(&root.join("cache"), &outside);
                assert!(
                    root.join("cache").join("inner").join("secret.bin").exists(),
                    "the queued path now leads outside the scan"
                );
            }
        };

        let snap = scan_inner(
            &root,
            &ScanOptions::default(),
            &mut |_| {},
            &CancelToken::new(),
            Some(&hook),
        )
        .expect("scan");

        assert!(swapped.load(Ordering::SeqCst), "the hook ran");
        let total = snap.tree(snap.roots()[0].dir).bytes;
        assert!(total < MIB, "the outside tree is not counted: {total}");
        assert_eq!(snap.stats().changed, 1);
        let inner = snap
            .dirs()
            .find(|&d| snap.name(d) == "inner")
            .expect("the queued folder is recorded");
        assert!(snap.flags(inner).contains(DirFlags::CHANGED));
        assert_eq!(snap.children(inner).len(), 0, "never listed");
    }

    /// A cancel lands while a big folder is being listed: the listing stops early instead of reading it all.
    #[test]
    fn cancel_stops_a_listing_part_way() {
        let fx = tempfile::tempdir().unwrap();
        for i in 0..3000 {
            fs::write(
                fx.path()
                    .join(format!("file-{i:05}-with-a-longish-name.txt")),
                b"x",
            )
            .unwrap();
        }
        let roots = platform::roots(fx.path()).unwrap();
        let links = LinkSet::new();
        let cancel = CancelToken::new();
        let mut listing = Listing::default();
        let mut lister = platform::Lister::default();

        let mut collector = Collector {
            listing: &mut listing,
            links: &links,
            keep_from: u64::MAX,
            now: i64::MAX,
            cancel: &cancel,
        };
        assert!(
            lister
                .list(fx.path(), None, &roots[0].volume, &mut collector)
                .is_ok()
        );
        assert_eq!(listing.own_files, 3000);

        listing.clear();
        cancel.cancel();
        let mut collector = Collector {
            listing: &mut listing,
            links: &links,
            keep_from: u64::MAX,
            now: i64::MAX,
            cancel: &cancel,
        };
        let result = lister.list(fx.path(), None, &roots[0].volume, &mut collector);
        assert!(matches!(result, Err(ListError::Cancelled)), "{result:?}");
        assert!(
            listing.own_files < 3000,
            "stopped part way: {}",
            listing.own_files
        );
    }
}

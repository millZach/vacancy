//! The scanner: walks a volume in parallel and returns a [`Snapshot`]. Read-only: it lists folders and reads their
//! entries' attributes, and never opens a file's contents.
//!
//! Safety rules it keeps on both platforms:
//! - It never follows a link. Symlinks, junctions, mount points and every other reparse point that names somewhere
//!   else are recorded and skipped, judged from the listing itself before descending. Folders are opened without
//!   following links (`FILE_FLAG_OPEN_REPARSE_POINT`, `O_NOFOLLOW`), so a folder swapped for a link mid-scan is not
//!   followed either.
//! - It never crosses into another file system: on Windows only reparse points lead elsewhere; on macOS every
//!   folder's device is compared with its root's, which also stops at firmlinks into the Data volume.
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
                listing.push_dir(e.name, flags, e.modified.min(self.now));
                if let Some(name) = e.name.to_str() {
                    listing.markers = listing.markers.with(Markers::for_dir(name));
                }
            }
            EntryKind::DirLink => listing.push_dir(e.name, DirFlags::LINK, 0),
            EntryKind::DirOtherVolume => listing.push_dir(e.name, DirFlags::OTHER_VOLUME, 0),
            EntryKind::FileLink => listing.file_links += 1,
            EntryKind::Other => {}
        }
    }

    pub(crate) fn mark_incomplete(&mut self) {
        self.listing.flags = self.listing.flags.with(DirFlags::INCOMPLETE);
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
        let mut collector = Collector {
            listing: &mut w.listing,
            links: &shared.links,
            keep_from: shared.keep_from,
            now: shared.now,
        };
        let listed = w.lister.list(&task.path, &task.volume, &mut collector);
        let flag = match listed {
            Ok(()) => None,
            Err(ListError::Gone) => return Vec::new(),
            Err(ListError::OtherVolume) => Some(DirFlags::OTHER_VOLUME),
            Err(ListError::Denied | ListError::Failed) => Some(DirFlags::UNREADABLE),
        };
        if let Some(flag) = flag {
            if flag == DirFlags::UNREADABLE {
                shared.unreadable.fetch_add(1, Ordering::Relaxed);
            }
            lock(&shared.builder).flag(task.dir, flag);
            return Vec::new();
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
                volume: task.volume.clone(),
            })
            .collect::<Vec<_>>()
    });
    for child in children {
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

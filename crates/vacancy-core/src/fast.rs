//! Fast scan (Windows, with admin): a [`Snapshot`] built from the elevated reader's stream of the volume's file
//! index instead of a walk of its folders. See ADR 0007.
//!
//! The stream is untrusted data. It is checked as it is read, and anything that doesn't add up ends the fast scan
//! (the app then runs the normal scan):
//! - frames and fields within bounds (`vacancy_index::wire`);
//! - the header names the volume this app opened itself;
//! - the root folder first, then each folder exactly once, only after a folder listed before announced it, and every
//!   announced folder listed (or reported locked) by the end: so the folders form one tree, without cycles;
//! - names that are names: no separators, no NUL, never `.` or `..`;
//! - every entry what its attributes say (a folder entered is a folder, a link is never entered);
//! - sizes no bigger than the volume, and everything counted together no bigger than the volume either.
//!
//! What is read goes through the same [`Collector`] and [`Builder`] as a normal scan's listings, so digests, hard
//! links, kept files and markers are made exactly as the normal scan makes them, and the planner, the Guard and the
//! executor treat the result the same way.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use vacancy_index::kind::{Kind, classify};
use vacancy_index::wire::{
    Decoder, ENTRY_DIR, ENTRY_ENTER, ENTRY_LOCKED, ErrorCode, Frame, Header, Progress, WireError,
};

use crate::scan::links::{LinkKey, LinkSet};
use crate::scan::{CancelToken, Collector, Entry, EntryKind, ScanOptions, ScanProgress, unix_now};
use crate::snapshot::{
    Builder, DirFlags, DirId, FastFacts, Listing, LockedFolder, RootKind, Snapshot,
};

#[cfg(windows)]
#[path = "fast/windows.rs"]
mod platform;
#[cfg(not(windows))]
#[path = "fast/unsupported.rs"]
mod platform;

pub(crate) use platform::{Launch, read_index};

#[cfg(test)]
#[path = "fast/tests.rs"]
mod tests;

/// The NTFS record number of the root folder.
const ROOT_RECORD: u64 = 5;
const RECORD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Why a fast scan didn't produce a snapshot.
#[derive(Debug, thiserror::Error)]
pub enum FastError {
    /// Cancelled.
    #[error("the scan was cancelled")]
    Cancelled,
    /// The reader says the volume isn't NTFS (or BitLocker has it locked).
    #[error("the drive isn't NTFS")]
    NotNtfs,
    /// The reader couldn't read the index, or the stream doesn't add up.
    #[error("the file index can't be read: {0}")]
    Unreadable(String),
}

impl From<WireError> for FastError {
    fn from(e: WireError) -> Self {
        Self::Unreadable(e.to_string())
    }
}

fn bad(why: &str) -> FastError {
    FastError::Unreadable(why.to_owned())
}

/// What the stream is read against: what the app knows itself.
#[derive(Clone, Debug)]
pub struct Expect {
    /// The folder the snapshot is of: the volume's root, or in fixture mode the fixture folder.
    pub root: PathBuf,
    /// The volume's serial number, from the app's own handle on `root`.
    pub volume_serial: u64,
    /// The file id of `root` when it isn't the volume's root (fixture mode): only its subtree is kept.
    pub subtree: Option<u64>,
    /// The volume's capacity in bytes, from the OS.
    pub capacity: u64,
}

/// How far the read has got, for the HUD before any folder is listed.
pub type IndexProgress = Progress;

/// A folder announced by a listing and not yet listed: where it goes in the snapshot, if it is kept.
type Pending = HashMap<u64, Option<DirId>>;

/// Reads a whole stream into a snapshot. `on_index` hears how far reading the index has got; `progress` is called
/// as a scan's is, about every [`ScanOptions::progress_every`], while the folders are recorded.
pub fn snapshot_from_stream(
    source: impl Read,
    expect: &Expect,
    options: &ScanOptions,
    on_index: &mut dyn FnMut(&IndexProgress),
    progress: &mut dyn FnMut(&ScanProgress<'_>),
    cancel: &CancelToken,
) -> Result<Snapshot, FastError> {
    let started = Instant::now();
    let started_at = SystemTime::now();
    let mut dec = Decoder::new(source);
    let header = match dec.next_frame()? {
        Some(Frame::Header(h)) => h,
        Some(Frame::Error(code)) => return Err(reader_error(code)),
        _ => return Err(bad("no header")),
    };
    check_header(&header, expect)?;

    let builder = Mutex::new(Builder::new(options.keep_files_from, started_at));
    let links = LinkSet::new();
    // Every file once, for the bound on what is counted (the collector keeps its own set for the listing).
    let seen = LinkSet::new();
    let now = unix_now();
    let mut state = State {
        pending: HashMap::new(),
        announced: HashSet::new(),
        locked_pending: HashMap::new(),
        locked: Vec::new(),
        current: None,
        listing: Listing::default(),
        subdirs: Vec::new(),
        name: Vec::new(),
        entries_here: 0,
        dirs: 0,
        entries: 0,
        unique_bytes: 0,
        root: None,
        records_bytes: None,
        ended: false,
    };
    let files = AtomicU64::new(0);
    let folders = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let mut next_report = Instant::now();
    let empty = PathBuf::new();
    let mut report = |force: bool| {
        if force || Instant::now() >= next_report {
            next_report = Instant::now() + options.progress_every.max(Duration::from_millis(1));
            progress(&ScanProgress {
                files: files.load(Ordering::Relaxed),
                folders: folders.load(Ordering::Relaxed),
                bytes: bytes.load(Ordering::Relaxed),
                unreadable: 0,
                current: &empty,
                elapsed: started.elapsed(),
                builder: &builder,
            });
        }
    };

    loop {
        if cancel.is_cancelled() {
            return Err(FastError::Cancelled);
        }
        let Some(frame) = dec.next_frame()? else {
            break;
        };
        if state.ended {
            return Err(bad("frames after the end"));
        }
        match frame {
            Frame::Header(_) => return Err(bad("a second header")),
            Frame::Error(code) => return Err(reader_error(code)),
            Frame::Progress(p) => {
                if state.dirs > 0 {
                    return Err(bad("progress after the listing began"));
                }
                on_index(&p);
            }
            Frame::Dir { frn } => {
                if state.current.is_some() {
                    return Err(bad("a folder inside a folder"));
                }
                let into = if state.dirs == 0 {
                    // The volume's root first, as the reader walks from it.
                    if frn & RECORD_MASK != ROOT_RECORD {
                        return Err(bad("the listing doesn't start at the root"));
                    }
                    state.announced.insert(frn);
                    if expect.subtree.is_none() {
                        let id = lock(&builder).add_root(
                            expect.root.clone(),
                            expect.root.clone(),
                            RootKind::Volume,
                            Some(expect.volume_serial),
                        );
                        state.root = Some(id);
                        Some(id)
                    } else {
                        None
                    }
                } else {
                    state
                        .pending
                        .remove(&frn)
                        .ok_or_else(|| bad("a folder nobody announced, or listed twice"))?
                };
                state.current = Some((frn, into));
                state.listing.clear();
                state.subdirs.clear();
                state.entries_here = 0;
            }
            Frame::Entries(entries) => {
                let Some((_, into)) = state.current else {
                    return Err(bad("entries outside a folder"));
                };
                let mut collector = Collector::new(
                    &mut state.listing,
                    &links,
                    options.keep_files_from,
                    now,
                    cancel,
                );
                for e in entries {
                    let e = e?;
                    state.entries_here += 1;
                    // Every name is a record's: a real index has a few names a record at most (hard links). A
                    // stream with far more is not an index, and would only make the app hold more.
                    if state.entries.saturating_add(state.entries_here)
                        > header.records.saturating_mul(4)
                    {
                        return Err(bad("more names than the index has room for"));
                    }
                    e.name_into(&mut state.name);
                    let name = checked_name(&state.name)?;
                    let kind = checked_kind(e.attributes, e.tag, e.flags)?;
                    let file = kind == Kind::File;
                    if file && e.allocated > expect.capacity {
                        return Err(bad("a file bigger than the drive"));
                    }
                    let entry_kind = match kind {
                        Kind::File => EntryKind::File,
                        Kind::Dir => EntryKind::Dir,
                        Kind::CloudDir => EntryKind::CloudDir,
                        Kind::DirLink => EntryKind::DirLink,
                        Kind::FileLink => EntryKind::FileLink,
                    };
                    let record = e.frn & RECORD_MASK;
                    if record < 16 || record >= header.records {
                        return Err(bad("an entry outside the index"));
                    }
                    if file && seen.first_sighting(LinkKey::Index(record)) {
                        state.unique_bytes = state.unique_bytes.saturating_add(e.allocated);
                        if state.unique_bytes > expect.capacity {
                            return Err(bad("more files than fit on the drive"));
                        }
                    }
                    let identity = matches!(kind, Kind::File | Kind::Dir | Kind::CloudDir)
                        .then_some(u128::from(e.frn));
                    let entry = Entry {
                        name: name.as_os_str(),
                        kind: entry_kind,
                        allocated: if file { e.allocated } else { 0 },
                        logical: if file { e.logical } else { 0 },
                        modified: unix_time(e.modified),
                        accessed: unix_time(e.accessed),
                        link_key: file.then_some(LinkKey::Index(record)),
                        identity,
                        key: u128::from(e.frn),
                    };
                    if into.is_some() {
                        collector.add(&entry);
                    }
                    // One per folder the collector records, links too, in its order.
                    if matches!(kind, Kind::Dir | Kind::CloudDir | Kind::DirLink) {
                        state
                            .subdirs
                            .push(kind.entered().then_some((e.frn, e.flags)));
                    }
                }
            }
            Frame::DirEnd { frn, count } => {
                let Some((dir_frn, into)) = state.current.take() else {
                    return Err(bad("the end of no folder"));
                };
                if frn != dir_frn || count != state.entries_here {
                    return Err(bad("a folder's end doesn't match it"));
                }
                state.entries = state.entries.saturating_add(count);
                state.dirs += 1;
                if state.dirs > header.records {
                    return Err(bad("more folders than records"));
                }
                let ids = match into {
                    Some(dir) => {
                        let mut b = lock(&builder);
                        let range = b.commit(dir, &state.listing);
                        files.fetch_add(u64::from(state.listing.own_files), Ordering::Relaxed);
                        bytes.fetch_add(state.listing.own_bytes, Ordering::Relaxed);
                        folders.fetch_add(range.len() as u64, Ordering::Relaxed);
                        Some(range)
                    }
                    None => None,
                };
                let mut subs = std::mem::take(&mut state.subdirs);
                for (i, sub) in subs.drain(..).enumerate() {
                    let Some((sub_frn, flags)) = sub else {
                        continue;
                    };
                    if !state.announced.insert(sub_frn) {
                        return Err(bad("a folder in two places"));
                    }
                    let id = ids.as_ref().map(|r| DirId(r.start + i as u32));
                    // In fixture mode the fixture folder becomes the snapshot's root when it is announced.
                    let id = match (id, expect.subtree) {
                        (None, Some(root)) if root == sub_frn => {
                            let r = lock(&builder).add_root(
                                expect.root.clone(),
                                expect.root.clone(),
                                RootKind::Volume,
                                Some(expect.volume_serial),
                            );
                            state.root = Some(r);
                            Some(r)
                        }
                        (id, _) => id,
                    };
                    if flags & ENTRY_LOCKED != 0 {
                        if let Some(id) = id {
                            lock(&builder).flag(id, DirFlags::UNREADABLE);
                        }
                        state.locked_pending.insert(sub_frn, id);
                    } else {
                        state.pending.insert(sub_frn, id);
                    }
                }
                state.subdirs = subs;
                report(false);
            }
            Frame::Locked(l) => {
                if state.current.is_some() {
                    return Err(bad("locked totals inside a folder"));
                }
                let id = state
                    .locked_pending
                    .remove(&l.frn)
                    .ok_or_else(|| bad("totals for a folder that isn't locked"))?;
                if l.bytes > expect.capacity || l.folders == 0 || l.folders > header.records {
                    return Err(bad("locked totals that don't fit"));
                }
                if let Some(dir) = id {
                    state.locked.push(LockedFolder {
                        dir,
                        bytes: l.bytes,
                        files: l.files,
                        folders: l.folders,
                    });
                }
            }
            Frame::Records { bytes } => {
                if state.records_bytes.is_some() || bytes > expect.capacity {
                    return Err(bad("records that don't fit"));
                }
                state.records_bytes = Some(bytes);
            }
            Frame::End(end) => {
                if state.current.is_some()
                    || !state.pending.is_empty()
                    || !state.locked_pending.is_empty()
                    || end.dirs != state.dirs
                    || end.entries != state.entries
                {
                    return Err(bad("the stream ended before everything was listed"));
                }
                state.ended = true;
            }
        }
    }
    if !state.ended {
        return Err(bad("the stream stopped early"));
    }
    let Some(_) = state.root else {
        return Err(bad("the folder to scan wasn't in the index"));
    };
    let locked_bytes = state
        .locked
        .iter()
        .fold(0u64, |n, l| n.saturating_add(l.bytes));
    let records_bytes = if expect.subtree.is_some() {
        // The fixture is a folder: NTFS's records belong to the volume, not to it.
        0
    } else {
        state.records_bytes.unwrap_or(0)
    };
    if state
        .unique_bytes
        .saturating_add(locked_bytes)
        .saturating_add(records_bytes)
        > expect.capacity.saturating_add(expect.capacity / 100)
    {
        return Err(bad("more counted than the drive holds"));
    }
    report(true);
    let mut builder = builder.into_inner().unwrap_or_else(|p| p.into_inner());
    builder.set_fast(FastFacts {
        locked: state.locked,
        records_bytes,
    });
    Ok(builder.finish(started.elapsed()))
}

struct State {
    pending: Pending,
    announced: HashSet<u64>,
    locked_pending: Pending,
    locked: Vec<LockedFolder>,
    /// The folder being listed, and where it goes.
    current: Option<(u64, Option<DirId>)>,
    listing: Listing,
    /// The folder being listed's subfolders, in the collector's order: entered and locked ones with their file ids
    /// and flags, links as `None`.
    subdirs: Vec<Option<(u64, u8)>>,
    name: Vec<u16>,
    entries_here: u64,
    dirs: u64,
    entries: u64,
    /// Allocated bytes of the files seen, each once.
    unique_bytes: u64,
    root: Option<DirId>,
    records_bytes: Option<u64>,
    ended: bool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn reader_error(code: ErrorCode) -> FastError {
    match code {
        ErrorCode::NotNtfs => FastError::NotNtfs,
        ErrorCode::OpenVolume => bad("the drive couldn't be opened"),
        ErrorCode::Unreadable => bad("the reader couldn't make sense of it"),
        ErrorCode::Access => bad("folders' permissions couldn't be checked"),
        ErrorCode::Other(n) => FastError::Unreadable(format!("the reader failed ({n})")),
    }
}

fn check_header(h: &Header, expect: &Expect) -> Result<(), FastError> {
    if h.volume_serial != expect.volume_serial {
        return Err(bad("the index is another drive's"));
    }
    if !h.cluster_size.is_power_of_two() || !(512..=(2 << 20)).contains(&h.cluster_size) {
        return Err(bad("the cluster size"));
    }
    let size = u64::from(h.cluster_size).saturating_mul(h.total_clusters);
    // The boot sector's size and the OS's capacity differ by a few sectors at most (NTFS keeps a copy of its boot
    // sector past the last cluster): a stream for a volume of another size is another volume's.
    let slack = expect.capacity / 100 + (1 << 20);
    if size.abs_diff(expect.capacity) > slack || h.records == 0 || h.records > size / 512 {
        return Err(bad("the drive's size"));
    }
    Ok(())
}

/// A name from the stream, refused if it could change a path: empty, `.`, `..`, or with a separator or NUL.
fn checked_name(units: &[u16]) -> Result<OsString, FastError> {
    let dot = u16::from(b'.');
    if units.is_empty()
        || units == [dot]
        || units == [dot, dot]
        || units
            .iter()
            .any(|&u| u == 0 || u == u16::from(b'\\') || u == u16::from(b'/'))
    {
        return Err(bad("a name that isn't one"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        Ok(OsString::from_wide(units))
    }
    #[cfg(not(windows))]
    {
        // Only tests read streams off Windows; names there are kept as UTF-8.
        Ok(OsString::from(String::from_utf16_lossy(units)))
    }
}

/// What an entry is, checked against the reader's flags.
fn checked_kind(attributes: u32, tag: u32, flags: u8) -> Result<Kind, FastError> {
    let kind = classify(attributes, tag);
    let is_dir = matches!(kind, Kind::Dir | Kind::CloudDir | Kind::DirLink);
    if is_dir != (flags & ENTRY_DIR != 0) {
        return Err(bad("an entry that is and isn't a folder"));
    }
    let enter = flags & ENTRY_ENTER != 0;
    let locked = flags & ENTRY_LOCKED != 0;
    let ok = if kind.entered() {
        enter != locked
    } else {
        !enter && !locked
    };
    if !ok || flags & !(ENTRY_DIR | ENTRY_ENTER | ENTRY_LOCKED) != 0 {
        return Err(bad("an entry whose flags don't match it"));
    }
    Ok(kind)
}

/// A FILETIME as Unix seconds; zero when unset (as the normal scan reads listings).
fn unix_time(filetime: i64) -> crate::snapshot::UnixTime {
    const TICKS: i64 = 10_000_000;
    const EPOCH_GAP: i64 = 11_644_473_600;
    if filetime <= 0 {
        0
    } else {
        (filetime / TICKS - EPOCH_GAP).max(0)
    }
}

/// How a fast scan's snapshot agrees with a normal scan's of the same drive, folder by folder (matched by file
/// id): for the comparison a person runs (`examples/fast_compare.rs`), never for anything Vacancy decides.
#[derive(Clone, Debug, Default)]
pub struct Agreement {
    /// Folders in both, in only the normal scan, in only the fast scan.
    pub folders_both: u64,
    pub only_normal: u64,
    pub only_fast: u64,
    /// Of the folders in both and listed by both: how many list exactly the same entries (the digests the executor
    /// checks), and the same bytes and files of their own.
    pub listed_both: u64,
    pub digests_equal: u64,
    pub own_equal: u64,
    /// A few folders that differ, with how.
    pub examples: Vec<String>,
}

/// Compares two snapshots of the same drive: see [`Agreement`].
pub fn agreement(normal: &Snapshot, fast: &Snapshot) -> Agreement {
    let mut by_id: HashMap<u128, DirId> = HashMap::with_capacity(normal.dir_count());
    for d in normal.dirs() {
        let id = if normal.parent(d).is_none() {
            u128::MAX
        } else {
            match normal.identity(d) {
                Some(id) => id,
                None => continue,
            }
        };
        by_id.insert(id, d);
    }
    let mut a = Agreement::default();
    let mut seen = 0u64;
    for f in fast.dirs() {
        let id = if fast.parent(f).is_none() {
            u128::MAX
        } else {
            match fast.identity(f) {
                Some(id) => id,
                None => continue,
            }
        };
        let Some(&n) = by_id.get(&id) else {
            a.only_fast += 1;
            continue;
        };
        seen += 1;
        a.folders_both += 1;
        let listed = |s: &Snapshot, d: DirId| s.flags(d).entered();
        if !listed(normal, n) || !listed(fast, f) {
            continue;
        }
        a.listed_both += 1;
        let (dn, df) = (normal.digests(n), fast.digests(f));
        let (on, of) = (normal.own(n), fast.own(f));
        if dn == df {
            a.digests_equal += 1;
        }
        if (on.bytes, on.files) == (of.bytes, of.files) {
            a.own_equal += 1;
        }
        if (dn != df || (on.bytes, on.files) != (of.bytes, of.files)) && a.examples.len() < 40 {
            a.examples.push(format!(
                "{}: {} files {} bytes (normal) vs {} files {} bytes (fast){}",
                normal.path(n).display(),
                on.files,
                on.bytes,
                of.files,
                of.bytes,
                if dn == df { "" } else { "; entries differ" }
            ));
        }
    }
    a.only_normal = by_id.len() as u64 - seen;
    a
}

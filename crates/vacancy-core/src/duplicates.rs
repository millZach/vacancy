//! The duplicate finder (ticket 09): exact copies of the user's own files, proposed for review as "Duplicate files",
//! keeping one copy of each.
//!
//! It runs after the scan, in the background, on the scan's snapshot: the overview is on screen long before it is
//! done, and it can be stopped at any point between two reads.
//!
//! **What it looks at.** Files of [`MIN_BYTES`] (1 MiB) or more, at their length, among the files the scan kept one
//! by one (those taking 1 MiB or more on disk: a file compressed or sparse to less is never looked at), in the
//! user's personal folders: Pictures, Videos
//! (Movies on macOS, and a Videos folder if one is there too), Documents, Downloads and the Desktop, found by
//! identity like every configured place. Never:
//! - anything the Guard protects, or a category it locks (the OS, a Photos library, cloud-sync folders);
//! - a cloud-sync folder, by flag, by category or by name (OneDrive, iCloud Drive, Dropbox, Google Drive, Box, as the
//!   categorizer names them), so nothing disappears from the user's other devices;
//! - the inside of a macOS package (a Photos or iMovie library, an app, a bundle), a folder whose name starts with a
//!   dot, or developer folders (a Git repository, `node_modules`), where a file belongs to something bigger;
//! - a file another proposal already deletes, or one inside a folder another proposal takes, so no byte is counted
//!   twice and no proposal can delete a copy this one keeps;
//! - links: the scanner never entered one, and every read goes through a chain opened without following one.
//!
//! **How copies are found.** By length first (the scan's own figure), then by a SHA-256 of the first 64 KiB, then by
//! a SHA-256 of the whole file. Only files still alike after each step are read further, so most files are never
//! read at all. Every file is opened by its identity (see [`crate::fsid::open_scanned`]) and must be the file the
//! scan listed, at the length it listed, unchanged since the scan began, and have only one name: hard links are one
//! file under two names, not two copies, and deleting a name frees nothing. A file that changes while it is read is
//! dropped. Contents are read for hashing only: nothing about them is kept, shown or sent anywhere.
//!
//! A file not all on this disk is never read, so hashing never downloads or recalls anything: on Windows one marked
//! offline or recalled on access (cloud placeholders, tiered storage), on macOS a dataless one (File Provider).
//! Files the Windows Overlay Filter compresses and Data Deduplication keeps are read like any other. On macOS a file
//! that shares its data with another (an APFS clone, as Finder's Duplicate makes) is left out like a hard link:
//! deleting it would free next to nothing. ReFS block clones on Windows can't be told, so the space a group frees is
//! at most its extra copies' size.
//!
//! **Which copy stays (the keeper rule).** In each group of identical files Vacancy keeps one copy:
//! 1. the tidiest: a copy that is not in Downloads and not named like a copy ("copy", "- Copy", "Copy of", "(1)")
//!    beats one that is, and one with neither mark beats one with both;
//! 2. among the tidiest, the oldest (the earliest of its created and modified times, so a copy made later, which
//!    keeps the original's modified time on Windows, still loses to the original);
//! 3. then the shorter path, then the path's order, so the choice is always the same.
//!
//! The group says why in a few words ("Kept: oldest, in Pictures"). The user can untick any group, and an unticked
//! group deletes nothing.
//!
//! **Deleting** goes through the planner and the executor like any other proposal: each extra copy is a file target
//! with its fingerprint, and before it goes the executor checks that the copy being kept is still there, the same
//! file at the same size, unchanged since the scan. If it isn't, nothing more in that group is deleted.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::categorize::{CHECK_EVERY, CategoryId, Env, ItemRef, sync_provider};
use crate::fsid::{self, Identity, Walker};
use crate::guard::Guard;
use crate::map::{Overview, ScannedVolume};
use crate::rules::{Claims, Proposal, ProposalAction, ProposalSource, Risk};
use crate::scan::CancelToken;
use crate::snapshot::{DirFlags, DirId, FileId, Snapshot};
use crate::text::group_digits;

/// The duplicates proposal's id.
pub const ID: &str = "duplicates";

/// Files smaller than this are never candidates: 1 MiB.
pub const MIN_BYTES: u64 = 1 << 20;

/// How much of each file the second pass hashes.
const PARTIAL_BYTES: u64 = 64 * 1024;

/// How much is read at a time. One buffer, reused: memory stays the same however big the files are.
const CHUNK: usize = 1 << 20;

/// How often progress is reported.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Sample extra copies the proposal names.
const SAMPLES: usize = 5;

/// What kind of file a group is, from its name's extension, for its icon and for counting photos and videos first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FileKind {
    Photo,
    Video,
    Audio,
    Document,
    Archive,
    Other,
}

impl FileKind {
    /// The kind a file name's extension says.
    pub fn of(name: &str) -> Self {
        let ext = match name.rfind('.') {
            Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
            _ => return FileKind::Other,
        };
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "heic" | "heif" | "gif" | "bmp" | "tif" | "tiff" | "webp"
            | "raw" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "rw2" | "raf" | "srw"
            | "psd" | "avif" | "jxl" => FileKind::Photo,
            "mp4" | "mov" | "m4v" | "avi" | "mkv" | "wmv" | "webm" | "mts" | "m2ts" | "3gp"
            | "mpg" | "mpeg" | "flv" | "vob" | "insv" | "lrv" => FileKind::Video,
            "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "aif" | "aiff" | "wma" | "opus"
            | "alac" | "caf" => FileKind::Audio,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
            | "rtf" | "txt" | "pages" | "numbers" | "key" | "epub" | "csv" | "md" | "psb"
            | "ai" | "indd" | "sketch" | "fig" => FileKind::Document,
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "iso" | "img" => {
                FileKind::Archive
            }
            _ => FileKind::Other,
        }
    }
}

/// One copy in a duplicate group, as the duplicates panel shows it: where it is and how big, never what is in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateCopy {
    /// Its path.
    pub path: String,
    /// Its file name.
    pub name: String,
    /// The personal folder it is in, by name ("Pictures", "Downloads").
    pub folder: String,
    /// Its folder below the home folder ("Pictures\2025\Lisbon").
    pub dir: String,
    /// Allocated bytes.
    #[ts(type = "number")]
    pub bytes: u64,
    /// When it was last modified, in milliseconds since the Unix epoch.
    #[ts(type = "number | null")]
    pub modified: Option<i64>,
    /// The map tile it is drawn in: for the kept copy the tile of the folder it lives in, for an extra copy the
    /// duplicates tile of its personal folder.
    pub tile: String,
}

/// A group of identical files: the copy Vacancy keeps, the extra copies it deletes, and why it keeps that one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateGroup {
    /// Names the group to the planner, to leave it out.
    pub id: String,
    pub kind: FileKind,
    /// The kept copy's name.
    pub name: String,
    /// One copy's length in bytes.
    #[ts(type = "number")]
    pub bytes: u64,
    /// What deleting the extra copies frees: their allocated bytes.
    #[ts(type = "number")]
    pub freed: u64,
    pub keep: DuplicateCopy,
    pub extras: Vec<DuplicateCopy>,
    /// "Kept: oldest, in Pictures".
    pub reason: String,
}

/// What the duplicate finder found, for the duplicates panel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Duplicates {
    /// The proposal they make, if any.
    pub proposal: Option<String>,
    /// Biggest saving first.
    pub groups: Vec<DuplicateGroup>,
    /// The personal folders it looked in, by name.
    pub folders: Vec<String>,
    /// Files compared by contents.
    #[ts(type = "number")]
    pub compared: u64,
    /// Files left out because they have other names (hard links): one file, not two copies.
    #[ts(type = "number")]
    pub hard_linked: u64,
    /// Files left out because they changed since the scan, or couldn't be read.
    #[ts(type = "number")]
    pub changed: u64,
    /// Files left out because part of them isn't on this disk (in the cloud, or offline): reading them would
    /// download or recall them.
    #[ts(type = "number")]
    pub not_local: u64,
    /// Files left out because they share their data with another file (APFS clones): deleting one frees next to
    /// nothing.
    #[ts(type = "number")]
    pub cloned: u64,
}

/// Which pass the duplicate finder is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DuplicateStage {
    /// Reading the start of each file that shares its length with another.
    Starts,
    /// Hashing whole files whose starts match.
    Contents,
}

/// How far the duplicate finder has got, for the map's progress pill.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateProgress {
    pub stage: DuplicateStage,
    /// Files in this pass.
    #[ts(type = "number")]
    pub files: u64,
    /// Files this pass has finished.
    #[ts(type = "number")]
    pub done: u64,
    /// The whole search, 0 to 100.
    pub percent: u32,
}

/// How a duplicate search ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum DuplicatesOutcome {
    /// It finished: the map and the proposals again, with the duplicates' tiles and proposal (if it found any).
    Done {
        overview: Box<Overview>,
        proposals: Vec<Proposal>,
        /// The Tidy up sorts again: a sort leaves out an extra copy the plan deletes (delete wins over sort).
        tidy: Vec<crate::tidy::TidyProposal>,
        duplicates: Duplicates,
    },
    /// It was stopped (a new scan, a run, the drive gone): nothing changed.
    Cancelled,
}

/// The search was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the duplicate search was stopped")]
pub struct Cancelled;

/// A personal folder the search looks in.
#[derive(Clone, Debug)]
struct Root {
    dir: DirId,
    name: String,
    downloads: bool,
}

/// A file the search may compare: one the scan kept, in a personal folder, nothing else's.
#[derive(Clone, Debug)]
struct Cand {
    file: FileId,
    path: PathBuf,
    identity: Identity,
    logical: u64,
    alloc: u64,
    root: usize,
}

/// A duplicate search, ready to run off the scan: everything it needs, owned, so the snapshot can be lent to others
/// while it reads (see [`search`]).
#[derive(Debug)]
pub struct Search {
    cands: Vec<Cand>,
    guard: Guard,
    /// A change at or after this moment (nanoseconds since the epoch) is since the scan.
    threshold: i128,
    roots: Vec<Root>,
    home: PathBuf,
}

/// What a search found. Attach it to the scan it was made from with [`attach`].
#[derive(Clone, Debug, Default)]
pub struct Found {
    groups: Vec<Group>,
    roots: Vec<Root>,
    home: PathBuf,
    compared: u64,
    hard_linked: u64,
    changed: u64,
    not_local: u64,
    cloned: u64,
}

impl Found {
    /// How many groups of copies it found.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Whether it found none.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// One copy of a found group, with what the keeper rule goes by.
#[derive(Clone, Debug)]
struct Member {
    cand: Cand,
    /// The earliest of its created and modified times, in seconds since the epoch.
    born: i64,
    /// Its modified time, in seconds since the epoch.
    modified: i64,
}

#[derive(Clone, Debug)]
struct Group {
    keep: Member,
    extras: Vec<Member>,
    reason: String,
}

/// The duplicates as the scan keeps them, for the map's tiles and the planner.
#[derive(Clone, Debug, Default)]
pub(crate) struct Attached {
    pub(crate) groups: Vec<AttachedGroup>,
    /// One tile per personal folder holding extra copies.
    pub(crate) tiles: Vec<DupTile>,
    /// Each extra copy's group, by index into `groups`.
    by_file: HashMap<FileId, usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct AttachedGroup {
    pub(crate) id: String,
    pub(crate) kind: FileKind,
    pub(crate) keep: FileId,
    pub(crate) extras: Vec<FileId>,
    /// Each extra copy's tile, by index into [`Attached::tiles`], in `extras`' order.
    pub(crate) extra_tiles: Vec<usize>,
}

/// A map tile of the duplicates proposal: the extra copies in one personal folder.
#[derive(Clone, Debug)]
pub(crate) struct DupTile {
    pub(crate) dir: DirId,
    pub(crate) category: CategoryId,
    pub(crate) targets: Vec<ItemRef>,
    /// "Extra copies in Downloads".
    pub(crate) note: String,
}

impl Attached {
    /// The group an extra copy belongs to.
    pub(crate) fn group_of(&self, file: FileId) -> Option<(usize, &AttachedGroup)> {
        let i = *self.by_file.get(&file)?;
        Some((i, &self.groups[i]))
    }

    /// A tile's id on the map.
    pub(crate) fn tile_id(&self, tile: usize) -> String {
        format!("p:{ID}:d{}", self.tiles[tile].dir.index())
    }
}

/// Prepares a duplicate search of a finished scan of the drive whose places `env` names: which files to compare.
/// Quick (no file is read), so it may run while the scan is held; [`Search::run`] then reads without it.
pub fn search(scanned: &ScannedVolume, env: &Env) -> Search {
    search_until(scanned, env, &|| false).expect("a search that is never stopped is prepared")
}

/// [`search`], giving up (`None`) as soon as `stop` says so.
pub fn search_until(scanned: &ScannedVolume, env: &Env, stop: &dyn Fn() -> bool) -> Option<Search> {
    let snap = &scanned.snapshot;
    let cats = &scanned.categories;
    let guard = Guard::new(env, &env.root);
    let roots = personal_roots(snap, env);
    let threshold = crate::exec::threshold(snap.started());

    // Which personal folder each folder is in, and whether it is one the search never looks in.
    let n = snap.dir_count();
    let mut root_of: Vec<Option<usize>> = vec![None; n];
    let mut barred = vec![false; n];
    for dir in snap.dirs() {
        if dir.index() % CHECK_EVERY == 0 && stop() {
            return None;
        }
        let i = dir.index();
        let parent = snap.parent(dir);
        root_of[i] = roots
            .iter()
            .position(|r| r.dir == dir)
            .or_else(|| parent.and_then(|p| root_of[p.index()]));
        if root_of[i].is_none() {
            continue;
        }
        barred[i] = parent.is_some_and(|p| barred[p.index()] && root_of[p.index()].is_some())
            || barred_folder(snap, cats.category_of(dir), dir, &guard);
    }

    // What other proposals delete: never a candidate, so no byte is counted twice.
    let mut claims = Claims::new(n);
    let proposals = &scanned.proposals;
    for (i, p) in proposals.list().iter().enumerate() {
        if p.action == ProposalAction::Delete && p.id != ID {
            for &t in proposals.targets(i) {
                claims.take(snap, t);
            }
        }
    }

    let mut cands = Vec::new();
    for index in 0..snap.file_count() {
        if index % CHECK_EVERY == 0 && stop() {
            return None;
        }
        let Some(file) = snap.file_at(index) else {
            break;
        };
        let info = snap.file(file);
        let Some(root) = root_of[info.parent.index()] else {
            continue;
        };
        let logical = snap.file_logical(file);
        if logical < MIN_BYTES
            || barred[info.parent.index()]
            || info.name.as_encoded_bytes().first() == Some(&b'.')
            || cats.category_of_file(snap, file).protected()
            || claims.overlaps(snap, ItemRef::File(file))
        {
            continue;
        }
        let Some(identity) = snap.file_full_identity(file) else {
            continue;
        };
        cands.push(Cand {
            file,
            path: snap.file_path(file),
            identity,
            logical,
            alloc: info.bytes,
            root,
        });
    }
    Some(Search {
        cands,
        guard,
        threshold,
        roots,
        home: env.home.clone(),
    })
}

/// The personal folders on this scan, by identity: a folder spelled like one but another folder isn't one.
fn personal_roots(snap: &Snapshot, env: &Env) -> Vec<Root> {
    let k = &env.known;
    let mut places: Vec<(&Path, bool)> = vec![
        (k.pictures.as_path(), false),
        (k.videos.as_path(), false),
        (k.documents.as_path(), false),
        (k.downloads.as_path(), true),
        (k.desktop.as_path(), false),
    ];
    // macOS calls the videos folder Movies; a Videos folder people made themselves counts too.
    let videos = env.home.join("Videos");
    if env.platform == crate::Platform::MacOs && k.videos != videos {
        places.push((videos.as_path(), false));
    }
    let mut out: Vec<Root> = Vec::new();
    for (path, downloads) in places {
        let Some(dir) = snap.find(path) else {
            continue;
        };
        let Some(scanned) = snap.dir_identity(dir) else {
            continue;
        };
        if fsid::identity(path) != Some(scanned) || out.iter().any(|r| r.dir == dir) {
            continue;
        }
        out.push(Root {
            dir,
            name: snap.name(dir).to_string_lossy().into_owned(),
            downloads,
        });
    }
    out
}

/// Extensions of macOS packages and libraries: folders that are one thing to the user, whose files belong to it.
const PACKAGES: [&str; 22] = [
    "app",
    "bundle",
    "framework",
    "plugin",
    "photoslibrary",
    "photolibrary",
    "migratedphotolibrary",
    "aplibrary",
    "musiclibrary",
    "tvlibrary",
    "imovielibrary",
    "fcpbundle",
    "theater",
    "lrlibrary",
    "lrdata",
    "logicx",
    "band",
    "sparsebundle",
    "xcodeproj",
    "xcworkspace",
    "rtfd",
    "pkg",
];

/// Whether the search never looks inside this folder (see the module docs).
fn barred_folder(snap: &Snapshot, category: CategoryId, dir: DirId, guard: &Guard) -> bool {
    let flags = snap.flags(dir);
    if category.protected()
        || category == CategoryId::Dev
        || flags.contains(DirFlags::CLOUD)
        || !flags.entered()
    {
        return true;
    }
    if snap.parent(dir).is_none() {
        return false;
    }
    let name = snap.name(dir);
    if name.as_encoded_bytes().first() == Some(&b'.') || guard.protected_os_name(name).is_some() {
        return true;
    }
    let Some(name) = name.to_str() else {
        return false;
    };
    sync_provider(name).is_some()
        || name.to_ascii_lowercase().starts_with("onedrive")
        || name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| PACKAGES.iter().any(|p| ext.eq_ignore_ascii_case(p)))
}

/// Why a file was dropped while it was read.
enum Dropped {
    /// It has other names: one file, not copies.
    HardLinked,
    /// It changed since the scan, or isn't what the scan saw, or couldn't be read.
    Changed,
    /// Part of it isn't on this disk: reading it would download or recall it.
    NotLocal,
    /// It shares its data with another file (an APFS clone).
    Cloned,
}

/// What a file's handle says about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Facts {
    len: u64,
    links: u64,
    /// The latest of its modified and change (and on Windows creation) times, in nanoseconds since the epoch.
    changed: i128,
    born: i64,
    modified: i64,
    /// All of it is on this disk: not offline, recalled on access, or dataless.
    local: bool,
    /// It shares its data with another file (an APFS clone).
    shared: bool,
}

impl Search {
    /// How many files it may compare.
    pub fn candidates(&self) -> usize {
        self.cands.len()
    }

    /// Runs the search: groups by length, then by the hash of each file's start, then by the hash of all of it.
    /// `on_progress` hears how far it has got about ten times a second; `cancel` stops it between two reads.
    pub fn run(
        self,
        on_progress: &mut dyn FnMut(DuplicateProgress),
        cancel: &CancelToken,
    ) -> Result<Found, Cancelled> {
        let mut found = Found {
            roots: self.roots.clone(),
            home: self.home.clone(),
            ..Found::default()
        };
        let mut buf = vec![0u8; CHUNK];
        let mut walker = Walker::default();
        let mut last = Instant::now();
        let mut report = |p: DuplicateProgress, force: bool| {
            if force || last.elapsed() >= PROGRESS_EVERY {
                last = Instant::now();
                on_progress(p);
            }
        };

        // 1. By length. The same file listed twice (it can't be, but identities say) counts once.
        let mut by_len: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, c) in self.cands.iter().enumerate() {
            by_len.entry(c.logical).or_default().push(i);
        }
        let mut alike: Vec<Vec<usize>> = by_len
            .into_values()
            .map(|mut g| {
                let mut seen = HashSet::new();
                g.retain(|&i| seen.insert(self.cands[i].identity));
                g
            })
            .filter(|g| g.len() > 1)
            .collect();
        alike.sort_by_key(|g| std::cmp::Reverse(self.cands[g[0]].logical));

        // 2. By the hash of the first 64 KiB.
        let files = alike.iter().map(Vec::len).sum::<usize>() as u64;
        let mut done = 0u64;
        report(progress(DuplicateStage::Starts, files, 0, 0), true);
        let mut started: Vec<Vec<(usize, Facts)>> = Vec::new();
        for group in alike {
            let mut by_start: HashMap<[u8; 32], Vec<(usize, Facts)>> = HashMap::new();
            for i in group {
                if cancel.is_cancelled() {
                    return Err(Cancelled);
                }
                let c = &self.cands[i];
                let read = self
                    .allowed(&mut walker, c)
                    .and_then(|()| self.hash(c, Some(PARTIAL_BYTES), &mut buf, cancel));
                done += 1;
                match read {
                    Ok((hash, facts)) => by_start.entry(hash).or_default().push((i, facts)),
                    Err(Some(Dropped::HardLinked)) => found.hard_linked += 1,
                    Err(Some(Dropped::Changed)) => found.changed += 1,
                    Err(Some(Dropped::NotLocal)) => found.not_local += 1,
                    Err(Some(Dropped::Cloned)) => found.cloned += 1,
                    Err(None) => return Err(Cancelled),
                }
                report(
                    progress(
                        DuplicateStage::Starts,
                        files,
                        done,
                        percent_starts(done, files),
                    ),
                    false,
                );
            }
            started.extend(by_start.into_values().filter(|g| g.len() > 1));
        }

        // 3. By the hash of everything.
        let files = started.iter().map(Vec::len).sum::<usize>() as u64;
        let bytes: u64 = started
            .iter()
            .flatten()
            .map(|&(i, _)| self.cands[i].logical)
            .sum();
        let mut done = 0u64;
        let mut read = 0u64;
        report(progress(DuplicateStage::Contents, files, 0, 15), true);
        for group in started {
            let mut by_hash: HashMap<[u8; 32], Vec<Member>> = HashMap::new();
            for (i, before) in group {
                if cancel.is_cancelled() {
                    return Err(Cancelled);
                }
                let c = &self.cands[i];
                let hashed = self.hash(c, None, &mut buf, cancel);
                done += 1;
                read += c.logical;
                found.compared += 1;
                match hashed {
                    // The same file as at the first read, still: its times say it wasn't touched in between.
                    Ok((hash, facts)) if facts == before => {
                        by_hash.entry(hash).or_default().push(Member {
                            cand: c.clone(),
                            born: facts.born,
                            modified: facts.modified,
                        });
                    }
                    Ok(_) | Err(Some(Dropped::Changed)) => found.changed += 1,
                    Err(Some(Dropped::HardLinked)) => found.hard_linked += 1,
                    Err(Some(Dropped::NotLocal)) => found.not_local += 1,
                    Err(Some(Dropped::Cloned)) => found.cloned += 1,
                    Err(None) => return Err(Cancelled),
                }
                let pct = 15 + (85 * u128::from(read) / u128::from(bytes.max(1))) as u32;
                report(
                    progress(DuplicateStage::Contents, files, done, pct.min(99)),
                    false,
                );
            }
            for copies in by_hash.into_values().filter(|g| g.len() > 1) {
                found.groups.push(keeper(copies, &self.roots));
            }
        }
        found
            .groups
            .sort_by_key(|g| std::cmp::Reverse(g.extras.iter().map(|m| m.cand.alloc).sum::<u64>()));
        report(progress(DuplicateStage::Contents, files, done, 100), true);
        Ok(found)
    }

    /// The Guard allows deleting `c` (a copy may be the one that goes), judged by the chain walked to it now, and
    /// the chain ends at the file the scan saw.
    fn allowed(&self, walker: &mut Walker, c: &Cand) -> Result<(), Option<Dropped>> {
        let chain = self
            .guard
            .check_with(walker, &c.path, false)
            .map_err(|_| Some(Dropped::Changed))?;
        match chain.last() {
            Some(s) if s.id == c.identity => Ok(()),
            _ => Err(Some(Dropped::Changed)),
        }
    }

    /// The SHA-256 of the first `limit` bytes of `c` (all of it with `None`), read through a handle opened by its
    /// identity, with what the handle says of the file. `Err(None)` if cancelled.
    fn hash(
        &self,
        c: &Cand,
        limit: Option<u64>,
        buf: &mut [u8],
        cancel: &CancelToken,
    ) -> Result<([u8; 32], Facts), Option<Dropped>> {
        let mut file =
            fsid::open_scanned(&c.path, c.identity).map_err(|_| Some(Dropped::Changed))?;
        let facts = facts(&file).ok_or(Some(Dropped::Changed))?;
        if facts.len != c.logical || facts.changed >= self.threshold {
            return Err(Some(Dropped::Changed));
        }
        if facts.links > 1 {
            return Err(Some(Dropped::HardLinked));
        }
        // Before any byte is read: nothing is downloaded or recalled.
        if !facts.local {
            return Err(Some(Dropped::NotLocal));
        }
        if facts.shared {
            return Err(Some(Dropped::Cloned));
        }
        let want = limit.unwrap_or(c.logical).min(c.logical);
        let mut hasher = Sha256::new();
        let mut got = 0u64;
        while got < want {
            if cancel.is_cancelled() {
                return Err(None);
            }
            let room = usize::try_from((want - got).min(buf.len() as u64)).unwrap_or(buf.len());
            let n = file
                .read(&mut buf[..room])
                .map_err(|_| Some(Dropped::Changed))?;
            if n == 0 {
                // Shorter than it says: it changed.
                return Err(Some(Dropped::Changed));
            }
            hasher.update(&buf[..n]);
            got += n as u64;
        }
        // Nothing changed while it was read.
        if facts_now(&file) != Some(facts) {
            return Err(Some(Dropped::Changed));
        }
        Ok((hasher.finalize().into(), facts))
    }
}

fn percent_starts(done: u64, files: u64) -> u32 {
    (15 * u128::from(done) / u128::from(files.max(1))) as u32
}

fn progress(stage: DuplicateStage, files: u64, done: u64, percent: u32) -> DuplicateProgress {
    DuplicateProgress {
        stage,
        files,
        done,
        percent,
    }
}

fn facts_now(file: &std::fs::File) -> Option<Facts> {
    facts(file)
}

/// What an open file's handle says: its length, how many names it has, and its times.
#[cfg(windows)]
fn facts(file: &std::fs::File) -> Option<Facts> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    };
    let handle = file.as_raw_handle();
    // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is valid.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is writable; the handle is open.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return None;
    }
    // SAFETY: an all-zero FILE_BASIC_INFO is valid.
    let mut basic: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: `basic` is writable and sized as passed; the handle is open.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&raw mut basic).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    // 100-nanosecond intervals since 1601.
    const EPOCH: i64 = 116_444_736_000_000_000;
    let nanos = |t: i64| i128::from(t - EPOCH) * 100;
    let secs = |t: i64| (t - EPOCH) / 10_000_000;
    Some(Facts {
        len: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        links: u64::from(info.nNumberOfLinks),
        changed: nanos(
            basic
                .CreationTime
                .max(basic.LastWriteTime)
                .max(basic.ChangeTime),
        ),
        born: secs(basic.CreationTime.min(basic.LastWriteTime)),
        modified: secs(basic.LastWriteTime),
        local: basic.FileAttributes & NOT_LOCAL == 0,
        // ReFS block clones can't be told from here.
        shared: false,
    })
}

/// Attributes of a file not all on this disk: offline, recalled when opened, or recalled when read.
#[cfg(windows)]
const NOT_LOCAL: u32 = windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_OFFLINE
    | windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_RECALL_ON_OPEN
    | windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;

/// What an open file says: its length, how many names it has, and its times.
#[cfg(unix)]
fn facts(file: &std::fs::File) -> Option<Facts> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().ok()?;
    let at = |s: i64, ns: i64| i128::from(s) * 1_000_000_000 + i128::from(ns);
    let born = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .map_or(m.mtime(), |b| b.min(m.mtime()));
    Some(Facts {
        len: m.len(),
        links: m.nlink(),
        changed: at(m.mtime(), m.mtime_nsec()).max(at(m.ctime(), m.ctime_nsec())),
        born,
        modified: m.mtime(),
        local: local(&m),
        shared: shared(file, m.blocks().saturating_mul(512)),
    })
}

/// Whether all of a file is on this disk. macOS marks a file whose data the File Provider keeps elsewhere dataless
/// (`SF_DATALESS`): reading it downloads it.
#[cfg(unix)]
fn local(m: &std::fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        /// `SF_DATALESS` in `<sys/stat.h>`.
        const SF_DATALESS: u32 = 0x4000_0000;
        m.st_flags() & SF_DATALESS == 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = m;
        true
    }
}

/// Whether a file shares its data with another (an APFS clone): its private size, the part of its allocation no
/// other file shares (`ATTR_CMNEXT_PRIVATESIZE`), is less than its allocation. Where that can't be read, it doesn't.
#[cfg(unix)]
fn shared(file: &std::fs::File, allocated: u64) -> bool {
    private_size(file).is_some_and(|private| private < allocated)
}

/// A file's private size (see [`shared`]), from `fgetattrlist`.
#[cfg(target_os = "macos")]
pub(crate) fn private_size(file: &std::fs::File) -> Option<u64> {
    use std::os::fd::AsRawFd;
    #[repr(C, packed(4))]
    struct Answer {
        length: u32,
        returned: libc::attribute_set_t,
        private: libc::off_t,
    }
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        // With FSOPT_ATTR_CMN_EXTENDED this group asks for the extended common attributes.
        forkattr: libc::ATTR_CMNEXT_PRIVATESIZE,
    };
    // SAFETY: an all-zero Answer is a valid value of plain integers.
    let mut answer: Answer = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is open; `list` and `answer` are live, writable and sized as passed.
    let rc = unsafe {
        libc::fgetattrlist(
            file.as_raw_fd(),
            (&raw mut list).cast(),
            (&raw mut answer).cast(),
            std::mem::size_of::<Answer>(),
            libc::FSOPT_ATTR_CMN_EXTENDED,
        )
    };
    let returned = answer.returned;
    if rc != 0 || returned.forkattr & libc::ATTR_CMNEXT_PRIVATESIZE == 0 {
        return None;
    }
    let private = answer.private;
    u64::try_from(private).ok()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn private_size(_file: &std::fs::File) -> Option<u64> {
    None
}

#[cfg(not(any(windows, unix)))]
fn facts(_file: &std::fs::File) -> Option<Facts> {
    None
}

/// How untidy a copy's place is: in Downloads, named like a copy, or both. Lower is tidier.
fn mess(m: &Member, roots: &[Root]) -> u8 {
    u8::from(roots[m.cand.root].downloads) + u8::from(copy_tag(&name_of(&m.cand.path)).is_some())
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The keeper rule (see the module docs): the tidiest copy, then the oldest, then the shortest path, then the
/// path's order. Says why in a few words.
fn keeper(mut copies: Vec<Member>, roots: &[Root]) -> Group {
    copies.sort_by(|a, b| {
        mess(a, roots)
            .cmp(&mess(b, roots))
            .then(a.born.cmp(&b.born))
            .then(
                a.cand
                    .path
                    .components()
                    .count()
                    .cmp(&b.cand.path.components().count()),
            )
            .then(a.cand.path.cmp(&b.cand.path))
    });
    let keep = copies.remove(0);
    let folder = &roots[keep.cand.root].name;
    let older: Vec<&Member> = copies.iter().filter(|m| m.born < keep.born).collect();
    let reason = if older.is_empty() {
        format!("Kept: oldest, in {folder}")
    } else if older.iter().any(|m| roots[m.cand.root].downloads) && !roots[keep.cand.root].downloads
    {
        format!("Kept: in {folder}, not Downloads")
    } else if let Some(tag) = older.iter().find_map(|m| copy_tag(&name_of(&m.cand.path))) {
        format!("Kept: original name, not \u{2018}{tag}\u{2019}")
    } else {
        format!("Kept: tidiest, in {folder}")
    };
    Group {
        keep,
        extras: copies,
        reason,
    }
}

/// How a file is named like a copy, if it is: "(1)", "- Copy", "copy" or "Copy of", spelled as in the name.
/// "IMG_4471 (1).jpg", "Report - Copy.docx", "Report - Copy (2).docx", "Photo copy.jpg", "Photo copy 2.jpg" (macOS)
/// and "Copy of Report.docx" are; "Photocopy.pdf" and "Scan (draft).pdf" aren't.
pub(crate) fn copy_tag(name: &str) -> Option<String> {
    let stem = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    let s = stem.trim_end();
    if let Some(tag) = copy_word(s) {
        return Some(tag);
    }
    // "(1)", "(23)" at the end.
    let open = s.rfind('(')?;
    let inner = s.get(open + 1..s.len().checked_sub(1)?)?;
    if !s.ends_with(')') || inner.is_empty() || !inner.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let rest = s[..open].trim_end();
    if rest.is_empty() {
        return None;
    }
    Some(copy_word(rest).unwrap_or_else(|| s[open..].to_owned()))
}

/// "- Copy", "copy" or "Copy of" ending (or, for "Copy of", starting) a file's stem.
fn copy_word(s: &str) -> Option<String> {
    let ends = |suffix: &str| {
        s.len() > suffix.len()
            && s.is_char_boundary(s.len() - suffix.len())
            && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    };
    if ends(" - copy") {
        return Some(s[s.len() - 6..].to_owned());
    }
    for sep in [" copy", "_copy", "-copy"] {
        if ends(sep) {
            return Some(s[s.len() - 4..].to_owned());
        }
    }
    // macOS: "Photo copy 2".
    let digits = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if digits.len() < s.len() && digits.ends_with(' ') {
        let t = digits.trim_end();
        if t.len() > 5
            && t.is_char_boundary(t.len() - 5)
            && t[t.len() - 5..].eq_ignore_ascii_case(" copy")
        {
            return Some(t[t.len() - 4..].to_owned());
        }
    }
    if s.len() > 8 && s.is_char_boundary(8) && s[..8].eq_ignore_ascii_case("copy of ") {
        return Some(s[..7].to_owned());
    }
    None
}

/// Adds what `found` found to the scan it was made from: the "Duplicate files" proposal (if there are copies), its
/// tiles on the map (one per personal folder holding extra copies, carved out of that folder's category) and the
/// overview redrawn with them. Gives what the duplicates panel shows. `found` must come from a [`search`] of this
/// scan.
pub fn attach(scanned: &mut ScannedVolume, found: Found) -> Duplicates {
    let mut out = Duplicates {
        proposal: None,
        groups: Vec::new(),
        folders: found.roots.iter().map(|r| r.name.clone()).collect(),
        compared: found.compared,
        hard_linked: found.hard_linked,
        changed: found.changed,
        not_local: found.not_local,
        cloned: found.cloned,
    };
    scanned.proposals.remove(ID);
    if found.groups.is_empty() {
        scanned.tidy.cover(&scanned.snapshot, ID, &[]);
        scanned.reshape();
        return out;
    }

    // One tile per personal folder that holds extra copies, in that folder's category.
    let mut attached = Attached::default();
    let mut tile_of_root: HashMap<usize, usize> = HashMap::new();
    for g in &found.groups {
        for m in &g.extras {
            let root = &found.roots[m.cand.root];
            let next = attached.tiles.len();
            let t = *tile_of_root.entry(m.cand.root).or_insert(next);
            if t == next {
                attached.tiles.push(DupTile {
                    dir: root.dir,
                    category: scanned.categories.category_of(root.dir),
                    targets: Vec::new(),
                    note: format!("Extra copies in {}", root.name),
                });
            }
            attached.tiles[t].targets.push(ItemRef::File(m.cand.file));
        }
    }
    let mut targets = Vec::new();
    for (i, g) in found.groups.iter().enumerate() {
        let id = format!("g{i}");
        let extras: Vec<FileId> = g.extras.iter().map(|m| m.cand.file).collect();
        for m in &g.extras {
            attached.by_file.insert(m.cand.file, i);
            targets.push(ItemRef::File(m.cand.file));
        }
        attached.groups.push(AttachedGroup {
            id,
            kind: FileKind::of(&name_of(&g.keep.cand.path)),
            keep: g.keep.cand.file,
            extra_tiles: g
                .extras
                .iter()
                .map(|m| tile_of_root[&m.cand.root])
                .collect(),
            extras,
        });
    }

    let proposal = proposal(&found);
    out.proposal = Some(proposal.id.clone());
    // Delete wins over sort: the extra copies are covered by this proposal in the sorts' reviews.
    scanned.tidy.cover(&scanned.snapshot, ID, &targets);
    scanned
        .proposals
        .insert_duplicates(proposal, targets, attached);
    scanned.reshape();

    // What the panel shows, with the tiles of the redrawn map.
    let snap = &scanned.snapshot;
    let Some(attached) = scanned.proposals.duplicates() else {
        return out;
    };
    let shown: HashSet<&str> = scanned
        .overview
        .categories
        .iter()
        .flat_map(|c| c.items.iter().map(|i| i.id.as_str()))
        .collect();
    for (g, a) in found.groups.iter().zip(&attached.groups) {
        let keep_tile = crate::map::tile_holding(snap, &scanned.categories, g.keep.cand.file)
            .filter(|id| shown.contains(id.as_str()))
            .unwrap_or_else(|| {
                let cat = scanned
                    .categories
                    .category_of(snap.file(g.keep.cand.file).parent);
                format!("more:{cat:?}").to_ascii_lowercase()
            });
        let copy = |m: &Member, tile: String| DuplicateCopy {
            path: m.cand.path.display().to_string(),
            name: name_of(&m.cand.path),
            folder: found.roots[m.cand.root].name.clone(),
            dir: m
                .cand
                .path
                .parent()
                .and_then(|p| p.strip_prefix(&found.home).ok())
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            bytes: m.cand.alloc,
            modified: (m.modified > 0).then(|| m.modified.saturating_mul(1000)),
            tile,
        };
        out.groups.push(DuplicateGroup {
            id: a.id.clone(),
            kind: a.kind,
            name: name_of(&g.keep.cand.path),
            bytes: g.keep.cand.logical,
            freed: g.extras.iter().map(|m| m.cand.alloc).sum(),
            keep: copy(&g.keep, keep_tile),
            extras: g
                .extras
                .iter()
                .zip(&a.extra_tiles)
                .map(|(m, &t)| copy(m, attached.tile_id(t)))
                .collect(),
            reason: g.reason.clone(),
        });
    }
    out
}

/// The "Duplicate files" proposal: photos and videos counted first.
fn proposal(found: &Found) -> Proposal {
    let extras: Vec<&Member> = found.groups.iter().flat_map(|g| &g.extras).collect();
    let count = |want: &dyn Fn(FileKind) -> bool| {
        extras
            .iter()
            .filter(|m| want(FileKind::of(&name_of(&m.cand.path))))
            .count() as u64
    };
    let photos = count(&|k| k == FileKind::Photo);
    let videos = count(&|k| k == FileKind::Video);
    let other = extras.len() as u64 - photos - videos;
    let mut parts = Vec::new();
    let mut n = |count: u64, one: &str, many: &str| {
        if count > 0 {
            parts.push(format!(
                "{} {}",
                group_digits(count),
                if count == 1 { one } else { many }
            ));
        }
    };
    n(photos, "photo", "photos");
    n(videos, "video", "videos");
    n(other, "other file", "other files");
    let what = match parts.as_slice() {
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        _ => String::new(),
    };
    let have = if extras.len() == 1 { "has" } else { "have" };
    let groups = found.groups.len() as u64;
    let folders = list(
        &found
            .roots
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
    );
    let mut samples: Vec<&Member> = extras.clone();
    samples.sort_by_key(|m| std::cmp::Reverse(m.cand.alloc));
    Proposal {
        id: ID.to_owned(),
        category: CategoryId::Media,
        title: "Duplicate files".into(),
        summary: format!(
            "{what} {have} an exact copy elsewhere, in {} group{}. Vacancy keeps one copy of each.",
            group_digits(groups),
            if groups == 1 { "" } else { "s" }
        ),
        why: format!(
            "Byte-for-byte copies of files of 1 MB or more in {folders}. Vacancy keeps one copy of each group: the \
             tidiest (one outside Downloads that isn\u{2019}t named like a copy: \u{201c}copy\u{201d}, \
             \u{201c}- Copy\u{201d}, \u{201c}(1)\u{201d}), and the oldest among those. Before each extra copy goes, \
             it checks the kept copy is still there and unchanged, and holds it while the copy is deleted. Untick \
             a group to keep every copy of it. Copies in cloud-sync folders and in a Photos library are left alone, \
             and so are files that aren\u{2019}t all on this disk. The space is what the extra copies take up: up \
             to that much comes back, less where a drive shares copied files\u{2019} data."
        ),
        bytes: extras.iter().map(|m| m.cand.alloc).sum(),
        item_count: extras.len() as u64,
        place_count: extras.len() as u64,
        risk: Risk::Review,
        source: ProposalSource::Rule,
        ai_note: None,
        action: ProposalAction::Delete,
        hint_action: None,
        regenerable: false,
        sample_paths: samples
            .iter()
            .take(SAMPLES)
            .map(|m| m.cand.path.display().to_string())
            .collect(),
        default_on: false,
    }
}

/// "Pictures", "Pictures and Videos", "Pictures, Videos and Documents".
fn list(names: &[&str]) -> String {
    match names {
        [] => "your personal folders".into(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Searches a scan for duplicates and attaches what it finds, in one go: [`search`], [`Search::run`] and [`attach`].
pub fn find(
    scanned: &mut ScannedVolume,
    env: &Env,
    on_progress: &mut dyn FnMut(DuplicateProgress),
    cancel: &CancelToken,
) -> Result<Duplicates, Cancelled> {
    let found = search(scanned, env).run(on_progress, cancel)?;
    Ok(attach(scanned, found))
}

/// Whether a name is one the keeper rule reads as a copy. For tests.
#[cfg(test)]
pub(crate) fn named_like_copy(name: &std::ffi::OsStr) -> bool {
    copy_tag(&name.to_string_lossy()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture profile scanned in fixture mode, with its duplicates found.
    fn found_fixture() -> (tempfile::TempDir, Env, ScannedVolume, Duplicates) {
        let dir = tempfile::tempdir().unwrap();
        let root = if cfg!(windows) {
            dir.path().to_path_buf()
        } else {
            dir.path().canonicalize().unwrap()
        }
        .join("fixture");
        crate::fixture::build(&root).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3200));
        let env = crate::fixture::env(&root);
        let moment = crate::Moment {
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            running_apps: Some(Vec::new()),
            installed_apps: Vec::new(),
        };
        let figures = root.clone();
        let mut scanned = crate::map::scan_for_map_with(
            &root,
            &env,
            &moment,
            &move |snap| Some(crate::fixture::volume(&figures, 1 << 30, snap)),
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap();
        let found = find(&mut scanned, &env, &mut |_| {}, &CancelToken::new()).unwrap();
        (dir, env, scanned, found)
    }

    /// The planner fails closed: an extra copy whose group can't be found is refused with a reason, never planned
    /// as a plain delete.
    #[test]
    fn an_extra_copy_without_its_group_is_refused_not_deleted() {
        let (_dir, env, mut scanned, found) = found_fixture();
        assert!(!found.groups.is_empty());
        scanned.proposals.forget_duplicate_groups_for_test();
        let plan = crate::plan::plan_with(&scanned, &env, &[ID.to_owned()], &[]).unwrap();
        let summary = plan.summary();
        assert_eq!(summary.places, 0, "nothing is planned as a plain delete");
        let copies: usize = found.groups.iter().map(|g| g.extras.len()).sum();
        assert_eq!(summary.refused.len(), copies, "{:#?}", summary.refused);
        assert!(
            summary
                .refused
                .iter()
                .all(|r| r.reason.contains("which copy of it stays"))
        );
    }

    /// The kept copy is held while each extra copy is deleted. Something deleting it in that moment (the hook runs
    /// after every check of the extra copy, right before it goes) never leaves a group with no copy: on Windows the
    /// held copy can't be deleted at all; on macOS the look right before the unlink finds it gone, and the extra copy
    /// stays.
    #[test]
    fn the_kept_copy_is_held_while_the_extra_copy_goes() {
        let (_dir, env, scanned, found) = found_fixture();
        let kept_of: HashMap<PathBuf, PathBuf> = found
            .groups
            .iter()
            .flat_map(|g| {
                g.extras
                    .iter()
                    .map(move |e| (PathBuf::from(&e.path), PathBuf::from(&g.keep.path)))
            })
            .collect();
        let plan = crate::plan::plan_with(&scanned, &env, &[ID.to_owned()], &[]).unwrap();
        let hook = |extra: &Path| {
            if let Some(kept) = kept_of.get(extra) {
                // Ignored: on Windows the held copy can't be deleted.
                let _ = std::fs::remove_file(kept);
            }
        };
        let hooks = crate::exec::Hooks {
            before_delete: Some(&hook),
            ..crate::exec::Hooks::default()
        };
        let report = crate::exec::execute_inner(
            &plan,
            &env,
            crate::exec::Sorts::default(),
            &|| None,
            &mut |_| {},
            &CancelToken::new(),
            &hooks,
        );
        for g in &found.groups {
            let kept = Path::new(&g.keep.path).exists();
            let extras = g
                .extras
                .iter()
                .filter(|e| Path::new(&e.path).exists())
                .count();
            assert!(kept || extras > 0, "{} still has a copy", g.name);
            if cfg!(windows) {
                assert!(
                    kept,
                    "the held copy of {} couldn\u{2019}t be deleted",
                    g.name
                );
                assert_eq!(extras, 0, "and its extra copies went");
            } else {
                assert!(
                    !kept && extras == g.extras.len(),
                    "{}: every extra copy stays",
                    g.name
                );
            }
        }
        let d = report.duplicates.unwrap();
        if cfg!(windows) {
            assert_eq!(d.kept_whole, 0);
        } else {
            assert_eq!(d.kept_whole, d.groups);
        }
    }

    /// Clones share their data, so deleting one frees next to nothing: macOS says so through the private size.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_apfs_clone_reports_a_shared_private_size() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        std::fs::write(&a, vec![7u8; 4 << 20]).unwrap();
        let (ca, cb) = (
            CString::new(a.as_os_str().as_bytes()).unwrap(),
            CString::new(b.as_os_str().as_bytes()).unwrap(),
        );
        // SAFETY: both paths are NUL-terminated.
        if unsafe { libc::clonefile(ca.as_ptr(), cb.as_ptr(), 0) } != 0 {
            eprintln!("skipped: this volume can't clone files");
            return;
        }
        let plain = dir.path().join("c.bin");
        std::fs::write(&plain, vec![9u8; 4 << 20]).unwrap();
        use std::os::unix::fs::MetadataExt;
        let alloc = |p: &Path| std::fs::metadata(p).unwrap().blocks() * 512;
        let open = |p: &Path| std::fs::File::open(p).unwrap();
        assert!(shared(&open(&b), alloc(&b)), "a clone shares its data");
        assert!(
            !shared(&open(&plain), alloc(&plain)),
            "a plain file doesn\u{2019}t"
        );
    }

    #[test]
    fn copy_names_are_read_as_copies() {
        for (name, tag) in [
            ("IMG_4471 (1).jpg", "(1)"),
            ("IMG_4471(12).JPG", "(12)"),
            ("Report - Copy.docx", "- Copy"),
            ("Report - Copy (2).docx", "- Copy"),
            ("Photo copy.jpg", "copy"),
            ("Photo copy 2.jpg", "copy"),
            ("Resume 2026 copy.docx", "copy"),
            ("holiday_copy.mp4", "copy"),
            ("Copy of Budget.xlsx", "Copy"),
            ("GX010231 - Copy.MP4", "Copy"),
        ] {
            let got = copy_tag(name);
            assert!(got.is_some(), "{name} reads as a copy");
            assert!(
                got.as_deref()
                    .unwrap()
                    .contains(tag.trim_start_matches("- ")),
                "{name}: {got:?}"
            );
        }
        for name in [
            "Photocopy.pdf",
            "Scan (draft).pdf",
            "IMG_4471.jpg",
            "(1).jpg",
            "copy.txt",
            "Wedding highlights.mp4",
            "Lisbon 2024.jpg",
        ] {
            assert!(
                !named_like_copy(std::ffi::OsStr::new(name)),
                "{name} is an original name"
            );
        }
    }

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(FileKind::of("IMG_1.HEIC"), FileKind::Photo);
        assert_eq!(FileKind::of("clip.MOV"), FileKind::Video);
        assert_eq!(FileKind::of("master.wav"), FileKind::Audio);
        assert_eq!(FileKind::of("Tax return.pdf"), FileKind::Document);
        assert_eq!(FileKind::of("presets.zip"), FileKind::Archive);
        assert_eq!(FileKind::of("data.bin"), FileKind::Other);
        assert_eq!(FileKind::of("noextension"), FileKind::Other);
    }

    fn member(root: usize, path: &str, born: i64) -> Member {
        Member {
            cand: Cand {
                file: FileId(0),
                path: PathBuf::from(path),
                identity: Identity::new(1, born as u128),
                logical: MIB,
                alloc: MIB,
                root,
            },
            born,
            modified: born,
        }
    }

    const MIB: u64 = 1 << 20;

    fn roots() -> Vec<Root> {
        ["Pictures", "Videos", "Documents", "Downloads", "Desktop"]
            .iter()
            .enumerate()
            .map(|(i, n)| Root {
                dir: DirId(i as u32),
                name: (*n).to_owned(),
                downloads: *n == "Downloads",
            })
            .collect()
    }

    fn kept(copies: Vec<Member>) -> (String, String) {
        let g = keeper(copies, &roots());
        (g.keep.cand.path.display().to_string(), g.reason)
    }

    #[test]
    fn the_keeper_is_the_oldest_of_the_tidiest() {
        // The oldest wins among tidy copies.
        let (path, why) = kept(vec![
            member(0, "/h/Pictures/Phone/IMG_1.jpg", 200),
            member(0, "/h/Pictures/Camera/IMG_1.jpg", 100),
        ]);
        assert_eq!(path, "/h/Pictures/Camera/IMG_1.jpg");
        assert_eq!(why, "Kept: oldest, in Pictures");

        // A tidy folder beats an older copy in Downloads.
        let (path, why) = kept(vec![
            member(3, "/h/Downloads/Wedding.mp4", 50),
            member(1, "/h/Videos/2024/Wedding.mp4", 300),
        ]);
        assert_eq!(path, "/h/Videos/2024/Wedding.mp4");
        assert_eq!(why, "Kept: in Videos, not Downloads");

        // An original name beats an older copy named like one.
        let (path, why) = kept(vec![
            member(2, "/h/Documents/Tax return - Copy.pdf", 50),
            member(2, "/h/Documents/Tax return.pdf", 300),
        ]);
        assert_eq!(path, "/h/Documents/Tax return.pdf");
        assert_eq!(why, "Kept: original name, not \u{2018}- Copy\u{2019}");

        // Both marks lose to one; one loses to none, however old.
        let (path, _) = kept(vec![
            member(3, "/h/Downloads/IMG (1).jpg", 10),
            member(4, "/h/Desktop/IMG (1).jpg", 20),
            member(3, "/h/Downloads/IMG.jpg", 30),
        ]);
        assert_eq!(path, "/h/Desktop/IMG (1).jpg", "one mark beats two");

        // Same age: the shorter path, then the path's order, so the choice never changes.
        let (path, why) = kept(vec![
            member(0, "/h/Pictures/a/b/IMG.jpg", 100),
            member(0, "/h/Pictures/z/IMG.jpg", 100),
        ]);
        assert_eq!(path, "/h/Pictures/z/IMG.jpg");
        assert_eq!(why, "Kept: oldest, in Pictures");
    }
}

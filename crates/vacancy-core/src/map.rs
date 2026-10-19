//! What the UI's map shows, built from a snapshot, its categories and the volume's own numbers: the overview
//! (categories and their biggest items, free space, the part of the disk Vacancy can't see), each folder's
//! drill-down, and the live numbers while a scan runs.
//!
//! The overview is reconciled with the volume. The OS's used figure (what Explorer and Finder show) is the truth;
//! when the scan counted less, the difference (folders Vacancy may not open, restore points or snapshots, swap,
//! the file system's own records) is shown as one item in System rather than spread over guesses.
//!
//! A fast scan (Windows, with admin: see [`crate::fast`]) counts what a normal scan can't: folders this account
//! may not open, as totals only in a "Locked folders" block with no names, and NTFS's own records, as one item in
//! System. Only what it still couldn't account for is left as the unseen item.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::categorize::{
    CHECK_EVERY, Categories, CategoryId, Env, Item, ItemRef, Platform, categorize_until,
};
use crate::rules::{Moment, Proposal, Proposals, Tile, propose_until};
use crate::scan::{CancelToken, ScanError, ScanOptions, ScanProgress, scan};
use crate::snapshot::{DirFlags, FileId, RootKind, Snapshot, UnixTime};
use crate::text::group_digits;
use crate::volume::{DriveKind, VolumeInfo, volume_info};

/// Items a category shows before the rest are folded into one "smaller items" tile.
const CATEGORY_ITEMS: usize = 48;
/// Items a folder's drill-down shows before folding.
const FOLDER_ITEMS: usize = 60;
/// Unreadable folders named in the summary.
const UNREADABLE_SAMPLES: usize = 8;

/// The finished scan, as the overview map shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Overview {
    /// The volume, with the OS's own total and used figures.
    pub volume: VolumeInfo,
    /// Bytes free: total minus used, from the OS.
    #[ts(type = "number")]
    pub free_bytes: u64,
    /// What the scan found.
    pub scan: ScanSummary,
    /// Categories with anything in them, biggest first. System includes the space Vacancy can't see.
    pub categories: Vec<MapCategory>,
}

/// The scan's numbers, for the map's notes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanSummary {
    /// Files counted, each hard-linked file once.
    #[ts(type = "number")]
    pub files: u64,
    /// Folders found.
    #[ts(type = "number")]
    pub folders: u64,
    /// Allocated bytes the scan counted.
    #[ts(type = "number")]
    pub scanned_bytes: u64,
    /// Used space the scan could not see: the OS's used figure minus what was counted, if positive.
    #[ts(type = "number")]
    pub hidden_bytes: u64,
    /// How much more the scan counted than the OS reports used, if it did (files changing during the scan).
    #[ts(type = "number")]
    pub over_count_bytes: u64,
    /// Folders the OS refused to list.
    #[ts(type = "number")]
    pub unreadable: u64,
    /// A few of them, shallowest first.
    pub unreadable_samples: Vec<String>,
    /// Links seen and not followed.
    #[ts(type = "number")]
    pub links_skipped: u64,
    /// Extra names of hard-linked files, counted once.
    #[ts(type = "number")]
    pub hard_links_merged: u64,
    /// How long the scan took.
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// How the drive was read. Runs kept before fast scan existed read as normal.
    #[serde(default)]
    pub method: ScanMethod,
    /// Why a fast scan that was asked for ran as a normal scan, if it did.
    #[serde(default)]
    pub fallback: Option<FastFallback>,
    /// Fast scan only: the folders this account can't open, as totals by why, never with a name from inside them.
    /// Nothing in them is proposed or deleted.
    #[serde(default)]
    pub locked: Vec<LockedGroup>,
    /// Fast scan only: bytes NTFS keeps for itself (the file index, its log, folder indexes, alternate streams),
    /// shown as one item in System.
    #[serde(default)]
    #[ts(type = "number")]
    pub records_bytes: u64,
}

/// How a scan read the drive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScanMethod {
    /// Folder by folder, as this account.
    #[default]
    Normal,
    /// From the drive's file index, read with admin (Windows, NTFS).
    Fast,
}

/// Why a fast scan that was asked for ran as a normal scan instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FastFallback {
    /// Windows' admin prompt was answered No, or closed.
    Declined,
    /// The drive isn't NTFS (an exFAT or FAT USB stick): it has no file index to read. No prompt was shown.
    NotNtfs,
    /// The helper that reads the index couldn't be started, or stopped before it was ready.
    HelperFailed,
    /// The index couldn't be read (a drive BitLocker has locked, a damaged index) or didn't add up.
    IndexUnreadable,
}

/// Where a fast scan is, for the HUD.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FastStage {
    /// Waiting for Windows' admin prompt to be answered.
    Asking,
    /// Reading the drive's file index.
    Reading,
    /// Sorting what the index holds into categories.
    Sorting,
}

/// Why a folder is locked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LockedKind {
    /// In another account's home folder.
    OtherAccounts,
    /// Anywhere else: restore points, app packages and other folders Windows keeps to itself.
    WindowsOnly,
}

/// Folders this account can't open, for one reason: totals only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LockedGroup {
    pub kind: LockedKind,
    /// Allocated bytes.
    #[ts(type = "number")]
    pub bytes: u64,
    #[ts(type = "number")]
    pub files: u64,
    /// Folders, each locked folder and every folder inside it.
    #[ts(type = "number")]
    pub folders: u64,
}

/// How a drive was read, and what only a fast scan counts: kept with the scan, so the map drawn again later (after
/// the duplicate finder) says the same.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HowScanned {
    pub method: ScanMethod,
    pub fallback: Option<FastFallback>,
    pub locked: Vec<LockedGroup>,
    pub records_bytes: u64,
}

/// A category on the map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MapCategory {
    /// Which category.
    pub id: CategoryId,
    /// Its name on this platform.
    pub name: String,
    /// One line about it.
    pub blurb: String,
    /// Locked: shown hatched; Vacancy never touches it.
    pub protected: bool,
    /// Allocated bytes.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Its items, biggest first, the smallest folded into one.
    pub items: Vec<MapItem>,
}

/// What an item on the map is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ItemKind {
    /// A folder; it can be opened.
    Folder,
    /// One file.
    File,
    /// A folder's files that are not shown one by one.
    Files,
    /// Several small items folded together.
    More,
    /// Used space the scan could not see.
    Hidden,
    /// NTFS's own records, which only a fast scan counts.
    Records,
    /// What a cleanup proposal would remove, gathered from wherever it is.
    Proposal,
}

/// One tile on the map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MapItem {
    /// Names the item to [`browse`]. Stable for one scan.
    pub id: String,
    /// What the tile says.
    pub name: String,
    /// Its path, or empty when it is not one place.
    pub path: String,
    /// Allocated bytes.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Files in it.
    #[ts(type = "number")]
    pub files: u64,
    /// For folded tiles: how many items were folded.
    #[ts(type = "number")]
    pub count: u64,
    /// When anything in it was last modified or opened, in milliseconds since the Unix epoch.
    #[ts(type = "number | null")]
    pub last_used: Option<i64>,
    /// What it is.
    pub kind: ItemKind,
    /// Whether [`browse`] can open it.
    pub browsable: bool,
    /// An explanation the hover card shows, for tiles that need one.
    pub note: Option<String>,
    /// The id of the cleanup proposal the tile is, for proposal tiles.
    pub proposal: Option<String>,
}

/// A folder opened on the map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MapFolder {
    /// The folder itself.
    pub item: MapItem,
    /// The category it is in.
    pub category: CategoryId,
    /// What it holds within its item, biggest first, the smallest folded into one.
    pub children: Vec<MapItem>,
}

/// One category's running total while a scan runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CategoryTotal {
    /// Which category.
    pub id: CategoryId,
    /// Its name on this platform.
    pub name: String,
    /// Whether it is locked.
    pub protected: bool,
    /// Bytes counted so far.
    #[ts(type = "number")]
    pub bytes: u64,
}

/// How far a scan has got, for the scanning HUD.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanUpdate {
    /// Files counted so far.
    #[ts(type = "number")]
    pub files: u64,
    /// Folders found so far.
    #[ts(type = "number")]
    pub folders: u64,
    /// Allocated bytes counted so far.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Folders the OS refused to list so far.
    #[ts(type = "number")]
    pub unreadable: u64,
    /// A folder being listed now.
    pub current_path: String,
    /// Time since the scan started.
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    /// The categories so far, when this update carries a preview of the map.
    pub categories: Option<Vec<CategoryTotal>>,
    /// How the drive is being read.
    pub method: ScanMethod,
    /// Why a fast scan that was asked for is running as a normal scan, if it is.
    pub fallback: Option<FastFallback>,
    /// A fast scan's stage.
    pub stage: Option<FastStage>,
    /// While a fast scan reads the index: records read, of how many.
    pub index: Option<IndexRead>,
}

/// How far reading the file index has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IndexRead {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
}

/// How a scan ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
#[ts(export)]
pub enum ScanOutcome {
    /// It finished.
    Done {
        /// The map.
        overview: Box<Overview>,
        /// The cleanups the rules propose, biggest first. Their tiles are on the map.
        proposals: Vec<Proposal>,
        /// The folders Tidy up can sort (Windows; none on macOS, nor on a drive without the known folders).
        tidy: Vec<crate::tidy::TidyProposal>,
    },
    /// It was cancelled; there is no map.
    Cancelled,
}

/// Why a map item could not be opened.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MapError {
    /// The id does not name a folder in this scan.
    #[error("there is no folder {0} in this scan")]
    NoSuchFolder(String),
}

/// A volume scanned for the map: the snapshot, its categories (kept for drill-down), the cleanups proposed for it
/// (with their targets, for the plan) and the overview.
#[derive(Debug)]
pub struct ScannedVolume {
    /// What the scan found.
    pub snapshot: Snapshot,
    /// How it sorts into categories, with each proposal's targets carved out into a tile of its own.
    pub categories: Categories,
    /// The cleanups proposed.
    pub proposals: Proposals,
    /// The Tidy up sorts proposed, with what planning them needs.
    pub tidy: crate::tidy::Tidy,
    /// The overview map.
    pub overview: Overview,
    /// How the drive was read.
    pub how: HowScanned,
}

impl ScannedVolume {
    /// Carves the categories again for the proposals as they are now and redraws the overview against the same
    /// drive figures: after the duplicate finder adds its proposal, whose tiles come out of the folders they are in.
    pub(crate) fn reshape(&mut self) {
        let tiles = self.proposals.tiles();
        let claims: Vec<(CategoryId, &[ItemRef])> =
            tiles.iter().map(|t| (t.category, t.targets)).collect();
        let carved = self.categories.recarve(&self.snapshot, &claims, &|| false);
        debug_assert!(carved, "carving that is never stopped finishes");
        self.overview = overview_until(
            &self.snapshot,
            &self.categories,
            &self.proposals,
            self.overview.volume.clone(),
            &self.how,
            &|| false,
        )
        .expect("an overview that is never stopped finishes");
    }
}

/// The id of the overview tile a kept file is drawn in, as [`overview`] names tiles: its item's. The tile may have
/// been folded into its category's "smaller items".
pub(crate) fn tile_holding(
    snapshot: &Snapshot,
    categories: &Categories,
    file: FileId,
) -> Option<String> {
    Some(match categories.item_holding_file(snapshot, file)? {
        ItemRef::Dir(d) => format!("d{}", d.index()),
        ItemRef::File(f) => format!("f{}", f.index()),
        ItemRef::FilesIn(d) => format!("a{}", d.index()),
        ItemRef::Proposal(_) => return None,
    })
}

/// Scans a volume for the map: [`scan`], then [`categorize`](crate::categorize) with `env`, then the rules'
/// [proposals](crate::propose) as of `moment`, then [`overview`] against the volume's own numbers, read when the
/// scan finishes.
///
/// `on_update` gets the HUD's numbers about ten times a second, and now and then a preview of the categories so
/// the map can fill in. A preview holds the scan's workers while it is taken, so previews come less often as the
/// tree grows: never more than about a tenth of the scan's time goes to them.
pub fn scan_for_map(
    volume: &Path,
    env: &Env,
    moment: &Moment,
    on_update: &mut dyn FnMut(ScanUpdate),
    cancel: &CancelToken,
) -> Result<ScannedVolume, ScanError> {
    scan_for_map_with(
        volume,
        env,
        moment,
        &|_| volume_info(volume).ok(),
        on_update,
        cancel,
    )
}

/// [`scan_for_map`], with the drive's figures from `figures` (given the finished scan) instead of the OS: fixture
/// mode's pseudo-drive has the figures of the folder it is (see [`crate::fixture::volume`]).
pub fn scan_for_map_with(
    volume: &Path,
    env: &Env,
    moment: &Moment,
    figures: &dyn Fn(&Snapshot) -> Option<VolumeInfo>,
    on_update: &mut dyn FnMut(ScanUpdate),
    cancel: &CancelToken,
) -> Result<ScannedVolume, ScanError> {
    normal_scan(volume, env, moment, figures, None, on_update, cancel)
}

/// The normal scan for the map, saying in every update why it runs instead of a fast scan, if it does.
fn normal_scan(
    volume: &Path,
    env: &Env,
    moment: &Moment,
    figures: &dyn Fn(&Snapshot) -> Option<VolumeInfo>,
    fallback: Option<FastFallback>,
    on_update: &mut dyn FnMut(ScanUpdate),
    cancel: &CancelToken,
) -> Result<ScannedVolume, ScanError> {
    let mut next_preview = Duration::ZERO;
    let snapshot = scan(
        volume,
        &ScanOptions::default(),
        &mut |progress| {
            let preview = (progress.elapsed >= next_preview).then(|| {
                let started = Instant::now();
                let categories = progress
                    .preview(|partial| categorize_until(partial, env, &|| cancel.is_cancelled()));
                next_preview =
                    progress.elapsed + (started.elapsed() * 10).max(Duration::from_millis(700));
                categories
            });
            let mut update = scan_update(progress, preview.flatten().as_ref());
            update.fallback = fallback;
            on_update(update);
        },
        cancel,
    )?;
    let how = HowScanned {
        fallback,
        ..HowScanned::default()
    };
    finish_map(snapshot, env, moment, volume, figures, how, cancel)
}

/// Scans a drive for the map with a fast scan where it can (Windows, NTFS): Windows asks the user for admin, and
/// a small reader started with it reads the drive's file index (see [`crate::fast`] and ADR 0007). Where it can't,
/// it runs the normal scan, saying why in every update and on the map: the prompt was declined or closed, the drive
/// isn't NTFS (no prompt is shown then), the reader couldn't start, or the index couldn't be read.
///
/// `owner` is the app's window, which Windows' admin prompt belongs to. `figures` is as for
/// [`scan_for_map_with`]. In fixture mode `volume` is the fixture folder: the reader reads its volume's index, and
/// only the folder's part of it is kept.
pub fn fast_scan_for_map(
    volume: &Path,
    env: &Env,
    moment: &Moment,
    figures: &dyn Fn(&Snapshot) -> Option<VolumeInfo>,
    owner: Option<isize>,
    on_update: &mut dyn FnMut(ScanUpdate),
    cancel: &CancelToken,
) -> Result<ScannedVolume, ScanError> {
    let fallback = |reason: FastFallback, on_update: &mut dyn FnMut(ScanUpdate)| {
        normal_scan(
            volume,
            env,
            moment,
            figures,
            Some(reason),
            on_update,
            cancel,
        )
    };
    let ntfs = cfg!(windows) && crate::volume::file_system_of(volume).eq_ignore_ascii_case("NTFS");
    if !ntfs {
        return fallback(FastFallback::NotNtfs, on_update);
    }
    let Some(expect) = fast_expect(volume) else {
        return fallback(FastFallback::IndexUnreadable, on_update);
    };
    let Some(letter) = expect.root_letter else {
        return fallback(FastFallback::NotNtfs, on_update);
    };
    let started = Instant::now();
    let stage = |stage: FastStage, index: Option<IndexRead>, files: u64, folders: u64| ScanUpdate {
        files,
        folders,
        bytes: 0,
        unreadable: 0,
        current_path: String::new(),
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        categories: None,
        method: ScanMethod::Fast,
        fallback: None,
        stage: Some(stage),
        index,
    };
    on_update(stage(FastStage::Asking, None, 0, 0));
    let mut next_preview = Duration::ZERO;
    // Both stages report through the one callback.
    let sink = std::cell::RefCell::new(&mut *on_update);
    let read = crate::fast::read_index(letter, owner, cancel, |stream| {
        // The prompt was answered and the reader is there: the HUD can show the read from now.
        (sink.borrow_mut())(stage(FastStage::Reading, None, 0, 0));
        crate::fast::snapshot_from_stream(
            stream,
            &expect.expect,
            &ScanOptions::default(),
            &mut |p| {
                (sink.borrow_mut())(stage(
                    FastStage::Reading,
                    Some(IndexRead {
                        done: p.records_done,
                        total: p.records_total,
                    }),
                    p.files,
                    p.folders,
                ));
            },
            &mut |progress| {
                let preview = (progress.elapsed >= next_preview).then(|| {
                    let t = Instant::now();
                    let categories = progress.preview(|partial| {
                        categorize_until(partial, env, &|| cancel.is_cancelled())
                    });
                    next_preview =
                        progress.elapsed + (t.elapsed() * 10).max(Duration::from_millis(700));
                    categories
                });
                let mut update = scan_update(progress, preview.flatten().as_ref());
                update.method = ScanMethod::Fast;
                update.stage = Some(FastStage::Sorting);
                update.elapsed_ms =
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                (sink.borrow_mut())(update);
            },
            cancel,
        )
    });
    let snapshot = match read {
        Err(crate::fast::Launch::Declined) => {
            return fallback(FastFallback::Declined, on_update);
        }
        Err(crate::fast::Launch::Failed(why)) => {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            eprintln!("fast scan: the reader didn't start: {why}");
            return fallback(FastFallback::HelperFailed, on_update);
        }
        Ok(Err(crate::fast::FastError::Cancelled)) => return Err(ScanError::Cancelled),
        Ok(Err(e)) => {
            if cancel.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            eprintln!("fast scan: {e}");
            return fallback(FastFallback::IndexUnreadable, on_update);
        }
        Ok(Ok(snapshot)) => snapshot,
    };
    let fast = snapshot.fast().cloned().unwrap_or_default();
    let how = HowScanned {
        method: ScanMethod::Fast,
        fallback: None,
        locked: locked_groups(&snapshot, &fast.locked, env),
        records_bytes: fast.records_bytes,
    };
    finish_map(snapshot, env, moment, volume, figures, how, cancel)
}

/// What a fast scan of `volume` is read against, from the app's own view of it.
struct FastExpect {
    expect: crate::fast::Expect,
    root_letter: Option<char>,
}

fn fast_expect(volume: &Path) -> Option<FastExpect> {
    let info = volume_info(volume).ok()?;
    let id = crate::fsid::identity(volume)?;
    let mount = Path::new(&info.mount_point);
    let whole = same_path(mount, volume);
    let letter = info
        .mount_point
        .chars()
        .next()
        .filter(|c| c.is_ascii_alphabetic() && info.mount_point.get(1..2) == Some(":"));
    Some(FastExpect {
        expect: crate::fast::Expect {
            root: volume.to_path_buf(),
            volume_serial: id.volume(),
            subtree: (!whole).then_some(id.scan_key() as u64),
            capacity: info.total_bytes,
        },
        root_letter: letter,
    })
}

/// Whether two paths name the same place, ignoring case and a trailing separator (Windows).
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.display()
            .to_string()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

/// Sorts a fast scan's locked folders by why they are locked: those in another account's home folder (under the
/// folder that holds this account's home, but not in it), and the rest.
fn locked_groups(
    snapshot: &Snapshot,
    locked: &[crate::snapshot::LockedFolder],
    env: &Env,
) -> Vec<LockedGroup> {
    let profiles = env.home.parent();
    let lower = |p: &Path| p.display().to_string().to_lowercase();
    let home = format!("{}\\", lower(&env.home).trim_end_matches('\\'));
    let mut groups: Vec<LockedGroup> = Vec::new();
    for l in locked {
        let path = lower(&snapshot.path(l.dir));
        let others = profiles.is_some_and(|p| {
            let p = format!("{}\\", lower(p).trim_end_matches('\\'));
            path.starts_with(&p) && path.len() > p.len() && !format!("{path}\\").starts_with(&home)
        });
        let kind = if others {
            LockedKind::OtherAccounts
        } else {
            LockedKind::WindowsOnly
        };
        match groups.iter_mut().find(|g| g.kind == kind) {
            Some(g) => {
                g.bytes = g.bytes.saturating_add(l.bytes);
                g.files = g.files.saturating_add(l.files);
                g.folders = g.folders.saturating_add(l.folders);
            }
            None => groups.push(LockedGroup {
                kind,
                bytes: l.bytes,
                files: l.files,
                folders: l.folders,
            }),
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.bytes));
    groups
}

/// Everything after the scan itself: categories, proposals, the overview against the drive's figures, Tidy up.
pub(crate) fn finish_map(
    snapshot: Snapshot,
    env: &Env,
    moment: &Moment,
    volume: &Path,
    figures: &dyn Fn(&Snapshot) -> Option<VolumeInfo>,
    how: HowScanned,
    cancel: &CancelToken,
) -> Result<ScannedVolume, ScanError> {
    // Building the map takes a moment on a big drive; a cancel during it stops it and means no map.
    let (categories, proposals, overview) =
        build_map_with(&snapshot, env, moment, volume, figures, &how, &|| {
            cancel.is_cancelled()
        })
        .ok_or(ScanError::Cancelled)?;
    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    // A fresh bounded inventory of the known folders, read now (names, sizes and dates only).
    let tidy = crate::tidy::propose(&snapshot, &proposals, env, cancel);
    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    Ok(ScannedVolume {
        snapshot,
        categories,
        proposals,
        tidy,
        overview,
        how,
    })
}

/// Categorizes a finished scan, proposes cleanups, carves their tiles out of the categories and builds the overview
/// against the drive's figures from `figures`, read now. Gives up (`None`) as soon as `stop` says so; it is asked
/// all along the way, not only between the steps.
fn build_map_with(
    snapshot: &Snapshot,
    env: &Env,
    moment: &Moment,
    volume: &Path,
    figures: &dyn Fn(&Snapshot) -> Option<VolumeInfo>,
    how: &HowScanned,
    stop: &dyn Fn() -> bool,
) -> Option<(Categories, Proposals, Overview)> {
    if stop() {
        return None;
    }
    let categories = categorize_until(snapshot, env, stop)?;
    let mut proposals = propose_until(snapshot, &categories, env, moment, stop)?;
    for hint in crate::hints::discover_until(env, snapshot, moment.now, stop)? {
        if stop() {
            return None;
        }
        proposals.append_hint(&hint);
    }
    let tiles = proposals.tiles();
    let claims: Vec<(CategoryId, &[ItemRef])> =
        tiles.iter().map(|t| (t.category, t.targets)).collect();
    let categories = categories.carve(snapshot, &claims, stop)?;
    let scanned = snapshot.stats().bytes;
    let info = figures(snapshot).unwrap_or_else(|| VolumeInfo {
        name: volume.display().to_string(),
        mount_point: volume.display().to_string(),
        id: String::new(),
        kind: DriveKind::Internal,
        system: false,
        total_bytes: scanned,
        used_bytes: scanned,
        free_bytes: 0,
        file_system: String::new(),
    });
    let overview = overview_until(snapshot, &categories, &proposals, info, how, stop)?;
    // A cancel that came during the last step still means no map.
    if stop() {
        return None;
    }
    Some((categories, proposals, overview))
}

/// The overview map of a finished normal scan. `volume` is the OS's view of the volume, read when the scan
/// finished. `proposals` names the proposal tiles of categories carved with them (none for uncarved categories).
pub fn overview(
    snapshot: &Snapshot,
    categories: &Categories,
    proposals: &Proposals,
    volume: VolumeInfo,
) -> Overview {
    overview_until(
        snapshot,
        categories,
        proposals,
        volume,
        &HowScanned::default(),
        &|| false,
    )
    .expect("an overview that is never stopped finishes")
}

/// [`overview`] for a scan read as `how` says, giving up (`None`) as soon as `stop` says so.
fn overview_until(
    snapshot: &Snapshot,
    categories: &Categories,
    proposals: &Proposals,
    volume: VolumeInfo,
    how: &HowScanned,
    stop: &dyn Fn() -> bool,
) -> Option<Overview> {
    let stats = snapshot.stats();
    let fast = how.method == ScanMethod::Fast;
    let scanned = categories.list().iter().map(|c| c.bytes).sum::<u64>();
    let locked = how.locked.iter().map(|g| g.bytes).sum::<u64>();
    let counted = scanned
        .saturating_add(locked)
        .saturating_add(how.records_bytes);
    let hidden = volume.used_bytes.saturating_sub(counted);
    let over = counted.saturating_sub(volume.used_bytes);
    let mut unreadable: Vec<String> = Vec::new();
    // A fast scan's unreadable folders are its locked ones, counted in their block: they are never named.
    if !fast {
        for dir in snapshot.dirs() {
            if dir.index() % CHECK_EVERY == 0 && stop() {
                return None;
            }
            if snapshot.flags(dir).contains(DirFlags::UNREADABLE) {
                unreadable.push(snapshot.path(dir).display().to_string());
            }
        }
    }
    unreadable.sort_by_key(|p| (Path::new(p).components().count(), p.clone()));
    unreadable.truncate(UNREADABLE_SAMPLES);
    let unreadable_count = if fast { 0 } else { stats.unreadable };
    let mac = categories.platform() == Platform::MacOs;
    let tiles = proposals.tiles();

    let mut list: Vec<MapCategory> = Vec::new();
    for cat in categories.list() {
        let mut items = fold(
            snapshot,
            categories,
            (proposals, &tiles),
            &cat.items,
            CATEGORY_ITEMS,
            &format!("more:{:?}", cat.id).to_ascii_lowercase(),
            stop,
        )?;
        let mut bytes = cat.bytes;
        if cat.id == CategoryId::System {
            if how.records_bytes > 0 {
                items.push(records_item(how.records_bytes));
                bytes += how.records_bytes;
            }
            if hidden > 0 {
                items.push(hidden_item(
                    hidden,
                    unreadable_count,
                    &unreadable,
                    mac,
                    fast,
                ));
                bytes += hidden;
            }
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
        }
        if bytes > 0 {
            list.push(MapCategory {
                id: cat.id,
                name: cat.name.clone(),
                blurb: cat.blurb.clone(),
                protected: cat.protected,
                bytes,
                items,
            });
        }
    }
    list.sort_by_key(|c| std::cmp::Reverse(c.bytes));

    Some(Overview {
        free_bytes: volume.total_bytes.saturating_sub(volume.used_bytes),
        volume,
        scan: ScanSummary {
            files: stats.files,
            folders: stats.folders,
            scanned_bytes: scanned,
            hidden_bytes: hidden,
            over_count_bytes: over,
            unreadable: unreadable_count,
            unreadable_samples: unreadable,
            links_skipped: stats.links_skipped,
            hard_links_merged: stats.hard_links_merged,
            duration_ms: u64::try_from(stats.elapsed.as_millis()).unwrap_or(u64::MAX),
            method: how.method,
            fallback: how.fallback,
            locked: how.locked.clone(),
            records_bytes: how.records_bytes,
        },
        categories: list,
    })
}

/// Opens a folder tile: what it holds within its item.
pub fn browse(
    snapshot: &Snapshot,
    categories: &Categories,
    id: &str,
) -> Result<MapFolder, MapError> {
    let dir = id
        .strip_prefix('d')
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| snapshot.dir_at(n))
        .ok_or_else(|| MapError::NoSuchFolder(id.to_owned()))?;
    let item = categories
        .item_of(dir)
        .ok_or_else(|| MapError::NoSuchFolder(id.to_owned()))?;
    let none = Proposals::default();
    let children = fold(
        snapshot,
        categories,
        (&none, &[]),
        &categories.contents(snapshot, dir),
        FOLDER_ITEMS,
        &format!("more:{id}"),
        &|| false,
    )
    .unwrap_or_default();
    Ok(MapFolder {
        item: map_item(snapshot, categories, (&none, &[]), &item, &|| false)
            .ok_or_else(|| MapError::NoSuchFolder(id.to_owned()))?,
        category: categories.category_of(dir),
        children,
    })
}

/// The live numbers for the scanning HUD, with a preview of the categories when `preview` is given.
pub fn scan_update(progress: &ScanProgress<'_>, preview: Option<&Categories>) -> ScanUpdate {
    ScanUpdate {
        files: progress.files,
        folders: progress.folders,
        bytes: progress.bytes,
        unreadable: progress.unreadable,
        current_path: progress.current.display().to_string(),
        elapsed_ms: u64::try_from(progress.elapsed.as_millis()).unwrap_or(u64::MAX),
        categories: preview.map(|cats| {
            cats.list()
                .iter()
                .filter(|c| c.bytes > 0)
                .map(|c| CategoryTotal {
                    id: c.id,
                    name: c.name.clone(),
                    protected: c.protected,
                    bytes: c.bytes,
                })
                .collect()
        }),
        method: ScanMethod::Normal,
        fallback: None,
        stage: None,
        index: None,
    }
}

/// Keeps the biggest `keep` items (sorted biggest first) and folds the rest into one tile, unless only one would
/// be folded. Proposal tiles are never folded: each one can be added to a plan. Only the kept items are mapped;
/// the folded ones are just summed. `None` if `stop` says so.
fn fold(
    snapshot: &Snapshot,
    categories: &Categories,
    proposals: Shown<'_>,
    items: &[Item],
    keep: usize,
    more_id: &str,
    stop: &dyn Fn() -> bool,
) -> Option<Vec<MapItem>> {
    let is_proposal = |i: &Item| matches!(i.at, ItemRef::Proposal(_));
    let pinned: Vec<&Item> = items
        .iter()
        .filter(|i| i.bytes > 0 && is_proposal(i))
        .collect();
    let shown: Vec<&Item> = items
        .iter()
        .filter(|i| i.bytes > 0 && !is_proposal(i))
        .collect();
    let keep = keep.saturating_sub(pinned.len());
    let cut = if shown.len() <= keep + 1 {
        shown.len()
    } else {
        keep
    };
    let mut out = Vec::with_capacity(pinned.len() + cut + 1);
    for item in pinned.iter().chain(&shown[..cut]) {
        if stop() {
            return None;
        }
        out.push(map_item(snapshot, categories, proposals, item, stop)?);
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    let rest = &shown[cut..];
    if !rest.is_empty() {
        out.push(MapItem {
            id: more_id.to_owned(),
            name: format!("{} smaller items", group_digits(rest.len() as u64)),
            path: String::new(),
            bytes: rest.iter().map(|i| i.bytes).sum(),
            files: rest.iter().map(|i| i.files).sum(),
            count: rest.len() as u64,
            last_used: rest.iter().filter_map(|i| millis(i.last_used)).max(),
            kind: ItemKind::More,
            browsable: false,
            note: None,
            proposal: None,
        });
    }
    Some(out)
}

/// One item as a map tile, or `None` if `stop` says so while looking into its folder (or the item is a proposal
/// `proposals` doesn't have).
fn map_item(
    snapshot: &Snapshot,
    categories: &Categories,
    (proposals, tiles): Shown<'_>,
    item: &Item,
    stop: &dyn Fn() -> bool,
) -> Option<MapItem> {
    let last_used = millis(item.last_used);
    Some(match item.at {
        ItemRef::Dir(dir) => {
            let root = snapshot.parent(dir).is_none();
            let name = if root && snapshot.root_of(dir).kind == RootKind::SealedSystem {
                "macOS".to_owned()
            } else {
                snapshot.name(dir).to_string_lossy().into_owned()
            };
            let browsable = categories.has_parts(snapshot, dir, stop)?;
            MapItem {
                id: format!("d{}", dir.index()),
                name,
                path: snapshot.path(dir).display().to_string(),
                bytes: item.bytes,
                files: item.files,
                count: 0,
                last_used,
                kind: ItemKind::Folder,
                browsable,
                note: None,
                proposal: None,
            }
        }
        ItemRef::File(file) => MapItem {
            id: format!("f{}", file.index()),
            name: snapshot.file(file).name.to_string_lossy().into_owned(),
            path: snapshot.file_path(file).display().to_string(),
            bytes: item.bytes,
            files: 1,
            count: 0,
            last_used,
            kind: ItemKind::File,
            browsable: false,
            note: None,
            proposal: None,
        },
        ItemRef::FilesIn(dir) => {
            let name = snapshot.name(dir).to_string_lossy().into_owned();
            MapItem {
                id: format!("a{}", dir.index()),
                name: format!("{} files in {name}", group_digits(item.files)),
                path: snapshot.path(dir).display().to_string(),
                bytes: item.bytes,
                files: item.files,
                count: item.files,
                last_used,
                kind: ItemKind::Files,
                browsable: false,
                note: None,
                proposal: None,
            }
        }
        ItemRef::Proposal(t) => {
            let tile = tiles.get(t)?;
            let proposal = proposals.list().get(tile.proposal)?;
            let targets = tile.targets;
            // One place has a path; a proposal gathered from many has none, and one tile of several is its folder.
            let path = match (tile.part, targets) {
                (Some((d, _)), _) => snapshot.path(d).display().to_string(),
                (None, [ItemRef::Dir(d)]) => snapshot.path(*d).display().to_string(),
                (None, [ItemRef::File(f)]) => snapshot.file_path(*f).display().to_string(),
                _ => String::new(),
            };
            let (id, name, note) = match tile.part {
                Some((d, note)) => (
                    format!("p:{}:d{}", proposal.id, d.index()),
                    "Duplicate copies".to_owned(),
                    Some(note.to_owned()),
                ),
                None => (format!("p:{}", proposal.id), proposal.title.clone(), None),
            };
            MapItem {
                id,
                name,
                path,
                bytes: item.bytes,
                files: item.files,
                count: targets.len() as u64,
                last_used,
                kind: ItemKind::Proposal,
                browsable: false,
                note,
                proposal: Some(proposal.id.clone()),
            }
        }
    })
}

/// The proposals a map is drawn with, and their tiles ([`Proposals::tiles`]).
type Shown<'a> = (&'a Proposals, &'a [Tile<'a>]);

fn hidden_item(bytes: u64, unreadable: u64, samples: &[String], mac: bool, fast: bool) -> MapItem {
    let examples = samples
        .iter()
        .filter_map(|p| Path::new(p).file_name())
        .take(2)
        .map(|n| n.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let folders = match (unreadable, examples.as_slice()) {
        (0, _) => String::new(),
        (n, [a, b]) => format!(
            "{} folder{} it isn't allowed to open (such as {a} and {b}), ",
            group_digits(n),
            if n == 1 { "" } else { "s" }
        ),
        (n, _) => format!(
            "{} folder{} it isn't allowed to open, ",
            group_digits(n),
            if n == 1 { "" } else { "s" }
        ),
    };
    let note = if mac {
        format!(
            "macOS counts this as used, but Vacancy can't see it: {folders}swap, local Time Machine snapshots and \
             macOS's other system volumes."
        )
    } else if fast {
        "Windows counts this as used, but the drive's file index doesn't account for it: files that changed while \
         it was read, and space Windows keeps outside any file."
            .to_owned()
    } else {
        format!(
            "Windows counts this as used, but Vacancy can't see it: {folders}restore points and the file system's \
             own records. A fast scan counts this space, as totals only."
        )
    };
    MapItem {
        id: "hidden".to_owned(),
        name: "Space Vacancy can't see".to_owned(),
        path: String::new(),
        bytes,
        files: 0,
        count: 0,
        last_used: None,
        kind: ItemKind::Hidden,
        browsable: false,
        note: Some(note),
        proposal: None,
    }
}

/// NTFS's own records, as a fast scan counted them.
fn records_item(bytes: u64) -> MapItem {
    MapItem {
        id: "records".to_owned(),
        name: "File system records".to_owned(),
        path: String::new(),
        bytes,
        files: 0,
        count: 0,
        last_used: None,
        kind: ItemKind::Records,
        browsable: false,
        note: Some(
            "The drive's own file index, its log, folder indexes and the other records NTFS keeps beside files. \
             Fast scan reads them; Windows keeps them up to date."
                .to_owned(),
        ),
        proposal: None,
    }
}

fn millis(t: UnixTime) -> Option<i64> {
    (t > 0).then(|| t.saturating_mul(1000))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use super::*;

    /// A fixture tree with enough folders for several stop checks per pass.
    fn tree(folders: usize) -> (tempfile::TempDir, Snapshot) {
        let fx = tempfile::tempdir().unwrap();
        for i in 0..folders {
            let dir = fx
                .path()
                .join(format!("d{:02}", i % 40))
                .join(format!("e{i}"));
            fs::create_dir_all(&dir).unwrap();
            if i % 10 == 0 || i < 40 {
                fs::write(dir.join("f.bin"), [1u8; 4096]).unwrap();
            }
        }
        let snapshot = scan(
            fx.path(),
            &ScanOptions::default(),
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap();
        (fx, snapshot)
    }

    /// Counts stop checks, and says stop from check number `at` on (never, if `at` is 0).
    fn stopper(checks: &Cell<u32>, at: u32) -> impl Fn() -> bool + '_ {
        move || {
            checks.set(checks.get() + 1);
            at != 0 && checks.get() >= at
        }
    }

    /// A cancel that arrives while categorizing stops it at the next check, not once the whole step has run.
    #[test]
    fn categorizing_stops_part_way_when_cancelled() {
        let (fx, snapshot) = tree(3000);
        let env = Env::windows(fx.path(), "alex");
        let checks = Cell::new(0);
        categorize_until(&snapshot, &env, &stopper(&checks, 0)).expect("categories");
        let all = checks.get();
        assert!(all >= 6, "checked all along each pass: {all}");
        for at in [1, all / 2, all] {
            checks.set(0);
            assert!(categorize_until(&snapshot, &env, &stopper(&checks, at)).is_none());
            assert_eq!(checks.get(), at, "nothing more was done after the cancel");
        }
    }

    /// The same for building the overview, on its own: it checks before every tile it maps and while looking into
    /// a tile's folder, so a cancel there stops it at once.
    #[test]
    fn the_overview_stops_part_way_when_cancelled() {
        let (fx, snapshot) = tree(3000);
        let categories = crate::categorize(&snapshot, &Env::windows(fx.path(), "alex"));
        let volume = || VolumeInfo {
            name: "Test".into(),
            mount_point: fx.path().display().to_string(),
            id: String::new(),
            kind: DriveKind::Internal,
            system: false,
            total_bytes: 1 << 40,
            used_bytes: snapshot.stats().bytes,
            free_bytes: (1 << 40) - snapshot.stats().bytes,
            file_system: String::new(),
        };
        let checks = Cell::new(0);
        let none = Proposals::default();
        let map = overview_until(
            &snapshot,
            &categories,
            &none,
            volume(),
            &HowScanned::default(),
            &stopper(&checks, 0),
        )
        .expect("the overview");
        let tiles = map.categories.iter().map(|c| c.items.len()).sum::<usize>() as u32;
        let all = checks.get();
        assert!(tiles >= 40, "the fixture maps many tiles: {tiles}");
        assert!(
            all > tiles,
            "a check before every tile, and inside its folder: {all} for {tiles} tiles"
        );
        for at in [1, all / 2, all] {
            checks.set(0);
            assert!(
                overview_until(
                    &snapshot,
                    &categories,
                    &none,
                    volume(),
                    &HowScanned::default(),
                    &stopper(&checks, at)
                )
                .is_none()
            );
            assert_eq!(checks.get(), at, "nothing more was mapped after the cancel");
        }
    }

    /// A cancel that comes after the last step has started still means no map: the final check catches it.
    #[test]
    fn a_cancel_during_the_last_step_still_means_no_map() {
        let (fx, snapshot) = tree(200);
        let mut env = Env::windows(fx.path(), "alex");
        env.confined_to = Some(fx.path().to_path_buf());
        let moment = Moment::default();
        let volume = || crate::fixture::volume(fx.path(), 1 << 40, &snapshot);
        let checks = Cell::new(0);
        let categories =
            categorize_until(&snapshot, &env, &stopper(&checks, 0)).expect("categories");
        let in_categorize = checks.get();
        checks.set(0);
        let mut proposals =
            propose_until(&snapshot, &categories, &env, &moment, &stopper(&checks, 0))
                .expect("proposals");
        let in_propose = checks.get();
        checks.set(0);
        for hint in crate::hints::discover_until(&env, &snapshot, moment.now, &stopper(&checks, 0))
            .expect("advice")
        {
            assert!(!stopper(&checks, 0)());
            proposals.append_hint(&hint);
        }
        let in_advice = checks.get();
        assert!(in_advice > 0, "advice must participate in cancellation");
        checks.set(0);
        let claims: Vec<_> = proposals
            .list()
            .iter()
            .enumerate()
            .map(|(i, p)| (p.category, proposals.targets(i)))
            .collect();
        let categories = categories
            .carve(&snapshot, &claims, &stopper(&checks, 0))
            .expect("carved");
        let in_carve = checks.get();
        checks.set(0);
        overview_until(
            &snapshot,
            &categories,
            &proposals,
            volume(),
            &HowScanned::default(),
            &stopper(&checks, 0),
        )
        .expect("the overview");
        let in_overview = checks.get();

        // One check before categorizing, the steps' own, and one after the last step.
        let all = 1 + in_categorize + in_propose + in_advice + in_carve + in_overview + 1;
        checks.set(0);
        build_map_with(
            &snapshot,
            &env,
            &moment,
            fx.path(),
            &|_| Some(volume()),
            &HowScanned::default(),
            &stopper(&checks, 0),
        )
        .expect("the map");
        assert_eq!(checks.get(), all, "a final check after the overview");
        checks.set(0);
        assert!(
            build_map_with(
                &snapshot,
                &env,
                &moment,
                fx.path(),
                &|_| Some(volume()),
                &HowScanned::default(),
                &stopper(&checks, all)
            )
            .is_none(),
            "a cancel after the overview's last check still means no map"
        );
    }
}

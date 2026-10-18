//! What the UI's map shows, built from a snapshot, its categories and the volume's own numbers: the overview
//! (categories and their biggest items, free space, the part of the disk Vacancy can't see), each folder's
//! drill-down, and the live numbers while a scan runs.
//!
//! The overview is reconciled with the volume. The OS's used figure (what Explorer and Finder show) is the truth;
//! when the scan counted less, the difference (folders Vacancy may not open, restore points or snapshots, swap,
//! the file system's own records) is shown as one item in System rather than spread over guesses.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::categorize::{
    CHECK_EVERY, Categories, CategoryId, Env, Item, ItemRef, Platform, categorize_until,
};
use crate::rules::{Moment, Proposal, Proposals, propose_until};
use crate::scan::{CancelToken, ScanError, ScanOptions, ScanProgress, scan};
use crate::snapshot::{DirFlags, RootKind, Snapshot, UnixTime};
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
            on_update(scan_update(progress, preview.flatten().as_ref()));
        },
        cancel,
    )?;
    // Building the map takes a moment on a big drive; a cancel during it stops it and means no map.
    let (categories, proposals, overview) =
        build_map_with(&snapshot, env, moment, volume, figures, &|| {
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
    let claims: Vec<(CategoryId, &[ItemRef])> = proposals
        .list()
        .iter()
        .enumerate()
        .map(|(i, p)| (p.category, proposals.targets(i)))
        .collect();
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
    });
    let overview = overview_until(snapshot, &categories, &proposals, info, stop)?;
    // A cancel that came during the last step still means no map.
    if stop() {
        return None;
    }
    Some((categories, proposals, overview))
}

/// The overview map of a finished scan. `volume` is the OS's view of the volume, read when the scan finished.
/// `proposals` names the proposal tiles of categories carved with them (none for uncarved categories).
pub fn overview(
    snapshot: &Snapshot,
    categories: &Categories,
    proposals: &Proposals,
    volume: VolumeInfo,
) -> Overview {
    overview_until(snapshot, categories, proposals, volume, &|| false)
        .expect("an overview that is never stopped finishes")
}

/// [`overview`], giving up (`None`) as soon as `stop` says so.
fn overview_until(
    snapshot: &Snapshot,
    categories: &Categories,
    proposals: &Proposals,
    volume: VolumeInfo,
    stop: &dyn Fn() -> bool,
) -> Option<Overview> {
    let stats = snapshot.stats();
    let scanned = categories.list().iter().map(|c| c.bytes).sum::<u64>();
    let hidden = volume.used_bytes.saturating_sub(scanned);
    let over = scanned.saturating_sub(volume.used_bytes);
    let mut unreadable: Vec<String> = Vec::new();
    for dir in snapshot.dirs() {
        if dir.index() % CHECK_EVERY == 0 && stop() {
            return None;
        }
        if snapshot.flags(dir).contains(DirFlags::UNREADABLE) {
            unreadable.push(snapshot.path(dir).display().to_string());
        }
    }
    unreadable.sort_by_key(|p| (Path::new(p).components().count(), p.clone()));
    unreadable.truncate(UNREADABLE_SAMPLES);
    let mac = categories.platform() == Platform::MacOs;

    let mut list: Vec<MapCategory> = Vec::new();
    for cat in categories.list() {
        let mut items = fold(
            snapshot,
            categories,
            proposals,
            &cat.items,
            CATEGORY_ITEMS,
            &format!("more:{:?}", cat.id).to_ascii_lowercase(),
            stop,
        )?;
        let mut bytes = cat.bytes;
        if cat.id == CategoryId::System && hidden > 0 {
            items.push(hidden_item(hidden, stats.unreadable, &unreadable, mac));
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            bytes += hidden;
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
            unreadable: stats.unreadable,
            unreadable_samples: unreadable,
            links_skipped: stats.links_skipped,
            hard_links_merged: stats.hard_links_merged,
            duration_ms: u64::try_from(stats.elapsed.as_millis()).unwrap_or(u64::MAX),
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
    let children = fold(
        snapshot,
        categories,
        &Proposals::default(),
        &categories.contents(snapshot, dir),
        FOLDER_ITEMS,
        &format!("more:{id}"),
        &|| false,
    )
    .unwrap_or_default();
    Ok(MapFolder {
        item: map_item(snapshot, categories, &Proposals::default(), &item, &|| {
            false
        })
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
    }
}

/// Keeps the biggest `keep` items (sorted biggest first) and folds the rest into one tile, unless only one would
/// be folded. Proposal tiles are never folded: each one can be added to a plan. Only the kept items are mapped;
/// the folded ones are just summed. `None` if `stop` says so.
fn fold(
    snapshot: &Snapshot,
    categories: &Categories,
    proposals: &Proposals,
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
    proposals: &Proposals,
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
        ItemRef::Proposal(p) => {
            let proposal = proposals.list().get(p)?;
            let targets = proposals.targets(p);
            // One place has a path; a proposal gathered from many has none.
            let path = match targets {
                [ItemRef::Dir(d)] => snapshot.path(*d).display().to_string(),
                [ItemRef::File(f)] => snapshot.file_path(*f).display().to_string(),
                _ => String::new(),
            };
            MapItem {
                id: format!("p:{}", proposal.id),
                name: proposal.title.clone(),
                path,
                bytes: item.bytes,
                files: item.files,
                count: targets.len() as u64,
                last_used,
                kind: ItemKind::Proposal,
                browsable: false,
                note: None,
                proposal: Some(proposal.id.clone()),
            }
        }
    })
}

fn hidden_item(bytes: u64, unreadable: u64, samples: &[String], mac: bool) -> MapItem {
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
    } else {
        format!(
            "Windows counts this as used, but Vacancy can't see it: {folders}restore points and the file system's \
             own records."
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
        };
        let checks = Cell::new(0);
        let none = Proposals::default();
        let map = overview_until(
            &snapshot,
            &categories,
            &none,
            volume(),
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
                &stopper(&checks, all)
            )
            .is_none(),
            "a cancel after the overview's last check still means no map"
        );
    }
}

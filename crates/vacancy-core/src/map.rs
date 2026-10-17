//! What the UI's map shows, built from a snapshot, its categories and the volume's own numbers: the overview
//! (categories and their biggest items, free space, the part of the disk Vacancy can't see), each folder's
//! drill-down, and the live numbers while a scan runs.
//!
//! The overview is reconciled with the volume. The OS's used figure (what Explorer and Finder show) is the truth;
//! when the scan counted less, the difference (folders Vacancy may not open, restore points or snapshots, swap,
//! the file system's own records) is shown as one item in System rather than spread over guesses.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;
use ts_rs::TS;

use crate::categorize::{Categories, CategoryId, Env, Item, ItemRef, Platform, categorize};
use crate::scan::{CancelToken, ScanError, ScanOptions, ScanProgress, scan};
use crate::snapshot::{RootKind, Snapshot, UnixTime};
use crate::text::group_digits;
use crate::volume::{VolumeInfo, volume_info};

/// Items a category shows before the rest are folded into one "smaller items" tile.
const CATEGORY_ITEMS: usize = 48;
/// Items a folder's drill-down shows before folding.
const FOLDER_ITEMS: usize = 60;
/// Unreadable folders named in the summary.
const UNREADABLE_SAMPLES: usize = 8;

/// The finished scan, as the overview map shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
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
}

/// One tile on the map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
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

/// A volume scanned for the map: the snapshot and its categories (kept for drill-down) and the overview.
#[derive(Debug)]
pub struct ScannedVolume {
    /// What the scan found.
    pub snapshot: Snapshot,
    /// How it sorts into categories.
    pub categories: Categories,
    /// The overview map.
    pub overview: Overview,
}

/// Scans a volume for the map: [`scan`], then [`categorize`] with `env`, then [`overview`] against the volume's
/// own numbers, read when the scan finishes.
///
/// `on_update` gets the HUD's numbers about ten times a second, and now and then a preview of the categories so
/// the map can fill in. A preview holds the scan's workers while it is taken, so previews come less often as the
/// tree grows: never more than about a tenth of the scan's time goes to them.
pub fn scan_for_map(
    volume: &Path,
    env: &Env,
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
                let categories = progress.preview(|partial| categorize(partial, env));
                next_preview =
                    progress.elapsed + (started.elapsed() * 10).max(Duration::from_millis(700));
                categories
            });
            on_update(scan_update(progress, preview.as_ref()));
        },
        cancel,
    )?;
    let categories = categorize(&snapshot, env);
    let scanned = snapshot.stats().bytes;
    let info = volume_info(volume).unwrap_or_else(|_| VolumeInfo {
        name: volume.display().to_string(),
        mount_point: volume.display().to_string(),
        total_bytes: scanned,
        used_bytes: scanned,
    });
    let overview = overview(&snapshot, &categories, info);
    Ok(ScannedVolume {
        snapshot,
        categories,
        overview,
    })
}

/// The overview map of a finished scan. `volume` is the OS's view of the volume, read when the scan finished.
pub fn overview(snapshot: &Snapshot, categories: &Categories, volume: VolumeInfo) -> Overview {
    let stats = snapshot.stats();
    let scanned = categories.list().iter().map(|c| c.bytes).sum::<u64>();
    let hidden = volume.used_bytes.saturating_sub(scanned);
    let over = scanned.saturating_sub(volume.used_bytes);
    let mut unreadable: Vec<String> = snapshot
        .unreadable()
        .map(|d| snapshot.path(d).display().to_string())
        .collect();
    unreadable.sort_by_key(|p| (Path::new(p).components().count(), p.clone()));
    unreadable.truncate(UNREADABLE_SAMPLES);
    let mac = categories.platform() == Platform::MacOs;

    let mut list: Vec<MapCategory> = categories
        .list()
        .iter()
        .map(|cat| {
            let mut items = fold(
                cat.items.iter().map(|i| map_item(snapshot, categories, i)),
                CATEGORY_ITEMS,
                &format!("more:{:?}", cat.id).to_ascii_lowercase(),
            );
            let mut bytes = cat.bytes;
            if cat.id == CategoryId::System && hidden > 0 {
                items.push(hidden_item(hidden, stats.unreadable, &unreadable, mac));
                items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
                bytes += hidden;
            }
            MapCategory {
                id: cat.id,
                name: cat.name.clone(),
                blurb: cat.blurb.clone(),
                protected: cat.protected,
                bytes,
                items,
            }
        })
        .filter(|c| c.bytes > 0)
        .collect();
    list.sort_by_key(|c| std::cmp::Reverse(c.bytes));

    Overview {
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
    }
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
        categories
            .contents(snapshot, dir)
            .iter()
            .map(|i| map_item(snapshot, categories, i)),
        FOLDER_ITEMS,
        &format!("more:{id}"),
    );
    Ok(MapFolder {
        item: map_item(snapshot, categories, &item),
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

/// Keeps the biggest `keep` items and folds the rest into one, unless only one would be folded.
fn fold(items: impl Iterator<Item = MapItem>, keep: usize, more_id: &str) -> Vec<MapItem> {
    let mut items: Vec<MapItem> = items.filter(|i| i.bytes > 0).collect();
    if items.len() <= keep + 1 {
        return items;
    }
    let rest = items.split_off(keep);
    items.push(MapItem {
        id: more_id.to_owned(),
        name: format!("{} smaller items", group_digits(rest.len() as u64)),
        path: String::new(),
        bytes: rest.iter().map(|i| i.bytes).sum(),
        files: rest.iter().map(|i| i.files).sum(),
        count: rest.len() as u64,
        last_used: rest.iter().filter_map(|i| i.last_used).max(),
        kind: ItemKind::More,
        browsable: false,
        note: None,
    });
    items
}

fn map_item(snapshot: &Snapshot, categories: &Categories, item: &Item) -> MapItem {
    let last_used = millis(item.last_used);
    match item.at {
        ItemRef::Dir(dir) => {
            let root = snapshot.parent(dir).is_none();
            let name = if root && snapshot.root_of(dir).kind == RootKind::SealedSystem {
                "macOS".to_owned()
            } else {
                snapshot.name(dir).to_string_lossy().into_owned()
            };
            let browsable = categories
                .contents(snapshot, dir)
                .iter()
                .any(|c| matches!(c.at, ItemRef::Dir(_) | ItemRef::File(_)));
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
            }
        }
    }
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
    }
}

fn millis(t: UnixTime) -> Option<i64> {
    (t > 0).then(|| t.saturating_mul(1000))
}

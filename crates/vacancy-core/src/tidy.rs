//! Tidy up: Vacancy's built-in sorting rules for the loose files at the top of Downloads, Desktop and Documents,
//! carried out (and undone) by the journaled reorganization engine in [`crate::reorg`]. Windows only: elsewhere
//! [`propose`] finds nothing and every entry point says so before touching the disk.
//!
//! The rules are offline and read names, sizes and dates only, never contents. A file goes into a folder named
//! for its kind, found by its extension (Images, Videos, Documents, Installers, Archives), and for images, videos
//! and documents into a year inside it, the year it was last modified (UTC). A file that fits no kind goes to
//! "Sort later". Some files stay where they are, each with a reason: shortcuts, the folder's own settings
//! (`desktop.ini`), downloads that haven't finished, hidden files. Only loose regular files at the top of a folder
//! move, and only within that folder; subfolders stay as they are.
//!
//! Each folder's sort is one proposal with at most the engine's entry limit of files, the first ones in the
//! folder's own (name) order; any more are left for a later run, and the proposal says how many the scan counted.
//! Every destination name is fixed before review, with a numbered suffix where a name is taken: nothing is ever
//! overwritten. **Delete wins over sort**: a file that a delete proposal in the same plan takes is left out of the
//! sort, and the proposal names that delete proposal so the review can show it as going.
//!
//! A sort's moves free nothing. Each folder's moves are journaled in a journal of their own in the app's data
//! folder, so each can be undone on its own, from the summary or from the run history after a restart
//! ([`undo`], [`refresh`]). A run that was interrupted is never resumed: reopened, it says what its journal shows.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use ts_rs::TS;

use crate::categorize::{CategoryId, Env, ItemRef, Platform};
use crate::reorg::{
    Draft, Engine, Error, Folder, Inventory, InventoryEntry, Ledger, LedgerMove, OpState,
    ValidatedPlan,
};
use crate::report::{Reorganized, RunReport, Skip, SkipKind, Undone};
use crate::rules::{ProposalAction, Proposals};
use crate::scan::CancelToken;
use crate::snapshot::Snapshot;

/// The id prefix every Tidy up proposal has: `tidy-downloads`, `tidy-desktop`, `tidy-documents`.
pub const PREFIX: &str = "tidy-";

/// The most files one folder's sort moves in one run: the engine's entry limit.
pub const LIMIT: usize = crate::reorg::ENTRY_LIMIT;

/// What kind of file a sort thinks something is, from its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TidyKind {
    Images,
    Videos,
    Documents,
    Installers,
    Archives,
    /// Fits no kind: it goes to "Sort later".
    Later,
}

impl TidyKind {
    /// The folder this kind goes into.
    pub fn folder_name(self) -> &'static str {
        match self {
            TidyKind::Images => "Images",
            TidyKind::Videos => "Videos",
            TidyKind::Documents => "Documents",
            TidyKind::Installers => "Installers",
            TidyKind::Archives => "Archives",
            TidyKind::Later => "Sort later",
        }
    }

    /// Whether files of this kind are sorted by year too.
    fn by_year(self) -> bool {
        matches!(
            self,
            TidyKind::Images | TidyKind::Videos | TidyKind::Documents
        )
    }
}

/// One file a sort moves, as the review shows it. Display text only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TidyMove {
    /// Its name now.
    pub name: String,
    /// The folder it goes into, below the folder being sorted: `Images\2026`.
    pub dest: String,
    /// Its name there: its own, or with a numbered suffix if a file there already has the name.
    pub to: String,
    pub kind: TidyKind,
    /// Allocated bytes. Moving it frees none of them.
    #[ts(type = "number")]
    pub bytes: u64,
    /// It gets a numbered suffix: a file there already has its name. Nothing is overwritten.
    pub renamed: bool,
    /// The delete proposal that takes this file too, if one does. With that proposal in the plan the file is
    /// deleted, not moved (delete wins over sort).
    #[ts(optional = nullable)]
    pub deleted_by: Option<String>,
}

/// A file a sort leaves where it is, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TidyStay {
    pub name: String,
    /// Why, as a sentence.
    pub reason: String,
    /// It was left out by the engine's checks (it changed since Vacancy looked, say), not by the rule.
    pub dropped: bool,
}

/// A folder already inside the folder being sorted, left as it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TidySubfolder {
    pub name: String,
    /// Files in it, as the scan counted them.
    #[ts(type = "number")]
    pub files: u64,
}

/// A Tidy up proposal: one known folder's loose files, sorted by the built-in rules. Never default-on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TidyProposal {
    /// `tidy-downloads`, `tidy-desktop` or `tidy-documents`.
    pub id: String,
    pub folder: Folder,
    /// The folder as the user knows it: "Downloads".
    pub name: String,
    /// Its path, for display.
    pub path: String,
    /// Loose files at its top that Vacancy looked at.
    #[ts(type = "number")]
    pub loose: u64,
    /// What moves where, in the folder's name order.
    pub moves: Vec<TidyMove>,
    /// What stays, and why.
    pub stays: Vec<TidyStay>,
    /// Folders already in it, left as they are.
    pub subfolders: Vec<TidySubfolder>,
    /// Destination folders (below it, `Images` or `Images\2026`) that already exist. The rest are made.
    pub existing: Vec<String>,
    /// More loose files than one run takes: how many more the scan counted, left for a later run.
    #[ts(type = "number")]
    pub later: u64,
    /// Vacancy looked at only the first [`LIMIT`] entries of the folder.
    pub capped: bool,
}

/// A drive's Tidy up proposals, with what is needed to plan them: each folder's inventory and drafts. Empty on
/// a drive that doesn't hold the known folders, and on macOS.
#[derive(Debug, Default)]
pub struct Tidy {
    folders: Vec<FolderSort>,
}

/// One folder's sort: the inventory taken for it, its drafts, and the proposal shown.
#[derive(Debug)]
struct FolderSort {
    inventory: Inventory,
    drafts: Vec<Drafted>,
    proposal: TidyProposal,
}

/// A draft, with the delete proposal that takes its file, if any.
#[derive(Debug)]
struct Drafted {
    draft: Draft,
    deleted_by: Option<String>,
}

impl Tidy {
    /// The proposals, in folder order: Downloads, Desktop, Documents.
    pub fn proposals(&self) -> Vec<TidyProposal> {
        self.folders.iter().map(|f| f.proposal.clone()).collect()
    }

    /// Whether `id` is one of these proposals.
    pub fn has(&self, id: &str) -> bool {
        self.folders.iter().any(|f| f.proposal.id == id)
    }

    /// Validates the sort `id` again, now, for a plan whose delete proposals are `deleting`: their files are left
    /// out. Gives the approved capability and the moves left out since the scan (changed, or no free name), or
    /// why nothing of it can run.
    pub(crate) fn validate(
        &self,
        id: &str,
        deleting: &HashSet<&str>,
        cancel: &CancelToken,
    ) -> Option<Result<Approved, String>> {
        let sort = self.folders.iter().find(|f| f.proposal.id == id)?;
        let drafts: Vec<Draft> = sort
            .drafts
            .iter()
            .filter(|d| {
                d.deleted_by
                    .as_deref()
                    .is_none_or(|by| !deleting.contains(by))
            })
            .map(|d| d.draft.clone())
            .collect();
        if drafts.is_empty() {
            return Some(Err(format!(
                "Every file Vacancy would sort in {} is in your Delete list, so nothing is left to move",
                sort.proposal.name
            )));
        }
        let names: HashMap<&str, &str> = sort
            .inventory
            .entries()
            .iter()
            .map(|e| (e.id.as_str(), e.name.as_str()))
            .collect();
        Some(match sort.inventory.validate(&drafts, cancel) {
            Ok(plan) => {
                let refused = plan
                    .dropped()
                    .iter()
                    .map(|d| {
                        let name = names.get(d.entry_id.as_str()).copied().unwrap_or("");
                        let (kind, reason) = said_dropped(&d.reason);
                        Skip {
                            path: join(&sort.proposal.path, name),
                            reason,
                            kind,
                            bytes: 0,
                            items: 1,
                        }
                    })
                    .collect();
                let mut into: Vec<(String, u64)> = Vec::new();
                for mv in plan.review() {
                    let top = mv.directories.first().cloned().unwrap_or_default();
                    match into.iter_mut().find(|(t, _)| *t == top) {
                        Some((_, n)) => *n += 1,
                        None => into.push((top, 1)),
                    }
                }
                into.sort_by_key(|b| std::cmp::Reverse(b.1));
                Ok(Approved {
                    id: sort.proposal.id.clone(),
                    folder: sort.proposal.folder,
                    name: sort.proposal.name.clone(),
                    path: sort.proposal.path.clone(),
                    moves: plan.review().len() as u64,
                    into: into.into_iter().map(|(t, _)| t).collect(),
                    refused,
                    plan: Arc::new(Mutex::new(Some(plan))),
                })
            }
            Err(e) => Err(format!(
                "Nothing in {} can be sorted now: {}",
                sort.proposal.name,
                said_dropped(&e.to_string()).1.to_lowercase()
            )),
        })
    }
}

/// One folder's sort, validated for a plan: the capability the run hands to the engine, and what the review shows.
#[derive(Clone, Debug)]
pub(crate) struct Approved {
    pub(crate) id: String,
    pub(crate) folder: Folder,
    pub(crate) name: String,
    pub(crate) path: String,
    /// Files it moves.
    pub(crate) moves: u64,
    /// The top folders the files go into, most files first ("Images").
    pub(crate) into: Vec<String>,
    /// Moves left out since the scan, with why.
    pub(crate) refused: Vec<Skip>,
    /// Taken by the run, once.
    pub(crate) plan: Arc<Mutex<Option<ValidatedPlan>>>,
}

impl Approved {
    /// The step's title: "Sort Downloads".
    pub(crate) fn title(&self) -> String {
        format!("Sort {}", self.name)
    }

    /// The map category its folder is in.
    pub(crate) fn category(&self) -> CategoryId {
        match self.folder {
            Folder::Downloads => CategoryId::Downloads,
            Folder::Desktop | Folder::Documents => CategoryId::Media,
        }
    }
}

const FOLDERS: [Folder; 3] = [Folder::Downloads, Folder::Desktop, Folder::Documents];

/// The folder as the user knows it.
pub fn folder_name(folder: Folder) -> &'static str {
    match folder {
        Folder::Downloads => "Downloads",
        Folder::Desktop => "Desktop",
        Folder::Documents => "Documents",
    }
}

fn folder_id(folder: Folder) -> &'static str {
    match folder {
        Folder::Downloads => "downloads",
        Folder::Desktop => "desktop",
        Folder::Documents => "documents",
    }
}

fn known(env: &Env, folder: Folder) -> &Path {
    match folder {
        Folder::Downloads => &env.known.downloads,
        Folder::Desktop => &env.known.desktop,
        Folder::Documents => &env.known.documents,
    }
}

fn join(folder: &str, rest: &str) -> String {
    if rest.is_empty() {
        folder.to_owned()
    } else {
        format!("{}\\{rest}", folder.trim_end_matches('\\'))
    }
}

/// Where the rules put a file, or why they leave it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorted {
    Into(TidyKind),
    Stays(&'static str),
}

const IMAGES: &[&str] = &[
    "jpg", "jpeg", "jfif", "png", "gif", "bmp", "webp", "heic", "heif", "tif", "tiff", "svg",
    "ico", "raw", "cr2", "cr3", "nef", "arw", "dng", "orf", "psd", "avif",
];
const VIDEOS: &[&str] = &[
    "mp4", "mov", "avi", "mkv", "wmv", "webm", "m4v", "flv", "mpg", "mpeg", "3gp", "mts", "m2ts",
];
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "xls", "xlsx", "xlsm", "ods", "csv", "ppt",
    "pptx", "odp", "pages", "numbers", "key", "epub", "xps", "oxps",
];
const INSTALLERS: &[&str] = &[
    "exe",
    "msi",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "iso",
    "img",
    "dmg",
    "pkg",
];
const ARCHIVES: &[&str] = &[
    "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "zst", "cab",
];
const SHORTCUTS: &[&str] = &["lnk", "url", "pif", "appref-ms", "website"];
const UNFINISHED: &[&str] = &[
    "crdownload",
    "part",
    "partial",
    "download",
    "opdownload",
    "tmp",
];

/// The built-in rules: where the file named `name` goes, or why it stays. Names and extensions only.
pub fn sort(name: &str) -> Sorted {
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), "desktop.ini" | "thumbs.db" | ".ds_store") {
        return Sorted::Stays("Windows uses it to show this folder.");
    }
    if lower.starts_with("~$") {
        return Sorted::Stays("An Office lock file: the document it belongs to is open.");
    }
    if lower.starts_with('.') {
        return Sorted::Stays("A hidden settings file. Apps expect it where it is.");
    }
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    if SHORTCUTS.contains(&ext) {
        return Sorted::Stays("Shortcuts stay where you put them.");
    }
    if UNFINISHED.contains(&ext) {
        return Sorted::Stays(
            "It looks like a download that hasn't finished, or a temporary file.",
        );
    }
    let kind = if IMAGES.contains(&ext) {
        TidyKind::Images
    } else if VIDEOS.contains(&ext) {
        TidyKind::Videos
    } else if DOCUMENTS.contains(&ext) {
        TidyKind::Documents
    } else if INSTALLERS.contains(&ext) {
        TidyKind::Installers
    } else if ARCHIVES.contains(&ext) {
        TidyKind::Archives
    } else {
        TidyKind::Later
    };
    Sorted::Into(kind)
}

/// The UTC year of a time in nanoseconds since the Unix epoch (1970 for anything before it).
pub fn year_of(ns: i128) -> i32 {
    let days = (ns.max(0) / 86_400_000_000_000) as i64;
    // Civil-from-days (Howard Hinnant's algorithm), for the year only.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2)) as i32
}

/// The folders a file of `kind` modified in `year` goes into, below the folder `from` being sorted. In
/// Documents, documents go straight into a year folder: it already is the documents folder.
pub fn destination(from: Folder, kind: TidyKind, year: i32) -> Vec<String> {
    let mut dirs = Vec::new();
    if !(from == Folder::Documents && kind == TidyKind::Documents) {
        dirs.push(kind.folder_name().to_owned());
    }
    if kind.by_year() {
        dirs.push(year.to_string());
    }
    dirs
}

/// What the delete proposals take, by lower-cased path: single files, folders whole, and folders' loose files.
struct Taken<'a> {
    files: HashMap<String, &'a str>,
    whole: Vec<(String, &'a str)>,
    loose: HashMap<String, &'a str>,
}

impl<'a> Taken<'a> {
    fn of(snap: &Snapshot, proposals: &'a Proposals) -> Self {
        let mut taken = Taken {
            files: HashMap::new(),
            whole: Vec::new(),
            loose: HashMap::new(),
        };
        for (i, p) in proposals.list().iter().enumerate() {
            if p.action != ProposalAction::Delete {
                continue;
            }
            for &t in proposals.targets(i) {
                match t {
                    ItemRef::File(f) => {
                        taken.files.insert(key(&snap.file_path(f)), p.id.as_str());
                    }
                    ItemRef::Dir(d) => taken.whole.push((key(&snap.path(d)), p.id.as_str())),
                    ItemRef::FilesIn(d) => {
                        taken.loose.insert(key(&snap.path(d)), p.id.as_str());
                    }
                    ItemRef::Proposal(_) => {}
                }
            }
        }
        taken
    }

    /// The delete proposal that takes the loose file `name` at the top of `folder`, if one does.
    fn by(&self, folder: &Path, name: &str) -> Option<String> {
        let dir = key(folder);
        let file = key(&folder.join(name));
        self.files
            .get(&file)
            .or_else(|| self.loose.get(&dir))
            .copied()
            .or_else(|| {
                self.whole
                    .iter()
                    .find(|(d, _)| dir == *d || dir.starts_with(&format!("{d}\\")))
                    .map(|(_, id)| *id)
            })
            .map(str::to_owned)
    }
}

fn key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// The Tidy up proposals for a finished scan whose environment is `env`, read now from fresh bounded inventories
/// of the three known folders (names, sizes and dates only). `proposals` are the scan's cleanups: a file one of
/// them deletes is marked so a plan with it leaves the file out of the sort. Nothing on macOS, nor on a drive that
/// doesn't hold the known folders; a folder Vacancy can't inventory is left out.
pub fn propose(snap: &Snapshot, proposals: &Proposals, env: &Env, cancel: &CancelToken) -> Tidy {
    if env.platform != Platform::Windows {
        return Tidy::default();
    }
    let taken = Taken::of(snap, proposals);
    let mut folders = Vec::new();
    for at in FOLDERS {
        if cancel.is_cancelled() {
            return Tidy::default();
        }
        let Ok(inventory) = Inventory::capture_folder(env, at, cancel) else {
            continue;
        };
        if let Some(sort) = sort_folder(snap, env, at, inventory, &taken, cancel) {
            folders.push(sort);
        }
    }
    Tidy { folders }
}

fn sort_folder(
    snap: &Snapshot,
    env: &Env,
    at: Folder,
    inventory: Inventory,
    taken: &Taken<'_>,
    cancel: &CancelToken,
) -> Option<FolderSort> {
    let root = known(env, at);
    let path = root.display().to_string();
    let mut entries: Vec<&InventoryEntry> = inventory.entries().iter().collect();
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut stays = Vec::new();
    let mut drafts = Vec::new();
    let mut kinds: HashMap<String, TidyKind> = HashMap::new();
    for e in &entries {
        match sort(&e.name) {
            Sorted::Stays(why) => stays.push(TidyStay {
                name: e.name.clone(),
                reason: why.to_owned(),
                dropped: false,
            }),
            Sorted::Into(kind) => {
                kinds.insert(e.id.clone(), kind);
                drafts.push(Drafted {
                    draft: Draft {
                        entry_id: e.id.clone(),
                        destination: at,
                        directories: destination(at, kind, year_of(e.modified_ns)),
                        filename: None,
                    },
                    deleted_by: taken.by(root, &e.name),
                });
            }
        }
    }
    if drafts.is_empty() {
        return None;
    }
    let all: Vec<Draft> = drafts.iter().map(|d| d.draft.clone()).collect();
    let plan = inventory.validate(&all, cancel).ok()?;
    let by_id: HashMap<&str, &InventoryEntry> =
        entries.iter().map(|e| (e.id.as_str(), *e)).collect();
    let deleted: HashMap<&str, Option<String>> = drafts
        .iter()
        .map(|d| (d.draft.entry_id.as_str(), d.deleted_by.clone()))
        .collect();
    let moves = plan
        .review()
        .iter()
        .filter_map(|mv| {
            let e = by_id.get(mv.entry_id.as_str())?;
            Some(TidyMove {
                name: e.name.clone(),
                dest: mv.directories.join("\\"),
                to: mv.filename.clone(),
                kind: kinds.get(&mv.entry_id).copied().unwrap_or(TidyKind::Later),
                bytes: e.allocated_bytes,
                renamed: mv.filename != e.name,
                deleted_by: deleted.get(mv.entry_id.as_str()).cloned().flatten(),
            })
        })
        .collect::<Vec<_>>();
    for d in plan.dropped() {
        if let Some(e) = by_id.get(d.entry_id.as_str()) {
            stays.push(TidyStay {
                name: e.name.clone(),
                reason: format!(
                    "{} So Vacancy left this one out.",
                    said_dropped(&d.reason).1
                ),
                dropped: true,
            });
        }
    }
    // Only drafts that made it into the review are planned; the rest are already said in `stays`.
    let kept: HashSet<String> = plan.review().iter().map(|m| m.entry_id.clone()).collect();
    let drafts: Vec<Drafted> = drafts
        .into_iter()
        .filter(|d| kept.contains(&d.draft.entry_id))
        .collect();
    let mut existing: Vec<String> = plan
        .directories()
        .into_iter()
        .filter(|d| d.existing)
        .map(|d| d.names.join("\\"))
        .collect();
    existing.sort();
    let dir = snap.find(root);
    let subfolders = dir
        .map(|d| {
            let mut subs: Vec<TidySubfolder> = snap
                .children(d)
                .map(|c| TidySubfolder {
                    name: snap.name(c).to_string_lossy().into_owned(),
                    files: snap.tree(c).files,
                })
                .collect();
            subs.sort_by_key(|a| a.name.to_lowercase());
            subs
        })
        .unwrap_or_default();
    let capped = inventory.truncated();
    let scanned = dir.map_or(0, |d| snap.own(d).files);
    let later = if capped {
        scanned.saturating_sub(entries.len() as u64)
    } else {
        0
    };
    Some(FolderSort {
        proposal: TidyProposal {
            id: format!("{PREFIX}{}", folder_id(at)),
            folder: at,
            name: folder_name(at).to_owned(),
            path,
            loose: entries.len() as u64,
            moves,
            stays,
            subfolders,
            existing,
            later,
            capped,
        },
        inventory,
        drafts,
    })
}

/// A draft validation dropped, said for the review: what kind of skip, and a sentence.
fn said_dropped(reason: &str) -> (SkipKind, String) {
    let is = |e: Error| reason == e.to_string();
    if is(Error::Changed) || is(Error::UnknownEntry) {
        (
            SkipKind::Changed,
            "It changed after Vacancy looked.".to_owned(),
        )
    } else if is(Error::InvalidName) {
        (
            SkipKind::Refused,
            "Its new name isn't one Windows allows.".to_owned(),
        )
    } else if is(Error::Bounds) {
        (
            SkipKind::Refused,
            "Every free name for it is taken.".to_owned(),
        )
    } else if reason.starts_with("the Guard refused") {
        (
            SkipKind::Protected,
            "Vacancy may not touch it there.".to_owned(),
        )
    } else {
        (
            SkipKind::Failed,
            format!("Vacancy couldn't check it ({reason})."),
        )
    }
}

/// A move the run or Undo skipped, said for the user: what kind of skip, and a reason in the run's style.
fn said_skip(reason: &str, undo: bool) -> (SkipKind, String) {
    let is = |e: Error| reason == e.to_string() || reason.starts_with(&e.to_string());
    if is(Error::InUse) {
        (
            SkipKind::InUse,
            "It was open in another app when its turn came".to_owned(),
        )
    } else if is(Error::Taken) {
        (
            SkipKind::Changed,
            if undo {
                "A file now has its old name".to_owned()
            } else {
                "A file there now has its new name".to_owned()
            },
        )
    } else if is(Error::Gone) {
        (
            SkipKind::Gone,
            if undo {
                "It is no longer where Vacancy put it".to_owned()
            } else {
                "It is no longer there".to_owned()
            },
        )
    } else if reason.contains("a directory now holds the destination name")
        || reason.contains("destination directory arrived")
    {
        (
            SkipKind::Changed,
            "A folder now has the name it was going to get".to_owned(),
        )
    } else if reason.contains("Guard") || reason.starts_with("the Guard refused") {
        (
            SkipKind::Protected,
            "Vacancy may not touch it there now".to_owned(),
        )
    } else if is(Error::Changed) || reason.contains("changed") {
        (
            SkipKind::Changed,
            if undo {
                "It, or its folder, changed after it moved".to_owned()
            } else {
                "It changed after you reviewed the plan".to_owned()
            },
        )
    } else {
        (
            SkipKind::Failed,
            format!("Windows didn't move it ({reason})"),
        )
    }
}

/// Where a run keeps its move journals: a folder of the app's data folder, which must be on the drive of the
/// folders being sorted.
#[derive(Clone, Debug)]
pub struct Journals {
    pub dir: PathBuf,
}

impl Journals {
    /// The environment the engine runs in: `env`, with the journal folder as its app data.
    fn env(&self, env: &Env) -> Env {
        let mut env = env.clone();
        env.app_data = self.dir.clone();
        env
    }
}

/// A journal's file name for the sort `id` of the run `run`. Letters, digits and dashes only.
pub(crate) fn journal_for(run: &str, id: &str) -> String {
    let clean: String = format!("{run}-{id}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{clean}.journal")
}

/// Whether `name` is a journal name [`journal_for`] could have made: so a report's field can never name a file
/// outside the journal folder.
fn plain_journal(name: &str) -> bool {
    name.ends_with(".journal")
        && name.len() < 200
        && name
            .trim_end_matches(".journal")
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What a sort step tells the run as it goes: how many of its moves it has handled, of how many.
pub(crate) type OnMoved<'a> = &'a mut dyn FnMut(u64, u64);

/// Runs one approved sort: begins its journal and runs the engine. Gives the step's skips, its folder's record for
/// the report, and how many files moved. Never resumes anything; a failure to journal stops the moves.
pub(crate) fn run_sort(
    approved: &Approved,
    run_id: &str,
    env: &Env,
    journals: Option<&Journals>,
    cancel: &CancelToken,
    on_moved: OnMoved<'_>,
) -> (Vec<Skip>, Reorganized) {
    let mut record = Reorganized {
        id: approved.id.clone(),
        folder: approved.name.clone(),
        moved_items: 0,
        planned_items: approved.moves,
        journal: String::new(),
        path: approved.path.clone(),
        into: approved.into.clone(),
        folders_made: 0,
        undoable: false,
        interrupted: false,
        note: None,
        undo: None,
    };
    let whole = |reason: String, kind: SkipKind| Skip {
        path: approved.path.clone(),
        reason,
        kind,
        bytes: 0,
        items: approved.moves,
    };
    if cancel.is_cancelled() {
        return (
            vec![whole(
                "You stopped the run before it got here".into(),
                SkipKind::Cancelled,
            )],
            record,
        );
    }
    let Some(journals) = journals else {
        return (
            vec![whole(
                "Vacancy had nowhere to keep the record of its moves, so it moved nothing".into(),
                SkipKind::Failed,
            )],
            record,
        );
    };
    let Some(plan) = approved.plan.lock().ok().and_then(|mut p| p.take()) else {
        return (
            vec![whole(
                "This sort already ran. Scan again to sort again".into(),
                SkipKind::Failed,
            )],
            record,
        );
    };
    let jenv = journals.env(env);
    if let Err(e) = std::fs::create_dir_all(&journals.dir) {
        return (
            vec![whole(
                format!(
                    "Vacancy couldn't make the folder for its record of moves ({e}), so it moved nothing"
                ),
                SkipKind::Failed,
            )],
            record,
        );
    }
    let name = journal_for(run_id, &approved.id);
    let mut engine = match Engine::begin(&jenv, &journals.dir.join(&name), plan) {
        Ok(engine) => engine,
        Err(e) => {
            return (
                vec![whole(
                    format!(
                        "Vacancy couldn't start the record of its moves ({e}), so it moved nothing"
                    ),
                    SkipKind::Failed,
                )],
                record,
            );
        }
    };
    record.journal = name;
    let result = engine.run_observed(cancel, &mut |done, total| {
        on_moved(done as u64, total as u64)
    });
    let ledger = engine.ledger();
    // Releases the journal lease now; on macOS the engine holds nothing.
    #[cfg_attr(not(windows), allow(clippy::drop_non_drop))]
    drop(engine);
    if let Err(e) = result {
        record.note = Some(format!(
            "Vacancy stopped moving files here: {e}. What moved can still be undone."
        ));
    }
    let skips = settle(&mut record, &ledger);
    (skips, record)
}

/// Fills a folder's record from its journal's ledger, and gives what the moves left in place, with why.
fn settle(record: &mut Reorganized, ledger: &Ledger) -> Vec<Skip> {
    let base = record.path.clone();
    let at = |names: &[String]| join(&base, &names.join("\\"));
    record.moved_items = ledger
        .moves
        .iter()
        .filter(|m| m.moved == OpState::Applied)
        .count() as u64;
    record.folders_made = ledger
        .directories
        .iter()
        .filter(|d| d.created == OpState::Applied)
        .count() as u64;
    record.undoable = ledger.moves.iter().any(undoable);
    record.interrupted = ledger.started && !ledger.finished;
    let mut skips = Vec::new();
    let mut not_run = 0;
    for m in &ledger.moves {
        match &m.moved {
            OpState::Skipped(reason) => {
                let (kind, reason) = said_skip(reason, false);
                skips.push(Skip {
                    path: at(&m.from),
                    reason,
                    kind,
                    bytes: m.allocated_bytes,
                    items: 1,
                });
            }
            OpState::Pending | OpState::Unresolved(_) => skips.push(Skip {
                path: at(&m.from),
                reason: "Its move wasn't confirmed. Vacancy kept a record of it to check".into(),
                kind: SkipKind::Failed,
                bytes: m.allocated_bytes,
                items: 1,
            }),
            OpState::NotRun => not_run += 1,
            OpState::Applied => {}
        }
    }
    if not_run > 0 {
        let cancelled = ledger.cancelled;
        skips.push(Skip {
            path: base.clone(),
            reason: if cancelled {
                format!(
                    "{} {} moved: you stopped the run",
                    crate::text::group_digits(not_run),
                    if not_run == 1 {
                        "file wasn't"
                    } else {
                        "files weren't"
                    }
                )
            } else {
                format!(
                    "{} {} moved: the run ended before their turn",
                    crate::text::group_digits(not_run),
                    if not_run == 1 {
                        "file wasn't"
                    } else {
                        "files weren't"
                    }
                )
            },
            kind: if cancelled {
                SkipKind::Cancelled
            } else {
                SkipKind::Failed
            },
            bytes: 0,
            items: not_run,
        });
    }
    skips
}

fn undoable(m: &LedgerMove) -> bool {
    m.moved == OpState::Applied && m.undo != OpState::Applied
}

/// What the journal says Undo did: what came back, what couldn't and why, which empty folders Vacancy made were
/// removed and which it kept.
fn undone(record: &Reorganized, ledger: &Ledger, at: i64) -> Undone {
    let base = &record.path;
    let rel = |names: &[String]| names.join("\\");
    let mut stuck = Vec::new();
    for m in ledger.moves.iter().filter(|m| m.moved == OpState::Applied) {
        let reason = match &m.undo {
            OpState::Applied => continue,
            OpState::Skipped(reason) => said_skip(reason, true),
            OpState::Pending | OpState::Unresolved(_) => (
                SkipKind::Failed,
                "Putting it back wasn't confirmed. Vacancy kept a record of it to check".to_owned(),
            ),
            OpState::NotRun => (
                SkipKind::Cancelled,
                "Undo stopped before it got to it".to_owned(),
            ),
        };
        stuck.push(Skip {
            path: join(base, &rel(&m.to)),
            reason: reason.1,
            kind: reason.0,
            bytes: m.allocated_bytes,
            items: 1,
        });
    }
    let made = ledger
        .directories
        .iter()
        .filter(|d| d.created == OpState::Applied);
    let (removed, kept): (Vec<_>, Vec<_>) = made.partition(|d| d.removed == OpState::Applied);
    Undone {
        at,
        back: ledger
            .moves
            .iter()
            .filter(|m| m.moved == OpState::Applied && m.undo == OpState::Applied)
            .count() as u64,
        stuck,
        removed: removed.iter().map(|d| rel(&d.names)).collect(),
        kept: kept.iter().map(|d| rel(&d.names)).collect(),
    }
}

/// Why a Tidy up can't be undone or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TidyError {
    #[error("Tidy up isn't available on this computer.")]
    Unsupported,
    #[error("This run has no folder tidy-up {0}.")]
    NoSuchSort(String),
    #[error("This tidy-up has no record of its moves, so it can't be undone.")]
    NoJournal,
    #[error(
        "Another Vacancy window is using this tidy-up's record of moves. Close it and try again."
    )]
    Busy,
    #[error("The record of this tidy-up's moves can't be read: {0}")]
    Unreadable(String),
    #[error("Some of this tidy-up's moves aren't confirmed, so Undo waits: {0}")]
    Unresolved(String),
}

fn open_engine(env: &Env, journals: &Journals, record: &Reorganized) -> Result<Engine, TidyError> {
    if env.platform != Platform::Windows {
        return Err(TidyError::Unsupported);
    }
    if record.journal.is_empty() {
        return Err(TidyError::NoJournal);
    }
    if !plain_journal(&record.journal) {
        return Err(TidyError::Unreadable(
            "its name isn't one Vacancy makes".into(),
        ));
    }
    match Engine::open(&journals.env(env), &journals.dir.join(&record.journal)) {
        Ok(engine) => Ok(engine),
        Err(Error::Busy) => Err(TidyError::Busy),
        Err(Error::Unsupported) => Err(TidyError::Unsupported),
        Err(e) => Err(TidyError::Unreadable(e.to_string())),
    }
}

/// Undoes the sort `id` of the run `report`, from its journal (which survives a restart): every file it moved
/// that is still as it left it goes back, and the empty folders it made go. Updates the report's record of that
/// sort with what came back, what couldn't and why. Never touches a file the journal didn't move.
pub fn undo(
    env: &Env,
    journals: &Journals,
    report: &mut RunReport,
    id: &str,
    cancel: &CancelToken,
) -> Result<(), TidyError> {
    let record = report
        .reorganized
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| TidyError::NoSuchSort(id.to_owned()))?;
    let mut engine = open_engine(env, journals, record)?;
    if engine.status().unresolved > 0 {
        engine
            .reconcile(cancel)
            .map_err(|e| TidyError::Unresolved(e.to_string()))?;
    }
    let result = engine.undo(cancel);
    let ledger = engine.ledger();
    // Releases the journal lease now; on macOS the engine holds nothing.
    #[cfg_attr(not(windows), allow(clippy::drop_non_drop))]
    drop(engine);
    settle(record, &ledger);
    record.undo = Some(undone(record, &ledger, now_ms()));
    match result {
        Ok(_) => {
            record.note = None;
            Ok(())
        }
        Err(Error::Unresolved | Error::StoppedUnresolved) => {
            let e = "a move back wasn't confirmed".to_owned();
            record.note = Some(format!(
                "Undo stopped: {e}. Vacancy kept a record of it to check."
            ));
            Ok(())
        }
        Err(e) => {
            record.note = Some(format!("Undo stopped: {e}."));
            Ok(())
        }
    }
}

/// Reads each sort of a reopened run from its journal, as it stands now: what moved, whether it can still be
/// undone, whether it was interrupted, and what an Undo did (if the report missed it). Metadata only: it may
/// record what a crash left unconfirmed, but it never moves, makes or removes anything, and never resumes a run.
/// Gives whether the report changed.
pub fn refresh(env: &Env, journals: &Journals, report: &mut RunReport) -> bool {
    let before = report.reorganized.clone();
    for record in report.reorganized.iter_mut() {
        if record.journal.is_empty() {
            continue;
        }
        let mut engine = match open_engine(env, journals, record) {
            Ok(engine) => engine,
            Err(e) => {
                record.undoable = false;
                record.note = Some(e.to_string());
                continue;
            }
        };
        if engine.status().unresolved > 0 {
            let _ = engine.reconcile(&CancelToken::new());
        }
        let ledger = engine.ledger();
        // Releases the journal lease now; on macOS the engine holds nothing.
        #[cfg_attr(not(windows), allow(clippy::drop_non_drop))]
        drop(engine);
        settle(record, &ledger);
        let undone_any = ledger.moves.iter().any(|m| m.undo != OpState::NotRun);
        if undone_any && record.undo.is_none() {
            record.undo = Some(undone(record, &ledger, now_ms()));
        }
        if record.interrupted {
            record.note = Some(
                "This tidy-up was interrupted: Vacancy closed while it ran. It isn't resumed; what moved can be undone."
                    .into(),
            );
        } else if record
            .note
            .as_deref()
            .is_some_and(|n| n.starts_with("The record of this tidy-up"))
        {
            record.note = None;
        }
    }
    report.moved_items = report.reorganized.iter().map(|r| r.moved_items).sum();
    report.reorganized != before
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_extension_and_leaves_shortcuts_and_settings() {
        assert_eq!(sort("IMG_3301.JPG"), Sorted::Into(TidyKind::Images));
        assert_eq!(sort("birthday.mov"), Sorted::Into(TidyKind::Videos));
        assert_eq!(sort("W-2 2025.pdf"), Sorted::Into(TidyKind::Documents));
        assert_eq!(
            sort("VSCodeUserSetup.exe"),
            Sorted::Into(TidyKind::Installers)
        );
        assert_eq!(sort("fonts.zip"), Sorted::Into(TidyKind::Archives));
        assert_eq!(sort("podcast.mp3"), Sorted::Into(TidyKind::Later));
        assert_eq!(sort("export_1734"), Sorted::Into(TidyKind::Later));
        assert!(matches!(sort("desktop.ini"), Sorted::Stays(_)));
        assert!(matches!(sort("Steam.lnk"), Sorted::Stays(_)));
        assert!(matches!(sort("video.mp4.crdownload"), Sorted::Stays(_)));
        assert!(matches!(sort(".hidden"), Sorted::Stays(_)));
        assert!(matches!(sort("~$budget.xlsx"), Sorted::Stays(_)));
    }

    #[test]
    fn years_come_from_the_modified_time() {
        let ns = |secs: i128| secs * 1_000_000_000;
        assert_eq!(year_of(0), 1970);
        assert_eq!(year_of(-5), 1970);
        assert_eq!(year_of(ns(1_704_067_199)), 2023); // 2023-12-31 23:59:59 UTC
        assert_eq!(year_of(ns(1_704_067_200)), 2024); // 2024-01-01 00:00:00 UTC
        assert_eq!(year_of(ns(1_709_164_800)), 2024); // 2024-02-29
        assert_eq!(year_of(ns(1_791_000_000)), 2026);
    }

    #[test]
    fn destinations_by_kind_and_year() {
        assert_eq!(
            destination(Folder::Downloads, TidyKind::Images, 2026),
            ["Images", "2026"]
        );
        assert_eq!(
            destination(Folder::Desktop, TidyKind::Documents, 2025),
            ["Documents", "2025"]
        );
        assert_eq!(
            destination(Folder::Documents, TidyKind::Documents, 2024),
            ["2024"]
        );
        assert_eq!(
            destination(Folder::Documents, TidyKind::Images, 2024),
            ["Images", "2024"]
        );
        assert_eq!(
            destination(Folder::Downloads, TidyKind::Installers, 2026),
            ["Installers"]
        );
        assert_eq!(
            destination(Folder::Downloads, TidyKind::Archives, 2020),
            ["Archives"]
        );
        assert_eq!(
            destination(Folder::Downloads, TidyKind::Later, 2020),
            ["Sort later"]
        );
    }

    #[test]
    fn journal_names_stay_plain() {
        let name = journal_for("run-1-2", "tidy-downloads");
        assert_eq!(name, "run-1-2-tidy-downloads.journal");
        assert!(plain_journal(&name));
        assert!(plain_journal(&journal_for("../x:y", "z")));
        for bad in ["..\\x.journal", "a/b.journal", "x.txt", "C:x.journal", ""] {
            assert!(!plain_journal(bad), "{bad}");
        }
    }

    #[test]
    fn skip_reasons_are_said_for_people() {
        assert_eq!(
            said_skip(&Error::InUse.to_string(), false).0,
            SkipKind::InUse
        );
        assert_eq!(
            said_skip(&Error::Taken.to_string(), true).1,
            "A file now has its old name"
        );
        assert_eq!(said_skip(&Error::Gone.to_string(), true).0, SkipKind::Gone);
        assert_eq!(
            said_skip("fingerprint or parent changed before rename", false).1,
            "It changed after you reviewed the plan"
        );
        assert_eq!(
            said_dropped(&Error::Changed.to_string()).0,
            SkipKind::Changed
        );
    }
}

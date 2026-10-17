//! What a run did, as the run screen streams it and the finish shows it (the highlight slides, then the summary):
//! per step, what was planned and what was freed, what was skipped and why; for the whole run, the totals
//! cross-checked against the drive's own free-space figures, and the map the run started from. A report holds every
//! figure the finish shows, so a run reopened from the history reads exactly as it did when it ended. Each finished
//! run is kept in the run history (see [`save`], [`history`] and [`open`]).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::categorize::CategoryId;
use crate::map::Overview;

/// How a step ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Outcome {
    /// Everything in it was deleted.
    Done,
    /// Some of it was deleted; the rest was skipped, each with a reason.
    Partial,
    /// Nothing in it was deleted.
    Skipped,
}

/// Why something was left in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SkipKind {
    /// An app had it open (a Windows sharing violation; on macOS, a process holding it open).
    InUse,
    /// It was modified, replaced or moved since the scan, so it is not what was approved.
    Changed,
    /// It is no longer there.
    Gone,
    /// It is a link (symlink, junction, mount point), or the way to it goes through one, since the scan.
    Link,
    /// It is, is inside, or holds a place Vacancy never touches.
    Protected,
    /// It is outside the places Vacancy may clean.
    Outside,
    /// Another volume is mounted there.
    OtherVolume,
    /// A kind of file Vacancy doesn't delete: a socket, a device, a cloud placeholder.
    Special,
    /// The OS refused (no permission, or locked).
    Refused,
    /// Something went wrong reading or deleting it.
    Failed,
    /// The run was stopped before it got there.
    Cancelled,
    /// Its name here was deleted, but the file has other names elsewhere (hard links), which keep its space in use.
    /// Not a failure: the step still counts as done.
    HardLinked,
    /// It was deleted while an app had it open (macOS): its space comes back when the app lets go of it, so it
    /// isn't counted as freed. Not a failure either.
    HeldOpen,
}

impl SkipKind {
    /// A note about something deleted rather than something left: it doesn't make a step partial, and isn't
    /// counted among the files left in place.
    pub fn is_note(self) -> bool {
        matches!(self, SkipKind::HardLinked | SkipKind::HeldOpen)
    }
}

/// Something the run left in place, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Skip {
    /// Where: the planned place, or the file inside it.
    pub path: String,
    /// Why, in a few words ("3 files were in use").
    pub reason: String,
    pub kind: SkipKind,
    /// Allocated bytes left in place, as far as is known (the scan's figure for a place skipped whole).
    #[ts(type = "number")]
    pub bytes: u64,
    /// Files left in place, as far as is known.
    #[ts(type = "number")]
    pub items: u64,
}

/// One step of a run: one proposal of the plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepReport {
    /// The proposal's id.
    pub id: String,
    /// The proposal's title.
    pub title: String,
    /// Why it was safe to remove, from the proposal.
    pub why: String,
    /// The category its tile is in.
    pub category: CategoryId,
    /// It comes back on its own if needed.
    pub regenerable: bool,
    /// Allocated bytes the plan meant to free, from the scan.
    #[ts(type = "number")]
    pub planned_bytes: u64,
    /// Allocated bytes freed: files deleted whose last name it was.
    #[ts(type = "number")]
    pub freed_bytes: u64,
    /// Files the plan meant to delete.
    #[ts(type = "number")]
    pub planned_items: u64,
    /// Files deleted.
    #[ts(type = "number")]
    pub deleted_items: u64,
    /// Places the plan took files from.
    #[ts(type = "number")]
    pub places: u64,
    pub outcome: Outcome,
    /// What was left in place, and why.
    pub skipped: Vec<Skip>,
}

/// What one category got back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CategoryFreed {
    pub id: CategoryId,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// Advice in the plan that Vacancy can't carry out itself (uninstall a game from its store), left for the user.
/// Never counted as freed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LeftForYou {
    /// The proposal's id.
    pub id: String,
    pub title: String,
    /// One line with the numbers.
    pub summary: String,
    /// What to do, and why.
    pub why: String,
    /// What doing it would free, from the scan.
    #[ts(type = "number")]
    pub bytes: u64,
}

/// A folder the run reorganized: its files moved into a tidier layout, every move journaled so it can be undone.
/// Reorganizations arrive with ticket 08; until then no run has any.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Reorganized {
    /// The reorganization's id.
    pub id: String,
    /// The folder, as the user knows it ("Downloads").
    pub folder: String,
    /// Files moved in it.
    #[ts(type = "number")]
    pub moved_items: u64,
}

/// What a run did. See the module docs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunReport {
    /// Names the run in the history.
    pub id: String,
    /// The plan it ran.
    pub plan_id: String,
    /// The drive, by mount point, and its name.
    pub mount_point: String,
    pub volume_name: String,
    /// When it started, in milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub started_at: i64,
    /// How long it took.
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// The user stopped it part way.
    pub cancelled: bool,
    /// Each step, in the plan's order.
    pub steps: Vec<StepReport>,
    /// Allocated bytes the plan meant to free.
    #[ts(type = "number")]
    pub planned_bytes: u64,
    /// Allocated bytes freed.
    #[ts(type = "number")]
    pub freed_bytes: u64,
    /// Files deleted.
    #[ts(type = "number")]
    pub deleted_items: u64,
    /// Files moved by reorganizations.
    #[serde(default)]
    #[ts(type = "number")]
    pub moved_items: u64,
    /// Files left in place, as far as is known.
    #[ts(type = "number")]
    pub skipped_items: u64,
    /// Freed bytes per category, biggest first.
    pub freed_by_category: Vec<CategoryFreed>,
    /// The drive's capacity, and its used and free bytes before and after, as the OS reports them.
    #[ts(type = "number")]
    pub total_bytes: u64,
    #[ts(type = "number")]
    pub used_before: u64,
    #[ts(type = "number")]
    pub used_after: u64,
    #[ts(type = "number")]
    pub free_before: u64,
    #[ts(type = "number")]
    pub free_after: u64,
    /// How much the drive's free space grew while the run went (negative if it shrank). Other apps write too, so it
    /// rarely equals `freed_bytes` exactly; the difference is shown as it is.
    #[ts(type = "number")]
    pub free_change: i64,
    /// The step that freed the most, by proposal id.
    pub biggest_win: Option<String>,
    /// Advice in the plan that Vacancy can't carry out itself. Never counted as freed.
    pub left_for_you: Vec<LeftForYou>,
    /// Folders reorganized, in the plan's order.
    #[serde(default)]
    pub reorganized: Vec<Reorganized>,
    /// The map the run started from: the scan's overview, with each category's name and size before the run. The
    /// summary draws its before/after map from it.
    pub map: Overview,
}

/// A run in the history list: enough to find it and to say what it did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunListing {
    /// Opens the run with [`open`].
    pub id: String,
    pub mount_point: String,
    pub volume_name: String,
    /// When it started, in milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number")]
    pub freed_bytes: u64,
    #[ts(type = "number")]
    pub deleted_items: u64,
    #[ts(type = "number")]
    pub moved_items: u64,
    /// The user stopped it part way.
    pub cancelled: bool,
}

impl From<&RunReport> for RunListing {
    fn from(r: &RunReport) -> Self {
        Self {
            id: r.id.clone(),
            mount_point: r.mount_point.clone(),
            volume_name: r.volume_name.clone(),
            started_at: r.started_at,
            freed_bytes: r.freed_bytes,
            deleted_items: r.deleted_items,
            moved_items: r.moved_items,
            cancelled: r.cancelled,
        }
    }
}

/// The run history: the runs that could be read, newest first, and how many files in the history folder couldn't
/// be (damaged, cut short, or kept by a version of Vacancy with other figures). Those are passed over, never fatal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunHistory {
    pub runs: Vec<RunListing>,
    pub unreadable: u32,
}

/// The run history's folder inside the app's data folder.
pub fn history_dir(app_data: &Path) -> PathBuf {
    app_data.join("runs")
}

/// A run's file name in the history folder: letters, digits and dashes kept, anything else made `_`, so no id can
/// name a file outside the folder.
fn file_name(id: &str) -> String {
    let name: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{name}.json")
}

/// Keeps a finished run in the history folder `dir` (made if it isn't there), one JSON file per run. Returns the
/// file. Written to a temporary name first and renamed into place, so a crash never leaves half a report.
pub fn save(dir: &Path, report: &RunReport) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(file_name(&report.id));
    let mut part = file.clone().into_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    let json = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    std::fs::write(&part, json)?;
    std::fs::rename(&part, &file)?;
    Ok(file)
}

/// The runs kept in the history folder `dir`, newest first. A `.json` file that can't be read as a report is
/// counted in `unreadable` and passed over; other files (a `.part` a crash left) are ignored. No folder yet is an
/// empty history.
pub fn history(dir: &Path) -> RunHistory {
    let mut out = RunHistory::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|x| x == "json") {
            continue;
        }
        match read(&path) {
            Ok(report) => out.runs.push(RunListing::from(&report)),
            Err(_) => out.unreadable += 1,
        }
    }
    out.runs.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    out
}

/// The run `id` from the history folder `dir`, as it was kept.
pub fn open(dir: &Path, id: &str) -> io::Result<RunReport> {
    let report = read(&dir.join(file_name(id)))?;
    if report.id != id {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("the history has no run {id}"),
        ));
    }
    Ok(report)
}

/// One kept run. Fails if the file can't be read or isn't a whole report.
fn read(file: &Path) -> io::Result<RunReport> {
    let bytes = std::fs::read(file)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{ItemKind, MapCategory, MapItem, ScanSummary};
    use crate::volume::{DriveKind, VolumeInfo};

    fn report(id: &str, at: i64) -> RunReport {
        RunReport {
            id: id.into(),
            plan_id: "plan-1".into(),
            mount_point: "C:\\".into(),
            volume_name: "Local Disk (C:)".into(),
            started_at: at,
            duration_ms: 1200,
            cancelled: false,
            steps: vec![StepReport {
                id: "w-temp".into(),
                title: "Temp folders".into(),
                why: "Apps make new ones as they need them.".into(),
                category: CategoryId::Caches,
                regenerable: true,
                planned_bytes: 10,
                freed_bytes: 8,
                planned_items: 2,
                deleted_items: 1,
                places: 1,
                outcome: Outcome::Partial,
                skipped: vec![Skip {
                    path: "C:\\x".into(),
                    reason: "1 file was in use".into(),
                    kind: SkipKind::InUse,
                    bytes: 2,
                    items: 1,
                }],
            }],
            planned_bytes: 10,
            freed_bytes: 8,
            deleted_items: 1,
            moved_items: 0,
            skipped_items: 1,
            freed_by_category: vec![CategoryFreed {
                id: CategoryId::Caches,
                bytes: 8,
            }],
            total_bytes: 100,
            used_before: 50,
            used_after: 42,
            free_before: 50,
            free_after: 58,
            free_change: 8,
            biggest_win: Some("w-temp".into()),
            left_for_you: vec![LeftForYou {
                id: "w-game".into(),
                title: "A game you haven't played in a year".into(),
                summary: "40 GB in Steam".into(),
                why: "Uninstall it from Steam.".into(),
                bytes: 40,
            }],
            reorganized: Vec::new(),
            map: map(),
        }
    }

    fn map() -> Overview {
        Overview {
            volume: VolumeInfo {
                name: "Local Disk (C:)".into(),
                mount_point: "C:\\".into(),
                id: "1234".into(),
                kind: DriveKind::Internal,
                system: true,
                total_bytes: 100,
                used_bytes: 50,
                free_bytes: 50,
            },
            free_bytes: 50,
            scan: ScanSummary {
                files: 3,
                folders: 2,
                scanned_bytes: 50,
                hidden_bytes: 0,
                over_count_bytes: 0,
                unreadable: 0,
                unreadable_samples: Vec::new(),
                links_skipped: 0,
                hard_links_merged: 0,
                duration_ms: 10,
            },
            categories: vec![MapCategory {
                id: CategoryId::Caches,
                name: "Caches & temp".into(),
                blurb: "Rebuilt on demand.".into(),
                protected: false,
                bytes: 50,
                items: vec![MapItem {
                    id: "p:w-temp".into(),
                    name: "Temp folders".into(),
                    path: String::new(),
                    bytes: 10,
                    files: 2,
                    count: 0,
                    last_used: Some(1),
                    kind: ItemKind::Proposal,
                    browsable: false,
                    note: None,
                    proposal: Some("w-temp".into()),
                }],
            }],
        }
    }

    fn ids(h: &RunHistory) -> Vec<&str> {
        h.runs.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn keeps_runs_and_lists_them_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let runs = history_dir(dir.path());
        assert_eq!(history(&runs), RunHistory::default(), "no history yet");
        save(&runs, &report("run/1:a", 1)).unwrap();
        save(&runs, &report("run-2", 2)).unwrap();
        let back = history(&runs);
        assert_eq!(ids(&back), ["run-2", "run/1:a"]);
        assert_eq!(back.unreadable, 0);
        assert_eq!(back.runs[1], RunListing::from(&report("run/1:a", 1)));
        assert!(
            std::fs::read_dir(&runs)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().ends_with(".part")),
            "no half-written report is left"
        );
    }

    #[test]
    fn opens_a_run_as_it_was_kept() {
        let dir = tempfile::tempdir().unwrap();
        let runs = history_dir(dir.path());
        save(&runs, &report("run/1:a", 1)).unwrap();
        assert_eq!(open(&runs, "run/1:a").unwrap(), report("run/1:a", 1));
        assert_eq!(
            open(&runs, "run-9").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        // Another id that maps to the same file name is not that run.
        assert_eq!(
            open(&runs, "run_1_a").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        // An id can't reach outside the history folder.
        std::fs::write(dir.path().join("outside.json"), "{}").unwrap();
        assert!(open(&runs, "../outside").is_err());
    }

    #[test]
    fn counts_damaged_files_and_passes_over_them() {
        let dir = tempfile::tempdir().unwrap();
        let runs = history_dir(dir.path());
        save(&runs, &report("run-1", 1)).unwrap();
        std::fs::write(runs.join("notes.txt"), "not a report").unwrap();
        std::fs::write(runs.join("run-2.json.part"), "{\"id\":").unwrap();
        std::fs::write(runs.join("broken.json"), "{").unwrap();
        std::fs::write(runs.join("empty.json"), "").unwrap();
        // A report from before reports kept their map.
        let mut old = serde_json::to_value(report("run-0", 0)).unwrap();
        old.as_object_mut().unwrap().remove("map");
        std::fs::write(runs.join("run-0.json"), old.to_string()).unwrap();
        std::fs::create_dir(runs.join("folder.json")).unwrap();
        let back = history(&runs);
        assert_eq!(ids(&back), ["run-1"]);
        assert_eq!(back.unreadable, 4, "broken, empty, old and the folder");
        assert_eq!(
            open(&runs, "run-0").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn reads_reports_kept_before_moves_were_counted() {
        let mut json = serde_json::to_value(report("run-1", 1)).unwrap();
        let fields = json.as_object_mut().unwrap();
        fields.remove("movedItems");
        fields.remove("reorganized");
        let back: RunReport = serde_json::from_value(json).unwrap();
        assert_eq!(back, report("run-1", 1));
    }
}

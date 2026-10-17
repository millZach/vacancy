//! What a run did, as the run screen streams it and the summary shows it: per step, what was planned and what was
//! freed, what was skipped and why; for the whole run, the totals cross-checked against the drive's own free-space
//! figures. Each finished run is kept in the run history (see [`save`] and [`history`]).

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::categorize::CategoryId;

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
    /// Advice in the plan that Vacancy can't carry out itself, by proposal id. Never counted as freed.
    pub left_for_you: Vec<String>,
}

/// The run history's folder inside the app's data folder.
pub fn history_dir(app_data: &Path) -> PathBuf {
    app_data.join("runs")
}

/// Keeps a finished run in the history folder `dir` (made if it isn't there), one JSON file per run. Returns the
/// file. Written to a temporary name first and renamed into place, so a crash never leaves half a report.
pub fn save(dir: &Path, report: &RunReport) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name: String = report
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = dir.join(format!("{name}.json"));
    let part = dir.join(format!("{name}.json.part"));
    let json = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    std::fs::write(&part, json)?;
    std::fs::rename(&part, &file)?;
    Ok(file)
}

/// The runs kept in the history folder `dir`, newest first. Files that aren't reports are passed over.
pub fn history(dir: &Path) -> Vec<RunReport> {
    let mut out: Vec<RunReport> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect();
    out.sort_by_key(|r: &RunReport| std::cmp::Reverse(r.started_at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
            left_for_you: Vec::new(),
        }
    }

    #[test]
    fn keeps_runs_and_reads_them_back_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let runs = history_dir(dir.path());
        assert!(history(&runs).is_empty(), "no history yet");
        save(&runs, &report("run/1:a", 1)).unwrap();
        save(&runs, &report("run-2", 2)).unwrap();
        std::fs::write(runs.join("notes.txt"), "not a report").unwrap();
        std::fs::write(runs.join("broken.json"), "{").unwrap();
        let back = history(&runs);
        assert_eq!(
            back.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["run-2", "run/1:a"]
        );
        assert_eq!(back[1], report("run/1:a", 1));
        assert!(
            std::fs::read_dir(&runs)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().ends_with(".part")),
            "no half-written report is left"
        );
    }
}

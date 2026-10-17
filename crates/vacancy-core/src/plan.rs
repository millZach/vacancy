//! The planner: turns the user's selection of proposals into an immutable [`Plan`], the only thing the
//! [executor](crate::execute) ever acts on.
//!
//! Each action in a plan is one target of a proposal (a folder deleted whole, a file, or a folder's loose files),
//! with its fingerprint from the scan:
//!
//! - **identity**: the volume and file id (Windows) or device and inode (macOS) the scan recorded, confirmed now by
//!   a walk of the target's real chain (never through a link) that the Guard judges (ADR 0002: every plan item
//!   passes the Guard when it is planned and again when it is carried out). A target the Guard refuses now, or
//!   found with another identity than the scan recorded, is left out of what runs and reported as skipped.
//! - **size**: the allocated bytes and file count the scan measured. For a folder's loose files, which the scan
//!   keeps no list of, the folder's totals of them, and the names of the files it kept individually (which are
//!   not loose).
//! - **modified time**: the moment the scan began. Nothing in a target may have been modified or changed (moved
//!   in, renamed, given a new link) at or after it: anything that was is not what the user approved.
//!
//! A plan can't be changed once made: its fields are private and it is only ever read.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use ts_rs::TS;

use crate::categorize::{CategoryId, Env, ItemRef};
use crate::fsid::{Identity, Walker};
use crate::guard::{Denied, Guard};
use crate::map::ScannedVolume;
use crate::report::{Skip, SkipKind};
use crate::rules::{ProposalAction, loose};
use crate::snapshot::{DirId, Snapshot, digest_key};

/// A plan, as the confirm dialog shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanSummary {
    /// Names the plan to [`crate::execute`]'s caller.
    pub id: String,
    /// The drive it is for.
    pub mount_point: String,
    /// Allocated bytes it deletes, permanently.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Files it deletes.
    #[ts(type = "number")]
    pub items: u64,
    /// Places it takes them from.
    #[ts(type = "number")]
    pub places: u64,
    /// Folder moves it makes, which can be undone. None until reorganizations arrive (ticket 08).
    #[ts(type = "number")]
    pub moves: u64,
    /// Its steps, one per proposal, in the order they run.
    pub steps: Vec<PlanStep>,
    /// Places the Guard refused, or found changed, while planning: left out of the numbers above, and reported
    /// as skipped when the plan runs.
    pub refused: Vec<Skip>,
    /// Advice in the selection that Vacancy can't carry out itself, by proposal id.
    pub left_for_you: Vec<String>,
}

/// One step of a plan, as the confirm dialog and the run screen list it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanStep {
    /// The proposal's id.
    pub id: String,
    pub title: String,
    pub category: CategoryId,
    #[ts(type = "number")]
    pub bytes: u64,
    #[ts(type = "number")]
    pub items: u64,
    #[ts(type = "number")]
    pub places: u64,
}

/// Why a selection couldn't become a plan.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// An id names no proposal of this scan.
    #[error("there is no proposal {0} in this scan")]
    NoSuchProposal(String),
    /// Nothing in the selection deletes anything.
    #[error("the plan is empty")]
    Empty,
}

/// What an action deletes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// A folder, with everything in it. `folders` has, for it and every folder in it the scan entered, the digest
    /// of what the scan listed there (see `snapshot::Digests`), by the folder's identity as the scan recorded it:
    /// the executor deletes from a folder only if it lists exactly that now.
    Dir { folders: HashMap<u128, u64> },
    /// One file.
    File,
    /// The files directly in a folder that the scan saw and didn't keep individually, by name and identity. They
    /// are read while planning and must then be exactly the ones the scan listed (their digest is the scan's);
    /// the executor deletes only these.
    Loose { files: Vec<(OsString, u128)> },
}

/// One place a plan deletes, with its fingerprint from the scan.
#[derive(Clone, Debug)]
pub(crate) struct Action {
    pub(crate) target: Target,
    /// Its path as the scan showed it.
    pub(crate) path: PathBuf,
    /// What it was when the scan listed it, confirmed while planning.
    pub(crate) identity: Identity,
    /// Allocated bytes and files the scan measured.
    pub(crate) bytes: u64,
    pub(crate) files: u64,
}

/// One proposal of a plan.
#[derive(Clone, Debug)]
pub(crate) struct Step {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) category: CategoryId,
    pub(crate) regenerable: bool,
    /// Its rule may take a folder that holds a macOS app bundle (Xcode's DerivedData).
    pub(crate) takes_built_apps: bool,
    pub(crate) actions: Vec<Action>,
    /// Targets refused while planning.
    pub(crate) refused: Vec<(Skip, u64)>,
}

/// An approved selection, ready to run. See the module docs.
#[derive(Clone, Debug)]
pub struct Plan {
    id: String,
    mount_point: String,
    /// The folder the scan walked: the volume's root, or the fixture's.
    volume_root: PathBuf,
    /// When the scan began.
    since: SystemTime,
    steps: Vec<Step>,
    left_for_you: Vec<String>,
}

impl Plan {
    /// Names the plan.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the confirm dialog shows.
    pub fn summary(&self) -> PlanSummary {
        let steps: Vec<PlanStep> = self
            .steps
            .iter()
            .map(|s| PlanStep {
                id: s.id.clone(),
                title: s.title.clone(),
                category: s.category,
                bytes: s.actions.iter().map(|a| a.bytes).sum(),
                items: s.actions.iter().map(|a| a.files).sum(),
                places: s.actions.len() as u64,
            })
            .collect();
        PlanSummary {
            id: self.id.clone(),
            mount_point: self.mount_point.clone(),
            bytes: steps.iter().map(|s| s.bytes).sum(),
            items: steps.iter().map(|s| s.items).sum(),
            places: steps.iter().map(|s| s.places).sum(),
            moves: 0,
            steps,
            refused: self
                .steps
                .iter()
                .flat_map(|s| s.refused.iter().map(|(skip, _)| skip.clone()))
                .collect(),
            left_for_you: self.left_for_you.clone(),
        }
    }

    pub(crate) fn mount_point(&self) -> &str {
        &self.mount_point
    }

    pub(crate) fn volume_root(&self) -> &std::path::Path {
        &self.volume_root
    }

    pub(crate) fn since(&self) -> SystemTime {
        self.since
    }

    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub(crate) fn left_for_you(&self) -> &[String] {
        &self.left_for_you
    }
}

/// Makes the plan for the proposals `ids` of the drive `scanned`, whose scan used `env`. Advice and tools in the
/// selection are kept aside as left for the user (tools arrive in ticket 06). Every target passes the Guard, built
/// from `env` now, and must still be what the scan saw; see the module docs.
pub fn plan(scanned: &ScannedVolume, env: &Env, ids: &[String]) -> Result<Plan, PlanError> {
    let snap = &scanned.snapshot;
    let proposals = &scanned.proposals;
    let volume_root = snap
        .roots()
        .first()
        .map(|r| r.shown.clone())
        .unwrap_or_else(|| env.root.clone());
    let guard = Guard::new(env, &env.root);
    let mut walker = Walker::default();
    let mut chosen: Vec<usize> = Vec::new();
    for id in ids {
        let i = proposals
            .position(id)
            .ok_or_else(|| PlanError::NoSuchProposal(id.clone()))?;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    // In the order the tray lists them: biggest first.
    chosen.sort_unstable();
    let mut steps = Vec::new();
    let mut left_for_you = Vec::new();
    for i in chosen {
        let p = &proposals.list()[i];
        if p.action != ProposalAction::Delete {
            left_for_you.push(p.id.clone());
            continue;
        }
        let mut step = Step {
            id: p.id.clone(),
            title: p.title.clone(),
            category: p.category,
            regenerable: p.regenerable,
            takes_built_apps: proposals.takes_built_apps(i),
            actions: Vec::new(),
            refused: Vec::new(),
        };
        for &at in proposals.targets(i) {
            let (path, scanned_id, bytes, files) = match at {
                ItemRef::Dir(d) => {
                    let t = snap.tree(d);
                    (snap.path(d), snap.dir_identity(d), t.bytes, t.files)
                }
                ItemRef::File(f) => (
                    snap.file_path(f),
                    snap.file_full_identity(f),
                    snap.file(f).bytes,
                    1,
                ),
                ItemRef::FilesIn(d) => {
                    let (bytes, files) = loose(snap, d);
                    (snap.path(d), snap.dir_identity(d), bytes, files)
                }
                ItemRef::Proposal(_) => continue,
            };
            let refuse = |kind: SkipKind, reason: &str| {
                (
                    Skip {
                        path: path.display().to_string(),
                        reason: reason.to_owned(),
                        kind,
                        bytes,
                        items: files,
                    },
                    files,
                )
            };
            let contents = matches!(at, ItemRef::FilesIn(_));
            let chain = match guard.check_with(&mut walker, &path, contents) {
                Ok(chain) => chain,
                Err(denied) => {
                    let (kind, reason) = denied_reason(&denied);
                    step.refused.push(refuse(kind, &reason));
                    continue;
                }
            };
            let Some(now) = chain.last().map(|s| s.id) else {
                continue;
            };
            match scanned_id {
                Some(id) if now == id => {}
                Some(id) if now.volume() != id.volume() => {
                    step.refused.push(refuse(
                        SkipKind::Changed,
                        "It is on another drive than the one scanned",
                    ));
                    continue;
                }
                Some(_) => {
                    step.refused
                        .push(refuse(SkipKind::Changed, "It was replaced since the scan"));
                    continue;
                }
                None => {
                    step.refused.push(refuse(SkipKind::Refused, UNCONFIRMABLE));
                    continue;
                }
            }
            let target = match at {
                ItemRef::Dir(d) => match folder_digests(snap, d) {
                    Some(folders) => Target::Dir { folders },
                    None => {
                        step.refused.push(refuse(SkipKind::Refused, UNCONFIRMABLE));
                        continue;
                    }
                },
                ItemRef::FilesIn(d) => match loose_now(snap, d, &path, now) {
                    Ok(files) => Target::Loose { files },
                    Err((kind, reason)) => {
                        step.refused.push(refuse(kind, &reason));
                        continue;
                    }
                },
                _ => Target::File,
            };
            step.actions.push(Action {
                target,
                path,
                identity: now,
                bytes,
                files,
            });
        }
        steps.push(step);
    }
    if steps
        .iter()
        .all(|s| s.actions.is_empty() && s.refused.is_empty())
    {
        return Err(PlanError::Empty);
    }
    Ok(Plan {
        id: next_id("plan"),
        mount_point: scanned.overview.volume.mount_point.clone(),
        volume_root,
        since: snap.started(),
        steps,
        left_for_you,
    })
}

/// Why a target the scan has no identities for is refused.
const UNCONFIRMABLE: &str = "This drive's file system can't confirm what is there";

/// The digests of what the scan listed in the folder `dir` and in every folder in it the scan entered, by each
/// folder's identity as the scan recorded it. `None` if one has no identity.
fn folder_digests(snap: &Snapshot, dir: DirId) -> Option<HashMap<u128, u64>> {
    let mut out = HashMap::new();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        out.insert(snap.identity(d)?, snap.digests(d).all);
        stack.extend(snap.children(d).filter(|&c| snap.flags(c).entered()));
    }
    Some(out)
}

/// The loose files of the folder `dir`, at `path`, read now: the files it lists that the scan didn't keep
/// individually, by name and identity. They must be exactly the ones the scan listed: their digest is the scan's.
/// A file the scan never saw can't be told by its times on Windows (a copy keeps the original's), only by its
/// identity, which this compares.
fn loose_now(
    snap: &Snapshot,
    dir: DirId,
    path: &Path,
    folder: Identity,
) -> Result<Vec<(OsString, u128)>, (SkipKind, String)> {
    let unconfirmable = || (SkipKind::Refused, UNCONFIRMABLE.to_owned());
    let mut expected = snap.digests(dir).files;
    let mut kept = Vec::new();
    for f in snap.files(dir) {
        let id = snap.file_identity(f).ok_or_else(unconfirmable)?;
        expected = expected.wrapping_sub(digest_key(id));
        kept.push(id);
    }
    let (chain, entries) = crate::exec::files_in(path).map_err(|f| {
        let said = f.said();
        (said.kind, format!("It {}", said.one))
    })?;
    if chain.last().map(|s| s.id) != Some(folder) {
        return Err((SkipKind::Changed, "It was replaced since the scan".into()));
    }
    let files: Vec<(OsString, u128)> = entries
        .into_iter()
        .filter(|e| e.counts && !kept.contains(&e.key()))
        .map(|e| {
            let key = e.key();
            (e.name, key)
        })
        .collect();
    let digest = files
        .iter()
        .fold(0u64, |sum, (_, key)| sum.wrapping_add(digest_key(*key)));
    if digest != expected {
        return Err((
            SkipKind::Changed,
            "Its files changed since the scan: they aren't the ones the scan saw".into(),
        ));
    }
    Ok(files)
}

/// What the run reports for a place the Guard refused.
pub(crate) fn denied_reason(denied: &Denied) -> (SkipKind, String) {
    match denied {
        Denied::Unclear => (
            SkipKind::Link,
            "It is no longer there, or the way to it now goes through a link".into(),
        ),
        Denied::OutsideAllowedRoots => (
            SkipKind::Outside,
            "It is outside the places Vacancy may clean".into(),
        ),
        Denied::Protected(what) => (
            SkipKind::Protected,
            format!("It is or holds {what}, which Vacancy never touches"),
        ),
    }
}

/// A new id: `{prefix}-{milliseconds since the epoch}-{a counter}`, unique within this process and ordered by time.
pub(crate) fn next_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{ms}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::fixture;
    use crate::map::scan_for_map_with;
    use crate::rules::Moment;
    use crate::scan::CancelToken;

    /// A fixture profile and its scan, in fixture mode.
    pub(crate) fn scanned_fixture() -> (tempfile::TempDir, PathBuf, Env, ScannedVolume) {
        let dir = tempfile::tempdir().unwrap();
        let root = if cfg!(windows) {
            dir.path().to_path_buf()
        } else {
            dir.path().canonicalize().unwrap()
        }
        .join("fixture");
        fixture::build(&root).unwrap();
        let env = fixture::env(&root);
        let moment = Moment {
            now: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            running_apps: Some(Vec::new()),
            installed_apps: Vec::new(),
        };
        let figures_root = root.clone();
        let scanned = scan_for_map_with(
            &root,
            &env,
            &moment,
            &move |snap| Some(fixture::volume(&figures_root, 1 << 30, snap)),
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap();
        (dir, root, env, scanned)
    }

    fn all_ids(scanned: &ScannedVolume) -> Vec<String> {
        scanned
            .proposals
            .list()
            .iter()
            .map(|p| p.id.clone())
            .collect()
    }

    /// The scan's targets are the plan's, with their identities from the scan.
    #[test]
    fn a_plan_holds_every_target_the_scan_saw() {
        let (_dir, _root, env, scanned) = scanned_fixture();
        let summary = plan(&scanned, &env, &all_ids(&scanned)).unwrap().summary();
        assert!(summary.refused.is_empty(), "{:#?}", summary.refused);
        assert!(summary.places > 5);
    }

    /// The same file ids on another volume are other files: a plan made from a scan of one disk refuses everything
    /// when another disk is at the same letter. (On Unix the device is part of every identity the scan records.)
    #[cfg(windows)]
    #[test]
    fn another_volume_at_the_same_place_matches_nothing() {
        let (_dir, _root, env, mut scanned) = scanned_fixture();
        let serial = scanned.snapshot.roots()[0]
            .volume
            .expect("NTFS has a serial");
        scanned.snapshot.set_volume_for_test(Some(serial ^ 0x5A5A));
        let summary = plan(&scanned, &env, &all_ids(&scanned)).unwrap().summary();
        assert_eq!(summary.places, 0, "nothing on another volume runs");
        assert!(!summary.refused.is_empty());
        assert!(
            summary
                .refused
                .iter()
                .all(|k| k.reason == "It is on another drive than the one scanned"),
            "{:#?}",
            summary.refused
        );
    }
}

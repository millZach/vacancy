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
use crate::map::{Overview, ScannedVolume};
use crate::report::{LeftForYou, Skip, SkipKind};
use crate::rules::{ProposalAction, loose};
use crate::snapshot::{DirId, Snapshot, file_digest_key};

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
    /// The duplicate files in it, if their proposal is: how many groups lose their extra copies and how many the
    /// user left out. One copy of each group stays.
    #[ts(optional = nullable)]
    pub duplicates: Option<PlannedDuplicates>,
}

/// The duplicate files in a plan, for the confirm dialog: one copy of each group stays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlannedDuplicates {
    /// Groups whose extra copies the plan deletes.
    #[ts(type = "number")]
    pub groups: u64,
    /// Extra copies it deletes.
    #[ts(type = "number")]
    pub copies: u64,
    /// What they take up.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Groups the user unticked: every copy of them stays.
    #[ts(type = "number")]
    pub skipped_groups: u64,
    /// What it deletes from each of the duplicates' map tiles.
    pub tiles: Vec<PlannedTile>,
}

/// What a plan deletes from one map tile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlannedTile {
    /// The tile's id on the map.
    pub id: String,
    #[ts(type = "number")]
    pub bytes: u64,
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
    /// No cleanup or advice was selected.
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
    Loose { files: Vec<LooseFile> },
}

/// One of a folder's loose files, as the planner read it: its name, identity and sizes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LooseFile {
    pub(crate) name: OsString,
    pub(crate) key: u128,
    pub(crate) logical: u64,
    pub(crate) alloc: u64,
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
    /// For a file, its length when the scan listed it.
    pub(crate) logical: u64,
    /// For an extra copy of a duplicate group, the copy that stays: checked before this one goes.
    pub(crate) keep: Option<KeptCopy>,
}

/// The copy of a duplicate group that stays, as the scan and the duplicate finder saw it. Before an extra copy
/// goes, the executor finds it again: the same file, at the same sizes, unchanged since the scan.
#[derive(Clone, Debug)]
pub(crate) struct KeptCopy {
    pub(crate) path: PathBuf,
    pub(crate) identity: Identity,
    pub(crate) logical: u64,
    pub(crate) alloc: u64,
    /// Its group, by index into the step's [`StepDuplicates::groups`].
    pub(crate) group: u32,
    /// The map tile the extra copy is drawn in, by index into [`StepDuplicates::tiles`].
    pub(crate) tile: u32,
}

/// The duplicates step's groups and tiles.
#[derive(Clone, Debug, Default)]
pub(crate) struct StepDuplicates {
    /// Groups with at least one extra copy in the plan.
    pub(crate) groups: Vec<PlannedGroup>,
    /// The duplicates' map tiles, by id.
    pub(crate) tiles: Vec<String>,
    /// Groups the user unticked.
    pub(crate) skipped_groups: u64,
}

/// A duplicate group in a plan, for the report's sample pairs.
#[derive(Clone, Debug)]
pub(crate) struct PlannedGroup {
    pub(crate) kind: crate::duplicates::FileKind,
    pub(crate) name: String,
    pub(crate) kept: String,
    pub(crate) removed: String,
}

/// One proposal of a plan.
#[derive(Clone, Debug)]
pub(crate) struct Step {
    pub(crate) id: String,
    pub(crate) title: String,
    /// Why it is safe to remove, from the proposal.
    pub(crate) why: String,
    pub(crate) category: CategoryId,
    pub(crate) regenerable: bool,
    /// Its rule may take a folder that holds a macOS app bundle (Xcode's DerivedData).
    pub(crate) takes_built_apps: bool,
    pub(crate) actions: Vec<Action>,
    /// Targets refused while planning.
    pub(crate) refused: Vec<(Skip, u64)>,
    /// For the duplicates step: its groups and tiles.
    pub(crate) duplicates: Option<StepDuplicates>,
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
    left_for_you: Vec<LeftForYou>,
    /// The map of the scan it was made from, for the run's report.
    map: Overview,
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
            left_for_you: self.left_for_you.iter().map(|l| l.id.clone()).collect(),
            duplicates: self.steps.iter().find_map(planned_duplicates),
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

    pub(crate) fn left_for_you(&self) -> &[LeftForYou] {
        &self.left_for_you
    }

    pub(crate) fn map(&self) -> &Overview {
        &self.map
    }
}

/// What the confirm dialog says of a plan's duplicates step.
fn planned_duplicates(step: &Step) -> Option<PlannedDuplicates> {
    let d = step.duplicates.as_ref()?;
    let mut groups = std::collections::HashSet::new();
    let mut tiles = vec![0u64; d.tiles.len()];
    for a in &step.actions {
        if let Some(k) = &a.keep {
            groups.insert(k.group);
            if let Some(t) = tiles.get_mut(k.tile as usize) {
                *t += a.bytes;
            }
        }
    }
    Some(PlannedDuplicates {
        groups: groups.len() as u64,
        copies: step.actions.len() as u64,
        bytes: step.actions.iter().map(|a| a.bytes).sum(),
        skipped_groups: d.skipped_groups,
        tiles: d
            .tiles
            .iter()
            .zip(tiles)
            .filter(|(_, bytes)| *bytes > 0)
            .map(|(id, bytes)| PlannedTile {
                id: id.clone(),
                bytes,
            })
            .collect(),
    })
}

/// Makes the plan for the proposals `ids` of the drive `scanned`, whose scan used `env`. Advice and tools in the
/// selection are kept aside as left for the user (tools arrive in ticket 06). Every target passes the Guard, built
/// from `env` now, and must still be what the scan saw; see the module docs.
pub fn plan(scanned: &ScannedVolume, env: &Env, ids: &[String]) -> Result<Plan, PlanError> {
    plan_with(scanned, env, ids, &[])
}

/// [`plan`], leaving out the duplicate groups `skipped_groups` (by group id) the user unticked: every copy of them
/// stays. Each extra copy of the other groups carries the copy that stays, which the executor checks again before
/// the extra copy goes.
pub fn plan_with(
    scanned: &ScannedVolume,
    env: &Env,
    ids: &[String],
    skipped_groups: &[String],
) -> Result<Plan, PlanError> {
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
            left_for_you.push(LeftForYou {
                id: p.id.clone(),
                title: p.title.clone(),
                summary: p.summary.clone(),
                why: p.why.clone(),
                bytes: p.bytes,
                hint_action: p.hint_action.clone(),
            });
            continue;
        }
        let mut step = Step {
            id: p.id.clone(),
            title: p.title.clone(),
            why: p.why.clone(),
            category: p.category,
            regenerable: p.regenerable,
            takes_built_apps: proposals.takes_built_apps(i),
            actions: Vec::new(),
            refused: Vec::new(),
            duplicates: None,
        };
        let dups = (p.id == crate::duplicates::ID)
            .then(|| proposals.duplicates())
            .flatten();
        let mut local: HashMap<usize, u32> = HashMap::new();
        if let Some(d) = dups {
            step.duplicates = Some(StepDuplicates {
                groups: Vec::new(),
                tiles: (0..d.tiles.len()).map(|t| d.tile_id(t)).collect(),
                skipped_groups: d
                    .groups
                    .iter()
                    .filter(|g| skipped_groups.contains(&g.id))
                    .count() as u64,
            });
        }
        for &at in proposals.targets(i) {
            let logical = match at {
                ItemRef::File(f) => snap.file_logical(f),
                _ => 0,
            };
            // An extra copy of a duplicate group: left out if the user unticked its group, and otherwise carrying
            // the copy that stays.
            let mut keep = None;
            if let (Some(d), ItemRef::File(f)) = (dups, at)
                && let Some((gi, g)) = d.group_of(f)
            {
                if skipped_groups.contains(&g.id) {
                    continue;
                }
                let Some(identity) = snap.file_full_identity(g.keep) else {
                    continue;
                };
                let tile = g
                    .extras
                    .iter()
                    .position(|&e| e == f)
                    .and_then(|n| g.extra_tiles.get(n))
                    .copied()
                    .unwrap_or(0);
                let planned = step
                    .duplicates
                    .as_mut()
                    .expect("the duplicates step has them");
                let next = planned.groups.len() as u32;
                let group = *local.entry(gi).or_insert_with(|| {
                    planned.groups.push(PlannedGroup {
                        kind: g.kind,
                        name: snap.file(g.keep).name.to_string_lossy().into_owned(),
                        kept: snap.file_path(g.keep).display().to_string(),
                        removed: snap.file_path(f).display().to_string(),
                    });
                    next
                });
                keep = Some(KeptCopy {
                    path: snap.file_path(g.keep),
                    identity,
                    logical: snap.file_logical(g.keep),
                    alloc: snap.file(g.keep).bytes,
                    group,
                    tile: tile as u32,
                });
            }
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
                logical,
                keep,
            });
        }
        // Duplicates with every group unticked delete nothing: no step.
        if step.duplicates.is_some() && step.actions.is_empty() && step.refused.is_empty() {
            continue;
        }
        steps.push(step);
    }
    if left_for_you.is_empty()
        && steps
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
        map: scanned.overview.clone(),
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
) -> Result<Vec<LooseFile>, (SkipKind, String)> {
    let unconfirmable = || (SkipKind::Refused, UNCONFIRMABLE.to_owned());
    let mut expected = snap.digests(dir).files;
    let mut kept = Vec::new();
    for f in snap.files(dir) {
        let id = snap.file_identity(f).ok_or_else(unconfirmable)?;
        expected = expected.wrapping_sub(file_digest_key(
            id,
            snap.file_logical(f),
            snap.file(f).bytes,
        ));
        kept.push(id);
    }
    let (chain, entries) = crate::exec::files_in(path).map_err(|f| {
        let said = f.said();
        (said.kind, format!("It {}", said.one))
    })?;
    if chain.last().map(|s| s.id) != Some(folder) {
        return Err((SkipKind::Changed, "It was replaced since the scan".into()));
    }
    let mut digest = 0u64;
    let files: Vec<LooseFile> = entries
        .into_iter()
        .filter(|e| e.counts && !kept.contains(&e.key()))
        .map(|e| {
            digest = digest.wrapping_add(e.digest());
            LooseFile {
                key: e.key(),
                logical: e.logical,
                alloc: e.alloc,
                name: e.name,
            }
        })
        .collect();
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

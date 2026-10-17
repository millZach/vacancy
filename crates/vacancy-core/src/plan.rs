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

use std::ffi::OsString;
use std::path::PathBuf;
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
    /// A folder, with everything in it.
    Dir,
    /// One file.
    File,
    /// The files directly in a folder, except `kept`: the files the scan kept individually there, which are
    /// targets of their own (or of nobody), never loose files.
    Loose { kept: Vec<OsString> },
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
            let (target, path, scanned_id, bytes, files) = match at {
                ItemRef::Dir(d) => {
                    let t = snap.tree(d);
                    (
                        Target::Dir,
                        snap.path(d),
                        snap.identity(d),
                        t.bytes,
                        t.files,
                    )
                }
                ItemRef::File(f) => (
                    Target::File,
                    snap.file_path(f),
                    snap.file_identity(f),
                    snap.file(f).bytes,
                    1,
                ),
                ItemRef::FilesIn(d) => {
                    let (bytes, files) = loose(snap, d);
                    let kept = snap
                        .files(d)
                        .map(|f| snap.file(f).name.to_owned())
                        .collect();
                    (
                        Target::Loose { kept },
                        snap.path(d),
                        snap.identity(d),
                        bytes,
                        files,
                    )
                }
                ItemRef::Proposal(_) => continue,
            };
            let contents = matches!(target, Target::Loose { .. });
            let refuse = |kind: SkipKind, reason: String| {
                (
                    Skip {
                        path: path.display().to_string(),
                        reason,
                        kind,
                        bytes,
                        items: files,
                    },
                    files,
                )
            };
            let chain = match guard.check_with(&mut walker, &path, contents) {
                Ok(chain) => chain,
                Err(denied) => {
                    let (kind, reason) = denied_reason(&denied);
                    step.refused.push(refuse(kind, reason));
                    continue;
                }
            };
            let Some(now) = chain.last().map(|s| s.id) else {
                continue;
            };
            match scanned_id {
                Some(id) if now.matches_scan(id) => {}
                Some(_) => {
                    step.refused.push(refuse(
                        SkipKind::Changed,
                        "It was replaced since the scan".into(),
                    ));
                    continue;
                }
                None => {
                    step.refused.push(refuse(
                        SkipKind::Refused,
                        "This drive's file system can't confirm what is there".into(),
                    ));
                    continue;
                }
            }
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

//! The executor: carries out a [`Plan`] and reports exactly what it did. Deletion is permanent, so every step here
//! fails closed: whatever can't be shown to be what the user approved is left in place, with the reason.
//!
//! For each action of the plan, right before acting on it:
//!
//! 1. **A fresh walk.** The target's chain is walked from the volume root, each folder opened by name inside the
//!    one before without following a link, and kept open. Everything after happens relative to those open folders:
//!    no path is resolved again between the checks and the delete.
//! 2. **The Guard, fresh.** The Guard judges that very chain (ADR 0002). Its places are resolved afresh for every
//!    step, from the [`Env`] the caller detected for this run.
//! 3. **The fingerprint.** The target must have the identity the scan recorded, and nothing in it may have been
//!    modified or changed since the scan began (see [`crate::plan`]). A folder is checked whole first, every entry
//!    of every folder in it: each folder the one its parent listed (so nothing was swapped in), on the target's
//!    volume and mount, nothing protected inside, and at least the bytes the scan measured (on macOS, exactly them,
//!    allowing for hard links). Any mismatch skips the whole target.
//! 4. **The delete**, relative to the open folders, depth first. A link inside a target (pnpm's `node_modules` are
//!    full of them) is deleted as a link, never followed. Each file is checked again right before it goes (same
//!    identity as listed, unchanged since the scan) and deleted: on Windows through a handle opened exclusively
//!    without following it (POSIX semantics, read-only ignored), on macOS by name in its verified folder right
//!    after `fstatat`, the tightest macOS allows. Files in use are skipped (a sharing violation on Windows; on
//!    macOS, a process holding it open), each with a reason, and the action becomes partial.
//!
//! Cancelling stops after the file being deleted. Freed bytes are the allocated bytes of the files deleted whose
//! last name they were, so hard links and skips are counted honestly; the totals are set beside the drive's own
//! change in free space.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use ts_rs::TS;

use crate::categorize::Env;
use crate::fsid::{self, Identity, Step};
use crate::guard::Guard;
use crate::plan::{Action, LooseFile, Plan, Step as PlanStep, Target, denied_reason, next_id};
use crate::report::{
    CategoryFreed, Outcome, Reorganized, RunReport, Skip, SkipKind, StepKind, StepReport,
};
use crate::scan::CancelToken;
use crate::snapshot::{digest_key, file_digest_key};
use crate::text::group_digits;
use crate::tidy::{Approved, Journals};
use crate::volume::VolumeInfo;

#[cfg(unix)]
#[path = "exec/unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "exec/windows.rs"]
pub(crate) mod sys;
#[cfg(not(any(unix, windows)))]
#[path = "exec/unsupported.rs"]
mod sys;

/// Something that happened in a running plan, as the run screen shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum RunEvent {
    /// A step began.
    StepStarted(StepStarted),
    /// How far the running step has got. About ten a second, and whenever a place is done.
    Progress(RunProgress),
    /// A place was deleted.
    Deleted(Deleted),
    /// Something was left in place.
    Skipped(Skipped),
    /// A step ended.
    StepFinished(StepFinished),
    /// How far a move step (a Tidy up sort) has got: after each of its moves.
    MoveProgress(MoveProgress),
}

/// See [`RunEvent::MoveProgress`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MoveProgress {
    pub step: u32,
    /// Moves handled so far (moved, or skipped with a reason).
    #[ts(type = "number")]
    pub done: u64,
    /// Moves the step makes.
    #[ts(type = "number")]
    pub total: u64,
}

/// See [`RunEvent::StepStarted`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepStarted {
    /// The step's index in the plan.
    pub step: u32,
    /// The proposal's id.
    pub id: String,
}

/// See [`RunEvent::Progress`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunProgress {
    pub step: u32,
    /// Freed by this step so far.
    #[ts(type = "number")]
    pub freed_bytes: u64,
    /// Files this step has deleted so far.
    #[ts(type = "number")]
    pub deleted_items: u64,
    /// Freed by the whole run so far.
    #[ts(type = "number")]
    pub total_freed_bytes: u64,
    /// What is being deleted now.
    pub current_path: String,
}

/// See [`RunEvent::Deleted`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Deleted {
    pub step: u32,
    /// The planned place.
    pub path: String,
    /// What deleting it freed.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Files deleted from it.
    #[ts(type = "number")]
    pub items: u64,
    /// The place itself went: a folder with everything in it, or a file. False when only files inside it went:
    /// a folder's loose files (the folder stays), or a folder that kept something left in place.
    pub whole: bool,
}

/// See [`RunEvent::Skipped`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Skipped {
    pub step: u32,
    pub skip: Skip,
}

/// See [`RunEvent::StepFinished`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepFinished {
    pub step: u32,
    pub report: StepReport,
}

/// Runs `plan` and reports what it did. See the module docs.
///
/// `env` is where things are on this computer now (detect it afresh for the run); the Guard is built from it for
/// every step. `figures` reads the drive's own numbers, once before the run and once after. `on_event` gets the run
/// as it goes; `cancel` stops it after the file being deleted.
pub fn execute(
    plan: &Plan,
    env: &Env,
    figures: &dyn Fn() -> Option<VolumeInfo>,
    on_event: &mut dyn FnMut(RunEvent),
    cancel: &CancelToken,
) -> RunReport {
    execute_inner(
        plan,
        env,
        Sorts::default(),
        figures,
        on_event,
        cancel,
        &Hooks::default(),
    )
}

/// [`execute`], for a plan that may hold Tidy up sorts: they run after the deletes, each with a journal of its own
/// in `journals`. Right before the first sort, `checkpoint` gets the report so far (the sorts marked interrupted),
/// for the caller to keep: if the app closes while files move, the history still finds their journals. If it can't
/// keep it (it gives why), no sort begins: each is reported skipped with that reason, since a journal nothing leads
/// to could never be undone from the app. A plan with sorts and no `journals` moves nothing and says why.
pub fn execute_with(
    plan: &Plan,
    env: &Env,
    journals: Option<&Journals>,
    checkpoint: &mut dyn FnMut(&RunReport) -> Result<(), String>,
    figures: &dyn Fn() -> Option<VolumeInfo>,
    on_event: &mut dyn FnMut(RunEvent),
    cancel: &CancelToken,
) -> RunReport {
    let sorts = Sorts {
        journals,
        checkpoint: Some(checkpoint),
    };
    execute_inner(
        plan,
        env,
        sorts,
        figures,
        on_event,
        cancel,
        &Hooks::default(),
    )
}

/// Where a run's sorts keep their journals, and who keeps the report as the sorts begin.
#[derive(Default)]
pub(crate) struct Sorts<'s> {
    journals: Option<&'s Journals>,
    checkpoint: Option<&'s mut Checkpoint<'s>>,
}

/// Keeps a run's report before its sorts begin; gives why it couldn't.
pub type Checkpoint<'a> = dyn FnMut(&RunReport) -> Result<(), String> + 'a;

/// The folder at `path` and what it lists, read now through a fresh walk that follows no link: its chain and its
/// entries. For the planner, which reads a folder's loose files by identity.
pub(crate) fn files_in(path: &Path) -> Result<(Vec<Step>, Vec<Entry>), Fail> {
    let name = path
        .file_name()
        .ok_or_else(|| Fail::Failed("the path has no name".into()))?;
    let parent = sys::walk_to_parent(path, false)?;
    let (dir, step) = sys::open_dir(&parent.dir, name, None, Open::Look)?;
    let entries = sys::list(&dir)?;
    let mut chain = parent.steps.clone();
    chain.push(step);
    Ok((chain, entries))
}

/// Points where the tests change the tree mid-run, to check that a change can't redirect a delete.
#[derive(Default)]
pub(crate) struct Hooks<'h> {
    /// A target passed every check; nothing in it is deleted yet.
    pub(crate) before_delete: Option<&'h dyn Fn(&Path)>,
    /// Just before a file inside a target is deleted.
    pub(crate) before_file: Option<&'h dyn Fn(&Path)>,
}

/// What an entry of a folder is, as its listing says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A file (on Windows also one compressed by the Windows Overlay Filter, or deduplicated).
    File,
    /// A folder, not a link.
    Dir,
    /// A symlink, junction, mount point or other reparse point that names somewhere else: deleted as itself,
    /// never followed.
    Link,
    /// Something Vacancy leaves alone, and what it is: "a socket", "a cloud placeholder".
    Special(&'static str),
}

/// One entry of a folder's listing.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    /// Its name, exactly as listed.
    pub(crate) name: OsString,
    pub(crate) kind: Kind,
    /// Windows: the 128-bit file id. Unix: the inode.
    pub(crate) id: u128,
    /// Unix: the device. Windows: 0 (the folder's volume).
    pub(crate) dev: u64,
    /// Allocated bytes, measured as the scanner measures them.
    pub(crate) alloc: u64,
    /// Length in bytes (files).
    pub(crate) logical: u64,
    /// The later of when it was last modified and when it last changed (renamed, moved, given a new link, its
    /// attributes set), in nanoseconds since the Unix epoch.
    pub(crate) changed: i128,
    /// Its hard links, when the listing says (Unix).
    pub(crate) links: Option<u64>,
    /// The scanner counts it as a file (so a target's size includes it): regular files, and on Windows every
    /// entry that isn't a folder or a link (a socket or a cloud placeholder too).
    pub(crate) counts: bool,
    /// A Windows mount-point reparse point: a junction, or a folder where a volume is mounted, which
    /// `sys::mounts_volume` tells apart.
    pub(crate) junction: bool,
}

impl Entry {
    /// What identifies it, as the scanner records identities (see `snapshot::digest_key`): Windows' file id,
    /// Unix's device and inode.
    pub(crate) fn key(&self) -> u128 {
        if cfg!(windows) {
            self.id
        } else {
            (u128::from(self.dev) << 64) | self.id
        }
    }

    /// Its part of its folder's digest, as the scanner adds it up: a file's with its sizes.
    pub(crate) fn digest(&self) -> u64 {
        if self.counts {
            file_digest_key(self.key(), self.logical, self.alloc)
        } else {
            digest_key(self.key())
        }
    }

    /// The entry as a step of a chain, in a folder on the volume `volume`.
    pub(crate) fn step(&self, volume: u64) -> Step {
        Step {
            name: self.name.clone(),
            id: Identity::new(if self.dev != 0 { self.dev } else { volume }, self.id),
            mount: None,
        }
    }
}

/// Why a platform call didn't do what was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    /// It isn't there (any more).
    Gone,
    /// It, or something on the way to it, is a link.
    Link,
    /// An app has it open; the app's name when known.
    InUse(Option<String>),
    /// It isn't what was listed or planned: another file or folder, or modified since.
    Changed,
    /// It is on another volume, or another volume is mounted there.
    OtherVolume,
    /// The OS refused: no permission, or it won't let go of it.
    Refused,
    /// It is locked (macOS's locked flag).
    #[cfg_attr(
        windows,
        allow(dead_code, reason = "Windows has no locked flag; read-only is ignored")
    )]
    Locked,
    /// A folder that still holds something.
    NotEmpty,
    /// The file system can't say what a thing is (no file ids): nothing is deleted there.
    #[cfg_attr(unix, allow(dead_code, reason = "every Unix file system has inodes"))]
    Unsupported,
    /// Anything else, with what the OS said.
    Failed(String),
}

/// How a reason is said of one thing and of several ("is in use", "are in use").
pub(crate) struct Said {
    pub(crate) kind: SkipKind,
    pub(crate) one: String,
    many: String,
}

impl Fail {
    pub(crate) fn said(&self) -> Said {
        let (kind, one, many): (SkipKind, String, String) = match self {
            Fail::Gone => (
                SkipKind::Gone,
                "is no longer there".into(),
                "are no longer there".into(),
            ),
            Fail::Link => (
                SkipKind::Link,
                "became a link since the scan".into(),
                "became links since the scan".into(),
            ),
            Fail::InUse(None) => (SkipKind::InUse, "is in use".into(), "are in use".into()),
            Fail::InUse(Some(app)) => (
                SkipKind::InUse,
                format!("is open in {app}"),
                format!("are open in {app}"),
            ),
            Fail::Changed => (
                SkipKind::Changed,
                "changed since the scan".into(),
                "changed since the scan".into(),
            ),
            Fail::OtherVolume => (
                SkipKind::OtherVolume,
                "is on another volume".into(),
                "are on another volume".into(),
            ),
            Fail::Refused => (
                SkipKind::Refused,
                format!("was refused by {OS}"),
                format!("were refused by {OS}"),
            ),
            Fail::Locked => (SkipKind::Refused, "is locked".into(), "are locked".into()),
            Fail::NotEmpty => (
                SkipKind::Failed,
                "still holds files".into(),
                "still hold files".into(),
            ),
            Fail::Unsupported => (
                SkipKind::Refused,
                "is on a file system that can't confirm what is there".into(),
                "are on a file system that can't confirm what is there".into(),
            ),
            Fail::Failed(why) => (
                SkipKind::Failed,
                format!("couldn't be deleted ({why})"),
                format!("couldn't be deleted ({why})"),
            ),
        };
        Said { kind, one, many }
    }
}

impl Said {
    fn changed() -> Self {
        Fail::Changed.said()
    }

    fn special(what: &str) -> Self {
        Said {
            kind: SkipKind::Special,
            one: format!("is {what}, which Vacancy leaves alone"),
            many: "are special files (sockets, devices, placeholders), which Vacancy leaves alone"
                .into(),
        }
    }

    fn hard_linked() -> Self {
        Said {
            kind: SkipKind::HardLinked,
            one: "is also kept elsewhere under another name (a hard link), so its space is still in use".into(),
            many: "are also kept elsewhere under other names (hard links), so their space is still in use".into(),
        }
    }

    fn moved() -> Self {
        Said {
            kind: SkipKind::Changed,
            one: "was moved during the run, so it was left where it went".into(),
            many: "were left: their folder was moved during the run".into(),
        }
    }

    fn held_open(app: Option<String>) -> Self {
        let app = app.unwrap_or_else(|| "an app".into());
        Said {
            kind: SkipKind::HeldOpen,
            one: format!(
                "was deleted while {app} had it open: its space comes back when {app} lets go of it"
            ),
            many: format!(
                "were deleted while {app} had them open: their space comes back when {app} lets go"
            ),
        }
    }

    fn cancelled() -> Self {
        Said {
            kind: SkipKind::Cancelled,
            one: "was left: you stopped the run".into(),
            many: "were left: you stopped the run".into(),
        }
    }
}

/// The OS, as the reasons name it.
const OS: &str = if cfg!(windows) { "Windows" } else { "macOS" };

/// How far back from the scan's start a change still counts as since the scan: the coarsest timestamps Vacancy
/// may meet (FAT's two seconds; HFS+ keeps whole seconds) must never let a change after the scan look older.
const SINCE_MARGIN: Duration = Duration::from_secs(2);

/// How often progress is reported.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Detailed skips a step reports per kind; the rest are summed into one.
const SKIPS_PER_KIND: usize = 12;

pub(crate) fn execute_inner(
    plan: &Plan,
    env: &Env,
    mut sorts: Sorts<'_>,
    figures: &dyn Fn() -> Option<VolumeInfo>,
    on_event: &mut dyn FnMut(RunEvent),
    cancel: &CancelToken,
    hooks: &Hooks<'_>,
) -> RunReport {
    let started = Instant::now();
    let started_at = SystemTime::now();
    let id = next_id("run");
    let before = figures();
    let mut run = Run {
        on_event,
        cancel,
        hooks,
        threshold: threshold(plan.since()),
        total_freed: 0,
        last_progress: Instant::now(),
    };
    let mut steps = Vec::new();
    for (i, step) in plan.steps().iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        (run.on_event)(RunEvent::StepStarted(StepStarted {
            step: index,
            id: step.id.clone(),
        }));
        let report = run.step(plan, env, index, step);
        (run.on_event)(RunEvent::StepFinished(StepFinished {
            step: index,
            report: report.clone(),
        }));
        steps.push(report);
    }
    let mut reorganized = Vec::new();
    // Why no sort may begin: the checkpoint report couldn't be kept.
    let mut blocked = None;
    if !plan.tidy().is_empty() {
        // Kept before any file moves: a run the app never finishes still leads the history to its journals.
        let pending: Vec<Reorganized> = plan
            .tidy()
            .iter()
            .map(|t| Reorganized {
                id: t.id.clone(),
                folder: t.name.clone(),
                moved_items: 0,
                planned_items: t.planned(),
                journal: crate::tidy::journal_for(&id, &t.id),
                path: t.path.clone(),
                into: t.into.clone(),
                folders_made: 0,
                undoable: false,
                interrupted: true,
                note: None,
                undo: None,
            })
            .collect();
        if let Some(checkpoint) = sorts.checkpoint.as_mut() {
            let mut at = report(
                plan,
                steps.clone(),
                started,
                started_at,
                before.clone(),
                before.clone(),
                cancel,
            );
            at.id = id.clone();
            at.reorganized = pending;
            blocked = checkpoint(&at).err();
        }
    }
    let first = steps.len();
    for (k, approved) in plan.tidy().iter().enumerate() {
        let index = u32::try_from(first + k).unwrap_or(u32::MAX);
        (run.on_event)(RunEvent::StepStarted(StepStarted {
            step: index,
            id: approved.id.clone(),
        }));
        let on_event = &mut *run.on_event;
        let (skips, record) = crate::tidy::run_sort(
            approved,
            &id,
            env,
            sorts.journals,
            blocked.as_deref(),
            cancel,
            &mut |done, total| {
                on_event(RunEvent::MoveProgress(MoveProgress {
                    step: index,
                    done,
                    total,
                }))
            },
        );
        // What planning left out of the sort is reported with what the run left, as a cleanup's refusals are.
        let skips = approved.refused.iter().cloned().chain(skips).collect();
        let report = sort_report(approved, skips, record.moved_items);
        (run.on_event)(RunEvent::StepFinished(StepFinished {
            step: index,
            report: report.clone(),
        }));
        steps.push(report);
        reorganized.push(record);
    }
    let after = figures();
    let mut out = report(plan, steps, started, started_at, before, after, cancel);
    out.id = id;
    out.moved_items = reorganized.iter().map(|r| r.moved_items).sum();
    out.reorganized = reorganized;
    out
}

/// The moment from which a change counts as since the scan: its start, to the whole second before, less the
/// margin. In nanoseconds since the Unix epoch.
fn threshold(since: SystemTime) -> i128 {
    let secs = since
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let start = Duration::from_secs(secs).saturating_sub(SINCE_MARGIN);
    i128::try_from(start.as_nanos()).unwrap_or(0)
}

struct Run<'a, 'h> {
    on_event: &'a mut dyn FnMut(RunEvent),
    cancel: &'a CancelToken,
    hooks: &'a Hooks<'h>,
    threshold: i128,
    /// Freed by the steps that have finished.
    total_freed: u64,
    last_progress: Instant,
}

/// How a folder is opened (Windows; POSIX has no sharing modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Open {
    /// To look at, sharing everything: a check, dropped at once.
    Look,
    /// Held while the action runs, without letting anyone delete, rename or move it.
    Pin,
    /// To delete, held the same way.
    Delete,
}

/// One step on its way: its index, its Guard, what apps have open (macOS), and its tally.
struct StepRun<'g> {
    index: u32,
    step: &'g PlanStep,
    guard: Guard,
    busy: sys::Busy,
    tally: Tally,
    /// Skip events sent live for the action running now (see [`LIVE_SKIPS`]).
    live: usize,
    /// Files deleted since the last look at what apps have open (macOS). One still open at the next look was
    /// deleted while an app held it: its space comes back only when the app lets go, so it isn't counted as freed.
    unlinked: Vec<Unlinked>,
}

/// A file deleted and counted as freed, until a look at what apps have open says otherwise.
struct Unlinked {
    dev: u64,
    ino: u128,
    bytes: u64,
    path: PathBuf,
}

/// What the running step has done: in finished actions, and in the action running now.
#[derive(Default)]
struct Tally {
    step: Counts,
    action: Counts,
    links: u64,
    /// What the running action left in place, by reason, in the order first met.
    left: Vec<Left>,
    skips: Vec<Skip>,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    freed: u64,
    deleted: u64,
}

/// Files of the running action left in place for one reason.
struct Left {
    said: Said,
    files: u64,
    bytes: u64,
    first: PathBuf,
}

impl Tally {
    fn leave(&mut self, said: Said, path: &Path, bytes: u64, files: u64) {
        match self
            .left
            .iter_mut()
            .find(|l| l.said.kind == said.kind && l.said.one == said.one)
        {
            Some(l) => {
                l.files += files;
                l.bytes += bytes;
            }
            None => self.left.push(Left {
                said,
                files,
                bytes,
                first: path.to_path_buf(),
            }),
        }
    }

    /// What the running action left in place, in bytes and files, notes about deleted files apart.
    fn left_behind(&self) -> (u64, u64) {
        self.left
            .iter()
            .filter(|l| !l.said.kind.is_note())
            .fold((0, 0), |(b, f), l| (b + l.bytes, f + l.files))
    }
}

/// Why an action was skipped whole, before anything in it was deleted.
struct Whole {
    kind: SkipKind,
    reason: String,
}

impl Whole {
    fn new(kind: SkipKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }

    /// The target itself (`what` is "It"), or something it depends on, failed a platform call.
    fn of(what: &str, fail: Fail) -> Self {
        let said = fail.said();
        Self::new(said.kind, format!("{what} {}", said.one))
    }
}

/// Live skip events an action sends; past that, its skips are only in the step's report, summed by reason.
const LIVE_SKIPS: usize = 50;

/// How old what apps have open may get before it is read again (macOS): reading it takes a few tens of
/// milliseconds, so it is read at the start of each folder's batch, at most this often.
const BUSY_EVERY: Duration = Duration::from_millis(250);

/// Entries of one folder between looks at what apps have open (each look itself rate-limited by [`BUSY_EVERY`]).
const BUSY_BATCH: usize = 256;

/// Entries deleted from one folder between walks of its chain from the volume root. On macOS, every one: nothing
/// stops a folder above from being renamed while its contents go, so the chain is walked again right before each
/// unlink. On Windows the folders on the chain are held without `FILE_SHARE_DELETE`, so none can be renamed; the
/// walk is done again now and then as well.
const ANCESTRY_EVERY: usize = if cfg!(windows) { 256 } else { 1 };

/// Why the delete of a folder target stopped part way.
enum Stop {
    Cancelled,
    Moved,
}

impl Run<'_, '_> {
    fn step(&mut self, plan: &Plan, env: &Env, index: u32, step: &PlanStep) -> StepReport {
        let mut sr = StepRun {
            index,
            step,
            // Built afresh for every step, from the places as they are now.
            guard: Guard::new(env, plan.volume_root()),
            busy: sys::Busy::now(),
            tally: Tally::default(),
            live: 0,
            unlinked: Vec::new(),
        };
        for (skip, _) in &step.refused {
            self.skipped(&mut sr, skip.clone());
        }
        for action in &step.actions {
            if self.cancel.is_cancelled() {
                let skip = Skip {
                    path: action.path.display().to_string(),
                    reason: "You stopped the run before it got here".into(),
                    kind: SkipKind::Cancelled,
                    bytes: action.bytes,
                    items: action.files,
                };
                self.skipped(&mut sr, skip);
                continue;
            }
            sr.tally.action = Counts::default();
            sr.tally.links = 0;
            sr.tally.left.clear();
            sr.live = 0;
            if let Err(whole) = self.action(&mut sr, action) {
                let skip = Skip {
                    path: action.path.display().to_string(),
                    reason: whole.reason,
                    kind: whole.kind,
                    bytes: action.bytes,
                    items: action.files,
                };
                self.skipped(&mut sr, skip);
                continue;
            }
            // A last look at what apps have open, for the files just deleted.
            self.look_at_open_files(&mut sr, true);
            let tally = &mut sr.tally;
            let done = tally.action;
            tally.step.freed += done.freed;
            tally.step.deleted += done.deleted;
            tally.action = Counts::default();
            if done.deleted > 0 || tally.links > 0 || tally.left.is_empty() {
                let whole = !matches!(action.target, Target::Loose { .. })
                    && tally.left.iter().all(|l| l.said.kind.is_note());
                (self.on_event)(RunEvent::Deleted(Deleted {
                    step: index,
                    path: action.path.display().to_string(),
                    bytes: done.freed,
                    items: done.deleted,
                    whole,
                }));
            }
            // What was left went out as it happened; the report has it summed by reason.
            for left in std::mem::take(&mut tally.left) {
                tally.skips.push(left_skip(action, &left));
            }
            self.progress(&sr, &action.path, true);
        }
        self.total_freed += sr.tally.step.freed;
        step_report(step, sr.tally)
    }

    /// A skip of a whole place (or one the planner refused): sent, and kept for the report.
    fn skipped(&mut self, sr: &mut StepRun<'_>, skip: Skip) {
        (self.on_event)(RunEvent::Skipped(Skipped {
            step: sr.index,
            skip: skip.clone(),
        }));
        sr.tally.skips.push(skip);
    }

    /// Something inside a place left behind, or a note about a file deleted: tallied for the report, and sent as it
    /// happens (the first [`LIVE_SKIPS`] of an action).
    fn leave(&mut self, sr: &mut StepRun<'_>, said: Said, at: &Path, bytes: u64, files: u64) {
        if sr.live < LIVE_SKIPS {
            sr.live += 1;
            let reason = if files <= 1 {
                format!("It {}", said.one)
            } else {
                format!("{} files {}", group_digits(files), said.many)
            };
            (self.on_event)(RunEvent::Skipped(Skipped {
                step: sr.index,
                skip: Skip {
                    path: at.display().to_string(),
                    reason,
                    kind: said.kind,
                    bytes,
                    items: files,
                },
            }));
        }
        sr.tally.leave(said, at, bytes, files);
    }

    fn fail(&mut self, sr: &mut StepRun<'_>, fail: &Fail, at: &Path, bytes: u64, files: u64) {
        self.leave(sr, fail.said(), at, bytes, files);
    }

    fn progress(&mut self, sr: &StepRun<'_>, at: &Path, now: bool) {
        if !now && self.last_progress.elapsed() < PROGRESS_EVERY {
            return;
        }
        self.last_progress = Instant::now();
        let tally = &sr.tally;
        let freed = tally.step.freed + tally.action.freed;
        (self.on_event)(RunEvent::Progress(RunProgress {
            step: sr.index,
            freed_bytes: freed,
            deleted_items: tally.step.deleted + tally.action.deleted,
            total_freed_bytes: self.total_freed + freed,
            current_path: at.display().to_string(),
        }));
    }

    /// Reads again what apps have open (macOS), when the last read is [`BUSY_EVERY`] old or with `force`, and
    /// takes back from the freed count every file deleted since that an app still has open: its space comes back
    /// only when the app lets go of it.
    fn look_at_open_files(&mut self, sr: &mut StepRun<'_>, force: bool) {
        let every = if force { Duration::ZERO } else { BUSY_EVERY };
        if sr.unlinked.is_empty() && force {
            return;
        }
        if !sr.busy.refresh(every) {
            return;
        }
        for u in std::mem::take(&mut sr.unlinked) {
            if let Some(app) = sr.busy.holder(u.dev, u.ino) {
                sr.tally.action.freed -= u.bytes;
                self.leave(sr, Said::held_open(app), &u.path, u.bytes, 1);
            }
        }
    }

    /// Carries out one action: the fresh walk, the Guard, the fingerprint, then the delete. `Err` when it is
    /// skipped whole, before anything in it was deleted.
    fn action(&mut self, sr: &mut StepRun<'_>, action: &Action) -> Result<(), Whole> {
        let path = &action.path;
        let name = path
            .file_name()
            .ok_or_else(|| Whole::new(SkipKind::Failed, "Its path has no name"))?;
        // Every folder on the way held: on Windows none of them can be renamed while the action runs.
        let parent = sys::walk_to_parent(path, true).map_err(|f| Whole::of("The way to it", f))?;
        let (parent_dir, parent_steps) = (&parent.dir, &parent.steps);
        let volume = parent_steps.last().map_or(0, |s| s.id.volume());
        match &action.target {
            Target::File => {
                let entry = sys::entry(parent_dir, name).map_err(|f| Whole::of("It", f))?;
                let mut chain = parent_steps.clone();
                chain.push(entry.step(volume));
                judge(&sr.guard, path, &chain, false)?;
                same_identity(&chain, action)?;
                if entry.kind != Kind::File {
                    return Err(Whole::new(
                        SkipKind::Changed,
                        "It is no longer a file: something else is in its place",
                    ));
                }
                if entry.changed >= self.threshold {
                    return Err(Whole::new(SkipKind::Changed, "It changed since the scan"));
                }
                if entry.alloc != action.bytes || entry.logical != action.logical {
                    return Err(Whole::new(
                        SkipKind::Changed,
                        "Its size changed since the scan",
                    ));
                }
                if let Some(hook) = self.hooks.before_delete {
                    hook(path);
                }
                self.look_at_open_files(sr, false);
                if !in_place(parent_steps, path.parent().unwrap_or(path)) {
                    return Err(Whole::new(SkipKind::Changed, MOVED));
                }
                self.delete_file(sr, parent_dir, path, &entry, false);
                Ok(())
            }
            Target::Loose { files } => {
                let (dir, target) = sys::open_dir(parent_dir, name, None, Open::Pin)
                    .map_err(|f| Whole::of("It", f))?;
                let mut chain = parent_steps.clone();
                chain.push(target);
                judge(&sr.guard, path, &chain, true)?;
                same_identity(&chain, action)?;
                self.loose(sr, &dir, &chain, path, files)
            }
            Target::Dir { folders } => {
                let (dir, target) = sys::open_dir(parent_dir, name, None, Open::Delete)
                    .map_err(|f| Whole::of("It", f))?;
                let mut chain = parent_steps.clone();
                chain.push(target.clone());
                judge(&sr.guard, path, &chain, false)?;
                same_identity(&chain, action)?;
                if sys::changed(&dir).map_err(|f| Whole::of("It", f))? >= self.threshold {
                    return Err(Whole::new(SkipKind::Changed, "It changed since the scan"));
                }
                let tree = Tree {
                    folders,
                    root_step: &target,
                    path,
                };
                self.verify_tree(sr, &dir, &tree, action)?;
                if let Some(hook) = self.hooks.before_delete {
                    hook(path);
                }
                self.delete_tree(sr, parent_dir, dir, chain, &tree, action);
                Ok(())
            }
        }
    }

    /// Checks a folder target whole before anything in it goes: every folder in it lists exactly what the scan
    /// listed there (by identity), every entry is unchanged since the scan, every folder is the one its parent
    /// listed, on the target's volume and mount, nothing protected is inside, and its size is the scan's. See the
    /// module docs.
    fn verify_tree(
        &self,
        sr: &StepRun<'_>,
        root: &sys::Dir,
        tree: &Tree<'_>,
        action: &Action,
    ) -> Result<(), Whole> {
        let path = tree.path;
        let mut measure = Measure::default();
        let entries = sys::list(root).map_err(|f| Whole::of("Its contents", f))?;
        if !tree.lists_as_scanned(tree.root_step, &entries) {
            return Err(changed_inside(path, path));
        }
        // The target's own folder is the caller's; the ones below are opened here, one per level.
        let mut stack: Vec<(Option<sys::Dir>, PathBuf, Vec<Entry>, usize)> =
            vec![(None, path.to_path_buf(), entries, 0)];
        while let Some((dir, at_dir, entries, next)) = stack.last_mut() {
            let Some(e) = entries.get(*next).cloned() else {
                stack.pop();
                continue;
            };
            *next += 1;
            let at = at_dir.join(&e.name);
            if e.changed >= self.threshold {
                return Err(changed_inside(path, &at));
            }
            if e.counts {
                measure.add(&e);
            }
            match e.kind {
                Kind::File | Kind::Special(_) => {}
                Kind::Link => {
                    let parent = dir.as_ref().unwrap_or(root);
                    let inside = |f: Fail| Whole::of(&format!("{} inside it", at.display()), f);
                    if e.junction && sys::mounts_volume(parent, &e).map_err(inside)? {
                        return Err(Whole::new(
                            SkipKind::OtherVolume,
                            format!("Another volume is mounted inside it, at {}", at.display()),
                        ));
                    }
                }
                Kind::Dir => {
                    let inside = |f: Fail| Whole::of(&format!("{} inside it", at.display()), f);
                    let parent = dir.as_ref().unwrap_or(root);
                    let (sub_dir, sub) =
                        sys::open_dir(parent, &e.name, Some(&e), Open::Look).map_err(inside)?;
                    if !same_volume(tree.root_step, &sub) {
                        return Err(Whole::new(
                            SkipKind::OtherVolume,
                            format!("Another volume is mounted inside it, at {}", at.display()),
                        ));
                    }
                    if let Some(what) = sr.guard.protects(sub.id) {
                        return Err(Whole::new(
                            SkipKind::Protected,
                            format!("It holds {what}, which Vacancy never touches"),
                        ));
                    }
                    if let Some((what, app)) = sr.guard.protected_os_name(&e.name)
                        && !(app && sr.step.takes_built_apps)
                    {
                        return Err(Whole::new(
                            SkipKind::Protected,
                            format!(
                                "It holds {what} ({}), which Vacancy never touches",
                                at.display()
                            ),
                        ));
                    }
                    let entries = sys::list(&sub_dir).map_err(inside)?;
                    if !tree.lists_as_scanned(&sub, &entries) {
                        return Err(changed_inside(path, &at));
                    }
                    stack.push((Some(sub_dir), at, entries, 0));
                }
            }
        }
        measure.check(action)
    }

    /// Deletes a verified folder target, depth first, relative to the open folders. Each folder is listed again
    /// and must still list exactly what the scan listed, or nothing in it goes; and it must still be where the
    /// Guard judged it: its chain from the volume root is walked again at the start of its batch and every
    /// [`ANCESTRY_EVERY`] entries (right before each unlink on macOS). One that moved stops the target. See the
    /// module docs.
    fn delete_tree(
        &mut self,
        sr: &mut StepRun<'_>,
        parent: &sys::Dir,
        root: sys::Dir,
        root_chain: Vec<Step>,
        tree: &Tree<'_>,
        action: &Action,
    ) {
        let path = tree.path;
        let entries = match sys::list(&root) {
            Ok(entries) if tree.lists_as_scanned(tree.root_step, &entries) => entries,
            Ok(_) => return self.leave(sr, Said::changed(), path, action.bytes, action.files),
            Err(fail) => return self.fail(sr, &fail, path, action.bytes, action.files),
        };
        let mut stack: Vec<Frame> = vec![Frame {
            dir: root,
            chain: root_chain,
            path: path.to_path_buf(),
            entries,
            next: 0,
            checked: None,
        }];
        let mut stopped: Option<Stop> = None;
        while let Some(top) = stack.last_mut() {
            let next = if stopped.is_some() {
                None
            } else {
                top.entries.get(top.next).cloned()
            };
            let Some(e) = next else {
                // Done with this folder: it goes if it is empty now, and still in its place. One still holding
                // something left in place stays, explained by what it holds. After a stop nothing more is removed.
                let frame = stack.pop().expect("a frame was on the stack");
                if stopped.is_none() {
                    if !in_place(&frame.chain, &frame.path) {
                        stopped = Some(Stop::Moved);
                        continue;
                    }
                    let step = frame.chain.last().expect("a chain has its folder").clone();
                    // The target's own folder is removed from its parent, which is not the target's to change.
                    let inside_target = !stack.is_empty();
                    let above = stack.last().map_or(parent, |f| &f.dir);
                    match sys::remove_dir(above, frame.dir, &step, inside_target) {
                        Ok(()) | Err(Fail::Gone) => {}
                        // Nothing left in it was left on purpose: something new arrived while it was emptied.
                        Err(Fail::NotEmpty)
                            if stack.is_empty() && sr.tally.left_behind().1 == 0 =>
                        {
                            let said = Said {
                                kind: SkipKind::Changed,
                                one: "stays: something was added to it during the run".into(),
                                many: "stay: something was added to them during the run".into(),
                            };
                            self.leave(sr, said, &frame.path, 0, 0);
                        }
                        Err(Fail::NotEmpty) => {}
                        Err(fail) => self.fail(sr, &fail, &frame.path, 0, 0),
                    }
                }
                continue;
            };
            if self.cancel.is_cancelled() {
                // Stop after the file just deleted. What is left of the action is counted below.
                stopped = Some(Stop::Cancelled);
                continue;
            }
            // Still where the Guard judged it? At the batch's start, and every few entries after.
            if top.checked.is_none_or(|at| top.next - at >= ANCESTRY_EVERY) {
                if !in_place(&top.chain, &top.path) {
                    stopped = Some(Stop::Moved);
                    continue;
                }
                top.checked = Some(top.next);
            }
            if top.next.is_multiple_of(BUSY_BATCH) {
                self.look_at_open_files(sr, false);
            }
            let top = stack.last_mut().expect("a frame is on the stack");
            top.next += 1;
            let at = top.path.join(&e.name);
            if e.changed >= self.threshold {
                let file = u64::from(e.counts);
                self.leave(sr, Said::changed(), &at, e.alloc, file);
                continue;
            }
            match e.kind {
                Kind::File => {
                    if let Some(hook) = self.hooks.before_file {
                        hook(&at);
                    }
                    let frame = stack.last().expect("a frame is on the stack");
                    self.delete_file(sr, &frame.dir, &at, &e, true);
                }
                Kind::Link => match sys::delete_link(&top.dir, &e, self.threshold) {
                    Ok(()) => sr.tally.links += 1,
                    Err(Fail::Gone) => {}
                    Err(fail) => self.fail(sr, &fail, &at, 0, 0),
                },
                Kind::Special(what) => {
                    self.leave(sr, Said::special(what), &at, e.alloc, u64::from(e.counts));
                }
                Kind::Dir => {
                    let threshold = self.threshold;
                    let chain = top.chain.clone();
                    let opened = sys::open_dir(&top.dir, &e.name, Some(&e), Open::Delete).and_then(
                        |(dir, sub)| {
                            if !same_volume(tree.root_step, &sub) {
                                return Err(Fail::OtherVolume);
                            }
                            if sys::changed(&dir)? >= threshold {
                                return Err(Fail::Changed);
                            }
                            let entries = sys::list(&dir)?;
                            if !tree.lists_as_scanned(&sub, &entries) {
                                return Err(Fail::Changed);
                            }
                            Ok((dir, sub, entries))
                        },
                    );
                    match opened {
                        Ok((dir, sub, entries)) => {
                            let mut chain = chain;
                            chain.push(sub);
                            // A new batch: what apps have open, read again unless just read.
                            self.look_at_open_files(sr, false);
                            stack.push(Frame {
                                dir,
                                chain,
                                path: at.clone(),
                                entries,
                                next: 0,
                                checked: None,
                            });
                        }
                        Err(Fail::Gone) => {}
                        Err(fail) => self.fail(sr, &fail, &at, 0, 0),
                    }
                }
            }
            self.progress(sr, &at, false);
        }
        if let Some(stop) = stopped {
            // What the scan measured of the action, less what went and what was left for other reasons.
            let (left_bytes, left_files) = sr.tally.left_behind();
            let files = action
                .files
                .saturating_sub(sr.tally.action.deleted + left_files);
            let bytes = action
                .bytes
                .saturating_sub(sr.tally.action.freed + left_bytes);
            let said = match stop {
                Stop::Cancelled => Said::cancelled(),
                Stop::Moved => Said::moved(),
            };
            self.leave(sr, said, path, bytes, files);
        }
    }

    /// Deletes one file `e` in the open folder `dir`, tallying what it freed or why it stayed. With
    /// `inside_target`, `dir` is a folder being deleted, which may be made writable to let its files go (macOS).
    fn delete_file(
        &mut self,
        sr: &mut StepRun<'_>,
        dir: &sys::Dir,
        at: &Path,
        e: &Entry,
        inside_target: bool,
    ) {
        match sys::delete_file(dir, e, self.threshold, &sr.busy, inside_target) {
            Ok(Some(freed)) => {
                sr.tally.action.freed += freed;
                sr.tally.action.deleted += 1;
                if cfg!(unix) && freed > 0 {
                    sr.unlinked.push(Unlinked {
                        dev: e.dev,
                        ino: e.id,
                        bytes: freed,
                        path: at.to_path_buf(),
                    });
                }
            }
            Ok(None) => {
                // Its name here is gone; its other names keep the file, and its space.
                sr.tally.action.deleted += 1;
                self.leave(sr, Said::hard_linked(), at, e.alloc, 1);
            }
            Err(Fail::Gone) => {}
            Err(fail) => self.fail(sr, &fail, at, e.alloc, 1),
        }
    }

    /// Deletes a folder's loose files: only those the plan holds by name and identity (the ones the scan saw),
    /// each unchanged since the scan, while the folder is still where the Guard judged it. Files new or changed
    /// since are left alone, and so is the folder.
    fn loose(
        &mut self,
        sr: &mut StepRun<'_>,
        dir: &sys::Dir,
        chain: &[Step],
        path: &Path,
        planned: &[LooseFile],
    ) -> Result<(), Whole> {
        let entries = sys::list(dir).map_err(|f| Whole::of("Its files", f))?;
        let mut found: Vec<Entry> = Vec::new();
        let mut seen = 0usize;
        for e in entries {
            let Some(p) = planned
                .iter()
                .find(|p| p.name == e.name && p.key == e.key())
            else {
                continue;
            };
            seen += 1;
            if (p.logical, p.alloc) != (e.logical, e.alloc) {
                // The file the scan saw, rewritten to another size since: not what was approved.
                let at = path.join(&e.name);
                self.leave(sr, Said::changed(), &at, e.alloc, 1);
                continue;
            }
            found.push(e);
        }
        if let Some(hook) = self.hooks.before_delete {
            hook(path);
        }
        for (i, e) in found.iter().enumerate() {
            let at = path.join(&e.name);
            let rest = |found: &[Entry]| (found.iter().map(|e| e.alloc).sum(), found.len() as u64);
            if self.cancel.is_cancelled() {
                let (bytes, files) = rest(&found[i..]);
                self.leave(sr, Said::cancelled(), path, bytes, files);
                break;
            }
            if i.is_multiple_of(ANCESTRY_EVERY) && !in_place(chain, path) {
                let (bytes, files) = rest(&found[i..]);
                self.leave(sr, Said::moved(), path, bytes, files);
                break;
            }
            if i.is_multiple_of(BUSY_BATCH) {
                self.look_at_open_files(sr, false);
            }
            if e.changed >= self.threshold {
                self.leave(sr, Said::changed(), &at, e.alloc, 1);
                continue;
            }
            if let Kind::Special(what) = e.kind {
                self.leave(sr, Said::special(what), &at, e.alloc, 1);
                continue;
            }
            if let Some(hook) = self.hooks.before_file {
                hook(&at);
            }
            self.delete_file(sr, dir, &at, e, false);
            self.progress(sr, &at, false);
        }
        // The files the scan saw that aren't there now: removed by someone else, or replaced.
        let missing = planned.len().saturating_sub(seen) as u64;
        if missing > 0 {
            let said = Said {
                kind: SkipKind::Gone,
                one: "was removed or replaced since the scan".into(),
                many: "were removed or replaced since the scan".into(),
            };
            self.leave(sr, said, path, 0, missing);
        }
        Ok(())
    }
}

/// Why a target stopped when a folder in it moved.
const MOVED: &str = "It was moved during the run, so the rest of it was left where it went";

/// Whether the folder at `path` is still the one at the end of `chain`, reached the way the chain was: its chain
/// walked again from the volume root without following a link, every step the same identity (and on macOS the
/// same mount). A folder renamed or moved since (into a cloud-sync folder, say), or one whose folders above were,
/// isn't, and nothing more in it goes.
fn in_place(chain: &[Step], path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let Ok(parent) = sys::walk_to_parent(path, false) else {
        return false;
    };
    let Ok((_, step)) = sys::open_dir(&parent.dir, name, None, Open::Look) else {
        return false;
    };
    let now = parent.steps.iter().chain(std::iter::once(&step));
    parent.steps.len() + 1 == chain.len()
        && now
            .zip(chain)
            .all(|(a, b)| a.id == b.id && a.mount == b.mount)
}

/// A folder target being checked and deleted: what the scan listed in each of its folders, its own step, and its
/// path.
struct Tree<'a> {
    folders: &'a HashMap<u128, u64>,
    root_step: &'a Step,
    path: &'a Path,
}

impl Tree<'_> {
    /// Whether the folder `step` lists exactly what the scan listed there: the digest of its entries' identities
    /// (and its files' sizes) is the scan's. A folder the scan didn't list is new, so no.
    fn lists_as_scanned(&self, step: &Step, entries: &[Entry]) -> bool {
        let digest = entries
            .iter()
            .fold(0u64, |sum, e| sum.wrapping_add(e.digest()));
        self.folders.get(&step.id.scan_key()) == Some(&digest)
    }
}

/// A folder being deleted: open with the right to delete it, its chain from the volume root (itself last), its
/// listing, how far through it, and where its chain was last walked again.
struct Frame {
    dir: sys::Dir,
    chain: Vec<Step>,
    path: PathBuf,
    entries: Vec<Entry>,
    next: usize,
    checked: Option<usize>,
}

fn judge(guard: &Guard, path: &Path, chain: &[Step], loose: bool) -> Result<(), Whole> {
    guard.judge_walked(path, chain, loose).map_err(|denied| {
        let (kind, reason) = denied_reason(&denied);
        Whole::new(kind, reason)
    })
}

/// What a target holds now, for its size check: each file once (a file with several names inside counts once),
/// split by whether it has other names, where the listing says (macOS).
#[derive(Default)]
struct Measure {
    seen: HashSet<(u64, u128)>,
    bytes: u64,
    files: u64,
    /// Files with one name only, when the listing gives link counts.
    single_bytes: u64,
    single_files: u64,
    links_known: bool,
}

impl Measure {
    fn add(&mut self, e: &Entry) {
        if !self.seen.insert((e.dev, e.id)) {
            return;
        }
        self.bytes += e.alloc;
        self.files += 1;
        if let Some(links) = e.links {
            self.links_known = true;
            if links <= 1 {
                self.single_bytes += e.alloc;
                self.single_files += 1;
            }
        }
    }

    /// A folder target holds what the scan measured: never less, and (where link counts are known) no more than
    /// its files with other names could explain. The scan counted each hard-linked file once, wherever it saw it
    /// first, so a target holding such files may count more now than the scan gave it, but its files with one name
    /// can't. Where link counts aren't known (Windows) only the lower bound is checked here: anything added since
    /// the scan carries a new change time, which the checks before have already refused.
    fn check(&self, action: &Action) -> Result<(), Whole> {
        if self.bytes < action.bytes || self.files < action.files {
            return Err(Whole::new(
                SkipKind::Changed,
                "It holds less than at the scan: something in it was removed since",
            ));
        }
        if self.links_known
            && (self.single_bytes > action.bytes || self.single_files > action.files)
        {
            return Err(Whole::new(
                SkipKind::Changed,
                "It holds more than at the scan: something was added since",
            ));
        }
        Ok(())
    }
}

/// The target found now is the one the scan saw.
fn same_identity(chain: &[Step], action: &Action) -> Result<(), Whole> {
    match chain.last() {
        Some(s) if s.id == action.identity => Ok(()),
        _ => Err(Whole::new(
            SkipKind::Changed,
            "It was replaced since the scan: something else is in its place",
        )),
    }
}

/// Whether a folder found inside a target is on the target's volume (and, on macOS, its mount).
fn same_volume(target: &Step, sub: &Step) -> bool {
    fsid::one_volume(&[target.clone(), sub.clone()])
}

fn changed_inside(path: &Path, at: &Path) -> Whole {
    let rel = at.strip_prefix(path).unwrap_or(at);
    // The target itself changed: name it, so the sentence never reads "It changed since the scan:  was modified".
    let what = if rel.as_os_str().is_empty() {
        path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    } else {
        rel.display().to_string()
    };
    Whole::new(
        SkipKind::Changed,
        format!("It changed since the scan: {what} was modified, added or moved"),
    )
}

/// The skip for what one action left in place for one reason.
fn left_skip(action: &Action, left: &Left) -> Skip {
    let (path, reason) = if left.files <= 1 {
        (
            left.first.display().to_string(),
            format!("It {}", left.said.one),
        )
    } else {
        (
            action.path.display().to_string(),
            format!("{} files {}", group_digits(left.files), left.said.many),
        )
    };
    Skip {
        path,
        reason,
        kind: left.said.kind,
        bytes: left.bytes,
        items: left.files,
    }
}

/// The step's report: its numbers, its outcome, and its skips, at most [`SKIPS_PER_KIND`] of each kind with the
/// rest summed into one.
fn step_report(step: &PlanStep, tally: Tally) -> StepReport {
    // What the confirm dialog showed: the places that were to run, not those refused while planning.
    let planned_bytes = step.actions.iter().map(|a| a.bytes).sum::<u64>();
    let planned_items = step.actions.iter().map(|a| a.files).sum::<u64>();
    let mut skipped: Vec<Skip> = Vec::new();
    let mut per_kind: HashMap<SkipKind, usize> = HashMap::new();
    let mut rest: Vec<Skip> = Vec::new();
    for skip in tally.skips {
        let n = per_kind.entry(skip.kind).or_default();
        *n += 1;
        if *n <= SKIPS_PER_KIND {
            skipped.push(skip);
            continue;
        }
        match rest.iter_mut().find(|r| r.kind == skip.kind) {
            Some(r) => {
                r.bytes += skip.bytes;
                r.items += skip.items;
            }
            None => rest.push(Skip {
                path: String::new(),
                reason: String::new(),
                ..skip
            }),
        }
    }
    for mut r in rest {
        let places = per_kind[&r.kind] - SKIPS_PER_KIND;
        r.reason = format!(
            "{} more place{} skipped for the same reason",
            group_digits(places as u64),
            if places == 1 { "" } else { "s" }
        );
        skipped.push(r);
    }
    let outcome = if skipped.iter().all(|k| k.kind.is_note()) {
        Outcome::Done
    } else if tally.step.deleted > 0 {
        Outcome::Partial
    } else {
        Outcome::Skipped
    };
    StepReport {
        id: step.id.clone(),
        title: step.title.clone(),
        why: step.why.clone(),
        category: step.category,
        regenerable: step.regenerable,
        planned_bytes,
        freed_bytes: tally.step.freed,
        planned_items,
        deleted_items: tally.step.deleted,
        kind: StepKind::Delete,
        moved_items: 0,
        places: step.actions.len() as u64,
        outcome,
        skipped,
    }
}

/// The report of a Tidy up sort step: what it moved and what it left, and why. It frees nothing.
fn sort_report(approved: &Approved, skips: Vec<Skip>, moved: u64) -> StepReport {
    let left: u64 = skips
        .iter()
        .filter(|k| !k.kind.is_note())
        .map(|k| k.items)
        .sum();
    let outcome = if left == 0 {
        Outcome::Done
    } else if moved > 0 {
        Outcome::Partial
    } else {
        Outcome::Skipped
    };
    StepReport {
        id: approved.id.clone(),
        title: approved.title(),
        why: format!(
            "Loose files at the top of {} sorted into folders by kind and year, by Vacancy's built-in rules. Moved, \
             not deleted.",
            approved.name
        ),
        category: approved.category(),
        regenerable: false,
        planned_bytes: 0,
        freed_bytes: 0,
        // What the review meant to move: moved and left add up to it.
        planned_items: approved.planned(),
        deleted_items: 0,
        kind: StepKind::Move,
        moved_items: moved,
        places: 0,
        outcome,
        skipped: skips,
    }
}

fn report(
    plan: &Plan,
    steps: Vec<StepReport>,
    started: Instant,
    started_at: SystemTime,
    before: Option<VolumeInfo>,
    after: Option<VolumeInfo>,
    cancel: &CancelToken,
) -> RunReport {
    let mut by_category: Vec<CategoryFreed> = Vec::new();
    for s in steps.iter().filter(|s| s.freed_bytes > 0) {
        match by_category.iter_mut().find(|c| c.id == s.category) {
            Some(c) => c.bytes += s.freed_bytes,
            None => by_category.push(CategoryFreed {
                id: s.category,
                bytes: s.freed_bytes,
            }),
        }
    }
    by_category.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    let biggest_win = steps
        .iter()
        .filter(|s| s.freed_bytes > 0)
        .max_by_key(|s| s.freed_bytes)
        .map(|s| s.id.clone());
    let (total_bytes, used_before, free_before) = before
        .as_ref()
        .map(|v| (v.total_bytes, v.used_bytes, v.free_bytes))
        .unwrap_or_default();
    let (used_after, free_after) = after
        .as_ref()
        .map(|v| (v.used_bytes, v.free_bytes))
        .unwrap_or((used_before, free_before));
    let free_change = match (&before, &after) {
        (Some(_), Some(_)) => {
            i64::try_from(i128::from(free_after) - i128::from(free_before)).unwrap_or(0)
        }
        _ => 0,
    };
    RunReport {
        id: next_id("run"),
        plan_id: plan.id().to_owned(),
        mount_point: plan.mount_point().to_owned(),
        volume_name: before
            .as_ref()
            .or(after.as_ref())
            .map_or_else(|| plan.mount_point().to_owned(), |v| v.name.clone()),
        started_at: started_at
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cancelled: cancel.is_cancelled(),
        planned_bytes: steps.iter().map(|s| s.planned_bytes).sum(),
        freed_bytes: steps.iter().map(|s| s.freed_bytes).sum(),
        deleted_items: steps.iter().map(|s| s.deleted_items).sum(),
        // Set by the caller from the sorts' records.
        moved_items: 0,
        // Names deleted while other names keep the file were deleted, not skipped.
        skipped_items: steps
            .iter()
            .flat_map(|s| &s.skipped)
            .filter(|k| !k.kind.is_note())
            .map(|k| k.items)
            .sum(),
        freed_by_category: by_category,
        total_bytes,
        used_before,
        used_after,
        free_before,
        free_after,
        free_change,
        biggest_win,
        left_for_you: plan.left_for_you().to_vec(),
        reorganized: Vec::new(),
        map: plan.map().clone(),
        steps,
    }
}

#[cfg(test)]
mod tests {
    //! Changes made in the middle of a run, through the executor's hooks: between a target's checks and its
    //! delete, and right before a file in it goes. The fixture profile and the rest of the run are tested in
    //! `tests/run.rs`.

    use std::cell::Cell;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::fixture::{self, Profile};
    use crate::map::{ScannedVolume, scan_for_map_with};
    use crate::rules::Moment;

    struct Fx {
        _dir: tempfile::TempDir,
        profile: Profile,
        env: Env,
        scanned: ScannedVolume,
    }

    impl Fx {
        fn new() -> Self {
            Self::with(|_| {})
        }

        /// The fixture, with `prepare` run on its profile before the scan.
        fn with(prepare: impl FnOnce(&Profile)) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = if cfg!(windows) {
                dir.path().to_path_buf()
            } else {
                dir.path().canonicalize().unwrap()
            }
            .join("fixture");
            let profile = fixture::build(&root).unwrap();
            prepare(&profile);
            let mut env = fixture::env(&root);
            // A cloud-sync folder in the home folder, which the Guard protects whole.
            let cloud = profile.home.join("Dropbox");
            std::fs::create_dir_all(&cloud).unwrap();
            env.cloud.push(crate::categorize::CloudRoot {
                provider: "Dropbox".into(),
                path: cloud,
            });
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
            Self {
                _dir: dir,
                profile,
                env,
                scanned,
            }
        }

        fn run(&self, id: &str, hooks: &Hooks<'_>, cancel: &CancelToken) -> RunReport {
            let plan = crate::plan::plan(&self.scanned, &self.env, &[id.to_owned()]).unwrap();
            execute_inner(
                &plan,
                &self.env,
                Sorts::default(),
                &|| None,
                &mut |_| {},
                cancel,
                hooks,
            )
        }

        fn documents(&self) -> Vec<String> {
            fixture::inventory(&self.profile.documents).unwrap()
        }

        fn cloud(&self) -> PathBuf {
            self.profile.home.join("Dropbox")
        }
    }

    fn node() -> &'static str {
        if cfg!(windows) { "w-node" } else { "m-node" }
    }

    fn cache() -> &'static str {
        if cfg!(windows) {
            "w-browser"
        } else {
            "m-caches"
        }
    }

    fn link_dir(link: &Path, to: &Path) {
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(to)
                .output()
                .unwrap()
                .status
                .success()
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(to, link).unwrap();
    }

    fn skips(report: &RunReport) -> Vec<&Skip> {
        report.steps.iter().flat_map(|s| &s.skipped).collect()
    }

    /// After `node_modules` passed its checks and before anything in it is deleted, its `.pnpm` folder is moved
    /// aside and a link to Documents put in its place. The delete never goes through the link: the link is new, so
    /// it is left (changed since the scan), Documents is untouched, and so is the folder moved aside.
    #[test]
    fn a_folder_swapped_for_a_link_after_the_checks_is_never_followed() {
        let fx = Fx::new();
        let docs = fx.documents();
        let pnpm = fx.profile.node_modules.join(".pnpm");
        let aside = fx.profile.node_modules.join(".pnpm-aside");
        let swapped = Cell::new(false);
        let swap = |_: &Path| {
            std::fs::rename(&pnpm, &aside).unwrap();
            link_dir(&pnpm, &fx.profile.documents);
            swapped.set(true);
        };
        let report = fx.run(
            node(),
            &Hooks {
                before_delete: Some(&swap),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        assert!(swapped.get(), "the hook ran");
        assert_eq!(fx.documents(), docs, "Documents is untouched");
        // The folder moved aside is approved content under a new name. Whether it goes depends on whether the file
        // system counts the rename as a change to it (NTFS does, so it stays there); either way nothing outside the
        // target is touched.
        if cfg!(windows) {
            assert!(aside.exists(), "renamed since the scan, so it stays");
        }
        assert!(
            std::fs::symlink_metadata(&pnpm)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the new link stays"
        );
        assert!(
            skips(&report).iter().any(|k| k.kind == SkipKind::Changed),
            "{:#?}",
            report.steps
        );
    }

    /// Right before a file in a cache folder goes, it is replaced by a link to Documents. The file opened is not the
    /// file listed, so nothing is deleted through it.
    #[test]
    fn a_file_swapped_for_a_link_just_before_its_delete_is_left() {
        let fx = Fx::new();
        let docs = fx.documents();
        let victim = fx.profile.cache.join("f_000001");
        let swapped = Cell::new(false);
        let swap = |at: &Path| {
            if at == victim && !swapped.get() {
                std::fs::remove_file(&victim).unwrap();
                link_dir(&victim, &fx.profile.documents);
                swapped.set(true);
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_file: Some(&swap),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        assert!(swapped.get(), "the hook ran");
        assert_eq!(fx.documents(), docs, "Documents is untouched");
        assert!(std::fs::symlink_metadata(&victim).is_ok(), "the link stays");
        assert!(
            skips(&report)
                .iter()
                .any(|k| k.kind == SkipKind::Changed && Path::new(&k.path) == victim),
            "{:#?}",
            report.steps
        );
    }

    /// A cancel inside a folder being deleted stops after the file being deleted: the files before it are gone,
    /// the rest are there, the folder stays, and the report counts exactly the files that went.
    #[test]
    fn a_cancel_inside_a_folder_stops_after_the_current_file() {
        let fx = Fx::new();
        let cancel = CancelToken::new();
        let seen = Cell::new(0u32);
        let count = |_: &Path| {
            seen.set(seen.get() + 1);
            if seen.get() == 3 {
                cancel.cancel();
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_file: Some(&count),
                ..Hooks::default()
            },
            &cancel,
        );
        assert!(report.cancelled);
        let left = std::fs::read_dir(&fx.profile.cache).unwrap().count() as u64;
        let step = &report.steps[0];
        assert_eq!(step.deleted_items, 3, "{step:#?}");
        assert!(left > 0, "the rest of the folder stays");
        let cancelled: u64 = step
            .skipped
            .iter()
            .filter(|k| k.kind == SkipKind::Cancelled)
            .map(|k| k.items)
            .sum();
        assert!(cancelled >= left, "every file left is counted: {step:#?}");
        assert!(fx.profile.cache.exists());
    }

    /// Rewrites `path` in place to `len` bytes and sets its times back as they were: on Windows all four, the
    /// change time too (any program allowed to write the file's attributes can); elsewhere the kernel sets the
    /// change time and only the others go back.
    fn rewrite_keeping_times(path: &Path, len: usize) {
        use std::io::Write;
        keeping_times(path, |f| {
            f.write_all(&vec![0xA5u8; len]).unwrap();
            f.set_len(len as u64).unwrap();
        });
    }

    /// Writes `len` bytes at `at` in `path`, inside its length, written out to disk as the system's lazy writer
    /// would within a second (NTFS allocates a sparse file's hole only then), and sets its times back.
    fn write_into_keeping_times(path: &Path, at: u64, len: usize) {
        use std::io::{Seek, SeekFrom, Write};
        keeping_times(path, |f| {
            f.seek(SeekFrom::Start(at)).unwrap();
            f.write_all(&vec![0xC3u8; len]).unwrap();
            f.sync_all().unwrap();
        });
    }

    /// Changes `path` through `change`, then sets its times back: on Windows all four, the change time included;
    /// elsewhere the modified and accessed times (the kernel keeps the change time).
    fn keeping_times(path: &Path, change: impl FnOnce(&mut std::fs::File)) {
        #[cfg(windows)]
        let times = win::times(path);
        #[cfg(not(windows))]
        let old = std::fs::metadata(path).unwrap();
        let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        change(&mut f);
        drop(f);
        #[cfg(windows)]
        win::set_times(path, &times);
        #[cfg(not(windows))]
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(old.modified().unwrap())
                    .set_accessed(old.accessed().unwrap()),
            )
            .unwrap();
    }

    #[cfg(windows)]
    mod win {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use std::path::Path;

        use windows_sys::Win32::Storage::FileSystem::{
            FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
            SetFileInformationByHandle,
        };

        fn open(path: &Path) -> std::fs::File {
            std::fs::OpenOptions::new()
                .access_mode(0x0080 | 0x0100) // FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES
                .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS
                .open(path)
                .unwrap()
        }

        pub(super) fn times(path: &Path) -> FILE_BASIC_INFO {
            let file = open(path);
            let mut info = FILE_BASIC_INFO::default();
            // SAFETY: `info` is writable and sized as passed; the handle is open.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileBasicInfo,
                    (&raw mut info).cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            };
            assert_ne!(ok, 0);
            info
        }

        pub(super) fn set_times(path: &Path, times: &FILE_BASIC_INFO) {
            let file = open(path);
            let info = FILE_BASIC_INFO {
                FileAttributes: 0,
                ..*times
            };
            // SAFETY: `info` is a valid FILE_BASIC_INFO of the size passed; the handle may write attributes.
            let ok = unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileBasicInfo,
                    (&raw const info).cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            };
            assert_ne!(ok, 0);
        }

        /// Marks `path` sparse: from then on, extending it or zeroing ranges allocates nothing for them.
        pub(super) fn make_sparse(path: &Path) {
            use windows_sys::Win32::System::IO::DeviceIoControl;
            use windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .unwrap();
            let mut returned = 0u32;
            // SAFETY: the handle is open for writing, and FSCTL_SET_SPARSE without a buffer sets the flag.
            let ok = unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    FSCTL_SET_SPARSE,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            assert_ne!(ok, 0);
        }

        /// The bytes allocated to `path`, as a handle to it reports them.
        pub(super) fn allocated(path: &Path) -> u64 {
            use windows_sys::Win32::Storage::FileSystem::{FILE_STANDARD_INFO, FileStandardInfo};
            let file = open(path);
            let mut info = FILE_STANDARD_INFO::default();
            // SAFETY: `info` is writable and sized as passed; the handle is open.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileStandardInfo,
                    (&raw mut info).cast(),
                    std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
                )
            };
            assert_ne!(ok, 0);
            u64::try_from(info.AllocationSize).unwrap()
        }
    }

    /// The bytes allocated to `path`.
    fn allocated(path: &Path) -> u64 {
        #[cfg(windows)]
        {
            win::allocated(path)
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(path).unwrap().blocks() * 512
        }
    }

    /// Between the checks and its delete, data is written into a hole of a sparse file, its length kept and its
    /// times set back. The file opened for deleting has the identity and the length listed, but not the space: it
    /// stays, and its space isn't counted as freed. A sparse file nothing touched still goes.
    #[test]
    fn a_sparse_file_filled_in_after_the_checks_is_left() {
        const LEN: u64 = 1 << 20;
        const DATA: usize = 64 * 1024;
        let fx = Fx::with(|p| {
            for name in ["sparse-filled.bin", "sparse-kept.bin"] {
                let path = p.cache.join(name);
                std::fs::write(&path, vec![0x5Au8; DATA]).unwrap();
                #[cfg(windows)]
                win::make_sparse(&path);
                // The rest of it is a hole: extending a sparse file allocates nothing.
                let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
                f.set_len(LEN).unwrap();
                let old = SystemTime::now() - Duration::from_secs(200 * 86_400);
                f.set_times(
                    std::fs::FileTimes::new()
                        .set_modified(old)
                        .set_accessed(old),
                )
                .unwrap();
            }
            // As the fixture does: the change times settle before the scan's threshold.
            std::thread::sleep(Duration::from_millis(3200));
        });
        let victim = fx.profile.cache.join("sparse-filled.bin");
        let untouched = fx.profile.cache.join("sparse-kept.bin");
        let before = allocated(&victim);
        assert!(
            before < LEN,
            "it is sparse: {before} of {LEN} bytes allocated"
        );
        let filled = Cell::new(false);
        let fill = |at: &Path| {
            if at == victim && !filled.get() {
                write_into_keeping_times(&victim, LEN / 2, DATA);
                filled.set(true);
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_file: Some(&fill),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        assert!(filled.get(), "the hook ran");
        assert_eq!(
            std::fs::metadata(&victim).map(|m| m.len()).ok(),
            Some(LEN),
            "it stays, at the length listed: {:#?}",
            report.steps
        );
        assert!(
            allocated(&victim) > before,
            "filling the hole took more space"
        );
        assert!(
            skips(&report)
                .iter()
                .any(|k| k.kind == SkipKind::Changed && Path::new(&k.path) == victim),
            "{:#?}",
            report.steps
        );
        assert!(
            !untouched.exists(),
            "the sparse file nothing touched goes: {:#?}",
            report.steps
        );
    }

    /// Between the checks and its delete, a file is rewritten in place to another length, its times set back. The
    /// file opened for deleting is the one listed (same identity) but not the length listed, so it stays.
    #[test]
    fn a_file_grown_after_the_checks_is_left() {
        let fx = Fx::new();
        let victim = fx.profile.cache.join("f_000003");
        let grown = Cell::new(false);
        let grow = |at: &Path| {
            if at == victim && !grown.get() {
                rewrite_keeping_times(&victim, 333 * 1024);
                grown.set(true);
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_file: Some(&grow),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        assert!(grown.get(), "the hook ran");
        assert_eq!(
            std::fs::metadata(&victim).unwrap().len(),
            333 * 1024,
            "it stays"
        );
        assert!(
            skips(&report)
                .iter()
                .any(|k| k.kind == SkipKind::Changed && Path::new(&k.path) == victim),
            "{:#?}",
            report.steps
        );
    }

    /// When a file's length and link count can't be read right before its delete, it isn't deleted: its size can't
    /// be confirmed, and freeing it couldn't be counted honestly.
    #[cfg(windows)]
    #[test]
    fn a_file_whose_size_and_links_cant_be_read_stays() {
        let fx = Fx::new();
        sys::test_hooks::set_fail_standard_info(true);
        let report = fx.run(cache(), &Hooks::default(), &CancelToken::new());
        sys::test_hooks::set_fail_standard_info(false);
        assert_eq!(report.freed_bytes, 0, "nothing counted as freed");
        assert_eq!(report.deleted_items, 0);
        assert!(fx.profile.cache.join("f_000001").exists());
        assert!(
            skips(&report)
                .iter()
                .any(|k| k.reason.contains("its size can't be read")),
            "{:#?}",
            report.steps
        );
    }

    /// On macOS a folder's volume is told by its mount as well as its device. When the mount can't be read, no
    /// folder on the way can be vouched for: every target is skipped, never taken as on the right volume.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_mount_that_cant_be_read_skips_everything() {
        let fx = Fx::new();
        // Planned while mounts can be read, so the plan holds the cache; run while they can't.
        let plan = crate::plan::plan(&fx.scanned, &fx.env, &[cache().to_owned()]).unwrap();
        let planned = plan.summary();
        assert!(
            planned.items > 0 && planned.refused.is_empty(),
            "{planned:#?}"
        );
        crate::fsid::test_hooks::set_fail_mount(true);
        let report = execute_inner(
            &plan,
            &fx.env,
            Sorts::default(),
            &|| None,
            &mut |_| {},
            &CancelToken::new(),
            &Hooks::default(),
        );
        crate::fsid::test_hooks::set_fail_mount(false);
        assert_eq!(report.deleted_items, 0, "{:#?}", report.steps);
        assert_eq!(report.freed_bytes, 0);
        assert!(fx.profile.cache.join("f_000001").exists());
        let skipped = skips(&report);
        assert!(
            !skipped.is_empty()
                && skipped
                    .iter()
                    .all(|k| k.reason.contains("which volume it is on can't be read")),
            "{:#?}",
            report.steps
        );
    }

    /// Right before a file of a cache folder goes, the folder holding the cache is moved into a cloud-sync folder,
    /// which the Guard protects. On Windows that rename is refused: every folder on the way is held without letting
    /// anyone rename it. On macOS it succeeds, and the delete stops at once: at most the file it was deleting goes,
    /// and the rest stays where it went.
    #[test]
    fn a_folder_moved_into_a_cloud_folder_mid_run_is_left_there() {
        let fx = Fx::new();
        let target = fx.profile.cache.clone();
        let holder = target.parent().unwrap().to_path_buf();
        let moved_to = fx.cloud().join(holder.file_name().unwrap());
        let files = std::fs::read_dir(&target).unwrap().count();
        let moved = Cell::new(None::<bool>);
        let move_it = |_: &Path| {
            if moved.get().is_none() {
                moved.set(Some(std::fs::rename(&holder, &moved_to).is_ok()));
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_file: Some(&move_it),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        let moved = moved.get().expect("the hook ran");
        if cfg!(windows) {
            assert!(
                !moved,
                "Windows refuses to rename a folder on a chain Vacancy holds"
            );
        }
        if moved {
            let there = moved_to.join(target.file_name().unwrap());
            let left = std::fs::read_dir(&there).unwrap().count();
            assert!(
                left + 1 >= files,
                "at most the file in flight went: {left} of {files} left"
            );
            assert!(
                skips(&report)
                    .iter()
                    .any(|k| k.kind == SkipKind::Changed
                        && k.reason.contains("moved during the run")),
                "{:#?}",
                report.steps
            );
        } else {
            assert_eq!(
                std::fs::read_dir(fx.cloud()).unwrap().count(),
                0,
                "nothing went into the cloud folder"
            );
        }
    }

    /// A file an app opens after the step began is seen at the start of its folder's batch (macOS reads again what
    /// apps hold open), and skipped as in use.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_opened_after_the_step_began_is_skipped_as_in_use() {
        let fx = Fx::new();
        let victim = fx.profile.cache.join("f_000004");
        let held = std::cell::RefCell::new(None);
        let open_it = |_: &Path| {
            if held.borrow().is_none() {
                *held.borrow_mut() = Some(std::fs::File::open(&victim).unwrap());
                std::thread::sleep(BUSY_EVERY + Duration::from_millis(100));
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_delete: Some(&open_it),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        assert!(victim.exists(), "the open file stays");
        assert!(
            skips(&report)
                .iter()
                .any(|k| k.kind == SkipKind::InUse && Path::new(&k.path) == victim),
            "{:#?}",
            report.steps
        );
    }

    /// A file an app opens in the moment before it is deleted (after the last read of what apps hold open) is
    /// deleted, but its space isn't counted as freed: the app still holds it, so it comes back only when the app
    /// lets go. (macOS; on Windows any open handle makes the delete's exclusive open fail.)
    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_held_open_when_deleted_isnt_counted_as_freed() {
        let fx = Fx::new();
        let victim = fx.profile.cache.join("f_000005");
        let bytes = std::fs::symlink_metadata(&victim).unwrap();
        let bytes = {
            use std::os::unix::fs::MetadataExt;
            bytes.blocks() * 512
        };
        let held = std::cell::RefCell::new(None);
        let open_it = |at: &Path| {
            if at == victim {
                *held.borrow_mut() = Some(std::fs::File::open(&victim).unwrap());
            }
        };
        let report = fx.run(
            cache(),
            &Hooks {
                before_file: Some(&open_it),
                ..Hooks::default()
            },
            &CancelToken::new(),
        );
        assert!(held.borrow().is_some(), "the hook ran");
        let note = skips(&report)
            .into_iter()
            .find(|k| k.kind == SkipKind::HeldOpen)
            .unwrap_or_else(|| panic!("{:#?}", report.steps));
        assert_eq!(note.bytes, bytes);
        let step = &report.steps[0];
        assert_eq!(
            step.outcome,
            Outcome::Done,
            "a note, not a failure: {step:#?}"
        );
        assert!(step.freed_bytes + bytes <= step.planned_bytes, "{step:#?}");
    }

    #[test]
    fn the_threshold_is_the_scan_start_to_the_second_less_the_margin() {
        let at = UNIX_EPOCH + Duration::from_millis(10_700);
        assert_eq!(threshold(at), 8_000_000_000);
        assert_eq!(threshold(UNIX_EPOCH), 0);
    }
}

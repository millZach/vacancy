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
use crate::plan::{Action, Plan, Step as PlanStep, Target, denied_reason, next_id};
use crate::report::{CategoryFreed, Outcome, RunReport, Skip, SkipKind, StepReport};
use crate::scan::CancelToken;
use crate::text::group_digits;
use crate::volume::VolumeInfo;

#[cfg(unix)]
#[path = "exec/unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "exec/windows.rs"]
mod sys;
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
    execute_inner(plan, env, figures, on_event, cancel, &Hooks::default())
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
    /// The later of when it was last modified and when it last changed (renamed, moved, given a new link, its
    /// attributes set), in nanoseconds since the Unix epoch.
    pub(crate) changed: i128,
    /// Its hard links, when the listing says (Unix).
    pub(crate) links: Option<u64>,
}

impl Entry {
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
    Unsupported,
    /// Anything else, with what the OS said.
    Failed(String),
}

/// How a reason is said of one thing and of several ("is in use", "are in use").
struct Said {
    kind: SkipKind,
    one: String,
    many: String,
}

impl Fail {
    fn said(&self) -> Said {
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
    figures: &dyn Fn() -> Option<VolumeInfo>,
    on_event: &mut dyn FnMut(RunEvent),
    cancel: &CancelToken,
    hooks: &Hooks<'_>,
) -> RunReport {
    let started = Instant::now();
    let started_at = SystemTime::now();
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
    let after = figures();
    report(plan, steps, started, started_at, before, after, cancel)
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
    total_freed: u64,
    last_progress: Instant,
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

    fn fail(&mut self, fail: &Fail, path: &Path, bytes: u64, files: u64) {
        self.leave(fail.said(), path, bytes, files);
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

impl Run<'_, '_> {
    fn step(&mut self, plan: &Plan, env: &Env, index: u32, step: &PlanStep) -> StepReport {
        let mut tally = Tally::default();
        for (skip, _) in &step.refused {
            self.skipped(index, &mut tally, skip.clone());
        }
        // Built afresh for every step, from the places as they are now.
        let guard = Guard::new(env, plan.volume_root());
        let busy = sys::Busy::now();
        for action in &step.actions {
            if self.cancel.is_cancelled() {
                let skip = Skip {
                    path: action.path.display().to_string(),
                    reason: "You stopped the run before it got here".into(),
                    kind: SkipKind::Cancelled,
                    bytes: action.bytes,
                    items: action.files,
                };
                self.skipped(index, &mut tally, skip);
                continue;
            }
            tally.action = Counts::default();
            tally.links = 0;
            tally.left.clear();
            if let Err(whole) = self.action(&guard, step, action, index, &busy, &mut tally) {
                let skip = Skip {
                    path: action.path.display().to_string(),
                    reason: whole.reason,
                    kind: whole.kind,
                    bytes: action.bytes,
                    items: action.files,
                };
                self.skipped(index, &mut tally, skip);
                continue;
            }
            let done = tally.action;
            tally.step.freed += done.freed;
            tally.step.deleted += done.deleted;
            tally.action = Counts::default();
            if done.deleted > 0 || tally.links > 0 || tally.left.is_empty() {
                (self.on_event)(RunEvent::Deleted(Deleted {
                    step: index,
                    path: action.path.display().to_string(),
                    bytes: done.freed,
                    items: done.deleted,
                }));
            }
            for left in std::mem::take(&mut tally.left) {
                let skip = left_skip(action, &left);
                self.skipped(index, &mut tally, skip);
            }
            self.progress(index, &tally, &action.path, true);
        }
        step_report(step, tally)
    }

    fn skipped(&mut self, index: u32, tally: &mut Tally, skip: Skip) {
        (self.on_event)(RunEvent::Skipped(Skipped {
            step: index,
            skip: skip.clone(),
        }));
        tally.skips.push(skip);
    }

    fn progress(&mut self, index: u32, tally: &Tally, at: &Path, now: bool) {
        if !now && self.last_progress.elapsed() < PROGRESS_EVERY {
            return;
        }
        self.last_progress = Instant::now();
        let freed = tally.step.freed + tally.action.freed;
        (self.on_event)(RunEvent::Progress(RunProgress {
            step: index,
            freed_bytes: freed,
            deleted_items: tally.step.deleted + tally.action.deleted,
            total_freed_bytes: self.total_freed + freed,
            current_path: at.display().to_string(),
        }));
    }

    /// Carries out one action: the fresh walk, the Guard, the fingerprint, then the delete. `Err` when it is
    /// skipped whole, before anything in it was deleted.
    fn action(
        &mut self,
        guard: &Guard,
        step: &PlanStep,
        action: &Action,
        index: u32,
        busy: &sys::Busy,
        tally: &mut Tally,
    ) -> Result<(), Whole> {
        let path = &action.path;
        let name = path
            .file_name()
            .ok_or_else(|| Whole::new(SkipKind::Failed, "Its path has no name"))?;
        let parent = sys::walk_to_parent(path).map_err(|f| Whole::of("The way to it", f))?;
        let (parent_dir, parent_steps) = (&parent.dir, &parent.steps);
        let volume = parent_steps.last().map_or(0, |s| s.id.volume());
        match &action.target {
            Target::File => {
                let entry = sys::entry(parent_dir, name).map_err(|f| Whole::of("It", f))?;
                let mut chain = parent_steps.clone();
                chain.push(entry.step(volume));
                judge(guard, path, &chain, false)?;
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
                if entry.alloc != action.bytes {
                    return Err(Whole::new(
                        SkipKind::Changed,
                        "Its size changed since the scan",
                    ));
                }
                if let Some(hook) = self.hooks.before_delete {
                    hook(path);
                }
                self.delete_file(parent_dir, path, &entry, busy, false, tally);
                Ok(())
            }
            Target::Loose { kept } => {
                let (dir, target) =
                    sys::open_dir(parent_dir, name, None, false).map_err(|f| Whole::of("It", f))?;
                let mut chain = parent_steps.clone();
                chain.push(target);
                judge(guard, path, &chain, true)?;
                same_identity(&chain, action)?;
                self.loose(&dir, path, action, kept, index, busy, tally)
            }
            Target::Dir => {
                let (dir, target) =
                    sys::open_dir(parent_dir, name, None, true).map_err(|f| Whole::of("It", f))?;
                let mut chain = parent_steps.clone();
                chain.push(target.clone());
                judge(guard, path, &chain, false)?;
                same_identity(&chain, action)?;
                if sys::changed(&dir).map_err(|f| Whole::of("It", f))? >= self.threshold {
                    return Err(Whole::new(SkipKind::Changed, "It changed since the scan"));
                }
                self.verify_tree(guard, step, &dir, &target, path, action)?;
                if let Some(hook) = self.hooks.before_delete {
                    hook(path);
                }
                self.delete_tree(parent_dir, dir, &target, path, index, busy, tally);
                Ok(())
            }
        }
    }

    /// Checks a folder target whole before anything in it goes: every entry of every folder in it unchanged since
    /// the scan, every folder the one its parent listed, on the target's volume and mount, nothing protected inside,
    /// and its size as the scan measured it. See the module docs.
    fn verify_tree(
        &self,
        guard: &Guard,
        step: &PlanStep,
        root: &sys::Dir,
        root_step: &Step,
        path: &Path,
        action: &Action,
    ) -> Result<(), Whole> {
        let mut measure = Measure::default();
        let entries = sys::list(root).map_err(|f| Whole::of("Its contents", f))?;
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
            match e.kind {
                Kind::File => measure.add(&e),
                Kind::Link | Kind::Special(_) => {}
                Kind::Dir => {
                    let inside = |f: Fail| Whole::of(&format!("{} inside it", at.display()), f);
                    let parent = dir.as_ref().unwrap_or(root);
                    let (sub_dir, sub) =
                        sys::open_dir(parent, &e.name, Some(&e), false).map_err(inside)?;
                    if !same_volume(root_step, &sub) {
                        return Err(Whole::new(
                            SkipKind::OtherVolume,
                            format!("Another volume is mounted inside it, at {}", at.display()),
                        ));
                    }
                    if let Some(what) = guard.protects(sub.id) {
                        return Err(Whole::new(
                            SkipKind::Protected,
                            format!("It holds {what}, which Vacancy never touches"),
                        ));
                    }
                    if let Some((what, app)) = guard.protected_os_name(&e.name)
                        && !(app && step.takes_built_apps)
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
                    stack.push((Some(sub_dir), at, entries, 0));
                }
            }
        }
        measure.check(action)
    }

    /// Deletes a verified folder target, depth first, relative to the open folders. See the module docs.
    #[allow(clippy::too_many_arguments)]
    fn delete_tree(
        &mut self,
        parent: &sys::Dir,
        root: sys::Dir,
        root_step: &Step,
        path: &Path,
        index: u32,
        busy: &sys::Busy,
        tally: &mut Tally,
    ) {
        let entries = match sys::list(&root) {
            Ok(entries) => entries,
            Err(fail) => return tally.fail(&fail, path, 0, 0),
        };
        let mut stack: Vec<Frame> = vec![Frame {
            dir: root,
            step: root_step.clone(),
            path: path.to_path_buf(),
            entries,
            next: 0,
        }];
        let mut stopped = false;
        while let Some(top) = stack.last_mut() {
            let next = if stopped {
                None
            } else {
                top.entries.get(top.next).cloned()
            };
            let Some(e) = next else {
                // Done with this folder: it goes if it is empty now. One still holding something left in place
                // stays, explained by what it holds. After a cancel nothing more is removed.
                let frame = stack.pop().expect("a frame was on the stack");
                if !stopped {
                    let above = stack.last().map_or(parent, |f| &f.dir);
                    match sys::remove_dir(above, frame.dir, &frame.step) {
                        Ok(()) | Err(Fail::NotEmpty | Fail::Gone) => {}
                        Err(fail) => tally.fail(&fail, &frame.path, 0, 0),
                    }
                }
                continue;
            };
            top.next += 1;
            let at = top.path.join(&e.name);
            if self.cancel.is_cancelled() {
                // Stop after the file just deleted. What is left in this folder (this entry on) is counted; what
                // is left in the folders above is unknown in files, and explained by the cancel.
                stopped = true;
                let left = (top.entries.len() + 1 - top.next) as u64;
                tally.leave(Said::cancelled(), &top.path, 0, left);
                continue;
            }
            if e.changed >= self.threshold {
                let file = u64::from(e.kind == Kind::File);
                tally.leave(Said::changed(), &at, e.alloc, file);
                continue;
            }
            match e.kind {
                Kind::File => {
                    if let Some(hook) = self.hooks.before_file {
                        hook(&at);
                    }
                    let dir = &stack.last().expect("a frame is on the stack").dir;
                    self.delete_file(dir, &at, &e, busy, true, tally);
                }
                Kind::Link => match sys::delete_link(&top.dir, &e, self.threshold) {
                    Ok(()) => tally.links += 1,
                    Err(Fail::Gone) => {}
                    Err(fail) => tally.fail(&fail, &at, 0, 0),
                },
                Kind::Special(what) => tally.leave(Said::special(what), &at, e.alloc, 0),
                Kind::Dir => {
                    let threshold = self.threshold;
                    let opened =
                        sys::open_dir(&top.dir, &e.name, Some(&e), true).and_then(|(dir, sub)| {
                            if !same_volume(root_step, &sub) {
                                return Err(Fail::OtherVolume);
                            }
                            if sys::changed(&dir)? >= threshold {
                                return Err(Fail::Changed);
                            }
                            let entries = sys::list(&dir)?;
                            Ok((dir, sub, entries))
                        });
                    match opened {
                        Ok((dir, step, entries)) => stack.push(Frame {
                            dir,
                            step,
                            path: at.clone(),
                            entries,
                            next: 0,
                        }),
                        Err(Fail::Gone) => {}
                        Err(fail) => tally.fail(&fail, &at, 0, 0),
                    }
                }
            }
            self.progress(index, tally, &at, false);
        }
    }

    /// Deletes one file `e` in the open folder `dir`, tallying what it freed or why it stayed. With
    /// `inside_target`, `dir` is a folder being deleted, which may be made writable to let its files go (macOS).
    fn delete_file(
        &mut self,
        dir: &sys::Dir,
        at: &Path,
        e: &Entry,
        busy: &sys::Busy,
        inside_target: bool,
        tally: &mut Tally,
    ) {
        match sys::delete_file(dir, e, self.threshold, busy, inside_target) {
            Ok(Some(freed)) => {
                tally.action.freed += freed;
                tally.action.deleted += 1;
            }
            Ok(None) => tally.leave(Said::hard_linked(), at, e.alloc, 1),
            Err(Fail::Gone) => {}
            Err(fail) => tally.fail(&fail, at, e.alloc, 1),
        }
    }

    /// Deletes a folder's loose files: those the scan saw (not kept individually, unchanged since), within the
    /// scan's totals of them. Files new or changed since are left alone. The folder itself stays.
    #[allow(clippy::too_many_arguments)]
    fn loose(
        &mut self,
        dir: &sys::Dir,
        path: &Path,
        action: &Action,
        kept: &[OsString],
        index: u32,
        busy: &sys::Busy,
        tally: &mut Tally,
    ) -> Result<(), Whole> {
        let entries = sys::list(dir).map_err(|f| Whole::of("Its files", f))?;
        let mut measure = Measure::default();
        let old: Vec<Entry> = entries
            .into_iter()
            .filter(|e| e.kind == Kind::File && !kept.iter().any(|k| k == &e.name))
            .filter(|e| e.changed < self.threshold)
            .inspect(|e| measure.add(e))
            .collect();
        let (single_bytes, single_files) = measure.single();
        if single_bytes > action.bytes || single_files > action.files {
            return Err(Whole::new(
                SkipKind::Changed,
                "Its files changed since the scan: there are more of them than the scan saw",
            ));
        }
        if let Some(hook) = self.hooks.before_delete {
            hook(path);
        }
        for (i, e) in old.iter().enumerate() {
            let at = path.join(&e.name);
            if self.cancel.is_cancelled() {
                let bytes = old[i..].iter().map(|e| e.alloc).sum();
                tally.leave(Said::cancelled(), path, bytes, (old.len() - i) as u64);
                break;
            }
            if let Some(hook) = self.hooks.before_file {
                hook(&at);
            }
            self.delete_file(dir, &at, e, busy, false, tally);
            self.progress(index, tally, &at, false);
        }
        // The files the scan saw that aren't there unchanged now.
        let missing = action.files.saturating_sub(measure.files);
        if missing > 0 {
            tally.leave(
                Said {
                    kind: SkipKind::Changed,
                    one: "changed or was removed since the scan".into(),
                    many: "changed or were removed since the scan".into(),
                },
                path,
                action.bytes.saturating_sub(measure.bytes),
                missing,
            );
        }
        Ok(())
    }
}

/// A folder being deleted: open with the right to delete it, its step (identity, and name in the folder above),
/// and its listing.
struct Frame {
    dir: sys::Dir,
    step: Step,
    path: PathBuf,
    entries: Vec<Entry>,
    next: usize,
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

    /// Bytes and files with no other name, as far as is known: all of them where link counts aren't known
    /// (Windows' listing doesn't give them, so a hard-linked file counts as having one name, which errs towards
    /// refusing).
    fn single(&self) -> (u64, u64) {
        if self.links_known {
            (self.single_bytes, self.single_files)
        } else {
            (self.bytes, self.files)
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
    Whole::new(
        SkipKind::Changed,
        format!(
            "It changed since the scan: {} was modified, added or moved",
            rel.display()
        ),
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
    let planned_bytes = step.actions.iter().map(|a| a.bytes).sum::<u64>()
        + step.refused.iter().map(|(s, _)| s.bytes).sum::<u64>();
    let planned_items = step.actions.iter().map(|a| a.files).sum::<u64>()
        + step.refused.iter().map(|(_, files)| files).sum::<u64>();
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
    let outcome = if skipped.iter().all(|k| k.kind == SkipKind::HardLinked) {
        Outcome::Done
    } else if tally.step.deleted > 0 {
        Outcome::Partial
    } else {
        Outcome::Skipped
    };
    StepReport {
        id: step.id.clone(),
        title: step.title.clone(),
        category: step.category,
        regenerable: step.regenerable,
        planned_bytes,
        freed_bytes: tally.step.freed,
        planned_items,
        deleted_items: tally.step.deleted,
        places: (step.actions.len() + step.refused.len()) as u64,
        outcome,
        skipped,
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
        skipped_items: steps.iter().flat_map(|s| &s.skipped).map(|k| k.items).sum(),
        freed_by_category: by_category,
        total_bytes,
        used_before,
        used_after,
        free_before,
        free_after,
        free_change,
        biggest_win,
        left_for_you: plan.left_for_you().to_vec(),
        steps,
    }
}

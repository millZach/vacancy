//! The rules: a declarative catalog per platform (see `rules/catalog.rs`) that turns a scan into cleanup
//! proposals with concrete targets.
//!
//! Each rule names its category, risk, whether what it removes comes back on its own, how it finds things (known
//! places from the [`Env`], folder names, project folders, old files), what it leaves alone, and its action kind.
//! Every target passes the [Guard](crate::Guard) twice over: the path on its own, and what the scan saw in and
//! around it. A target that is a link, or is, is inside or holds a protected place, or holds a folder the scanner
//! couldn't read or found changed (its size is unknown) is dropped. A link deeper inside a target is kept, since
//! pnpm's `node_modules` are full of them, and counted as nothing: the executor (ticket 04) deletes a link itself,
//! never what it points at. Targets never overlap: a place one rule took is not offered by a later rule, and
//! catalogs list the specific before the general.
//!
//! Sizes and counts come from the scan; nothing is estimated. Nothing here changes the disk; the only questions
//! asked of it are the long forms of 8.3 short names.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use ts_rs::TS;

use crate::categorize::{CHECK_EVERY, Categories, CategoryId, Env, ItemRef, is_library_folder};
use crate::guard::Guard;
use crate::pathkey::{Case, PathKey, name_key, name_key_in};
use crate::snapshot::{DirFlags, DirId, FileId, Markers, Snapshot, UnixTime};
use crate::text::group_digits;

mod catalog;
mod running;

use catalog::{Find, Proof, Rule};

/// How careful to be with a proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Risk {
    /// Comes back on its own or was meant to be thrown away. On by default.
    Safe,
    /// Probably fine, but worth a look. Off by default unless the rule says otherwise.
    Review,
    /// Only if you're sure. Off by default.
    Careful,
}

/// Where a proposal came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ProposalSource {
    /// A built-in rule.
    Rule,
    /// The user's AI agent (ticket 07).
    Ai,
}

/// What carrying out a proposal does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ProposalAction {
    /// Deletes the targets, permanently.
    Delete,
    /// Runs a tool (docker prune, simctl, brew cleanup). Ticket 06.
    Tool,
    /// Advice the user carries out; never counted as freed space. Ticket 06.
    Hint,
}

/// How a rule acts on what it finds: the catalog's action kind. The UI sees [`ProposalAction`]; this also says
/// what a delete takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// Deletes what is inside each place found, and keeps the place itself.
    DeleteContents,
    /// Deletes each place found, whole: a folder with everything in it, or a file.
    DeleteDirectory,
    /// Runs a tool. Ticket 06.
    Tool,
    /// Advice only. Ticket 06.
    Hint,
}

/// A cleanup Vacancy proposes, as the plan tray and the map show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Proposal {
    /// Stable for one scan: the rule's id for rules.
    pub id: String,
    /// The category its tile is in.
    pub category: CategoryId,
    /// What it is, in a few words ("node_modules in 12 idle projects").
    pub title: String,
    /// One line with the numbers.
    pub summary: String,
    /// Why it's safe to remove, and what to know first.
    pub why: String,
    /// Allocated bytes it frees, from the scan.
    #[ts(type = "number")]
    pub bytes: u64,
    /// Files it removes.
    #[ts(type = "number")]
    pub item_count: u64,
    /// Places it removes them from: folders, files and folders' loose files.
    #[ts(type = "number")]
    pub place_count: u64,
    /// How careful to be.
    pub risk: Risk,
    /// Who proposed it.
    pub source: ProposalSource,
    /// What the AI agent said, for AI proposals.
    pub ai_note: Option<String>,
    /// What carrying it out does.
    pub action: ProposalAction,
    /// It comes back on its own if needed (caches, build output).
    pub regenerable: bool,
    /// A few of the places it removes, biggest first, as real paths.
    pub sample_paths: Vec<String>,
    /// Whether a new plan includes it.
    pub default_on: bool,
}

/// What is true now that the rules depend on: the time (for idle projects and old files) and which apps are
/// running (macOS caches of running apps are left alone).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moment {
    /// Seconds since the Unix epoch.
    pub now: UnixTime,
    /// Running apps' names and bundle ids, as the OS reports them.
    pub running_apps: Vec<String>,
}

impl Moment {
    /// The clock, and the apps running now (macOS; none are looked for on Windows).
    pub fn detect() -> Self {
        Self {
            now: unix_now(),
            running_apps: running::apps(),
        }
    }
}

fn unix_now() -> UnixTime {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as UnixTime)
        .unwrap_or(0)
}

/// The proposals for one scan, with what each one would remove.
#[derive(Clone, Debug, Default)]
pub struct Proposals {
    list: Vec<Proposal>,
    targets: Vec<Vec<ItemRef>>,
    kinds: Vec<ActionKind>,
}

impl Proposals {
    /// Every proposal, biggest first.
    pub fn list(&self) -> &[Proposal] {
        &self.list
    }

    /// What proposal `index` removes: folders, kept files, and folders' loose files. Never overlapping another
    /// proposal's.
    pub fn targets(&self, index: usize) -> &[ItemRef] {
        &self.targets[index]
    }

    /// How proposal `index` acts.
    pub fn kind(&self, index: usize) -> ActionKind {
        self.kinds[index]
    }

    /// The index of the proposal with this id.
    pub fn position(&self, id: &str) -> Option<usize> {
        self.list.iter().position(|p| p.id == id)
    }

    /// The sum of what the delete proposals free.
    pub fn reclaimable_bytes(&self) -> u64 {
        self.list
            .iter()
            .filter(|p| p.action == ProposalAction::Delete)
            .map(|p| p.bytes)
            .sum()
    }
}

/// The proposals the platform's rules make for `snapshot`, which `categories` sorted with the same `env`.
pub fn propose(
    snapshot: &Snapshot,
    categories: &Categories,
    env: &Env,
    moment: &Moment,
) -> Proposals {
    propose_until(snapshot, categories, env, moment, &|| false)
        .expect("proposing that is never stopped finishes")
}

/// [`propose`], giving up (`None`) as soon as `stop` says so. It is asked every [`CHECK_EVERY`] folders.
pub(crate) fn propose_until(
    snapshot: &Snapshot,
    categories: &Categories,
    env: &Env,
    moment: &Moment,
    stop: &dyn Fn() -> bool,
) -> Option<Proposals> {
    let guard = Guard::new(env, &env.root);
    let guarded = Guarded::new(snapshot, categories, guard, stop)?;
    let k = &env.known;
    let mut personal: Vec<&Path> = vec![
        &env.root,
        &env.home,
        &env.local_app_data,
        &env.app_data,
        &k.documents,
        &k.desktop,
        &k.pictures,
        &k.videos,
        &k.music,
    ];
    personal.extend(env.home.parent());
    let app_data = env.home.join("AppData");
    let library = env.home.join("Library");
    personal.push(&app_data);
    personal.push(&library);
    let mut personal: Vec<(PathKey, bool)> = personal
        .into_iter()
        .filter_map(PathKey::of)
        .map(|k| (k, false))
        .collect();
    personal.extend(PathKey::of(&k.downloads).map(|k| (k, true)));
    let mut engine = Engine {
        snap: snapshot,
        case: env.case,
        guarded,
        personal,
        claims: Claims::new(snapshot.dir_count()),
        moment,
        running: moment
            .running_apps
            .iter()
            .map(|a| a.to_lowercase())
            .collect(),
        stop,
        checks: 0,
    };
    let mut out = Proposals::default();
    for rule in catalog::rules(env) {
        let found = engine.run(&rule)?;
        if found.bytes == 0 {
            continue;
        }
        out.list.push(proposal(snapshot, &rule, &found));
        out.targets
            .push(found.targets.into_iter().map(|t| t.at).collect());
        out.kinds.push(rule.action);
    }
    // Biggest first; the targets travel with their proposal.
    let mut order: Vec<usize> = (0..out.list.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(out.list[i].bytes));
    Some(Proposals {
        list: order.iter().map(|&i| out.list[i].clone()).collect(),
        targets: order.iter().map(|&i| out.targets[i].clone()).collect(),
        kinds: order.iter().map(|&i| out.kinds[i]).collect(),
    })
}

/// A target and its numbers.
#[derive(Clone, Debug)]
struct Target {
    at: ItemRef,
    bytes: u64,
    files: u64,
    /// What it belongs to, for the proposal's text: a browser, a package manager, an app's folder.
    label: Option<String>,
}

/// What one rule found.
#[derive(Debug, Default)]
struct Found {
    targets: Vec<Target>,
    bytes: u64,
    files: u64,
}

/// The Guard, with what the scan saw: which folders are protected, and which hold something protected.
struct Guarded<'a> {
    guard: Guard,
    snap: &'a Snapshot,
    /// The folder is protected (the OS, a Photos library, a cloud-sync folder, an app bundle...), or is inside one.
    under: Vec<bool>,
    /// The folder holds, somewhere below it, a protected folder other than an app bundle, or a folder whose contents
    /// are unknown.
    holds: Vec<bool>,
    /// The folder holds a macOS app bundle somewhere below it: only build-output rules may take it whole.
    holds_app: Vec<bool>,
}

impl<'a> Guarded<'a> {
    fn new(
        snap: &'a Snapshot,
        cats: &Categories,
        guard: Guard,
        stop: &dyn Fn() -> bool,
    ) -> Option<Self> {
        let n = snap.dir_count();
        let mut under = vec![false; n];
        let mut holding = vec![false; n];
        let mut holding_app = vec![false; n];
        for dir in snap.dirs() {
            if dir.index() % CHECK_EVERY == 0 && stop() {
                return None;
            }
            let flags = snap.flags(dir);
            let by_scan = cats.category_of(dir).protected() || flags.contains(DirFlags::CLOUD);
            let by_name = snap
                .parent(dir)
                .and_then(|_| guard.protected_os_name(snap.name(dir)));
            // A folder the scanner couldn't read, found changed, or that is another volume: nobody knows what is
            // in it, so nothing that holds it is offered. (A link is fine: deleting it never touches its target.)
            let unknown = !flags.entered() && !flags.contains(DirFlags::LINK);
            holding[dir.index()] = by_scan || unknown || by_name.is_some_and(|(_, app)| !app);
            holding_app[dir.index()] = by_name.is_some_and(|(_, app)| app);
            under[dir.index()] =
                by_scan || by_name.is_some() || snap.parent(dir).is_some_and(|p| under[p.index()]);
        }
        let mut holds = vec![false; n];
        let mut holds_app = vec![false; n];
        for dir in snap.dirs().rev() {
            if dir.index() % CHECK_EVERY == 0 && stop() {
                return None;
            }
            if let Some(p) = snap.parent(dir) {
                let i = dir.index();
                if holding[i] || holds[i] {
                    holds[p.index()] = true;
                }
                if holding_app[i] || holds_app[i] {
                    holds_app[p.index()] = true;
                }
            }
        }
        Some(Self {
            guard,
            snap,
            under,
            holds,
            holds_app,
        })
    }

    /// Whether `at` may be deleted: the Guard allows its path, and nothing the scan saw in or around it is
    /// protected, a link, or unread.
    /// `build_output` says the rule takes build output or dependencies, which may hold the apps they build.
    fn allows(&self, at: ItemRef, build_output: bool) -> bool {
        let snap = self.snap;
        match at {
            ItemRef::Dir(d) => {
                let flags = snap.flags(d);
                flags.entered()
                    && !flags.contains(DirFlags::LINK)
                    && !self.under[d.index()]
                    && !self.holds[d.index()]
                    && (build_output || !self.holds_app[d.index()])
                    && PathKey::of(&snap.path(d)).is_some_and(|k| self.guard.check_key(&k).is_ok())
            }
            ItemRef::File(f) => {
                let parent = snap.file(f).parent;
                !self.under[parent.index()]
                    && PathKey::of(&snap.file_path(f))
                        .is_some_and(|k| self.guard.check_key(&k).is_ok())
            }
            ItemRef::FilesIn(d) => {
                // Judged as one file directly in the folder: the folder itself may be a place that must stay.
                !self.under[d.index()]
                    && PathKey::of(&snap.path(d))
                        .is_some_and(|k| self.guard.check_key(&k.child(LOOSE_FILES)).is_ok())
            }
            ItemRef::Proposal(_) => false,
        }
    }
}

/// The key a folder's loose files are judged under: a NUL, which no real name holds.
const LOOSE_FILES: &str = "\0";

/// Which places rules have taken, so no two proposals overlap.
struct Claims {
    dir: Vec<bool>,
    /// Something inside the folder (a subfolder, a kept file, its loose files) is taken.
    below: Vec<bool>,
    files: HashSet<FileId>,
    loose: HashSet<DirId>,
}

impl Claims {
    fn new(dirs: usize) -> Self {
        Self {
            dir: vec![false; dirs],
            below: vec![false; dirs],
            files: HashSet::new(),
            loose: HashSet::new(),
        }
    }

    /// Whether `dir` or a folder above it is taken.
    fn taken_from(&self, snap: &Snapshot, dir: DirId) -> bool {
        let mut at = Some(dir);
        while let Some(d) = at {
            if self.dir[d.index()] {
                return true;
            }
            at = snap.parent(d);
        }
        false
    }

    fn overlaps(&self, snap: &Snapshot, at: ItemRef) -> bool {
        match at {
            ItemRef::Dir(d) => self.below[d.index()] || self.taken_from(snap, d),
            ItemRef::File(f) => {
                self.files.contains(&f) || self.taken_from(snap, snap.file(f).parent)
            }
            ItemRef::FilesIn(d) => self.loose.contains(&d) || self.taken_from(snap, d),
            ItemRef::Proposal(_) => true,
        }
    }

    fn take(&mut self, snap: &Snapshot, at: ItemRef) {
        let mut up = match at {
            ItemRef::Dir(d) => {
                self.dir[d.index()] = true;
                snap.parent(d)
            }
            ItemRef::File(f) => {
                self.files.insert(f);
                Some(snap.file(f).parent)
            }
            ItemRef::FilesIn(d) => {
                self.loose.insert(d);
                Some(d)
            }
            ItemRef::Proposal(_) => None,
        };
        while let Some(d) = up {
            if self.below[d.index()] {
                break; // already marked from here up
            }
            self.below[d.index()] = true;
            up = snap.parent(d);
        }
    }
}

struct Engine<'a> {
    snap: &'a Snapshot,
    /// How the volume compares names: places are found by their exact spelling on a case-sensitive one.
    case: Case,
    guarded: Guarded<'a>,
    /// Places no rule sweeps whole, nor anything holding them (see [`Engine::holds_personal`]), each marked if it
    /// is the Downloads folder.
    personal: Vec<(PathKey, bool)>,
    claims: Claims,
    moment: &'a Moment,
    /// Running apps, lowercased.
    running: Vec<String>,
    stop: &'a dyn Fn() -> bool,
    checks: usize,
}

/// Something a rule turned up: the place, what it belongs to (for the text), and for rules that skip running apps,
/// the folder its owner is named in.
type Candidate = (ItemRef, Option<String>, Option<DirId>);

/// A place a rule looks in, and what to call what it finds there.
struct Spot {
    dir: DirId,
    label: Option<String>,
    /// For rules that skip running apps: the folder whose subfolders are named after apps.
    owners_in: Option<DirId>,
}

impl Engine<'_> {
    /// Runs one rule: finds its candidates, drops what its exclusions, the Guard or an earlier rule rule out, and
    /// takes the rest. `None` if stopped.
    fn run(&mut self, rule: &Rule) -> Option<Found> {
        let mut found = Found::default();
        if rule
            .skip
            .unless_running
            .iter()
            .any(|app| self.running.iter().any(|r| r == &app.to_lowercase()))
        {
            return Some(found);
        }
        let candidates = self.candidates(rule)?;
        for (at, label, owners_in) in candidates {
            self.tick()?;
            if !self.wanted(rule, at, owners_in) || !self.guarded.allows(at, rule.build_output) {
                continue;
            }
            if self.claims.overlaps(self.snap, at) {
                continue;
            }
            let t = self.target(at, label);
            if t.bytes == 0 && t.files == 0 {
                continue;
            }
            self.claims.take(self.snap, at);
            found.bytes += t.bytes;
            found.files += t.files;
            found.targets.push(t);
        }
        found.targets.sort_by_key(|t| std::cmp::Reverse(t.bytes));
        Some(found)
    }

    /// Everything the rule's finder turns up, as targets with a label and (for rules that skip running apps) the
    /// folder their owner is named in.
    fn candidates(&mut self, rule: &Rule) -> Option<Vec<Candidate>> {
        let mut spots: Vec<Spot> = Vec::new();
        let mut direct: Vec<Candidate> = Vec::new();
        match &rule.find {
            Find::Places(places) => {
                for place in places {
                    for (dir, owners_in) in self.locate_pattern(&place.path) {
                        // Unlabelled places found through a `*` are named after the folder the `*` stood for.
                        let label = place
                            .label
                            .map(str::to_owned)
                            .or_else(|| owners_in.map(|base| self.owner(base, dir)));
                        spots.push(Spot {
                            dir,
                            label,
                            owners_in,
                        });
                    }
                }
            }
            Find::Named {
                names,
                under,
                prune,
            } => {
                let prune: Vec<DirId> = prune.iter().filter_map(|p| self.locate(p)).collect();
                for base in under {
                    let Some(base_dir) = self.locate(base) else {
                        continue;
                    };
                    for dir in self.named_below(base_dir, names, &prune)? {
                        spots.push(Spot {
                            dir,
                            label: Some(self.owner(base_dir, dir)),
                            owners_in: Some(base_dir),
                        });
                    }
                }
            }
            Find::Projects {
                folder,
                project,
                proof,
                idle_days,
                under,
                prune,
            } => {
                let prune: Vec<DirId> = prune.iter().filter_map(|p| self.locate(p)).collect();
                for base in under {
                    let Some(base_dir) = self.locate(base) else {
                        continue;
                    };
                    for dir in
                        self.projects_below(base_dir, folder, *project, *proof, *idle_days, &prune)?
                    {
                        direct.push((ItemRef::Dir(dir), None, None));
                    }
                }
            }
            Find::Files {
                under,
                anywhere,
                top_only,
            } => {
                if let Some(base) = self.locate(under)
                    && !self.holds_personal(base, true)
                {
                    for f in self.files_below(base, anywhere, top_only)? {
                        direct.push((ItemRef::File(f), None, None));
                    }
                }
            }
        }
        for spot in spots {
            match rule.action {
                ActionKind::DeleteContents => {
                    if self.holds_personal(spot.dir, false) {
                        continue; // a temp folder pointed at the home folder empties nothing
                    }
                    for at in self.contents(spot.dir) {
                        direct.push((at, spot.label.clone(), spot.owners_in));
                    }
                }
                ActionKind::DeleteDirectory => {
                    direct.push((ItemRef::Dir(spot.dir), spot.label, spot.owners_in));
                }
                // Tools and advice act on nothing directly; their rules arrive in ticket 06.
                ActionKind::Tool | ActionKind::Hint => {}
            }
        }
        if matches!(rule.action, ActionKind::Tool | ActionKind::Hint) {
            direct.clear();
        }
        Some(direct)
    }

    /// Whether the rule's own exclusions keep `at`: names it skips, things used recently, running apps' caches.
    fn wanted(&self, rule: &Rule, at: ItemRef, owners_in: Option<DirId>) -> bool {
        let snap = self.snap;
        let skip = &rule.skip;
        if let ItemRef::Dir(d) = at {
            let name = name_key(snap.name(d));
            if skip.names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                return false;
            }
        }
        if let Some(base) = owners_in
            && let Some(owner) = self.owner_of(base, at)
            && skip.names.iter().any(|n| n.eq_ignore_ascii_case(&owner))
        {
            return false;
        }
        if skip.idle_days > 0 {
            let used = self.last_used(at);
            if used <= 0 || used > self.moment.now - days(skip.idle_days) {
                return false;
            }
        }
        if skip.running_apps
            && let Some(base) = owners_in
            && let Some(owner) = self.owner_of(base, at)
            && self.is_running(&owner)
        {
            return false;
        }
        true
    }

    /// Whether `dir` is, or holds, one of the places a rule must never sweep: the drive, the users' folder, the
    /// home folder, its known folders and app data. With `downloads_ok`, the Downloads folder itself may be swept
    /// (old installers), but not when it is also one of the others, as when it is redirected to a drive's root.
    fn holds_personal(&self, dir: DirId, downloads_ok: bool) -> bool {
        let Some(key) = PathKey::of(&self.snap.path(dir)) else {
            return true;
        };
        self.personal
            .iter()
            .filter(|(_, downloads)| !(downloads_ok && *downloads))
            .any(|(p, _)| p.starts_with(&key))
    }

    /// Whether an app folder named `owner` (`com.spotify.client`, `Slack`, `Google`) belongs to a running app.
    fn is_running(&self, owner: &str) -> bool {
        let owner = owner.to_lowercase();
        self.running.iter().any(|r| {
            *r == owner
                || r.starts_with(&format!("{owner} "))
                || owner.starts_with(&format!("{r}."))
        })
    }

    /// The name of the folder directly in `base` that `at` is in (or is).
    fn owner_of(&self, base: DirId, at: ItemRef) -> Option<String> {
        let mut d = match at {
            ItemRef::Dir(d) | ItemRef::FilesIn(d) => d,
            ItemRef::File(f) => self.snap.file(f).parent,
            ItemRef::Proposal(_) => return None,
        };
        loop {
            let parent = self.snap.parent(d)?;
            if parent == base {
                return Some(self.snap.name(d).to_string_lossy().into_owned());
            }
            d = parent;
        }
    }

    /// The owner of a folder found below `base`, for the proposal's text: the app's folder, or for a vendor's
    /// folder (`Microsoft`, `Google`), the product's folder inside it.
    fn owner(&self, base: DirId, dir: DirId) -> String {
        let Some(first) = self.owner_of(base, ItemRef::Dir(dir)) else {
            return self.snap.name(dir).to_string_lossy().into_owned();
        };
        const VENDORS: [&str; 7] = [
            "microsoft",
            "google",
            "adobe",
            "apple",
            "mozilla",
            "jetbrains",
            "nvidia",
        ];
        if !VENDORS.contains(&first.to_lowercase().as_str()) {
            return first;
        }
        // Walk up to the folder two below `base`: the vendor's product.
        let mut d = dir;
        while let Some(parent) = self.snap.parent(d) {
            if self.snap.parent(parent) == Some(base) {
                return if d == dir {
                    first
                } else {
                    self.snap.name(d).to_string_lossy().into_owned()
                };
            }
            d = parent;
        }
        first
    }

    fn last_used(&self, at: ItemRef) -> UnixTime {
        match at {
            ItemRef::Dir(d) => self.snap.tree(d).last_used,
            ItemRef::File(f) => self.snap.file(f).last_used,
            ItemRef::FilesIn(d) => self.snap.own(d).last_used,
            ItemRef::Proposal(_) => 0,
        }
    }

    fn target(&self, at: ItemRef, label: Option<String>) -> Target {
        let snap = self.snap;
        let (bytes, files) = match at {
            ItemRef::Dir(d) => {
                let t = snap.tree(d);
                (t.bytes, t.files)
            }
            ItemRef::File(f) => (snap.file(f).bytes, 1),
            ItemRef::FilesIn(d) => loose(snap, d),
            ItemRef::Proposal(_) => (0, 0),
        };
        Target {
            at,
            bytes,
            files,
            label,
        }
    }

    /// What is inside a folder, as targets: each subfolder, each kept file, and the loose files as one.
    fn contents(&self, dir: DirId) -> Vec<ItemRef> {
        let snap = self.snap;
        let mut out: Vec<ItemRef> = snap.children(dir).map(ItemRef::Dir).collect();
        out.extend(snap.files(dir).map(ItemRef::File));
        let (bytes, files) = loose(snap, dir);
        if bytes > 0 || files > 0 {
            out.push(ItemRef::FilesIn(dir));
        }
        out
    }

    fn tick(&mut self) -> Option<()> {
        self.checks += 1;
        if self.checks.is_multiple_of(CHECK_EVERY) && (self.stop)() {
            return None;
        }
        Some(())
    }

    /// The folder at `path`, compared as keys (any case, any spelling), if the scan has it.
    fn locate(&self, path: &Path) -> Option<DirId> {
        let key = PathKey::of(path)?;
        let case = self.case;
        self.snap.roots().iter().find_map(|root| {
            let root_key = PathKey::of(&root.shown)?;
            if !key.starts_with_in(&root_key, case) {
                return None;
            }
            let mut at = root.dir;
            for name in &key.keys_in(case)[root_key.keys().len()..] {
                at = self
                    .snap
                    .children(at)
                    .find(|&c| name_key_in(self.snap.name(c), case) == *name)?;
            }
            Some(at)
        })
    }

    /// The folders a place pattern names: any name after the root may be `*` (any folder) or `*.ext` (any folder
    /// whose name ends so). Each comes with the folder the first `*` stood in, where its owners are named.
    fn locate_pattern(&self, pattern: &Path) -> Vec<(DirId, Option<DirId>)> {
        let Some(key) = PathKey::of(pattern) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for root in self.snap.roots() {
            let Some(root_key) = PathKey::of(&root.shown) else {
                continue;
            };
            if !key.starts_with_in(&root_key, self.case) {
                continue;
            }
            let mut level: Vec<(DirId, Option<DirId>)> = vec![(root.dir, None)];
            for k in &key.keys_in(self.case)[root_key.keys().len()..] {
                let mut next = Vec::new();
                for (d, owners) in level {
                    for c in self.snap.children(d) {
                        if glob(k, &name_key_in(self.snap.name(c), self.case)) {
                            let owners = owners.or(k.starts_with('*').then_some(d));
                            next.push((c, owners));
                        }
                    }
                }
                level = next;
            }
            out.extend(level);
        }
        out
    }

    /// Folders below `base` named one of `names`, not looking inside a match, code libraries or `prune`.
    fn named_below(&mut self, base: DirId, names: &[&str], prune: &[DirId]) -> Option<Vec<DirId>> {
        let mut out = Vec::new();
        let mut stack = vec![base];
        while let Some(d) = stack.pop() {
            self.tick()?;
            for c in self.snap.children(d) {
                if !self.snap.flags(c).entered() || prune.contains(&c) {
                    continue;
                }
                let name = self.snap.name(c);
                if is_library_folder(name) {
                    continue;
                }
                let key = name_key(name);
                if names.iter().any(|n| n.eq_ignore_ascii_case(&key)) {
                    out.push(c);
                } else {
                    stack.push(c);
                }
            }
        }
        Some(out)
    }

    /// Project folders below `base`: a folder named `folder` beside the `project` marker, with the `proof` that
    /// it is a real project's, in a project nobody has used for `idle_days`. Never looks inside `prune`, folders
    /// starting with a dot in the home folder or a drive's root, or code libraries.
    fn projects_below(
        &mut self,
        base: DirId,
        folder: &str,
        project: Markers,
        proof: Proof,
        idle_days: u32,
        prune: &[DirId],
    ) -> Option<Vec<DirId>> {
        let snap = self.snap;
        let mut repo = false;
        let mut up = Some(base);
        while let Some(d) = up {
            repo |= snap.markers(d).contains(Markers::GIT);
            up = snap.parent(d);
        }
        let idle_before = self.moment.now - days(idle_days);
        let mut out = Vec::new();
        let mut stack = vec![(base, repo)];
        while let Some((d, repo)) = stack.pop() {
            self.tick()?;
            for c in snap.children(d) {
                if !snap.flags(c).entered() || prune.contains(&c) {
                    continue;
                }
                let name = snap.name(c);
                let top = d == base || snap.parent(d).is_none();
                if top && name.as_encoded_bytes().first() == Some(&b'.') {
                    continue; // tools' folders in the home folder; system folders at a drive's root
                }
                if name.eq_ignore_ascii_case(folder) {
                    let proven = match proof {
                        Proof::NodeLock => snap.markers(d).contains(Markers::NODE_LOCK) || repo,
                        Proof::CargoTarget => snap.markers(c).contains(Markers::CARGO_TARGET),
                    };
                    let used = snap.tree(d).last_used;
                    if snap.markers(d).contains(project) && proven && used > 0 && used < idle_before
                    {
                        out.push(c);
                        continue;
                    }
                }
                if is_library_folder(name) {
                    continue; // dependencies of dependencies are part of the outer folder
                }
                stack.push((c, repo || snap.markers(c).contains(Markers::GIT)));
            }
        }
        Some(out)
    }

    /// Kept files below `base` with one of the `anywhere` extensions, or one of the `top_only` ones directly in it.
    fn files_below(
        &mut self,
        base: DirId,
        anywhere: &[&str],
        top_only: &[&str],
    ) -> Option<Vec<FileId>> {
        let snap = self.snap;
        let has_ext = |name: &OsStr, exts: &[&str]| {
            let key = name_key(name);
            exts.iter().any(|e| {
                key.len() > e.len() + 1
                    && key.ends_with(e)
                    && key.as_bytes()[key.len() - e.len() - 1] == b'.'
            })
        };
        let mut out = Vec::new();
        let mut stack = vec![base];
        while let Some(d) = stack.pop() {
            self.tick()?;
            for f in snap.files(d) {
                let name = snap.file(f).name;
                if has_ext(name, anywhere) || (d == base && has_ext(name, top_only)) {
                    out.push(f);
                }
            }
            for c in snap.children(d) {
                let name = snap.name(c);
                if snap.flags(c).entered() && !is_library_folder(name) && name_key(name) != ".git" {
                    stack.push(c);
                }
            }
        }
        Some(out)
    }
}

/// Whether a name key matches a pattern key: `*` matches any name, `*.ext` any name ending in `.ext`, anything
/// else only itself.
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.strip_prefix('*') {
        Some(suffix) => name.len() > suffix.len() && name.ends_with(suffix),
        None => pattern == name,
    }
}

/// A folder's loose files: its files minus the ones kept individually.
fn loose(snap: &Snapshot, dir: DirId) -> (u64, u64) {
    let own = snap.own(dir);
    let (mut bytes, mut files) = (own.bytes, own.files);
    for f in snap.files(dir) {
        bytes = bytes.saturating_sub(snap.file(f).bytes);
        files = files.saturating_sub(1);
    }
    (bytes, files)
}

fn days(n: u32) -> UnixTime {
    UnixTime::from(n) * 86_400
}

/// The proposal a rule's findings make.
fn proposal(snap: &Snapshot, rule: &Rule, found: &Found) -> Proposal {
    let mut labels: Vec<(String, u64)> = Vec::new();
    for t in &found.targets {
        if let Some(label) = &t.label {
            match labels
                .iter_mut()
                .find(|(l, _)| l.eq_ignore_ascii_case(label))
            {
                Some((_, bytes)) => *bytes += t.bytes,
                None => labels.push((label.clone(), t.bytes)),
            }
        }
    }
    labels.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
    let names: Vec<&str> = labels.iter().map(|(l, _)| l.as_str()).collect();
    let n = found.targets.len() as u64;
    let fill = |template: &str| {
        template
            .replace("{n}", &group_digits(n))
            .replace("{s}", if n == 1 { "" } else { "s" })
            .replace("{files}", &group_digits(found.files))
            .replace("{fs}", if found.files == 1 { "" } else { "s" })
            .replace("{labels_n}", &group_digits(names.len() as u64))
            .replace("{ls}", if names.len() == 1 { "" } else { "s" })
            .replace("{labels}", &list(&names))
    };
    let mut samples: Vec<String> = Vec::new();
    for t in &found.targets {
        let path = match t.at {
            ItemRef::Dir(d) | ItemRef::FilesIn(d) => snap.path(d),
            ItemRef::File(f) => snap.file_path(f),
            ItemRef::Proposal(_) => continue,
        };
        let path = path.display().to_string();
        if !samples.contains(&path) {
            samples.push(path);
        }
        if samples.len() == SAMPLES {
            break;
        }
    }
    Proposal {
        id: rule.id.to_owned(),
        category: rule.category,
        title: fill(rule.text.title),
        summary: fill(rule.text.summary),
        why: fill(rule.text.why),
        bytes: found.bytes,
        item_count: found.files,
        place_count: n,
        risk: rule.risk,
        source: ProposalSource::Rule,
        ai_note: None,
        action: match rule.action {
            ActionKind::DeleteContents | ActionKind::DeleteDirectory => ProposalAction::Delete,
            ActionKind::Tool => ProposalAction::Tool,
            ActionKind::Hint => ProposalAction::Hint,
        },
        regenerable: rule.regenerable,
        sample_paths: samples,
        default_on: rule.default_on.unwrap_or(rule.risk == Risk::Safe),
    }
}

/// Sample paths a proposal shows.
const SAMPLES: usize = 5;

/// "Edge", "Edge and Chrome", "Edge, Chrome and Firefox", "Edge, Chrome, Firefox and 2 more".
fn list(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, c, rest @ ..] => format!("{a}, {b}, {c} and {} more", rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_names_the_way_people_write_them() {
        assert_eq!(list(&[]), "");
        assert_eq!(list(&["Edge"]), "Edge");
        assert_eq!(list(&["Edge", "Chrome"]), "Edge and Chrome");
        assert_eq!(list(&["a", "b", "c"]), "a, b and c");
        assert_eq!(list(&["a", "b", "c", "d", "e"]), "a, b, c and 2 more");
    }

    #[test]
    fn globs_match_whole_names() {
        assert!(glob("*", "default"));
        assert!(glob("*.xcarchive", "app 2.1.xcarchive"));
        assert!(!glob("*.xcarchive", ".xcarchive"));
        assert!(!glob("*.xcarchive", "app.xcarchivex"));
        assert!(glob("cache", "cache"));
        assert!(!glob("cache", "cache2"));
    }
}

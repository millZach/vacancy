use super::{
    Draft, DroppedDraft, Error, Folder, InventoryEntry, Ledger, LedgerDirectory, LedgerMove,
    MAX_COMPONENTS, MAX_ENTRIES, OpState, PlannedDirectory, ReviewedMove, Status,
    journal::Store,
    model::{Directory, Event, Move, Operation, Place, Plan, Receipt, Root, State},
    names::Name,
};
use crate::exec::{
    Fail, Kind,
    sys::{Parent, reorg as native},
};
use crate::{CancelToken, Env, Guard, Platform};
use std::collections::{BTreeMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const FOLDERS: [Folder; 3] = [Folder::Downloads, Folder::Desktop, Folder::Documents];

#[derive(Debug)]
pub struct Inventory {
    env: Env,
    root: native::Id,
    roots: Vec<Root>,
    captured: BTreeMap<String, (Place, native::Fingerprint)>,
    entries: Vec<InventoryEntry>,
    truncated: bool,
    /// Regular files listed after the cap, counted in the same listing pass (one-folder captures only).
    later: u64,
}

/// Opaque approval capability. No public fields, Deserialize, mutable move-list accessor or path constructor.
#[derive(Debug)]
pub struct ValidatedPlan {
    data: Plan,
    review: Vec<ReviewedMove>,
    dropped: Vec<DroppedDraft>,
}
impl ValidatedPlan {
    pub fn review(&self) -> &[ReviewedMove] {
        &self.review
    }
    /// Drafts validation left out, each with its reason. None of them is in the plan.
    pub fn dropped(&self) -> &[DroppedDraft] {
        &self.dropped
    }
    /// The directories the plan's moves go into, outermost first, as display components below their known folder,
    /// and whether each already existed at validation (an existing one is never created or removed by the run).
    pub fn directories(&self) -> Vec<PlannedDirectory> {
        self.data
            .directories
            .iter()
            .map(|dir| PlannedDirectory {
                folder: dir.at.folder,
                names: dir.at.names.iter().map(Name::display).collect(),
                existing: dir.expected.is_some(),
            })
            .collect()
    }
}

/// A draft that passed the checks needing no filesystem access.
struct Parsed<'a> {
    draft: &'a Draft,
    source: &'a Place,
    fingerprint: &'a native::Fingerprint,
    names: Vec<Name>,
    original: Name,
}

/// A destination's directories, outermost first, with the identity of each that already exists.
type Chain = Vec<(Place, Option<native::Id>)>;

/// Names already taken while a batch is validated. Keys are Windows case keys, so aliases collide too.
#[derive(Default)]
struct Reserved {
    /// The exact source of every known draft; a draft may not ask for another draft's source by name.
    selected: HashSet<Place>,
    /// Every known draft's source, by case key. A collision suffix never lands on one.
    sources: HashSet<(Folder, Vec<Vec<u16>>)>,
    /// Every directory any parsed draft names, including prefixes. File destinations avoid all of them.
    planned: HashSet<(Folder, Vec<Vec<u16>>)>,
    /// Directories of kept drafts, with the identity they had if they already existed.
    directories: BTreeMap<Place, Option<native::Id>>,
    aliases: BTreeMap<(Folder, Vec<Vec<u16>>), Place>,
    /// File destinations of kept drafts.
    targets: HashSet<(Folder, Vec<Vec<u16>>)>,
}

#[derive(Debug)]
pub struct Engine {
    env: Env,
    store: Store,
    state: State,
    poisoned: bool,
}

fn folder(env: &Env, at: Folder) -> &Path {
    match at {
        Folder::Downloads => &env.known.downloads,
        Folder::Desktop => &env.known.desktop,
        Folder::Documents => &env.known.documents,
    }
}
fn path(env: &Env, place: &Place) -> PathBuf {
    place
        .names
        .iter()
        .fold(folder(env, place.folder).to_path_buf(), |path, name| {
            path.join(name.os())
        })
}
fn chain(parent: &Parent) -> Vec<native::Id> {
    parent.steps.iter().map(native::Id::step).collect()
}
fn changed(_: Fail) -> Error {
    Error::Changed
}
/// Why a move's source couldn't be opened for its rename: an app has it open, it is gone, or it isn't what was
/// approved. Said apart so the app can tell the user which.
fn source_open(fail: Fail) -> Error {
    match fail {
        Fail::InUse(_) => Error::InUse,
        Fail::Gone => Error::Gone,
        _ => Error::Changed,
    }
}
fn denied(error: crate::Denied) -> Error {
    Error::Denied(error.to_string())
}
fn trust(env: &Env) -> Result<(), Error> {
    if env.platform != Platform::Windows {
        return Err(Error::Unsupported);
    }
    for at in [
        &env.root,
        &env.home,
        &env.app_data,
        &env.known.downloads,
        &env.known.desktop,
        &env.known.documents,
    ] {
        if !native::plain(at) {
            return Err(Error::InvalidName);
        }
        if let Some(confine) = &env.confined_to
            && (!native::plain(confine) || !at.starts_with(confine))
        {
            return Err(Error::Denied("outside fixture profile".into()));
        }
    }
    if let Some(confine) = &env.confined_to {
        let places = [
            &env.known.pictures,
            &env.known.videos,
            &env.known.music,
            &env.local_app_data,
            &env.system_root,
            &env.program_data,
        ]
        .into_iter()
        .chain(env.temp.iter())
        .chain(env.program_files.iter())
        .chain(env.cloud.iter().map(|cloud| &cloud.path));
        for at in places {
            if !native::plain(at) || !at.starts_with(confine) {
                return Err(Error::Denied(
                    "fixture environment contains an outside place".into(),
                ));
            }
        }
    }
    Ok(())
}

impl Inventory {
    /// A fresh bounded metadata snapshot, independent of the aggregate disk scan. It never reads file contents.
    /// This first engine inventories only loose regular files in the three known folders; no directory moves.
    pub fn capture(env: &Env, cancel: &CancelToken) -> Result<Self, Error> {
        Self::capture_in(env, None, cancel)
    }
    /// [`Inventory::capture`] of one known folder: the other folders' roots are still sealed (so validation still
    /// refuses a replaced or nested known folder), but only `only` is listed. Its cap counts listed regular files,
    /// not subfolders: the first 512 files in the order Windows lists the folder (by name on NTFS), visiting at most
    /// four times that many entries. Regular files listed after the cap are counted, not opened ([`Self::later`]).
    pub fn capture_folder(env: &Env, only: Folder, cancel: &CancelToken) -> Result<Self, Error> {
        Self::capture_in(env, Some(only), cancel)
    }
    fn capture_in(env: &Env, only: Option<Folder>, cancel: &CancelToken) -> Result<Self, Error> {
        trust(env)?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let base = native::directory(&env.root).map_err(changed)?;
        let root = native::Id::step(base.steps.last().ok_or(Error::Changed)?);
        let guard = Guard::new(env, &env.root);
        let tag = native::nonce().map_err(changed)?;
        let mut captured = BTreeMap::new();
        let mut entries = Vec::new();
        let mut roots = Vec::new();
        let mut visited = 0;
        let mut truncated = false;
        let mut later = 0;
        for at in FOLDERS {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let parent = native::directory(folder(env, at)).map_err(changed)?;
            let ids = chain(&parent);
            if !ids.contains(&root) || ids.iter().any(|id| id.volume != root.volume) {
                return Err(Error::Changed);
            }
            if roots
                .iter()
                .any(|other: &Root| ids.starts_with(&other.chain) || other.chain.starts_with(&ids))
            {
                return Err(Error::Unsupported);
            }
            roots.push(Root {
                folder: at,
                chain: ids,
            });
            if only.is_some_and(|only| only != at) {
                continue;
            }
            if guard
                .judge_walked(folder(env, at), &parent.steps, true)
                .is_err()
            {
                continue;
            }
            let (listed, capped) = if only.is_some() {
                let (listed, capped, more) =
                    native::list_files_bounded(&parent.dir, MAX_ENTRIES, 4 * MAX_ENTRIES, cancel)
                        .map_err(changed)?;
                later += more;
                (listed, capped)
            } else {
                native::list_bounded(&parent.dir, MAX_ENTRIES - visited, cancel).map_err(changed)?
            };
            truncated |= capped;
            visited += listed.len();
            for entry in listed {
                if cancel.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                if entry.kind != Kind::File {
                    continue;
                }
                let Ok(name) = Name::checked(entry.name.encode_wide().collect()) else {
                    continue;
                };
                let Ok((_, info)) = native::file(&parent.dir, &name, false) else {
                    continue;
                };
                if info.id.file != entry.id || info.id.volume != root.volume {
                    continue;
                }
                let mut steps = parent.steps.clone();
                steps.push(entry.step(root.volume));
                if guard
                    .judge_walked(&folder(env, at).join(name.os()), &steps, false)
                    .is_err()
                {
                    continue;
                }
                let id = format!("{tag}-e{}", entries.len());
                entries.push(InventoryEntry {
                    id: id.clone(),
                    folder: at,
                    name: name.display(),
                    logical_bytes: info.logical,
                    allocated_bytes: info.allocated,
                    changed_ns: info.latest(),
                    modified_ns: info.written,
                    file_id: info.id.file,
                });
                captured.insert(
                    id,
                    (
                        Place {
                            folder: at,
                            names: vec![name],
                        },
                        info,
                    ),
                );
            }
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Self {
            env: env.clone(),
            root,
            roots,
            captured,
            entries,
            truncated,
            later,
        })
    }
    pub fn entries(&self) -> &[InventoryEntry] {
        &self.entries
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    /// For a one-folder capture that hit its cap: regular files listed after it, left for a later run.
    pub fn later(&self) -> u64 {
        self.later
    }
    /// [`Inventory::validate`], with every draft's file name pinned: a draft naming its file must get exactly that
    /// name (as the user reviewed it), or it is dropped with [`Error::Taken`]. It is never renumbered.
    pub fn validate_pinned(
        &self,
        drafts: &[Draft],
        cancel: &CancelToken,
    ) -> Result<ValidatedPlan, Error> {
        self.validate_with(drafts, true, cancel)
    }

    /// Validates untrusted drafts one at a time. An invalid draft (unknown or repeated source, a source changed since
    /// capture, a bad or refused name, a directory that conflicts with another draft's, no free name) is dropped
    /// with its reason, and the rest are kept. A source named by more than one draft is dropped from all of them:
    /// the proposals conflict and neither is chosen. The batch fails only for batch-wide reasons (bounds,
    /// cancellation, a replaced known folder, a manifest over the journal limit) or when no draft is left, and then
    /// with the first dropped draft's error.
    pub fn validate(&self, drafts: &[Draft], cancel: &CancelToken) -> Result<ValidatedPlan, Error> {
        self.validate_with(drafts, false, cancel)
    }
    fn validate_with(
        &self,
        drafts: &[Draft],
        pinned: bool,
        cancel: &CancelToken,
    ) -> Result<ValidatedPlan, Error> {
        if drafts.is_empty() || drafts.len() > MAX_ENTRIES {
            return Err(Error::Bounds);
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        // A replaced known folder makes the whole inventory stale, not one draft.
        for at in FOLDERS {
            self.root_parent(at)?;
        }
        let mut uses = BTreeMap::<&Place, usize>::new();
        for draft in drafts {
            if let Some((place, _)) = self.captured.get(&draft.entry_id) {
                *uses.entry(place).or_default() += 1;
            }
        }
        let mut reserved = Reserved {
            selected: uses.keys().map(|place| (*place).clone()).collect(),
            sources: uses.keys().map(|place| reservation(place)).collect(),
            ..Reserved::default()
        };
        let mut dropped = Vec::new();
        let mut first = None;
        let mut reject = |draft: &Draft, error: Error| {
            dropped.push(DroppedDraft {
                entry_id: draft.entry_id.clone(),
                reason: error.to_string(),
            });
            first.get_or_insert(error);
        };
        let mut parsed = Vec::new();
        for draft in drafts {
            match self.parse(draft, &uses) {
                Ok(item) => {
                    for count in 1..=item.names.len() {
                        reserved.planned.insert(reservation(&Place {
                            folder: draft.destination,
                            names: item.names[..count].to_vec(),
                        }));
                    }
                    parsed.push(item);
                }
                Err(error) => reject(draft, error),
            }
        }
        let guard = Guard::new(&self.env, &self.env.root);
        let mut moves = Vec::new();
        let mut review = Vec::new();
        for item in parsed {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let exact = pinned && item.draft.filename.is_some();
            let (chain, to) = match self.resolve(&guard, &item, &reserved, exact) {
                Ok(value) => value,
                Err(error) => {
                    reject(item.draft, error);
                    continue;
                }
            };
            // Only a kept draft's directories enter the plan, so a dropped draft leaves no unused directory behind.
            for (at, identity) in chain {
                reserved.aliases.insert(reservation(&at), at.clone());
                reserved.directories.insert(at, identity);
            }
            reserved.targets.insert(reservation(&to));
            review.push(ReviewedMove {
                entry_id: item.draft.entry_id.clone(),
                from: super::names::display(path(&self.env, item.source).as_os_str().encode_wide()),
                to: super::names::display(path(&self.env, &to).as_os_str().encode_wide()),
                directories: to.names[..to.names.len() - 1]
                    .iter()
                    .map(Name::display)
                    .collect(),
                filename: to.names.last().map(Name::display).unwrap_or_default(),
            });
            moves.push(Move {
                entry: item.draft.entry_id.clone(),
                from: item.source.clone(),
                to,
                fingerprint: *item.fingerprint,
            });
        }
        if moves.is_empty() {
            return Err(first.unwrap_or(Error::UnknownEntry));
        }
        // Unreachable after the reservations above; kept as a last structural check.
        if moves
            .iter()
            .any(|mv| moves.iter().any(|other| mv.to == other.from))
        {
            return Err(Error::UnknownEntry);
        }
        let data = Plan {
            id: format!("run-{}", native::nonce().map_err(changed)?),
            root: self.root,
            roots: self.roots.clone(),
            directories: reserved
                .directories
                .into_iter()
                .map(|(at, expected)| Directory { at, expected })
                .collect(),
            moves,
        };
        data.checked()?;
        super::model::check_approval(&data)?;
        Ok(ValidatedPlan {
            data,
            review,
            dropped,
        })
    }
    /// The checks a draft can fail without touching the filesystem.
    fn parse<'a>(
        &'a self,
        draft: &'a Draft,
        uses: &BTreeMap<&Place, usize>,
    ) -> Result<Parsed<'a>, Error> {
        let (source, fingerprint) = self
            .captured
            .get(&draft.entry_id)
            .ok_or(Error::UnknownEntry)?;
        if uses.get(source) != Some(&1) {
            return Err(Error::UnknownEntry);
        }
        if draft.directories.len() >= MAX_COMPONENTS {
            return Err(Error::Bounds);
        }
        let names = draft
            .directories
            .iter()
            .map(|text| Name::text(text))
            .collect::<Result<Vec<_>, _>>()?;
        let original = draft
            .filename
            .as_deref()
            .map(Name::text)
            .transpose()?
            .unwrap_or_else(|| source.names[0].clone());
        Ok(Parsed {
            draft,
            source,
            fingerprint,
            names,
            original,
        })
    }
    /// Checks one parsed draft against the filesystem and the names kept drafts already hold. Gives its directory
    /// chain (with identities of those that exist) and its final destination, collision suffix included.
    fn resolve(
        &self,
        guard: &Guard,
        item: &Parsed<'_>,
        reserved: &Reserved,
        exact: bool,
    ) -> Result<(Chain, Place), Error> {
        let Parsed {
            draft,
            source,
            fingerprint,
            names,
            original,
        } = item;
        let source_parent = self.root_parent(source.folder)?;
        let (_, current) =
            native::file(&source_parent.dir, &source.names[0], false).map_err(changed)?;
        if current != **fingerprint {
            return Err(Error::Changed);
        }
        let mut source_steps = source_parent.steps.clone();
        source_steps.push(crate::fsid::Step {
            name: source.names[0].os(),
            id: crate::fsid::Identity::new(current.id.volume, current.id.file),
            mount: None,
        });
        guard
            .judge_walked(&path(&self.env, source), &source_steps, false)
            .map_err(denied)?;
        let mut existing = self.root_parent(draft.destination)?;
        let mut existing_path = folder(&self.env, draft.destination).to_path_buf();
        let mut missing = false;
        let mut missing_names = PathBuf::new();
        let mut chain = Vec::new();
        for count in 1..=names.len() {
            let at = Place {
                folder: draft.destination,
                names: names[..count].to_vec(),
            };
            let identity = if !missing {
                match native::child_dir(&existing.dir, &names[count - 1]) {
                    Ok((dir, step)) => {
                        let id = native::Id::step(&step);
                        drop(dir);
                        existing_path.push(names[count - 1].os());
                        existing = native::directory(&existing_path).map_err(changed)?;
                        if native::Id::step(existing.steps.last().ok_or(Error::Changed)?) != id
                            || id.volume != self.root.volume
                        {
                            return Err(Error::Changed);
                        }
                        guard
                            .judge_walked(&existing_path, &existing.steps, true)
                            .map_err(denied)?;
                        Some(id)
                    }
                    Err(Fail::Gone) => {
                        missing = true;
                        missing_names.push(names[count - 1].os());
                        None
                    }
                    Err(error) => return Err(changed(error)),
                }
            } else {
                missing_names.push(names[count - 1].os());
                None
            };
            if reserved
                .aliases
                .get(&reservation(&at))
                .is_some_and(|previous| *previous != at)
            {
                return Err(Error::InvalidName);
            }
            if reserved
                .directories
                .get(&at)
                .is_some_and(|previous| *previous != identity)
            {
                return Err(Error::Changed);
            }
            chain.push((at, identity));
        }
        let mut requested = names.clone();
        requested.push(original.clone());
        if reserved.selected.contains(&Place {
            folder: draft.destination,
            names: requested,
        }) {
            return Err(Error::UnknownEntry);
        }
        // A pinned name is used exactly or not at all: never renumbered after the user reviewed it.
        for suffix in 0..if exact { 1 } else { 100 } {
            let name = collision_name(original, suffix)?;
            let mut candidate = names.clone();
            candidate.push(name.clone());
            let to = Place {
                folder: draft.destination,
                names: candidate,
            };
            let mut proposed = missing_names.clone();
            proposed.push(name.os());
            guard
                .judge_absent(&existing_path, &existing.steps, &proposed)
                .map_err(denied)?;
            let key = reservation(&to);
            // A planned directory takes its name at validation time, so no file is reviewed into a name a
            // directory will hold when the run creates it.
            if to != **source
                && !reserved.targets.contains(&key)
                && !reserved.planned.contains(&key)
                && !reserved.sources.contains(&key)
                && (missing || !native::exists(&existing.dir, &name).map_err(changed)?)
            {
                return Ok((chain, to));
            }
        }
        Err(if exact { Error::Taken } else { Error::Bounds })
    }
    fn root_parent(&self, folder_id: Folder) -> Result<Parent, Error> {
        let parent = native::directory(folder(&self.env, folder_id)).map_err(changed)?;
        if self
            .roots
            .iter()
            .find(|root| root.folder == folder_id)
            .is_none_or(|root| root.chain != chain(&parent))
        {
            return Err(Error::Changed);
        }
        Ok(parent)
    }
}

fn reservation(at: &Place) -> (Folder, Vec<Vec<u16>>) {
    (at.folder, at.names.iter().map(native::case_key).collect())
}
fn collision_name(name: &Name, suffix: u32) -> Result<Name, Error> {
    if suffix == 0 {
        return Ok(name.clone());
    }
    let dot = name
        .0
        .iter()
        .rposition(|unit| *unit == 46)
        .filter(|index| *index > 0)
        .unwrap_or(name.0.len());
    Name::checked(
        name.0[..dot]
            .iter()
            .copied()
            .chain(format!(" ({suffix})").encode_utf16())
            .chain(name.0[dot..].iter().copied())
            .collect(),
    )
}

impl Engine {
    pub fn begin(env: &Env, journal: &Path, approved: ValidatedPlan) -> Result<Self, Error> {
        trust(env)?;
        approved.data.checked()?;
        super::model::check_approval(&approved.data)?;
        let expected = journal_parent_for_root(env, journal, Some(approved.data.root))?;
        boundary("journal-expected");
        let mut store = Store::open(journal, true, &expected)?;
        store.append(&Event::Approved {
            version: 1,
            plan: approved.data,
        })?;
        let state = State::load(store.records())?;
        let engine = Self {
            env: env.clone(),
            store,
            state,
            poisoned: false,
        };
        engine.root_check()?;
        Ok(engine)
    }
    /// Acquires the stable lease and reads evidence. It performs no rename, mkdir, cleanup, or automatic resume.
    pub fn open(env: &Env, journal: &Path) -> Result<Self, Error> {
        trust(env)?;
        let expected = journal_parent(env, journal)?;
        let store = Store::open(journal, false, &expected)?;
        let state = State::load(store.records())?;
        let engine = Self {
            env: env.clone(),
            store,
            state,
            poisoned: false,
        };
        engine.root_check()?;
        Ok(engine)
    }
    pub fn status(&self) -> Status {
        self.state.status()
    }
    /// A move that was applied and not yet put back.
    fn undoable(&self, index: usize) -> bool {
        matches!(
            self.state.receipts.get(&Operation::Move.key(index)),
            Some(Receipt::Applied(_))
        ) && !matches!(
            self.state.receipts.get(&Operation::Undo.key(index)),
            Some(Receipt::Applied(_))
        )
    }
    /// What happened to each move and directory of the plan, as the journal records it. Display text only: it
    /// grants no filesystem authority.
    pub fn ledger(&self) -> Ledger {
        let state = |op: Operation, index: usize| -> OpState {
            let key = op.key(index);
            let note = || self.state.notes.get(&key).cloned().unwrap_or_default();
            match (
                self.state.pending.contains_key(&key),
                self.state.receipts.get(&key),
            ) {
                (_, Some(Receipt::Unresolved)) => OpState::Unresolved(note()),
                (true, _) => OpState::Pending,
                (false, Some(Receipt::Applied(_))) => OpState::Applied,
                (false, Some(Receipt::Skipped)) => OpState::Skipped(note()),
                (false, None) => OpState::NotRun,
            }
        };
        let names = |place: &Place| place.names.iter().map(Name::display).collect::<Vec<_>>();
        Ledger {
            run_id: self.state.plan.id.clone(),
            started: self.state.started,
            finished: self.state.finished,
            cancelled: self.state.cancelled,
            moves: self
                .state
                .plan
                .moves
                .iter()
                .enumerate()
                .map(|(index, mv)| LedgerMove {
                    entry_id: mv.entry.clone(),
                    folder: mv.from.folder,
                    from: names(&mv.from),
                    to: names(&mv.to),
                    allocated_bytes: mv.fingerprint.allocated,
                    moved: state(Operation::Move, index),
                    undo: state(Operation::Undo, index),
                })
                .collect(),
            directories: self
                .state
                .plan
                .directories
                .iter()
                .enumerate()
                .map(|(index, dir)| LedgerDirectory {
                    folder: dir.at.folder,
                    names: names(&dir.at),
                    existing: dir.expected.is_some(),
                    created: if dir.expected.is_some() {
                        OpState::NotRun
                    } else {
                        state(Operation::Mkdir, index)
                    },
                    removed: state(Operation::RemoveDirectory, index),
                })
                .collect(),
        }
    }
    pub fn durability_flushes(&self) -> u64 {
        self.store.flushes
    }
    fn root_check(&self) -> Result<(), Error> {
        let parent = native::directory(&self.env.root).map_err(changed)?;
        if native::Id::step(parent.steps.last().ok_or(Error::Changed)?) != self.state.plan.root {
            return Err(Error::Changed);
        }
        Ok(())
    }
    fn record(&mut self, event: Event) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::StoppedUnresolved);
        }
        let mut candidate = self.state.clone();
        if let Err(error) = candidate.apply(&event) {
            self.poisoned = true;
            return if self.state.pending.is_empty() {
                Err(error)
            } else {
                Err(Error::StoppedUnresolved)
            };
        }
        if let Err(error) = self.store.append(&event) {
            self.poisoned = true;
            return if self.state.pending.is_empty() {
                Err(error)
            } else {
                Err(Error::StoppedUnresolved)
            };
        }
        self.state = candidate;
        Ok(())
    }
    fn skip(
        &mut self,
        op: Operation,
        index: usize,
        reason: impl Into<String>,
    ) -> Result<(), Error> {
        let reason = reason.into();
        self.record(if self.state.pending.contains_key(&op.key(index)) {
            Event::Skipped {
                operation: op,
                index,
                reason,
            }
        } else {
            Event::NotAttempted {
                operation: op,
                index,
                reason,
            }
        })
    }
    fn parent(&self, place: &Place) -> Result<Parent, Error> {
        self.root_check()?;
        let parent = native::directory(&path(&self.env, place)).map_err(changed)?;
        let mut expected = self
            .state
            .plan
            .roots
            .iter()
            .find(|root| root.folder == place.folder)
            .ok_or(Error::CorruptJournal)?
            .chain
            .clone();
        for count in 1..=place.names.len() {
            expected.push(
                self.state
                    .directory_identity(&Place {
                        folder: place.folder,
                        names: place.names[..count].to_vec(),
                    })
                    .ok_or(Error::Changed)?,
            );
        }
        if chain(&parent) != expected {
            return Err(Error::Changed);
        }
        Guard::new(&self.env, &self.env.root)
            .judge_walked(&path(&self.env, place), &parent.steps, true)
            .map_err(denied)?;
        Ok(parent)
    }
    fn inspect(&self, at: &Place) -> Result<Option<native::Fingerprint>, Error> {
        let parent = self.parent(&at.parent())?;
        match native::file(
            &parent.dir,
            at.names.last().ok_or(Error::CorruptJournal)?,
            false,
        ) {
            Ok((_, info)) => Ok(Some(info)),
            Err(Fail::Gone) => Ok(None),
            Err(error) => Err(changed(error)),
        }
    }
    pub fn run(&mut self, cancel: &CancelToken) -> Result<Status, Error> {
        self.run_observed(cancel, &mut |_, _| {})
    }
    /// [`Engine::run`], telling `observe` after each move how many of the plan's moves have been handled and how
    /// many there are. The observer only watches: it can't change what runs.
    pub fn run_observed(
        &mut self,
        cancel: &CancelToken,
        observe: &mut dyn FnMut(usize, usize),
    ) -> Result<Status, Error> {
        if self.poisoned {
            return Err(Error::StoppedUnresolved);
        }
        // No automatic or implicit resume policy. A previously started run is inspectable/undoable only.
        if self.state.started {
            return if self.state.pending.is_empty() {
                Ok(self.status())
            } else {
                Err(Error::Unresolved)
            };
        }
        self.root_check()?;
        self.record(Event::Started)?;
        for index in 0..self.state.plan.directories.len() {
            if cancel.is_cancelled() {
                break;
            }
            let spec = self.state.plan.directories[index].clone();
            if spec.expected.is_some() {
                continue;
            }
            let parent = match self.parent(&spec.at.parent()) {
                Ok(parent) => parent,
                Err(error) => {
                    self.skip(Operation::Mkdir, index, error.to_string())?;
                    continue;
                }
            };
            let name = spec.at.names.last().ok_or(Error::CorruptJournal)?;
            let guard = Guard::new(&self.env, &self.env.root);
            guard
                .judge_absent(
                    &path(&self.env, &spec.at.parent()),
                    &parent.steps,
                    Path::new(&name.os()),
                )
                .map_err(denied)?;
            self.record(Event::Intent {
                operation: Operation::Mkdir,
                index,
                identity: None,
            })?;
            boundary("mkdir-intent");
            if native::revalidate(&parent).is_err() {
                self.skip(Operation::Mkdir, index, "parent changed before creation")?;
                continue;
            }
            if Guard::new(&self.env, &self.env.root)
                .judge_absent(
                    &path(&self.env, &spec.at.parent()),
                    &parent.steps,
                    Path::new(&name.os()),
                )
                .is_err()
            {
                self.skip(Operation::Mkdir, index, "Guard changed before creation")?;
                continue;
            }
            match native::mkdir(&parent.dir, name) {
                Ok((dir, step)) => {
                    boundary("mkdir-namespace");
                    self.record(Event::Applied {
                        operation: Operation::Mkdir,
                        index,
                        identity: native::Id::step(&step),
                    })?;
                    boundary("mkdir-result");
                    drop(dir);
                }
                // FILE_CREATE returning NAME_COLLISION positively proves this call did not create it.
                // Preserve the external directory, never adopt it, and leave its dependent moves skipped.
                Err(native::MkdirFailure::Collision) => self.skip(
                    Operation::Mkdir,
                    index,
                    "destination directory arrived after approval",
                )?,
                Err(native::MkdirFailure::Uncertain(error)) => {
                    if matches!(native::exists(&parent.dir, name), Ok(false)) {
                        self.skip(Operation::Mkdir, index, format!("{error:?}"))?;
                    } else {
                        self.record(Event::Unresolved {
                            operation: Operation::Mkdir,
                            index,
                            reason: "creation outcome/ownership is uncertain".into(),
                        })?;
                        return Err(Error::Unresolved);
                    }
                }
            }
        }
        let total = self.state.plan.moves.len();
        for index in 0..total {
            if cancel.is_cancelled() {
                break;
            }
            self.move_one(index, false)?;
            observe(index + 1, total);
        }
        self.record(Event::Finished {
            cancelled: cancel.is_cancelled(),
        })?;
        Ok(self.status())
    }

    fn move_one(&mut self, index: usize, undo: bool) -> Result<(), Error> {
        let mv = self.state.plan.moves[index].clone();
        let op = if undo {
            Operation::Undo
        } else {
            Operation::Move
        };
        let (from, to) = if undo {
            (&mv.to, &mv.from)
        } else {
            (&mv.from, &mv.to)
        };
        let prepare = || -> Result<_, Error> {
            let source_parent = self.parent(&from.parent())?;
            let destination_parent = self.parent(&to.parent())?;
            let (file, fingerprint) = native::file(
                &source_parent.dir,
                from.names.last().ok_or(Error::CorruptJournal)?,
                true,
            )
            .map_err(source_open)?;
            if fingerprint.id != mv.fingerprint.id
                || fingerprint.links != mv.fingerprint.links
                || (!undo && fingerprint != mv.fingerprint)
            {
                return Err(Error::Changed);
            }
            let guard = Guard::new(&self.env, &self.env.root);
            let mut steps = source_parent.steps.clone();
            steps.push(crate::fsid::Step {
                name: from.names.last().ok_or(Error::CorruptJournal)?.os(),
                id: crate::fsid::Identity::new(fingerprint.id.volume, fingerprint.id.file),
                mount: None,
            });
            guard
                .judge_walked(&path(&self.env, from), &steps, false)
                .map_err(denied)?;
            guard
                .judge_absent(
                    &path(&self.env, &to.parent()),
                    &destination_parent.steps,
                    Path::new(&to.names.last().ok_or(Error::CorruptJournal)?.os()),
                )
                .map_err(denied)?;
            if native::exists(
                &destination_parent.dir,
                to.names.last().ok_or(Error::CorruptJournal)?,
            )
            .map_err(changed)?
            {
                return Err(Error::Taken);
            }
            Ok((source_parent, destination_parent, file))
        };
        let (source_parent, destination_parent, file) = match prepare() {
            Ok(value) => value,
            Err(error) => return self.skip(op, index, error.to_string()),
        };
        self.record(Event::Intent {
            operation: op,
            index,
            identity: Some(mv.fingerprint.id),
        })?;
        boundary(if undo { "undo-intent" } else { "move-intent" });
        let fresh = match native::fingerprint(&file) {
            Ok(info) => info,
            Err(_) => return self.skip(op, index, "fingerprint unavailable before rename"),
        };
        if fresh.id != mv.fingerprint.id
            || fresh.links != mv.fingerprint.links
            || (!undo && fresh != mv.fingerprint)
            || native::revalidate(&source_parent).is_err()
            || native::revalidate(&destination_parent).is_err()
        {
            return self.skip(op, index, "fingerprint or parent changed before rename");
        }
        let guard = Guard::new(&self.env, &self.env.root);
        let mut steps = source_parent.steps.clone();
        steps.push(crate::fsid::Step {
            name: from.names.last().ok_or(Error::CorruptJournal)?.os(),
            id: crate::fsid::Identity::new(fresh.id.volume, fresh.id.file),
            mount: None,
        });
        if guard
            .judge_walked(&path(&self.env, from), &steps, false)
            .is_err()
            || guard
                .judge_absent(
                    &path(&self.env, &to.parent()),
                    &destination_parent.steps,
                    Path::new(&to.names.last().ok_or(Error::CorruptJournal)?.os()),
                )
                .is_err()
        {
            return self.skip(op, index, "Guard changed before rename");
        }
        let result = native::rename(
            &file,
            &destination_parent.dir,
            to.names.last().ok_or(Error::CorruptJournal)?,
        );
        boundary(if undo {
            "undo-namespace"
        } else {
            "move-namespace"
        });
        // Both retained parent owners stay alive until after the durable result. No guessed success on an error.
        let observed = self.classify_move(&mv, undo);
        match observed {
            Reconciliation::Applied => self.record(Event::Applied {
                operation: op,
                index,
                identity: mv.fingerprint.id,
            })?,
            Reconciliation::NotApplied => self.skip(
                op,
                index,
                format!("rename left source in place: {result:?}"),
            )?,
            Reconciliation::Kept(reason) => {
                self.skip(op, index, format!("{reason}: {result:?}"))?
            }
            Reconciliation::Unresolved => {
                self.record(Event::Unresolved {
                    operation: op,
                    index,
                    reason: "rename identity state is ambiguous".into(),
                })?;
                return Err(Error::Unresolved);
            }
        }
        boundary(if undo { "undo-result" } else { "move-result" });
        drop(file);
        drop(source_parent);
        drop(destination_parent);
        Ok(())
    }
    fn classify_move(&self, mv: &Move, undo: bool) -> Reconciliation {
        let (from, to) = if undo {
            (&mv.to, &mv.from)
        } else {
            (&mv.from, &mv.to)
        };
        let expected = |info: native::Fingerprint| {
            info.id == mv.fingerprint.id && info.links == mv.fingerprint.links
        };
        let Ok(source) = self.inspect(from) else {
            return Reconciliation::Unresolved;
        };
        let target = match self.inspect(to) {
            Ok(target) => target,
            // A directory is never this file. With the file still at its source under the same identity and link
            // count, the refused rename provably did not happen.
            Err(_) if source.is_some_and(expected) && self.plain_directory(to) => {
                return Reconciliation::Kept(
                    "a directory now holds the destination name; the file was left where it was",
                );
            }
            Err(_) => return Reconciliation::Unresolved,
        };
        match (source, target) {
            (None, Some(info)) if expected(info) => Reconciliation::Applied,
            (Some(info), target)
                if expected(info) && target.is_none_or(|other| other.id != info.id) =>
            {
                Reconciliation::NotApplied
            }
            _ => Reconciliation::Unresolved,
        }
    }
    /// Whether a plain directory (not a link or junction) holds this name now.
    fn plain_directory(&self, at: &Place) -> bool {
        let (Ok(parent), Some(name)) = (self.parent(&at.parent()), at.names.last()) else {
            return false;
        };
        native::child_dir(&parent.dir, name).is_ok()
    }
    /// Metadata/status reconciliation only. It never invokes rename, mkdir or directory removal.
    pub fn reconcile(&mut self, cancel: &CancelToken) -> Result<Status, Error> {
        if self.poisoned {
            return Err(Error::StoppedUnresolved);
        }
        self.root_check()?;
        let pending = self.state.pending.clone();
        for ((kind, index), identity) in pending {
            if cancel.is_cancelled() {
                break;
            }
            let op = match kind {
                0 => Operation::Move,
                1 => Operation::Undo,
                2 => Operation::Mkdir,
                3 => Operation::RemoveDirectory,
                _ => return Err(Error::CorruptJournal),
            };
            let outcome = if matches!(op, Operation::Move | Operation::Undo) {
                self.classify_move(&self.state.plan.moves[index], op == Operation::Undo)
            } else {
                self.classify_directory(index, identity, op == Operation::RemoveDirectory)
            };
            match outcome {
                Reconciliation::Applied => self.record(Event::Applied {
                    operation: op,
                    index,
                    identity: identity.ok_or(Error::CorruptJournal)?,
                })?,
                Reconciliation::NotApplied => self.skip(
                    op,
                    index,
                    "reconciled: namespace action not currently applied",
                )?,
                Reconciliation::Kept(reason) => self.skip(op, index, reason)?,
                Reconciliation::Unresolved
                    if self.state.receipts.get(&op.key(index)) != Some(&Receipt::Unresolved) =>
                {
                    self.record(Event::Unresolved {
                        operation: op,
                        index,
                        reason: "recovery preserved ambiguous identities".into(),
                    })?
                }
                Reconciliation::Unresolved => (),
            }
            boundary("reconcile-result");
        }
        Ok(self.status())
    }
    fn classify_directory(
        &self,
        index: usize,
        expected: Option<native::Id>,
        removing: bool,
    ) -> Reconciliation {
        let at = &self.state.plan.directories[index].at;
        let observed = self.parent(&at.parent()).map(|parent| {
            native::child_dir(
                &parent.dir,
                at.names.last().expect("validated directory name"),
            )
            .map(|(_, step)| native::Id::step(&step))
        });
        match (removing, observed) {
            (false, Ok(Err(Fail::Gone))) => Reconciliation::NotApplied,
            // A mkdir intent without its durable receipt never gains ownership. Whatever holds the name is not
            // claimed: it is skipped, never removed by Undo, reported, and does not block Undo of the moves.
            (false, _) => Reconciliation::Kept(
                "the directory's creation was not recorded, so it is not claimed by this run and is left in place",
            ),
            (true, Ok(Err(Fail::Gone))) => Reconciliation::Applied,
            (true, Ok(Ok(id))) if Some(id) == expected => Reconciliation::NotApplied,
            (true, _) => Reconciliation::Unresolved,
        }
    }
    pub fn undo(&mut self, cancel: &CancelToken) -> Result<Status, Error> {
        self.undo_observed(cancel, &mut |_, _| {})
    }
    /// [`Engine::undo`], telling `observe` after each move it tries to put back how many it has tried and how many
    /// there are to try. The observer only watches.
    pub fn undo_observed(
        &mut self,
        cancel: &CancelToken,
        observe: &mut dyn FnMut(usize, usize),
    ) -> Result<Status, Error> {
        if self.poisoned {
            return Err(Error::StoppedUnresolved);
        }
        if !self.state.pending.is_empty() {
            return Err(Error::Unresolved);
        }
        self.root_check()?;
        let total = (0..self.state.plan.moves.len())
            .filter(|index| self.undoable(*index))
            .count();
        let mut tried = 0;
        for index in (0..self.state.plan.moves.len()).rev() {
            if cancel.is_cancelled() {
                return Ok(self.status());
            }
            if self.undoable(index) {
                self.move_one(index, true)?;
                tried += 1;
                observe(tried, total);
            }
        }
        for index in (0..self.state.plan.directories.len()).rev() {
            if cancel.is_cancelled() {
                break;
            }
            let Some(Receipt::Applied(id)) = self
                .state
                .receipts
                .get(&Operation::Mkdir.key(index))
                .copied()
            else {
                continue;
            };
            if matches!(
                self.state
                    .receipts
                    .get(&Operation::RemoveDirectory.key(index)),
                Some(Receipt::Applied(_))
            ) {
                continue;
            }
            let at = self.state.plan.directories[index].at.clone();
            let parent = match self.parent(&at.parent()) {
                Ok(parent) => parent,
                Err(error) => {
                    self.skip(Operation::RemoveDirectory, index, error.to_string())?;
                    continue;
                }
            };
            let (dir, step) = match native::prepare_removal(
                &parent.dir,
                at.names.last().ok_or(Error::CorruptJournal)?,
                id,
            ) {
                Ok(value) => value,
                Err(error) => {
                    self.skip(Operation::RemoveDirectory, index, format!("{error:?}"))?;
                    continue;
                }
            };
            let mut steps = parent.steps.clone();
            steps.push(step);
            if Guard::new(&self.env, &self.env.root)
                .judge_walked(&path(&self.env, &at), &steps, false)
                .is_err()
            {
                self.skip(
                    Operation::RemoveDirectory,
                    index,
                    "Guard refused directory cleanup",
                )?;
                continue;
            }
            self.record(Event::Intent {
                operation: Operation::RemoveDirectory,
                index,
                identity: Some(id),
            })?;
            boundary("remove-intent");
            if native::revalidate(&parent).is_err()
                || native::validate_directory(&dir, id).is_err()
                || Guard::new(&self.env, &self.env.root)
                    .judge_walked(&path(&self.env, &at), &steps, false)
                    .is_err()
            {
                self.skip(
                    Operation::RemoveDirectory,
                    index,
                    "directory changed before cleanup",
                )?;
                continue;
            }
            match native::remove_prepared(
                &parent.dir,
                at.names.last().ok_or(Error::CorruptJournal)?,
                dir,
            ) {
                Ok(()) => {
                    boundary("remove-namespace");
                    self.record(Event::Applied {
                        operation: Operation::RemoveDirectory,
                        index,
                        identity: id,
                    })?;
                    boundary("remove-result");
                }
                Err(error) => match self.classify_directory(index, Some(id), true) {
                    Reconciliation::Applied => self.record(Event::Applied {
                        operation: Operation::RemoveDirectory,
                        index,
                        identity: id,
                    })?,
                    Reconciliation::NotApplied => {
                        self.skip(Operation::RemoveDirectory, index, format!("{error:?}"))?
                    }
                    Reconciliation::Kept(reason) => self.skip(
                        Operation::RemoveDirectory,
                        index,
                        format!("{reason}: {error:?}"),
                    )?,
                    Reconciliation::Unresolved => {
                        self.record(Event::Unresolved {
                            operation: Operation::RemoveDirectory,
                            index,
                            reason: "directory removal outcome is uncertain".into(),
                        })?;
                        return Err(Error::Unresolved);
                    }
                },
            }
        }
        Ok(self.status())
    }
}

pub(super) fn boundary(name: &str) {
    #[cfg(test)]
    tests::hook(name);
    #[cfg(test)]
    if std::env::var("VACANCY_T08_CRASH_AT").ok().as_deref() == Some(name) {
        std::process::exit(73);
    }
    #[cfg(not(test))]
    let _ = name;
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

enum Reconciliation {
    Applied,
    NotApplied,
    /// Provably not applied, and something else now holds the name: skipped with this reason, never adopted.
    Kept(&'static str),
    Unresolved,
}
fn journal_parent(env: &Env, journal: &Path) -> Result<Vec<native::Id>, Error> {
    journal_parent_for_root(env, journal, None)
}

fn journal_parent_for_root(
    env: &Env,
    journal: &Path,
    sealed_root: Option<native::Id>,
) -> Result<Vec<native::Id>, Error> {
    if !native::plain(journal) || journal.parent() != Some(env.app_data.as_path()) {
        return Err(Error::Denied(
            "journal must be a new file directly in trusted app data".into(),
        ));
    }
    let parent = native::directory(&env.app_data).map_err(changed)?;
    let root = native::directory(&env.root).map_err(changed)?;
    let id = native::Id::step(root.steps.last().ok_or(Error::Changed)?);
    if sealed_root.is_some_and(|sealed| sealed != id) {
        return Err(Error::Changed);
    }
    let chain = chain(&parent);
    if !chain.contains(&id) || chain.iter().any(|entry| entry.volume != id.volume) {
        return Err(Error::Changed);
    }
    Ok(chain)
}

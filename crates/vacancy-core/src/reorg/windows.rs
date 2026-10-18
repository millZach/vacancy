use std::collections::{BTreeMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use crate::{CancelToken, Env, Guard, Platform};
use crate::exec::{Fail, Kind, sys::{Parent, reorg as native}};
use super::{Draft, Error, Folder, InventoryEntry, MAX_COMPONENTS, MAX_ENTRIES, ReviewedMove, Status, journal::Store, model::{Directory, Event, Move, Operation, Place, Plan, Receipt, Root, State}, names::Name};

const FOLDERS: [Folder; 3] = [Folder::Downloads, Folder::Desktop, Folder::Documents];

#[derive(Debug)]
pub struct Inventory {
    env: Env,
    root: native::Id,
    roots: Vec<Root>,
    captured: BTreeMap<String, (Place, native::Fingerprint)>,
    entries: Vec<InventoryEntry>,
    truncated: bool,
}

/// Opaque approval capability. No public fields, Deserialize, mutable move-list accessor or path constructor.
#[derive(Debug)]
pub struct ValidatedPlan { data: Plan, review: Vec<ReviewedMove> }
impl ValidatedPlan { pub fn review(&self) -> &[ReviewedMove] { &self.review } }

#[derive(Debug)]
pub struct Engine { env: Env, store: Store, state: State, poisoned: bool }

fn folder(env: &Env, at: Folder) -> &Path {
    match at { Folder::Downloads => &env.known.downloads, Folder::Desktop => &env.known.desktop, Folder::Documents => &env.known.documents }
}
fn path(env: &Env, place: &Place) -> PathBuf { place.names.iter().fold(folder(env, place.folder).to_path_buf(), |path, name| path.join(name.os())) }
fn chain(parent: &Parent) -> Vec<native::Id> { parent.steps.iter().map(native::Id::step).collect() }
fn changed(_: Fail) -> Error { Error::Changed }
fn denied(error: crate::Denied) -> Error { Error::Denied(error.to_string()) }
fn trust(env: &Env) -> Result<(), Error> {
    if env.platform != Platform::Windows { return Err(Error::Unsupported); }
    for at in [&env.root, &env.home, &env.app_data, &env.known.downloads, &env.known.desktop, &env.known.documents] {
        if !native::plain(at) { return Err(Error::InvalidName); }
        if let Some(confine) = &env.confined_to && (!native::plain(confine) || !at.starts_with(confine)) { return Err(Error::Denied("outside fixture profile".into())); }
    }
    if let Some(confine) = &env.confined_to {
        let places = [&env.known.pictures, &env.known.videos, &env.known.music, &env.local_app_data, &env.system_root, &env.program_data].into_iter()
            .chain(env.temp.iter()).chain(env.program_files.iter()).chain(env.cloud.iter().map(|cloud| &cloud.path));
        for at in places { if !native::plain(at) || !at.starts_with(confine) { return Err(Error::Denied("fixture environment contains an outside place".into())); } }
    }
    Ok(())
}

impl Inventory {
    /// A fresh bounded metadata snapshot, independent of the aggregate disk scan. It never reads file contents.
    /// This first engine inventories only loose regular files in the three known folders; no directory moves.
    pub fn capture(env: &Env, cancel: &CancelToken) -> Result<Self, Error> {
        trust(env)?;
        if cancel.is_cancelled() { return Err(Error::Cancelled); }
        let base = native::directory(&env.root).map_err(changed)?;
        let root = native::Id::step(base.steps.last().ok_or(Error::Changed)?);
        let guard = Guard::new(env, &env.root);
        let tag = native::nonce().map_err(changed)?;
        let mut captured = BTreeMap::new(); let mut entries = Vec::new(); let mut roots = Vec::new(); let mut visited = 0; let mut truncated = false;
        for at in FOLDERS {
            if cancel.is_cancelled() { return Err(Error::Cancelled); }
            let parent = native::directory(folder(env, at)).map_err(changed)?;
            let ids = chain(&parent);
            if !ids.contains(&root) || ids.iter().any(|id| id.volume != root.volume) { return Err(Error::Changed); }
            roots.push(Root { folder: at, chain: ids });
            if guard.judge_walked(folder(env, at), &parent.steps, true).is_err() { continue; }
            let (listed, capped) = native::list_bounded(&parent.dir, MAX_ENTRIES - visited, cancel).map_err(changed)?;
            truncated |= capped; visited += listed.len();
            for entry in listed {
                if cancel.is_cancelled() { return Err(Error::Cancelled); }
                if entry.kind != Kind::File { continue; }
                let Ok(name) = Name::checked(entry.name.encode_wide().collect()) else { continue; };
                let Ok((_, info)) = native::file(&parent.dir, &name, false) else { continue; };
                if info.id.file != entry.id || info.id.volume != root.volume { continue; }
                let mut steps = parent.steps.clone(); steps.push(entry.step(root.volume));
                if guard.judge_walked(&folder(env, at).join(name.os()), &steps, false).is_err() { continue; }
                let id = format!("{tag}-e{}", entries.len());
                entries.push(InventoryEntry { id: id.clone(), folder: at, name: name.display(), logical_bytes: info.logical, allocated_bytes: info.allocated, changed_ns: info.changed });
                captured.insert(id, (Place { folder: at, names: vec![name] }, info));
            }
        }
        if cancel.is_cancelled() { return Err(Error::Cancelled); }
        Ok(Self { env: env.clone(), root, roots, captured, entries, truncated })
    }
    pub fn entries(&self) -> &[InventoryEntry] { &self.entries }
    pub fn truncated(&self) -> bool { self.truncated }

    pub fn validate(&self, drafts: &[Draft], cancel: &CancelToken) -> Result<ValidatedPlan, Error> {
        if drafts.is_empty() || drafts.len() > MAX_ENTRIES { return Err(Error::Bounds); }
        if cancel.is_cancelled() { return Err(Error::Cancelled); }
        let selected = drafts.iter().map(|draft| self.captured.get(&draft.entry_id).map(|(place, _)| place.clone()).ok_or(Error::UnknownEntry)).collect::<Result<HashSet<_>, _>>()?;
        if selected.len() != drafts.len() { return Err(Error::UnknownEntry); }
        let guard = Guard::new(&self.env, &self.env.root);
        let mut directories = BTreeMap::<Place, Option<native::Id>>::new(); let mut moves = Vec::new(); let mut review = Vec::new(); let mut sources = HashSet::new(); let mut targets = HashSet::new();
        for draft in drafts {
            if cancel.is_cancelled() { return Err(Error::Cancelled); }
            let (source, fingerprint) = self.captured.get(&draft.entry_id).ok_or(Error::UnknownEntry)?;
            if !sources.insert(source.clone()) || draft.directories.len() >= MAX_COMPONENTS { return Err(Error::UnknownEntry); }
            let source_parent = self.root_parent(source.folder)?;
            let (_, current) = native::file(&source_parent.dir, &source.names[0], false).map_err(changed)?;
            if current != *fingerprint { return Err(Error::Changed); }
            let mut source_steps = source_parent.steps.clone(); source_steps.push(crate::fsid::Step { name: source.names[0].os(), id: crate::fsid::Identity::new(current.id.volume, current.id.file), mount: None });
            guard.judge_walked(&path(&self.env, source), &source_steps, false).map_err(denied)?;
            let mut names = draft.directories.iter().map(|text| Name::text(text)).collect::<Result<Vec<_>, _>>()?;
            let base_parent = self.root_parent(draft.destination)?;
            let mut existing = base_parent;
            let mut existing_path = folder(&self.env, draft.destination).to_path_buf();
            let mut missing = false; let mut missing_names = PathBuf::new();
            for count in 1..=names.len() {
                let at = Place { folder: draft.destination, names: names[..count].to_vec() };
                let identity = if !missing {
                    match native::child_dir(&existing.dir, &names[count - 1]) {
                        Ok((dir, step)) => {
                            let id = native::Id::step(&step); drop(dir);
                            existing_path.push(names[count - 1].os());
                            existing = native::directory(&existing_path).map_err(changed)?;
                            if native::Id::step(existing.steps.last().ok_or(Error::Changed)?) != id || id.volume != self.root.volume { return Err(Error::Changed); }
                            guard.judge_walked(&existing_path, &existing.steps, true).map_err(denied)?;
                            Some(id)
                        }
                        Err(Fail::Gone) => { missing = true; missing_names.push(names[count - 1].os()); None }
                        Err(error) => return Err(changed(error)),
                    }
                } else { missing_names.push(names[count - 1].os()); None };
                if let Some(previous) = directories.insert(at, identity) && previous != identity { return Err(Error::Changed); }
            }
            let original = draft.filename.as_deref().map(Name::text).transpose()?.unwrap_or_else(|| source.names[0].clone());
            let mut requested = names.clone(); requested.push(original.clone());
            if selected.contains(&Place { folder: draft.destination, names: requested }) { return Err(Error::UnknownEntry); }
            let mut selected = None;
            for suffix in 0..100 {
                let name = collision_name(&original, suffix)?;
                let mut candidate = names.clone(); candidate.push(name.clone());
                let to = Place { folder: draft.destination, names: candidate };
                let mut proposed = missing_names.clone(); proposed.push(name.os());
                guard.judge_absent(&existing_path, &existing.steps, &proposed).map_err(denied)?;
                if to != *source && !targets.contains(&reservation(&to)) && (missing || !native::exists(&existing.dir, &name).map_err(changed)?) { selected = Some(to); break; }
            }
            let to = selected.ok_or(Error::Bounds)?;
            targets.insert(reservation(&to));
            review.push(ReviewedMove { entry_id: draft.entry_id.clone(), from: path(&self.env, source).to_string_lossy().into_owned(), to: path(&self.env, &to).to_string_lossy().into_owned() });
            moves.push(Move { entry: draft.entry_id.clone(), from: source.clone(), to, fingerprint: *fingerprint });
            names.clear();
        }
        if moves.iter().any(|mv| moves.iter().any(|other| mv.to == other.from)) { return Err(Error::UnknownEntry); }
        let data = Plan { id: format!("run-{}", native::nonce().map_err(changed)?), root: self.root, roots: self.roots.clone(), directories: directories.into_iter().map(|(at, expected)| Directory { at, expected }).collect(), moves };
        data.checked()?;
        Ok(ValidatedPlan { data, review })
    }
    fn root_parent(&self, folder_id: Folder) -> Result<Parent, Error> {
        let parent = native::directory(folder(&self.env, folder_id)).map_err(changed)?;
        if self.roots.iter().find(|root| root.folder == folder_id).is_none_or(|root| root.chain != chain(&parent)) { return Err(Error::Changed); }
        Ok(parent)
    }
}

fn reservation(at: &Place) -> (Folder, Vec<String>) { (at.folder, at.names.iter().map(|name| name.display().to_uppercase()).collect()) }
fn collision_name(name: &Name, suffix: u32) -> Result<Name, Error> {
    if suffix == 0 { return Ok(name.clone()); }
    let dot = name.0.iter().rposition(|unit| *unit == 46).filter(|index| *index > 0).unwrap_or(name.0.len());
    Name::checked(name.0[..dot].iter().copied().chain(format!(" ({suffix})").encode_utf16()).chain(name.0[dot..].iter().copied()).collect())
}

impl Engine {
    pub fn begin(env: &Env, journal: &Path, approved: ValidatedPlan) -> Result<Self, Error> {
        trust(env)?; approved.data.checked()?;
        let expected = journal_parent(env, journal)?;
        let mut store = Store::open(journal, true, &expected)?;
        store.append(&Event::Approved { version: 1, plan: approved.data })?;
        let state = State::load(store.records())?;
        let engine = Self { env: env.clone(), store, state, poisoned: false };
        engine.root_check()?;
        Ok(engine)
    }
    /// Acquires the stable lease and reads evidence. It performs no rename, mkdir, cleanup, or automatic resume.
    pub fn open(env: &Env, journal: &Path) -> Result<Self, Error> {
        trust(env)?;
        let expected = journal_parent(env, journal)?;
        let store = Store::open(journal, false, &expected)?;
        let state = State::load(store.records())?;
        let engine = Self { env: env.clone(), store, state, poisoned: false };
        engine.root_check()?;
        Ok(engine)
    }
    pub fn status(&self) -> Status { self.state.status() }
    pub fn durability_flushes(&self) -> u64 { self.store.flushes }
    fn root_check(&self) -> Result<(), Error> {
        let parent = native::directory(&self.env.root).map_err(changed)?;
        if native::Id::step(parent.steps.last().ok_or(Error::Changed)?) != self.state.plan.root { return Err(Error::Changed); }
        Ok(())
    }
    fn record(&mut self, event: Event) -> Result<(), Error> {
        if self.poisoned { return Err(Error::StoppedUnresolved); }
        let mut candidate = self.state.clone();
        if let Err(error) = candidate.apply(&event) { self.poisoned = true; return if self.state.pending.is_empty() { Err(error) } else { Err(Error::StoppedUnresolved) }; }
        if let Err(error) = self.store.append(&event) {
            self.poisoned = true;
            return if self.state.pending.is_empty() { Err(error) } else { Err(Error::StoppedUnresolved) };
        }
        self.state = candidate;
        Ok(())
    }
    fn skip(&mut self, op: Operation, index: usize, reason: impl Into<String>) -> Result<(), Error> {
        let reason = reason.into();
        self.record(if self.state.pending.contains_key(&op.key(index)) { Event::Skipped { operation: op, index, reason } } else { Event::NotAttempted { operation: op, index, reason } })
    }
    fn parent(&self, place: &Place) -> Result<Parent, Error> {
        self.root_check()?;
        let parent = native::directory(&path(&self.env, place)).map_err(changed)?;
        let mut expected = self.state.plan.roots.iter().find(|root| root.folder == place.folder).ok_or(Error::CorruptJournal)?.chain.clone();
        for count in 1..=place.names.len() {
            expected.push(self.state.directory_identity(&Place { folder: place.folder, names: place.names[..count].to_vec() }).ok_or(Error::Changed)?);
        }
        if chain(&parent) != expected { return Err(Error::Changed); }
        Guard::new(&self.env, &self.env.root).judge_walked(&path(&self.env, place), &parent.steps, true).map_err(denied)?;
        Ok(parent)
    }
    fn inspect(&self, at: &Place) -> Result<Option<native::Fingerprint>, Error> {
        let parent = self.parent(&at.parent())?;
        match native::file(&parent.dir, at.names.last().ok_or(Error::CorruptJournal)?, false) { Ok((_, info)) => Ok(Some(info)), Err(Fail::Gone) => Ok(None), Err(error) => Err(changed(error)) }
    }
    pub fn run(&mut self, cancel: &CancelToken) -> Result<Status, Error> {
        if self.poisoned { return Err(Error::StoppedUnresolved); }
        // No automatic or implicit resume policy. A previously started run is inspectable/undoable only.
        if self.state.started { return if self.state.pending.is_empty() { Ok(self.status()) } else { Err(Error::Unresolved) }; }
        self.root_check()?; self.record(Event::Started)?;
        for index in 0..self.state.plan.directories.len() {
            if cancel.is_cancelled() { break; }
            let spec = self.state.plan.directories[index].clone();
            if spec.expected.is_some() { continue; }
            let parent = match self.parent(&spec.at.parent()) { Ok(parent) => parent, Err(error) => { self.skip(Operation::Mkdir, index, error.to_string())?; continue; } };
            let name = spec.at.names.last().ok_or(Error::CorruptJournal)?;
            let guard = Guard::new(&self.env, &self.env.root);
            guard.judge_absent(&path(&self.env, &spec.at.parent()), &parent.steps, Path::new(&name.os())).map_err(denied)?;
            self.record(Event::Intent { operation: Operation::Mkdir, index, identity: None })?;
            boundary("mkdir-intent");
            if native::revalidate(&parent).is_err() { self.skip(Operation::Mkdir, index, "parent changed before creation")?; continue; }
            if Guard::new(&self.env, &self.env.root).judge_absent(&path(&self.env, &spec.at.parent()), &parent.steps, Path::new(&name.os())).is_err() { self.skip(Operation::Mkdir, index, "Guard changed before creation")?; continue; }
            match native::mkdir(&parent.dir, name) {
                Ok((dir, step)) => { boundary("mkdir-namespace"); self.record(Event::Applied { operation: Operation::Mkdir, index, identity: native::Id::step(&step) })?; boundary("mkdir-result"); drop(dir); }
                Err(error) => {
                    if matches!(native::exists(&parent.dir, name), Ok(false)) { self.skip(Operation::Mkdir, index, format!("{error:?}"))?; }
                    else { self.record(Event::Unresolved { operation: Operation::Mkdir, index, reason: "creation outcome/ownership is uncertain".into() })?; return Err(Error::Unresolved); }
                }
            }
        }
        for index in 0..self.state.plan.moves.len() {
            if cancel.is_cancelled() { break; }
            self.move_one(index, false)?;
        }
        self.record(Event::Finished { cancelled: cancel.is_cancelled() })?;
        Ok(self.status())
    }

    fn move_one(&mut self, index: usize, undo: bool) -> Result<(), Error> {
        let mv = self.state.plan.moves[index].clone();
        let op = if undo { Operation::Undo } else { Operation::Move };
        let (from, to) = if undo { (&mv.to, &mv.from) } else { (&mv.from, &mv.to) };
        let prepare = || -> Result<_, Error> {
            let source_parent = self.parent(&from.parent())?;
            let destination_parent = self.parent(&to.parent())?;
            let (file, fingerprint) = native::file(&source_parent.dir, from.names.last().ok_or(Error::CorruptJournal)?, true).map_err(changed)?;
            if fingerprint.id != mv.fingerprint.id || fingerprint.links != mv.fingerprint.links || (!undo && fingerprint != mv.fingerprint) { return Err(Error::Changed); }
            let guard = Guard::new(&self.env, &self.env.root);
            let mut steps = source_parent.steps.clone(); steps.push(crate::fsid::Step { name: from.names.last().ok_or(Error::CorruptJournal)?.os(), id: crate::fsid::Identity::new(fingerprint.id.volume, fingerprint.id.file), mount: None });
            guard.judge_walked(&path(&self.env, from), &steps, false).map_err(denied)?;
            guard.judge_absent(&path(&self.env, &to.parent()), &destination_parent.steps, Path::new(&to.names.last().ok_or(Error::CorruptJournal)?.os())).map_err(denied)?;
            if native::exists(&destination_parent.dir, to.names.last().ok_or(Error::CorruptJournal)?).map_err(changed)? { return Err(Error::Changed); }
            Ok((source_parent, destination_parent, file))
        };
        let (source_parent, destination_parent, file) = match prepare() { Ok(value) => value, Err(error) => return self.skip(op, index, error.to_string()) };
        self.record(Event::Intent { operation: op, index, identity: Some(mv.fingerprint.id) })?;
        boundary(if undo { "undo-intent" } else { "move-intent" });
        let fresh = match native::fingerprint(&file) { Ok(info) => info, Err(_) => return self.skip(op, index, "fingerprint unavailable before rename") };
        if fresh.id != mv.fingerprint.id || fresh.links != mv.fingerprint.links || (!undo && fresh != mv.fingerprint) || native::revalidate(&source_parent).is_err() || native::revalidate(&destination_parent).is_err() { return self.skip(op, index, "fingerprint or parent changed before rename"); }
        let guard = Guard::new(&self.env, &self.env.root);
        let mut steps = source_parent.steps.clone(); steps.push(crate::fsid::Step { name: from.names.last().ok_or(Error::CorruptJournal)?.os(), id: crate::fsid::Identity::new(fresh.id.volume, fresh.id.file), mount: None });
        if guard.judge_walked(&path(&self.env, from), &steps, false).is_err() || guard.judge_absent(&path(&self.env, &to.parent()), &destination_parent.steps, Path::new(&to.names.last().ok_or(Error::CorruptJournal)?.os())).is_err() { return self.skip(op, index, "Guard changed before rename"); }
        let result = native::rename(&file, &destination_parent.dir, to.names.last().ok_or(Error::CorruptJournal)?);
        boundary(if undo { "undo-namespace" } else { "move-namespace" });
        // Both retained parent owners stay alive until after the durable result. No guessed success on an error.
        let observed = self.classify_move(&mv, undo);
        match observed {
            Reconciliation::Applied => self.record(Event::Applied { operation: op, index, identity: mv.fingerprint.id })?,
            Reconciliation::NotApplied => self.skip(op, index, format!("rename left source in place: {result:?}"))?,
            Reconciliation::Unresolved => { self.record(Event::Unresolved { operation: op, index, reason: "rename identity state is ambiguous".into() })?; return Err(Error::Unresolved); }
        }
        boundary(if undo { "undo-result" } else { "move-result" });
        drop(file);
        drop(source_parent); drop(destination_parent);
        Ok(())
    }
    fn classify_move(&self, mv: &Move, undo: bool) -> Reconciliation {
        let (source, target) = if undo { (&mv.to, &mv.from) } else { (&mv.from, &mv.to) };
        let (Ok(source), Ok(target)) = (self.inspect(source), self.inspect(target)) else { return Reconciliation::Unresolved; };
        let expected = |info: native::Fingerprint| info.id == mv.fingerprint.id && info.links == mv.fingerprint.links;
        match (source, target) {
            (None, Some(info)) if expected(info) => Reconciliation::Applied,
            (Some(info), target) if expected(info) && target.is_none_or(|other| other.id != info.id) => Reconciliation::NotApplied,
            _ => Reconciliation::Unresolved,
        }
    }
    /// Metadata/status reconciliation only. It never invokes rename, mkdir or directory removal.
    pub fn reconcile(&mut self, cancel: &CancelToken) -> Result<Status, Error> {
        if self.poisoned { return Err(Error::StoppedUnresolved); }
        self.root_check()?;
        let pending = self.state.pending.clone();
        for ((kind, index), identity) in pending {
            if cancel.is_cancelled() { break; }
            let op = match kind { 0 => Operation::Move, 1 => Operation::Undo, 2 => Operation::Mkdir, 3 => Operation::RemoveDirectory, _ => return Err(Error::CorruptJournal) };
            let outcome = if matches!(op, Operation::Move | Operation::Undo) { self.classify_move(&self.state.plan.moves[index], op == Operation::Undo) }
                else { self.classify_directory(index, identity, op == Operation::RemoveDirectory) };
            match outcome {
                Reconciliation::Applied => self.record(Event::Applied { operation: op, index, identity: identity.ok_or(Error::CorruptJournal)? })?,
                Reconciliation::NotApplied => self.skip(op, index, "reconciled: namespace action not currently applied")?,
                Reconciliation::Unresolved if self.state.receipts.get(&op.key(index)) != Some(&Receipt::Unresolved) => self.record(Event::Unresolved { operation: op, index, reason: "recovery preserved ambiguous identities".into() })?,
                Reconciliation::Unresolved => (),
            }
            boundary("reconcile-result");
        }
        Ok(self.status())
    }
    fn classify_directory(&self, index: usize, expected: Option<native::Id>, removing: bool) -> Reconciliation {
        let at = &self.state.plan.directories[index].at;
        let Ok(parent) = self.parent(&at.parent()) else { return Reconciliation::Unresolved; };
        match native::child_dir(&parent.dir, at.names.last().expect("validated directory name")) {
            Err(Fail::Gone) => if removing { Reconciliation::Applied } else { Reconciliation::NotApplied },
            Ok((_, step)) if removing && Some(native::Id::step(&step)) == expected => Reconciliation::NotApplied,
            _ => Reconciliation::Unresolved, // mkdir without a durable ownership receipt never gains ownership.
        }
    }
    pub fn undo(&mut self, cancel: &CancelToken) -> Result<Status, Error> {
        if self.poisoned { return Err(Error::StoppedUnresolved); }
        if !self.state.pending.is_empty() { return Err(Error::Unresolved); }
        self.root_check()?;
        for index in (0..self.state.plan.moves.len()).rev() {
            if cancel.is_cancelled() { return Ok(self.status()); }
            if matches!(self.state.receipts.get(&Operation::Move.key(index)), Some(Receipt::Applied(_))) && !matches!(self.state.receipts.get(&Operation::Undo.key(index)), Some(Receipt::Applied(_))) { self.move_one(index, true)?; }
        }
        for index in (0..self.state.plan.directories.len()).rev() {
            if cancel.is_cancelled() { break; }
            let Some(Receipt::Applied(id)) = self.state.receipts.get(&Operation::Mkdir.key(index)).copied() else { continue; };
            if matches!(self.state.receipts.get(&Operation::RemoveDirectory.key(index)), Some(Receipt::Applied(_))) { continue; }
            let at = self.state.plan.directories[index].at.clone();
            let parent = match self.parent(&at.parent()) { Ok(parent) => parent, Err(error) => { self.skip(Operation::RemoveDirectory, index, error.to_string())?; continue; } };
            let (dir, step) = match native::prepare_removal(&parent.dir, at.names.last().ok_or(Error::CorruptJournal)?, id) { Ok(value) => value, Err(error) => { self.skip(Operation::RemoveDirectory, index, format!("{error:?}"))?; continue; } };
            let mut steps = parent.steps.clone(); steps.push(step);
            if Guard::new(&self.env, &self.env.root).judge_walked(&path(&self.env, &at), &steps, false).is_err() { self.skip(Operation::RemoveDirectory, index, "Guard refused directory cleanup")?; continue; }
            self.record(Event::Intent { operation: Operation::RemoveDirectory, index, identity: Some(id) })?;
            boundary("remove-intent");
            if native::revalidate(&parent).is_err() || native::validate_directory(&dir, id).is_err() || Guard::new(&self.env, &self.env.root).judge_walked(&path(&self.env, &at), &steps, false).is_err() { self.skip(Operation::RemoveDirectory, index, "directory changed before cleanup")?; continue; }
            match native::remove_prepared(&parent.dir, at.names.last().ok_or(Error::CorruptJournal)?, dir) {
                Ok(()) => { boundary("remove-namespace"); self.record(Event::Applied { operation: Operation::RemoveDirectory, index, identity: id })?; boundary("remove-result"); }
                Err(error) => match self.classify_directory(index, Some(id), true) {
                    Reconciliation::Applied => self.record(Event::Applied { operation: Operation::RemoveDirectory, index, identity: id })?,
                    Reconciliation::NotApplied => self.skip(Operation::RemoveDirectory, index, format!("{error:?}"))?,
                    Reconciliation::Unresolved => { self.record(Event::Unresolved { operation: Operation::RemoveDirectory, index, reason: "directory removal outcome is uncertain".into() })?; return Err(Error::Unresolved); }
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
    if std::env::var("VACANCY_T08_CRASH_AT").ok().as_deref() == Some(name) { std::process::exit(73); }
    #[cfg(not(test))]
    let _ = name;
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

enum Reconciliation { Applied, NotApplied, Unresolved }
fn journal_parent(env: &Env, journal: &Path) -> Result<Vec<native::Id>, Error> {
    if !native::plain(journal) || journal.parent() != Some(env.app_data.as_path()) { return Err(Error::Denied("journal must be a new file directly in trusted app data".into())); }
    let parent = native::directory(&env.app_data).map_err(changed)?;
    let root = native::directory(&env.root).map_err(changed)?;
    let id = native::Id::step(root.steps.last().ok_or(Error::Changed)?);
    let chain = chain(&parent);
    if !chain.contains(&id) || chain.iter().any(|entry| entry.volume != id.volume) { return Err(Error::Changed); }
    Ok(chain)
}

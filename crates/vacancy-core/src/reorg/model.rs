use super::{Error, Folder, MAX_COMPONENTS, MAX_ENTRIES, Status, names::Name};
use crate::exec::sys::reorg::{Fingerprint, Id};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Place {
    pub folder: Folder,
    pub names: Vec<Name>,
}

impl Place {
    pub fn checked(&self, leaf: bool) -> Result<(), Error> {
        if self.names.len() > MAX_COMPONENTS || (leaf && self.names.is_empty()) {
            return Err(Error::Bounds);
        }
        for name in &self.names {
            Name::checked(name.0.clone())?;
        }
        Ok(())
    }
    pub fn parent(&self) -> Self {
        Self {
            folder: self.folder,
            names: self.names[..self.names.len() - 1].to_vec(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Root {
    pub folder: Folder,
    pub chain: Vec<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Directory {
    pub at: Place,
    pub expected: Option<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Move {
    pub entry: String,
    pub from: Place,
    pub to: Place,
    pub fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub id: String,
    pub root: Id,
    pub roots: Vec<Root>,
    pub directories: Vec<Directory>,
    pub moves: Vec<Move>,
}

impl Plan {
    pub fn checked(&self) -> Result<(), Error> {
        if self.id.len() != 36
            || !self.id.starts_with("run-")
            || !self.id[4..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(Error::CorruptJournal);
        }
        if self.moves.is_empty()
            || self.moves.len() > MAX_ENTRIES
            || self.roots.len() != 3
            || self.directories.len() > MAX_ENTRIES * MAX_COMPONENTS
        {
            return Err(Error::CorruptJournal);
        }
        let mut roots = HashSet::new();
        for root in &self.roots {
            if !roots.insert(root.folder)
                || root.chain.is_empty()
                || root.chain.len() > 128
                || root.chain.iter().any(|id| id.volume != self.root.volume)
                || !root.chain.contains(&self.root)
            {
                return Err(Error::CorruptJournal);
            }
        }
        for (index, root) in self.roots.iter().enumerate() {
            if self.roots[..index].iter().any(|other| {
                root.chain.starts_with(&other.chain) || other.chain.starts_with(&root.chain)
            }) {
                return Err(Error::CorruptJournal);
            }
        }
        let mut dirs = HashSet::new();
        let mut directory_aliases = HashSet::new();
        for dir in &self.directories {
            dir.at.checked(true)?;
            if !dirs.insert(dir.at.clone())
                || !directory_aliases.insert((
                    dir.at.folder,
                    dir.at
                        .names
                        .iter()
                        .map(crate::exec::sys::reorg::case_key)
                        .collect::<Vec<_>>(),
                ))
                || dir.expected.is_some_and(|id| id.volume != self.root.volume)
            {
                return Err(Error::CorruptJournal);
            }
        }
        let mut sources = HashSet::new();
        let mut destinations = HashSet::new();
        let mut used_directories = HashSet::new();
        for directory in &self.directories {
            for count in 1..directory.at.names.len() {
                let parent = Place {
                    folder: directory.at.folder,
                    names: directory.at.names[..count].to_vec(),
                };
                let Some(spec) = self.directories.iter().find(|spec| spec.at == parent) else {
                    return Err(Error::CorruptJournal);
                };
                if directory.expected.is_some() && spec.expected.is_none() {
                    return Err(Error::CorruptJournal);
                }
            }
        }
        for mv in &self.moves {
            mv.from.checked(true)?;
            mv.to.checked(true)?;
            if mv.from.names.len() != 1
                || mv.fingerprint.id.volume != self.root.volume
                || mv.fingerprint.links == 0
                || !sources.insert(mv.from.clone())
                || !destinations.insert(mv.to.clone())
                || mv.from == mv.to
            {
                return Err(Error::CorruptJournal);
            }
            for count in 1..mv.to.names.len() {
                let parent = Place {
                    folder: mv.to.folder,
                    names: mv.to.names[..count].to_vec(),
                };
                if !dirs.contains(&parent) {
                    return Err(Error::CorruptJournal);
                }
                used_directories.insert(parent);
            }
        }
        if dirs != used_directories || sources.iter().any(|p| destinations.contains(p)) {
            return Err(Error::CorruptJournal);
        }
        Ok(())
    }
}

pub(super) fn check_approval(plan: &Plan) -> Result<(), Error> {
    #[derive(Serialize)]
    #[serde(tag = "kind", rename_all = "camelCase")]
    enum Approval<'a> {
        Approved { version: u32, plan: &'a Plan },
    }
    super::journal::check_record(&Approval::Approved { version: 1, plan })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Operation {
    Move,
    Undo,
    Mkdir,
    RemoveDirectory,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "camelCase")]
pub(super) enum Event {
    Approved {
        version: u32,
        plan: Plan,
    },
    Started,
    Intent {
        operation: Operation,
        index: usize,
        identity: Option<Id>,
    },
    Applied {
        operation: Operation,
        index: usize,
        identity: Id,
    },
    Skipped {
        operation: Operation,
        index: usize,
        reason: String,
    },
    NotAttempted {
        operation: Operation,
        index: usize,
        reason: String,
    },
    Unresolved {
        operation: Operation,
        index: usize,
        reason: String,
    },
    Finished {
        cancelled: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Receipt {
    Applied(Id),
    Skipped,
    Unresolved,
}

#[derive(Clone, Debug)]
pub(super) struct State {
    pub plan: Arc<Plan>,
    pub started: bool,
    pub finished: bool,
    pub cancelled: bool,
    rollback_started: bool,
    pub pending: BTreeMap<(u8, usize), Option<Id>>,
    pub receipts: BTreeMap<(u8, usize), Receipt>,
    pub reasons: Vec<String>,
}

impl Operation {
    pub fn key(self, index: usize) -> (u8, usize) {
        (self as u8, index)
    }
}
impl State {
    pub fn load(values: &[serde_json::Value]) -> Result<Self, Error> {
        let events = values
            .iter()
            .cloned()
            .map(serde_json::from_value)
            .collect::<Result<Vec<Event>, _>>()
            .map_err(|_| Error::CorruptJournal)?;
        let Some(Event::Approved { version: 1, plan }) = events.first() else {
            return Err(Error::CorruptJournal);
        };
        plan.checked().map_err(|_| Error::CorruptJournal)?;
        for (event, value) in events.iter().zip(values) {
            if serde_json::to_value(event).map_err(|_| Error::CorruptJournal)? != *value {
                return Err(Error::CorruptJournal);
            }
        }
        let mut state = Self {
            plan: Arc::new(plan.clone()),
            started: false,
            finished: false,
            cancelled: false,
            rollback_started: false,
            pending: BTreeMap::new(),
            receipts: BTreeMap::new(),
            reasons: Vec::new(),
        };
        for event in &events[1..] {
            state.apply(event)?;
        }
        Ok(state)
    }
    pub fn apply(&mut self, event: &Event) -> Result<(), Error> {
        match event {
            Event::Approved { .. } => return Err(Error::CorruptJournal),
            Event::Started if !self.started => self.started = true,
            Event::Started => return Err(Error::CorruptJournal),
            Event::Finished { cancelled }
                if self.started
                    && !self.finished
                    && !self.rollback_started
                    && self.pending.is_empty() =>
            {
                if !cancelled
                    && ((0..self.plan.moves.len())
                        .any(|index| !self.terminal(Operation::Move.key(index)))
                        || !self.directories_terminal())
                {
                    return Err(Error::CorruptJournal);
                }
                self.finished = true;
                self.cancelled = *cancelled;
            }
            Event::Finished { .. } => return Err(Error::CorruptJournal),
            Event::Intent {
                operation,
                index,
                identity,
            } => {
                let expected = self.before_operation(*operation, *index, true)?;
                let key = operation.key(*index);
                if *identity != expected {
                    return Err(Error::CorruptJournal);
                }
                self.rollback_started |=
                    matches!(operation, Operation::Undo | Operation::RemoveDirectory);
                self.pending.insert(key, *identity);
                self.receipts.remove(&key);
            }
            Event::Applied {
                operation,
                index,
                identity,
            } => {
                self.valid_index(*operation, *index)?;
                let key = operation.key(*index);
                let expected = self.pending.remove(&key).ok_or(Error::CorruptJournal)?;
                if identity.volume != self.plan.root.volume
                    || expected.is_some_and(|id| id != *identity)
                {
                    return Err(Error::CorruptJournal);
                }
                self.receipts.insert(key, Receipt::Applied(*identity));
            }
            Event::Skipped {
                operation,
                index,
                reason,
            }
            | Event::Unresolved {
                operation,
                index,
                reason,
            } => {
                self.valid_index(*operation, *index)?;
                if reason.len() > 1024 {
                    return Err(Error::CorruptJournal);
                }
                let key = operation.key(*index);
                // Unresolved may later gain a reconciled receipt for the same original intent.
                if !self.pending.contains_key(&key) {
                    return Err(Error::CorruptJournal);
                }
                if matches!(event, Event::Skipped { .. }) {
                    self.pending.remove(&key);
                    self.receipts.insert(key, Receipt::Skipped);
                } else {
                    self.receipts.insert(key, Receipt::Unresolved);
                }
                self.reasons.push(reason.clone());
            }
            Event::NotAttempted {
                operation,
                index,
                reason,
            } => {
                self.before_operation(*operation, *index, false)?;
                let key = operation.key(*index);
                if reason.len() > 1024 {
                    return Err(Error::CorruptJournal);
                }
                self.rollback_started |=
                    matches!(operation, Operation::Undo | Operation::RemoveDirectory);
                self.receipts.insert(key, Receipt::Skipped);
                self.reasons.push(reason.clone());
            }
        }
        Ok(())
    }
    fn terminal(&self, key: (u8, usize)) -> bool {
        matches!(
            self.receipts.get(&key),
            Some(Receipt::Applied(_) | Receipt::Skipped)
        )
    }
    fn directories_terminal(&self) -> bool {
        self.plan
            .directories
            .iter()
            .enumerate()
            .all(|(index, directory)| {
                directory.expected.is_some() || self.terminal(Operation::Mkdir.key(index))
            })
    }
    fn parent_owned(&self, place: &Place) -> bool {
        (1..place.names.len()).all(|count| {
            self.directory_identity(&Place {
                folder: place.folder,
                names: place.names[..count].to_vec(),
            })
            .is_some()
        })
    }
    /// Validate the whole execution phase before either an intent or a pre-action skip. Parent ownership is
    /// required only for intents: a missing/skipped parent is precisely why a legitimate skip can occur.
    fn before_operation(
        &self,
        operation: Operation,
        index: usize,
        intent: bool,
    ) -> Result<Option<Id>, Error> {
        self.valid_index(operation, index)?;
        let reverse = matches!(operation, Operation::Undo | Operation::RemoveDirectory);
        let key = operation.key(index);
        if !self.started
            || !self.pending.is_empty()
            || (!reverse && (self.finished || self.rollback_started))
            || (self.receipts.contains_key(&key)
                && !(reverse && self.receipts.get(&key) == Some(&Receipt::Skipped)))
        {
            return Err(Error::CorruptJournal);
        }
        let expected = match operation {
            Operation::Move => {
                if !self.directories_terminal()
                    || (0..index).any(|previous| !self.terminal(Operation::Move.key(previous)))
                {
                    return Err(Error::CorruptJournal);
                }
                let mv = &self.plan.moves[index];
                if intent && (!self.parent_owned(&mv.from) || !self.parent_owned(&mv.to)) {
                    return Err(Error::CorruptJournal);
                }
                Some(mv.fingerprint.id)
            }
            Operation::Undo => {
                if !matches!(
                    self.receipts.get(&Operation::Move.key(index)),
                    Some(Receipt::Applied(_))
                ) || (index + 1..self.plan.moves.len()).any(|later| {
                    matches!(
                        self.receipts.get(&Operation::Move.key(later)),
                        Some(Receipt::Applied(_))
                    ) && !self.terminal(Operation::Undo.key(later))
                }) {
                    return Err(Error::CorruptJournal);
                }
                let mv = &self.plan.moves[index];
                if intent && (!self.parent_owned(&mv.from) || !self.parent_owned(&mv.to)) {
                    return Err(Error::CorruptJournal);
                }
                Some(mv.fingerprint.id)
            }
            Operation::Mkdir => {
                let dir = &self.plan.directories[index];
                if dir.expected.is_some() || (intent && !self.parent_owned(&dir.at)) {
                    return Err(Error::CorruptJournal);
                }
                None
            }
            Operation::RemoveDirectory => {
                if self.plan.moves.iter().enumerate().any(|(index, _)| {
                    matches!(
                        self.receipts.get(&Operation::Move.key(index)),
                        Some(Receipt::Applied(_))
                    ) && !self.terminal(Operation::Undo.key(index))
                }) {
                    return Err(Error::CorruptJournal);
                }
                if intent && !self.parent_owned(&self.plan.directories[index].at) {
                    return Err(Error::CorruptJournal);
                }
                match self.receipts.get(&Operation::Mkdir.key(index)) {
                    Some(Receipt::Applied(id)) => Some(*id),
                    _ => return Err(Error::CorruptJournal),
                }
            }
        };
        Ok(expected)
    }
    fn valid_index(&self, operation: Operation, index: usize) -> Result<(), Error> {
        if index
            >= match operation {
                Operation::Move | Operation::Undo => self.plan.moves.len(),
                _ => self.plan.directories.len(),
            }
        {
            return Err(Error::CorruptJournal);
        }
        Ok(())
    }
    pub fn directory_identity(&self, at: &Place) -> Option<Id> {
        self.plan
            .directories
            .iter()
            .enumerate()
            .find(|(_, dir)| &dir.at == at)
            .and_then(|(index, dir)| {
                if matches!(
                    self.receipts.get(&Operation::RemoveDirectory.key(index)),
                    Some(Receipt::Applied(_))
                ) {
                    return None;
                }
                dir.expected
                    .or_else(|| match self.receipts.get(&Operation::Mkdir.key(index)) {
                        Some(Receipt::Applied(id)) => Some(*id),
                        _ => None,
                    })
            })
    }
    pub fn status(&self) -> Status {
        Status {
            run_id: self.plan.id.clone(),
            applied: self
                .receipts
                .iter()
                .filter(|((op, _), r)| {
                    *op == Operation::Move as u8 && matches!(r, Receipt::Applied(_))
                })
                .count() as u32,
            undone: self
                .receipts
                .iter()
                .filter(|((op, _), r)| {
                    *op == Operation::Undo as u8 && matches!(r, Receipt::Applied(_))
                })
                .count() as u32,
            skipped: self
                .receipts
                .values()
                .filter(|r| **r == Receipt::Skipped)
                .count() as u32,
            unresolved: self.pending.len() as u32,
            cancelled: self.cancelled,
            finished: self.finished,
            reasons: self.reasons.clone(),
        }
    }
}

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
        let mut dirs = HashSet::new();
        for dir in &self.directories {
            dir.at.checked(true)?;
            if !dirs.insert(dir.at.clone())
                || dir.expected.is_some_and(|id| id.volume != self.root.volume)
            {
                return Err(Error::CorruptJournal);
            }
        }
        let mut sources = HashSet::new();
        let mut destinations = HashSet::new();
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
            }
        }
        if sources.iter().any(|p| destinations.contains(p)) {
            return Err(Error::CorruptJournal);
        }
        Ok(())
    }
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
                if self.started && !self.finished && self.pending.is_empty() =>
            {
                self.finished = true;
                self.cancelled = *cancelled;
            }
            Event::Finished { .. } => return Err(Error::CorruptJournal),
            Event::Intent {
                operation,
                index,
                identity,
            } => {
                self.valid_index(*operation, *index)?;
                let key = operation.key(*index);
                if !self.started || self.pending.contains_key(&key) {
                    return Err(Error::CorruptJournal);
                }
                if self.receipts.contains_key(&key)
                    && !(matches!(operation, Operation::Undo | Operation::RemoveDirectory)
                        && self.receipts.get(&key) == Some(&Receipt::Skipped))
                {
                    return Err(Error::CorruptJournal);
                }
                if self.finished && matches!(operation, Operation::Move | Operation::Mkdir) {
                    return Err(Error::CorruptJournal);
                }
                let expected = match operation {
                    Operation::Move | Operation::Undo => {
                        Some(self.plan.moves[*index].fingerprint.id)
                    }
                    Operation::Mkdir => None,
                    Operation::RemoveDirectory => {
                        match self.receipts.get(&Operation::Mkdir.key(*index)) {
                            Some(Receipt::Applied(id)) => Some(*id),
                            _ => return Err(Error::CorruptJournal),
                        }
                    }
                };
                if *identity != expected
                    || (*operation == Operation::Undo
                        && !matches!(
                            self.receipts.get(&Operation::Move.key(*index)),
                            Some(Receipt::Applied(_))
                        ))
                    || (*operation == Operation::Mkdir
                        && self.plan.directories[*index].expected.is_some())
                {
                    return Err(Error::CorruptJournal);
                }
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
                self.valid_index(*operation, *index)?;
                let key = operation.key(*index);
                if !self.started
                    || self.pending.contains_key(&key)
                    || reason.len() > 1024
                    || matches!(
                        self.receipts.get(&key),
                        Some(Receipt::Applied(_) | Receipt::Unresolved)
                    )
                {
                    return Err(Error::CorruptJournal);
                }
                self.receipts.insert(key, Receipt::Skipped);
                self.reasons.push(reason.clone());
            }
        }
        Ok(())
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

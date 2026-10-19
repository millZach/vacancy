//! The Windows journal/Undo backend. The app's Tidy up (see [`crate::tidy`]) drafts its moves from built-in
//! sorting rules; no provider or AI output reaches it. Non-Windows entry points are `Unsupported`.
#[cfg(windows)]
mod journal;
#[cfg(windows)]
mod model;
#[cfg(any(windows, test))]
pub(crate) mod names;
#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use unsupported::{Engine, Inventory, ValidatedPlan};
#[cfg(windows)]
pub use windows::{Engine, Inventory, ValidatedPlan};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The most entries one inventory lists, and the most moves one plan makes.
pub const ENTRY_LIMIT: usize = 512;

/// A known folder whose loose files can be reorganized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TidyFolder")]
pub enum Folder {
    Downloads,
    Desktop,
    Documents,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryEntry {
    pub id: String,
    pub folder: Folder,
    pub name: String,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    /// The latest of its creation, last-write and change times, in nanoseconds since the Unix epoch.
    pub changed_ns: i128,
    /// Its last-write (modified) time, in nanoseconds since the Unix epoch.
    pub modified_ns: i128,
    /// Its file id on its volume, as the scan records file identities.
    pub file_id: u128,
}

/// Untrusted metadata-only proposal input. It has no native mutation authority.
#[derive(Clone, Debug)]
pub struct Draft {
    pub entry_id: String,
    pub destination: Folder,
    pub directories: Vec<String>,
    pub filename: Option<String>,
}

/// Final reviewed names, including collision suffixes. These display strings never authorize an operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedMove {
    pub entry_id: String,
    pub from: String,
    pub to: String,
    /// The destination's directories below its known folder, and its final file name (collision suffix
    /// included), as display components.
    pub directories: Vec<String>,
    pub filename: String,
}

/// A directory a validated plan moves files into, below its known folder, as display components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedDirectory {
    pub folder: Folder,
    pub names: Vec<String>,
    /// It already existed at validation: the run never creates or removes it.
    pub existing: bool,
}

/// Where one journaled operation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpState {
    /// No record of it: never attempted (or, for an existing directory, never needed).
    NotRun,
    /// Its intent is recorded without a result.
    Pending,
    Applied,
    /// Skipped or not attempted, with the recorded reason.
    Skipped(String),
    /// Its outcome is ambiguous and preserved for explicit reconciliation.
    Unresolved(String),
}

/// One move of a journaled plan, and what became of it and of its Undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerMove {
    pub entry_id: String,
    pub folder: Folder,
    /// Display components below the known folder.
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub allocated_bytes: u64,
    pub moved: OpState,
    pub undo: OpState,
}

/// One directory of a journaled plan: whether this run made it, and whether Undo removed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerDirectory {
    pub folder: Folder,
    pub names: Vec<String>,
    pub existing: bool,
    pub created: OpState,
    pub removed: OpState,
}

/// Everything a journal records about its plan, for the app to report. Display text only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    pub run_id: String,
    pub started: bool,
    pub finished: bool,
    pub cancelled: bool,
    pub moves: Vec<LedgerMove>,
    pub directories: Vec<LedgerDirectory>,
}

/// A draft validation left out of the plan, and why. Other drafts are still validated and kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedDraft {
    pub entry_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub run_id: String,
    pub applied: u32,
    pub undone: u32,
    /// Current skipped operation receipts across moves, directories and Undo; this is not a file count.
    pub skipped: u32,
    pub unresolved: u32,
    pub cancelled: bool,
    pub finished: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("reorganization mutation is unsupported on this platform or filesystem")]
    Unsupported,
    #[error("the bounded inventory, plan or journal limit was reached")]
    Bounds,
    #[error("the proposed name is not a single unambiguous native component")]
    InvalidName,
    #[error("the journal cannot be durably written: {0}")]
    JournalUnavailable(String),
    #[error("another process holds the stable journal lease")]
    Busy,
    #[error("the journal has a corrupt, unsupported or illegal record")]
    CorruptJournal,
    #[error("reorganization was cancelled before approval")]
    Cancelled,
    #[error("the draft does not belong to this inventory, or repeats a source")]
    UnknownEntry,
    #[error("a file, parent or volume identity changed")]
    Changed,
    #[error("an app has the file open")]
    InUse,
    #[error("the file is no longer there")]
    Gone,
    #[error("another file or folder already has that name")]
    Taken,
    #[error("the Guard refused this operation: {0}")]
    Denied(String),
    #[error("writer failed; later mutations stopped with unresolved intent")]
    StoppedUnresolved,
    #[error("an interrupted operation is ambiguous; preserve it for explicit reconciliation")]
    Unresolved,
}

#[cfg(windows)]
const MAX_ENTRIES: usize = ENTRY_LIMIT;
#[cfg(windows)]
const MAX_COMPONENTS: usize = 12;

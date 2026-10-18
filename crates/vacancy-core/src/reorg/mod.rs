//! Isolated Windows journal/Undo backend. No UI, provider or Tauri command integration.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub changed_ns: i128,
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
    #[error("the Guard refused this operation: {0}")]
    Denied(String),
    #[error("writer failed; later mutations stopped with unresolved intent")]
    StoppedUnresolved,
    #[error("an interrupted operation is ambiguous; preserve it for explicit reconciliation")]
    Unresolved,
}

#[cfg(windows)]
const MAX_ENTRIES: usize = 512;
#[cfg(windows)]
const MAX_COMPONENTS: usize = 12;

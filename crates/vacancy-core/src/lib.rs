//! Vacancy's core library. Everything that reads or changes the disk lives here, behind a small public API, with
//! no Tauri dependency. The desktop app (`src-tauri`) is a thin shell that exposes these calls to the UI.
//!
//! Types the UI sees derive [`ts_rs::TS`]. `cargo test` writes their TypeScript to `src/bindings/` at the repo
//! root, and CI fails if the checked-in copy is stale.

pub mod advisor;
mod categorize;
mod exec;
pub mod fixture;
mod fsid;
mod guard;
pub mod hints;
mod machine;
mod map;
mod pathkey;
pub mod permissions;
mod plan;
pub mod reorg;
pub mod report;
mod rules;
mod scan;
mod snapshot;
mod text;
pub mod tools;
mod volume;

// Shared fixture setup is compiled only into Windows unit tests, never the library API.
#[cfg(all(test, windows))]
#[path = "../tests/support/windows_case.rs"]
mod windows_case;

pub use categorize::{
    Categories, Category, CategoryId, CloudRoot, Env, Item, ItemRef, KnownFolders, Platform,
    categorize,
};
pub use exec::{Deleted, RunEvent, RunProgress, Skipped, StepFinished, StepStarted, execute};
pub use guard::{Denied, Guard};
pub use machine::machine_name;
pub use map::{
    CategoryTotal, ItemKind, MapCategory, MapError, MapFolder, MapItem, Overview, ScanOutcome,
    ScanSummary, ScanUpdate, ScannedVolume, browse, overview, scan_for_map, scan_for_map_with,
    scan_update,
};
pub use plan::{Plan, PlanError, PlanStep, PlanSummary, plan};
pub use report::{
    CategoryFreed, LeftForYou, Outcome, Reorganized, RunHistory, RunListing, RunReport, Skip,
    SkipKind, StepReport,
};
pub use rules::{
    ActionKind, Moment, Proposal, ProposalAction, ProposalSource, Proposals, Risk, propose,
};
pub use scan::{CancelToken, ScanError, ScanOptions, ScanProgress, scan};
pub use snapshot::{
    DirFlags, DirId, FileId, FileInfo, Markers, Root, RootKind, ScanStats, Snapshot, Totals,
    UnixTime,
};
pub use volume::{DriveKind, VolumeError, VolumeInfo, list_volumes, system_volume, volume_info};

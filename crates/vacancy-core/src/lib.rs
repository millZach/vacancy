//! Vacancy's core library. Everything that reads or changes the disk lives here, behind a small public API, with
//! no Tauri dependency. The desktop app (`src-tauri`) is a thin shell that exposes these calls to the UI.
//!
//! Types the UI sees derive [`ts_rs::TS`]. `cargo test` writes their TypeScript to `src/bindings/` at the repo
//! root, and CI fails if the checked-in copy is stale.

mod machine;
mod volume;

pub use machine::machine_name;
pub use volume::{VolumeError, VolumeInfo, system_volume, volume_info};

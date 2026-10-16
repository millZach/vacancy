//! The Vacancy desktop app: a thin Tauri shell. Each command forwards to `vacancy-core` and turns its errors into
//! messages for the UI. Disk logic never lives here.
//!
//! The UI calls these commands through `src/bridge/tauri.ts`; keep the command names in step with it.

use vacancy_core::VolumeInfo;

/// The drive the OS runs from, for the scan screen's plate. Read-only.
#[tauri::command]
async fn system_volume() -> Result<VolumeInfo, String> {
    vacancy_core::system_volume().map_err(|e| e.to_string())
}

/// This computer's name, for the plate's caption.
#[tauri::command]
async fn machine_name() -> String {
    vacancy_core::machine_name()
}

/// Starts the app and blocks until its last window closes.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![system_volume, machine_name])
        .run(tauri::generate_context!())
        .expect("Vacancy failed to start");
}

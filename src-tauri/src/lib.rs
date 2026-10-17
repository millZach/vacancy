//! The Vacancy desktop app: a thin Tauri shell. Each command forwards to `vacancy-core` and turns its errors into
//! messages for the UI. Disk logic never lives here.
//!
//! The UI calls these commands through `src/bridge/tauri.ts`; keep the command names in step with it.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::State;
use tauri::ipc::Channel;
use vacancy_core::permissions::{self, FullDiskAccess};
use vacancy_core::{
    CancelToken, Env, MapFolder, ScanError, ScanOutcome, ScanUpdate, ScannedVolume, VolumeInfo,
};

/// What the app remembers between commands: the running scan's cancel token and the last finished scan, which
/// the map's drill-down reads.
#[derive(Default)]
struct Session {
    cancel: Mutex<Option<CancelToken>>,
    last: Mutex<Option<ScannedVolume>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

/// Local volumes for the drive picker, the system volume first. Read-only.
#[tauri::command]
async fn list_volumes() -> Vec<VolumeInfo> {
    vacancy_core::list_volumes()
}

/// Whether macOS lets Vacancy see everything (always "notApplicable" on Windows).
#[tauri::command]
async fn full_disk_access() -> FullDiskAccess {
    permissions::full_disk_access()
}

/// Opens System Settings at Full Disk Access. macOS only.
#[tauri::command]
async fn open_full_disk_access_settings() -> Result<(), String> {
    permissions::open_full_disk_access_settings().map_err(|e| e.to_string())
}

/// Scans a volume, read-only, streaming the HUD's numbers to `on_update`, and returns the overview map (or
/// "cancelled"). A scan already running is cancelled first.
#[tauri::command]
async fn scan_volume(
    mount_point: String,
    on_update: Channel<ScanUpdate>,
    session: State<'_, Session>,
) -> Result<ScanOutcome, String> {
    let cancel = CancelToken::new();
    if let Some(previous) = lock(&session.cancel).replace(cancel.clone()) {
        previous.cancel();
    }
    // The last map's memory goes back before a new scan builds its own.
    lock(&session.last).take();
    let root = PathBuf::from(&mount_point);
    let token = cancel.clone();
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        let env = Env::detect(&root);
        vacancy_core::scan_for_map(
            &root,
            &env,
            &mut |update| {
                // The UI may have gone away mid-scan; the scan finishes either way.
                let _ = on_update.send(update);
            },
            &token,
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    {
        // Forget this scan's token, unless a newer scan has replaced it.
        let mut running = lock(&session.cancel);
        if running.as_ref() == Some(&cancel) {
            running.take();
        }
    }
    match scanned {
        Ok(scanned) => {
            let overview = Box::new(scanned.overview.clone());
            *lock(&session.last) = Some(scanned);
            Ok(ScanOutcome::Done { overview })
        }
        Err(ScanError::Cancelled) => Ok(ScanOutcome::Cancelled),
        Err(e) => Err(e.to_string()),
    }
}

/// Stops the running scan, if there is one.
#[tauri::command]
fn cancel_scan(session: State<'_, Session>) {
    if let Some(cancel) = lock(&session.cancel).take() {
        cancel.cancel();
    }
}

/// Opens a folder tile of the last finished scan.
#[tauri::command]
async fn browse(id: String, session: State<'_, Session>) -> Result<MapFolder, String> {
    let last = lock(&session.last);
    let scanned = last
        .as_ref()
        .ok_or("there is no finished scan to explore")?;
    vacancy_core::browse(&scanned.snapshot, &scanned.categories, &id).map_err(|e| e.to_string())
}

/// Starts the app and blocks until its last window closes.
pub fn run() {
    tauri::Builder::default()
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            system_volume,
            machine_name,
            list_volumes,
            full_disk_access,
            open_full_disk_access_settings,
            scan_volume,
            cancel_scan,
            browse,
        ])
        .run(tauri::generate_context!())
        .expect("Vacancy failed to start");
}

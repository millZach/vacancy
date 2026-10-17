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

/// What the app remembers between commands: the scans, and the last finished one, which the map's drill-down
/// reads.
#[derive(Default)]
struct Session {
    scans: Runs<ScannedVolume>,
}

/// One scan at a time. Each new scan (and each cancel) moves to the next generation and cancels the scan before
/// it; a scan that finishes keeps its result only if its generation is still current, checked and stored under
/// one lock. So a slow scan that was replaced or cancelled can never overwrite the map of the scan after it.
struct Runs<T> {
    state: Mutex<RunState<T>>,
}

struct RunState<T> {
    generation: u64,
    running: Option<CancelToken>,
    last: Option<T>,
}

impl<T> Default for Runs<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(RunState {
                generation: 0,
                running: None,
                last: None,
            }),
        }
    }
}

impl<T> Runs<T> {
    /// Starts a scan: cancels the one running, drops the last result, and hands back this scan's generation and
    /// cancel token.
    fn begin(&self) -> (u64, CancelToken) {
        let mut state = lock(&self.state);
        state.generation += 1;
        if let Some(previous) = state.running.take() {
            previous.cancel();
        }
        // The last map's memory goes back before a new scan builds its own.
        state.last = None;
        let cancel = CancelToken::new();
        state.running = Some(cancel.clone());
        (state.generation, cancel)
    }

    /// Stores a finished scan's result if no newer scan has started and it wasn't cancelled. Returns whether it
    /// was stored.
    fn finish(&self, generation: u64, result: T) -> bool {
        let mut state = lock(&self.state);
        if state.generation != generation {
            return false;
        }
        state.running = None;
        state.last = Some(result);
        true
    }

    /// Forgets a scan that ended without a result, unless a newer one has started.
    fn end(&self, generation: u64) {
        let mut state = lock(&self.state);
        if state.generation == generation {
            state.running = None;
        }
    }

    /// Cancels the running scan; whatever it was about to store is now stale.
    fn cancel(&self) {
        let mut state = lock(&self.state);
        state.generation += 1;
        if let Some(running) = state.running.take() {
            running.cancel();
        }
    }

    /// Runs `f` with the last stored result.
    fn with_last<R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        f(lock(&self.state).last.as_ref())
    }
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
    let (generation, token) = session.scans.begin();
    let root = PathBuf::from(&mount_point);
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
    .await;
    match scanned {
        Ok(Ok(scanned)) => {
            let overview = Box::new(scanned.overview.clone());
            if session.scans.finish(generation, scanned) {
                Ok(ScanOutcome::Done { overview })
            } else {
                Ok(ScanOutcome::Cancelled)
            }
        }
        Ok(Err(ScanError::Cancelled)) => {
            session.scans.end(generation);
            Ok(ScanOutcome::Cancelled)
        }
        Ok(Err(e)) => {
            session.scans.end(generation);
            Err(e.to_string())
        }
        Err(e) => {
            session.scans.end(generation);
            Err(e.to_string())
        }
    }
}

/// Stops the running scan, if there is one.
#[tauri::command]
fn cancel_scan(session: State<'_, Session>) {
    session.scans.cancel();
}

/// Opens a folder tile of the last finished scan.
#[tauri::command]
async fn browse(id: String, session: State<'_, Session>) -> Result<MapFolder, String> {
    session.scans.with_last(|last| {
        let scanned = last.ok_or("there is no finished scan to explore")?;
        vacancy_core::browse(&scanned.snapshot, &scanned.categories, &id).map_err(|e| e.to_string())
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_result_of_the_current_scan() {
        let runs = Runs::<u32>::default();
        let (generation, _cancel) = runs.begin();
        assert!(runs.finish(generation, 1));
        assert_eq!(runs.with_last(|last| last.copied()), Some(1));
    }

    #[test]
    fn a_replaced_scan_never_overwrites_the_newer_one() {
        let runs = Runs::<u32>::default();
        let (first, first_cancel) = runs.begin();
        let (second, _) = runs.begin();
        assert!(
            first_cancel.is_cancelled(),
            "starting a scan cancels the one before"
        );
        assert!(runs.finish(second, 2));
        assert!(!runs.finish(first, 1), "the older scan finished late");
        assert_eq!(runs.with_last(|last| last.copied()), Some(2));
    }

    #[test]
    fn a_cancelled_scan_stores_nothing_even_if_it_finishes() {
        let runs = Runs::<u32>::default();
        let (generation, cancel) = runs.begin();
        runs.cancel();
        assert!(cancel.is_cancelled());
        assert!(!runs.finish(generation, 1));
        assert_eq!(runs.with_last(|last| last.copied()), None);
    }

    #[test]
    fn a_new_scan_drops_the_last_map() {
        let runs = Runs::<u32>::default();
        let (generation, _) = runs.begin();
        assert!(runs.finish(generation, 1));
        let (_, _) = runs.begin();
        assert_eq!(runs.with_last(|last| last.copied()), None);
    }
}

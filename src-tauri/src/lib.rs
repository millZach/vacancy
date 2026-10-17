//! The Vacancy desktop app: a thin Tauri shell. Each command forwards to `vacancy-core` and turns its errors into
//! messages for the UI. Disk logic never lives here.
//!
//! The UI calls these commands through `src/bridge/tauri.ts`; keep the command names in step with it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::State;
use tauri::ipc::Channel;
use vacancy_core::permissions::{self, FullDiskAccess};
use vacancy_core::{
    CancelToken, Env, MapFolder, ScanError, ScanOutcome, ScanUpdate, ScannedVolume, VolumeInfo,
};

/// What the app remembers between commands: each drive's last finished scan, which its map's drill-down reads.
#[derive(Default)]
struct Session {
    scans: Scans<ScannedVolume>,
}

/// Scans, kept per drive for the session, so each drive keeps its own map and switching never mixes two drives'
/// data. One scan runs at a time: starting one cancels the scan running on any drive.
///
/// Every scan and every cancel takes the next generation, and each drive remembers the generation of its newest
/// scan or cancel. A finished scan keeps its result only if that is still its drive's newest, checked and stored
/// under one lock, and only on its own drive. So a slow scan that was replaced or cancelled never overwrites the
/// map after it, and a scan of D: can never land on C:'s map.
struct Scans<T> {
    state: Mutex<ScansState<T>>,
}

struct ScansState<T> {
    /// The newest generation handed out, on any drive.
    generation: u64,
    running: Option<Running>,
    /// By mount point.
    drives: HashMap<String, DriveScans<T>>,
}

/// The scan that is running: its drive, its generation and how to stop it.
struct Running {
    drive: String,
    generation: u64,
    cancel: CancelToken,
}

/// One drive's newest scan or cancel, and its last finished result.
struct DriveScans<T> {
    generation: u64,
    last: Option<T>,
}

impl<T> Default for Scans<T> {
    fn default() -> Self {
        Self {
            state: Mutex::new(ScansState {
                generation: 0,
                running: None,
                drives: HashMap::new(),
            }),
        }
    }
}

impl<T> ScansState<T> {
    /// Cancels the running scan, on whichever drive, so whatever it was about to store is stale.
    fn stop_running(&mut self) {
        if let Some(running) = self.running.take() {
            running.cancel.cancel();
            self.generation += 1;
            if let Some(drive) = self.drives.get_mut(&running.drive) {
                drive.generation = self.generation;
            }
        }
    }

    /// The scan of `drive` at `generation` is over: it no longer counts as running.
    fn end_running(&mut self, drive: &str, generation: u64) {
        if self
            .running
            .as_ref()
            .is_some_and(|r| r.drive == drive && r.generation == generation)
        {
            self.running = None;
        }
    }
}

impl<T> Scans<T> {
    /// Starts scanning `drive`: cancels the scan running on any drive, drops `drive`'s last result (other drives
    /// keep theirs), and hands back this scan's generation and cancel token.
    fn begin(&self, drive: &str) -> (u64, CancelToken) {
        let mut state = lock(&self.state);
        state.stop_running();
        state.generation += 1;
        let generation = state.generation;
        // The drive's last map gives its memory back before the new scan builds its own.
        state.drives.insert(
            drive.to_owned(),
            DriveScans {
                generation,
                last: None,
            },
        );
        let cancel = CancelToken::new();
        state.running = Some(Running {
            drive: drive.to_owned(),
            generation,
            cancel: cancel.clone(),
        });
        (generation, cancel)
    }

    /// Stores a finished scan of `drive` if it is still that drive's newest scan and wasn't cancelled. Returns
    /// whether it was stored.
    fn finish(&self, drive: &str, generation: u64, result: T) -> bool {
        let mut state = lock(&self.state);
        let Some(scans) = state
            .drives
            .get_mut(drive)
            .filter(|d| d.generation == generation)
        else {
            return false;
        };
        scans.last = Some(result);
        state.end_running(drive, generation);
        true
    }

    /// Forgets a scan that ended without a result, unless a newer one has started.
    fn end(&self, drive: &str, generation: u64) {
        lock(&self.state).end_running(drive, generation);
    }

    /// Cancels the running scan, on whichever drive.
    fn cancel(&self) {
        lock(&self.state).stop_running();
    }

    /// Drops everything about `drive`: its map, and its scan if one is running. For a drive that has gone away.
    fn forget(&self, drive: &str) {
        let mut state = lock(&self.state);
        if state.running.as_ref().is_some_and(|r| r.drive == drive) {
            state.stop_running();
        }
        state.drives.remove(drive);
    }

    /// Runs `f` with `drive`'s last stored result.
    fn with_last<R>(&self, drive: &str, f: impl FnOnce(Option<&T>) -> R) -> R {
        let state = lock(&self.state);
        f(state.drives.get(drive).and_then(|d| d.last.as_ref()))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// This computer's name, for the plate's caption.
#[tauri::command]
async fn machine_name() -> String {
    vacancy_core::machine_name()
}

/// The drives to offer for scanning, the system drive first. Read-only. Runs on a blocking thread: a drive slow to
/// answer must not hold up the async workers that serve scans and the map.
#[tauri::command]
async fn list_volumes() -> Result<Vec<VolumeInfo>, String> {
    tauri::async_runtime::spawn_blocking(vacancy_core::list_volumes)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Forgets a drive that has gone away (an unplugged USB stick): its map, and its scan if one is running.
#[tauri::command]
fn forget_drive(mount_point: String, session: State<'_, Session>) {
    session.scans.forget(&mount_point);
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
/// "cancelled"). The map is kept as this drive's, beside other drives' maps. A scan already running, on any drive,
/// is cancelled first.
#[tauri::command]
async fn scan_volume(
    mount_point: String,
    on_update: Channel<ScanUpdate>,
    session: State<'_, Session>,
) -> Result<ScanOutcome, String> {
    let (generation, token) = session.scans.begin(&mount_point);
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
            if session.scans.finish(&mount_point, generation, scanned) {
                Ok(ScanOutcome::Done { overview })
            } else {
                Ok(ScanOutcome::Cancelled)
            }
        }
        Ok(Err(ScanError::Cancelled)) => {
            session.scans.end(&mount_point, generation);
            Ok(ScanOutcome::Cancelled)
        }
        Ok(Err(e)) => {
            session.scans.end(&mount_point, generation);
            Err(e.to_string())
        }
        Err(e) => {
            session.scans.end(&mount_point, generation);
            Err(e.to_string())
        }
    }
}

/// Stops the running scan, on whichever drive, if there is one.
#[tauri::command]
fn cancel_scan(session: State<'_, Session>) {
    session.scans.cancel();
}

/// Opens a folder tile of a drive's map.
#[tauri::command]
async fn browse(
    mount_point: String,
    id: String,
    session: State<'_, Session>,
) -> Result<MapFolder, String> {
    session.scans.with_last(&mount_point, |last| {
        let scanned =
            last.ok_or_else(|| format!("there is no finished scan of {mount_point} to explore"))?;
        vacancy_core::browse(&scanned.snapshot, &scanned.categories, &id).map_err(|e| e.to_string())
    })
}

/// Starts the app and blocks until its last window closes.
pub fn run() {
    tauri::Builder::default()
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            machine_name,
            list_volumes,
            forget_drive,
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

    const C: &str = r"C:\";
    const D: &str = r"D:\";

    fn last(scans: &Scans<u32>, drive: &str) -> Option<u32> {
        scans.with_last(drive, |last| last.copied())
    }

    #[test]
    fn keeps_the_result_of_the_current_scan() {
        let scans = Scans::<u32>::default();
        let (generation, _cancel) = scans.begin(C);
        assert!(scans.finish(C, generation, 1));
        assert_eq!(last(&scans, C), Some(1));
    }

    #[test]
    fn keeps_each_drives_map_side_by_side() {
        let scans = Scans::<u32>::default();
        let (c, _) = scans.begin(C);
        assert!(scans.finish(C, c, 1));
        let (d, _) = scans.begin(D);
        assert!(scans.finish(D, d, 2));
        assert_eq!(last(&scans, C), Some(1), "scanning D: keeps C:'s map");
        assert_eq!(last(&scans, D), Some(2));
    }

    #[test]
    fn a_scan_never_lands_on_another_drives_map() {
        let scans = Scans::<u32>::default();
        let (c, _) = scans.begin(C);
        assert!(scans.finish(C, c, 1));
        let (d, _) = scans.begin(D);
        assert!(!scans.finish(C, d, 2), "D:'s scan is not C:'s");
        assert_eq!(last(&scans, C), Some(1));
        assert_eq!(last(&scans, D), None);
    }

    #[test]
    fn a_replaced_scan_never_overwrites_the_newer_one() {
        let scans = Scans::<u32>::default();
        let (first, first_cancel) = scans.begin(C);
        let (second, _) = scans.begin(C);
        assert!(
            first_cancel.is_cancelled(),
            "starting a scan cancels the one before"
        );
        assert!(scans.finish(C, second, 2));
        assert!(!scans.finish(C, first, 1), "the older scan finished late");
        assert_eq!(last(&scans, C), Some(2));
    }

    #[test]
    fn scanning_another_drive_cancels_the_running_scan() {
        let scans = Scans::<u32>::default();
        let (c, c_cancel) = scans.begin(C);
        let (d, _) = scans.begin(D);
        assert!(c_cancel.is_cancelled(), "one scan at a time");
        assert!(
            !scans.finish(C, c, 1),
            "the cancelled scan of C: stores nothing"
        );
        assert!(scans.finish(D, d, 2));
        assert_eq!(last(&scans, C), None);
        assert_eq!(last(&scans, D), Some(2));
    }

    #[test]
    fn a_cancelled_scan_stores_nothing_even_if_it_finishes() {
        let scans = Scans::<u32>::default();
        let (generation, cancel) = scans.begin(C);
        scans.cancel();
        assert!(cancel.is_cancelled());
        assert!(!scans.finish(C, generation, 1));
        assert_eq!(last(&scans, C), None);
    }

    #[test]
    fn cancelling_keeps_other_drives_maps() {
        let scans = Scans::<u32>::default();
        let (c, _) = scans.begin(C);
        assert!(scans.finish(C, c, 1));
        let (_, d_cancel) = scans.begin(D);
        scans.cancel();
        assert!(d_cancel.is_cancelled());
        assert_eq!(last(&scans, C), Some(1));
    }

    #[test]
    fn a_new_scan_drops_only_that_drives_map() {
        let scans = Scans::<u32>::default();
        let (c, _) = scans.begin(C);
        assert!(scans.finish(C, c, 1));
        let (d, _) = scans.begin(D);
        assert!(scans.finish(D, d, 2));
        let (_, _) = scans.begin(C);
        assert_eq!(last(&scans, C), None);
        assert_eq!(last(&scans, D), Some(2));
    }

    #[test]
    fn forgetting_a_drive_drops_its_map_and_stops_its_scan() {
        let scans = Scans::<u32>::default();
        let (c, _) = scans.begin(C);
        assert!(scans.finish(C, c, 1));
        let (d, d_cancel) = scans.begin(D);
        scans.forget(D);
        assert!(d_cancel.is_cancelled());
        assert!(!scans.finish(D, d, 2), "a gone drive's scan stores nothing");
        assert_eq!(last(&scans, D), None);
        assert_eq!(last(&scans, C), Some(1));

        scans.forget(C);
        assert_eq!(last(&scans, C), None);
        let (again, _) = scans.begin(C);
        assert!(
            scans.finish(C, again, 3),
            "a drive plugged back in scans afresh"
        );
        assert_eq!(last(&scans, C), Some(3));
    }

    #[test]
    fn forgetting_an_idle_drive_leaves_the_running_scan_alone() {
        let scans = Scans::<u32>::default();
        let (c, _) = scans.begin(C);
        assert!(scans.finish(C, c, 1));
        let (d, d_cancel) = scans.begin(D);
        scans.forget(C);
        assert!(!d_cancel.is_cancelled());
        assert!(scans.finish(D, d, 2));
    }
}

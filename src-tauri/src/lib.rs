//! The Vacancy desktop app: a thin Tauri shell. Each command forwards to `vacancy-core` and turns its errors into
//! messages for the UI. Disk logic never lives here.
//!
//! The UI calls these commands through `src/bridge/tauri.ts`; keep the command names in step with it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use vacancy_core::advisor::{self, AdvisorStatus};
use vacancy_core::permissions::{self, FullDiskAccess};
use vacancy_core::{
    CancelToken, Env, MapFolder, Moment, Plan, PlanSummary, RunEvent, RunHistory, RunReport,
    ScanError, ScanOptions, ScanOutcome, ScanUpdate, ScannedVolume, Snapshot, VolumeInfo, fixture,
    report, tidy,
};

/// What the app remembers between commands: each drive's last finished scan (which its map's drill-down reads) with
/// the plan made from it, the plan running if one is, and whether the app works on the real drives or a fixture.
struct Session {
    scans: Scans<Scanned>,
    mode: Mode,
    /// The plan running now, if one is: its drive and how to stop it. One at a time, and no scan while it runs.
    run: Mutex<Option<(String, CancelToken)>>,
}

impl Session {
    fn new(mode: Mode) -> Self {
        Self {
            scans: Scans::default(),
            mode,
            run: Mutex::new(None),
        }
    }

    fn running(&self) -> bool {
        lock(&self.run).is_some()
    }
}

/// A drive's finished scan, the plan made from it, and whether a plan from it already ran. A scan that ran a plan
/// is spent: what it saw is gone, so another plan needs another scan.
struct Scanned {
    volume: ScannedVolume,
    plan: Option<Plan>,
    spent: bool,
}

/// Where the app works.
enum Mode {
    /// The computer's drives.
    Real,
    /// Fixture mode (`VACANCY_FIXTURE_ROOT`): one pseudo-drive, the fixture folder, with the Guard confined to it.
    Fixture { root: PathBuf, capacity: u64 },
    /// `VACANCY_FIXTURE_ROOT` named a folder that can't be a fixture root: nothing is scanned or deleted, and the
    /// drive list says why. Never quietly the real drives instead.
    Refused(String),
}

impl Mode {
    /// Fixture modes must bypass discovery before any host PATH, home or filesystem lookup can happen.
    fn advisor_status_with(&self, discover: impl FnOnce() -> AdvisorStatus) -> AdvisorStatus {
        match self {
            Mode::Real => discover(),
            Mode::Fixture { .. } | Mode::Refused(_) => AdvisorStatus::fixture_disabled(),
        }
    }

    /// Only the retained scan supplies an action. Refuse fixtures before even consulting host-side closures;
    /// saved report JSON is deliberately not an authority to open an app.
    fn open_hint_with(
        &self,
        mount_point: &str,
        lookup: impl FnOnce() -> Option<(String, vacancy_core::hints::HintAction)>,
        volumes: impl FnOnce() -> Result<Vec<VolumeInfo>, String>,
        launch: impl FnOnce(u32) -> Result<(), String>,
    ) -> Result<(), String> {
        if !matches!(self, Mode::Real) {
            return Err("Steam can't be opened from fixture mode. No app was launched.".into());
        }
        let (expected_volume, action) = lookup().ok_or(
            "This advice needs a current scan. Scan this drive again before opening Steam.",
        )?;
        let vacancy_core::hints::HintAction::SteamUninstall { app_id } = action else {
            return Err("This advice has local instructions and does not open an app.".into());
        };
        if app_id == 0 {
            return Err("This Steam advice is not valid. Scan this drive again.".into());
        }
        let current = volumes()?.into_iter().find(|v| v.mount_point == mount_point)
            .ok_or("This drive is no longer available. Connect it and scan again before opening Steam.")?;
        if expected_volume.is_empty() || current.id.is_empty() || current.id != expected_volume {
            return Err(
                "The drive changed since this scan. Scan it again before opening Steam.".into(),
            );
        }
        launch(app_id)
    }

    /// Vendor documentation is independent of disk state and stays useful from history after restart.
    /// Only a closed compiled guide identity is accepted; fixture modes refuse before host dispatch.
    fn open_guide_with(
        &self,
        guide: vacancy_core::hints::GuideId,
        launch: impl FnOnce(vacancy_core::hints::HintAction) -> Result<(), String>,
    ) -> Result<(), String> {
        if !matches!(self, Mode::Real) {
            return Err(
                "Vendor references can't be opened from fixture mode. No app was launched.".into(),
            );
        }
        launch(guide.action())
    }

    /// The mode `VACANCY_FIXTURE_ROOT` asks for, its folder checked by `fixture::check_root`.
    fn from_env() -> Self {
        let Some(root) = std::env::var_os(fixture::ROOT_VAR) else {
            return Mode::Real;
        };
        let root = PathBuf::from(root);
        let Some(home) = std::env::home_dir() else {
            return Mode::Refused("Fixture mode: your home folder can't be found.".into());
        };
        // Only a folder the fixture generator made: anything else could hold real files the fixture's own,
        // narrower places don't protect.
        if !fixture::is_profile(&root) {
            return Mode::Refused(format!(
                "Fixture mode refused: {} is not a fixture profile (it has no {}).",
                root.display(),
                fixture::MARKER
            ));
        }
        match fixture::check_root(&root, &home) {
            Ok(root) => {
                let used = measure(&root).map_or(0, |s| s.stats().bytes);
                Mode::Fixture {
                    capacity: fixture::capacity(used),
                    root,
                }
            }
            Err(e) => Mode::Refused(format!("Fixture mode refused: {e}.")),
        }
    }

    /// Checks that `mount_point` is a drive this mode works on (one the drive list offers now), and gives the
    /// places to scan and judge it with. Reads the drives and the OS's known folders: call it off the async threads.
    fn env_for(&self, mount_point: &str) -> Result<Env, String> {
        match self {
            Mode::Real => {
                let offered = vacancy_core::list_volumes()
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|v| v.mount_point == mount_point);
                if !offered {
                    return Err(format!("{mount_point} is not a drive Vacancy offers."));
                }
                Ok(Env::detect(Path::new(mount_point)))
            }
            Mode::Fixture { root, .. } if Path::new(mount_point) == root => Ok(fixture::env(root)),
            Mode::Fixture { .. } => Err(format!("{mount_point} is not the fixture.")),
            Mode::Refused(why) => Err(why.clone()),
        }
    }

    /// The drive's figures: the OS's for a real drive; for the fixture, its own (see `fixture::volume`), from
    /// `snapshot` if a scan of it is at hand.
    fn figures(&self, mount_point: &str, snapshot: Option<&Snapshot>) -> Option<VolumeInfo> {
        match self {
            Mode::Real => vacancy_core::volume_info(Path::new(mount_point)).ok(),
            Mode::Fixture { root, capacity } => match snapshot {
                Some(snap) => Some(fixture::volume(root, *capacity, snap)),
                None => measure(root).map(|snap| fixture::volume(root, *capacity, &snap)),
            },
            Mode::Refused(_) => None,
        }
    }

    /// The app's data folder, or in fixture mode the fixture's. None when fixture mode was refused.
    fn app_data(&self, app: &AppHandle) -> Option<PathBuf> {
        match self {
            Mode::Fixture { root, .. } => Some(fixture::app_data(root)),
            Mode::Real => app.path().app_data_dir().ok(),
            Mode::Refused(_) => None,
        }
    }

    /// Where the run history is kept: the app's data folder, or in fixture mode the fixture's.
    fn history(&self, app: &AppHandle) -> Option<PathBuf> {
        self.app_data(app).map(|dir| report::history_dir(&dir))
    }

    /// Where Tidy up keeps its move journals: a folder of the app's data folder (in fixture mode, the fixture's).
    fn journals(&self, app: &AppHandle) -> Option<tidy::Journals> {
        self.app_data(app).map(|dir| tidy::Journals {
            dir: dir.join("tidy"),
        })
    }
}

/// A quick read-only scan of the fixture, for its figures.
fn measure(root: &Path) -> Option<Snapshot> {
    vacancy_core::scan(
        root,
        &ScanOptions::default(),
        &mut |_| {},
        &CancelToken::new(),
    )
    .ok()
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

    /// Runs `f` with `drive`'s last stored result, to change it.
    fn with_last_mut<R>(&self, drive: &str, f: impl FnOnce(Option<&mut T>) -> R) -> R {
        let mut state = lock(&self.state);
        f(state.drives.get_mut(drive).and_then(|d| d.last.as_mut()))
    }

    /// Whether a scan is running, on any drive.
    fn scanning(&self) -> bool {
        lock(&self.state).running.is_some()
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

/// Read-only availability with no executable paths. No CLI is launched, signed into or enabled.
#[tauri::command]
async fn advisor_status(app: AppHandle) -> Result<AdvisorStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>()
            .mode
            .advisor_status_with(advisor::passive_status)
    })
    .await
    .map_err(|e| e.to_string())
}

/// The drives to offer for scanning, the system drive first. Read-only. Runs on a blocking thread: a drive slow to
/// answer must not hold up the async workers that serve scans and the map.
///
/// In fixture mode, the one pseudo-drive; if fixture mode was refused, why.
#[tauri::command]
async fn list_volumes(app: AppHandle) -> Result<Vec<VolumeInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mode = &app.state::<Session>().mode;
        match mode {
            Mode::Real => vacancy_core::list_volumes().map_err(|e| e.to_string()),
            Mode::Fixture { root, .. } => mode
                .figures(&root.display().to_string(), None)
                .map(|v| vec![v])
                .ok_or_else(|| format!("The fixture {} can't be read.", root.display())),
            Mode::Refused(why) => Err(why.clone()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Opens only Steam's review screen for a typed hint retained by the current scan. No URL or arguments arrive
/// from the frontend; stale history needs a new scan, and fixture modes never inspect or launch host apps.
#[tauri::command]
async fn open_hint(mount_point: String, proposal_id: String, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        session.mode.open_hint_with(
            &mount_point,
            || {
                session.scans.with_last(&mount_point, |last| {
                    let scanned = last?;
                    let action = scanned.volume.proposals.hint_action(&proposal_id)?.clone();
                    Some((scanned.volume.overview.volume.id.clone(), action))
                })
            },
            || vacancy_core::list_volumes().map_err(|e| e.to_string()),
            |app_id| vacancy_core::hints::open_steam_uninstall(app_id).map_err(|e| e.to_string()),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A fixed vendor reference needs no new scan. No URL, executable, command or filesystem path is accepted.
#[tauri::command]
async fn open_guide(guide: vacancy_core::hints::GuideId, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        session.mode.open_guide_with(guide, |action| {
            vacancy_core::hints::open_external_hint(&action).map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Scans a volume, read-only, streaming the HUD's numbers to `on_update`, and returns the overview map with the
/// cleanups proposed for it (or "cancelled"). The map and the proposals' targets are kept as this drive's, beside
/// other drives'. A scan already running, on any drive, is cancelled first.
#[tauri::command]
async fn scan_volume(
    mount_point: String,
    on_update: Channel<ScanUpdate>,
    app: AppHandle,
    session: State<'_, Session>,
) -> Result<ScanOutcome, String> {
    let env = env_for(&app, &mount_point).await?;
    let (generation, token) = {
        // Under the run's lock, which a run takes to check that no scan is running: never both at once.
        let run = lock(&session.run);
        if run.is_some() {
            return Err("A plan is running. Scan again when it has finished.".into());
        }
        session.scans.begin(&mount_point)
    };
    let root = PathBuf::from(&mount_point);
    let drive = mount_point.clone();
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        // Which apps run is read as the scan starts; the executor skips anything busy when the plan runs.
        let moment = Moment::detect();
        let mode = &app.state::<Session>().mode;
        vacancy_core::scan_for_map_with(
            &root,
            &env,
            &moment,
            &|snap| mode.figures(&drive, Some(snap)),
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
            let proposals = scanned.proposals.list().to_vec();
            let tidy = scanned.tidy.proposals();
            let scanned = Scanned {
                volume: scanned,
                plan: None,
                spent: false,
            };
            if session.scans.finish(&mount_point, generation, scanned) {
                Ok(ScanOutcome::Done {
                    overview,
                    proposals,
                    tidy,
                })
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

/// [`Mode::env_for`], on a blocking thread: a drive slow to answer must not hold up the async workers.
async fn env_for(app: &AppHandle, mount_point: &str) -> Result<Env, String> {
    let app = app.clone();
    let mount_point = mount_point.to_owned();
    tauri::async_runtime::spawn_blocking(move || app.state::<Session>().mode.env_for(&mount_point))
        .await
        .map_err(|e| e.to_string())?
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
        let volume = &scanned.volume;
        vacancy_core::browse(&volume.snapshot, &volume.categories, &id).map_err(|e| e.to_string())
    })
}

/// Makes the plan for the proposals `ids` of a drive's last scan, for the confirm dialog, and keeps it as the
/// drive's plan in place of any made before. Every place passes the Guard again, built afresh.
#[tauri::command]
async fn build_plan(
    mount_point: String,
    ids: Vec<String>,
    app: AppHandle,
) -> Result<PlanSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        if session.running() {
            return Err("A plan is running.".to_owned());
        }
        let env = session.mode.env_for(&mount_point)?;
        session.scans.with_last_mut(&mount_point, |last| {
            let scanned = last.ok_or_else(|| {
                format!("There is no finished scan of {mount_point} to plan from.")
            })?;
            if scanned.spent {
                return Err("This scan already ran a plan. Scan again to make another.".to_owned());
            }
            let plan =
                vacancy_core::plan(&scanned.volume, &env, &ids).map_err(|e| e.to_string())?;
            let summary = plan.summary();
            scanned.plan = Some(plan);
            Ok(summary)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs the drive's plan `plan_id`, streaming what happens to `on_event`, and returns the report, which is kept in
/// the run history too. A plan runs once: the scan it came from is spent.
#[tauri::command]
async fn run_plan(
    mount_point: String,
    plan_id: String,
    on_event: Channel<RunEvent>,
    app: AppHandle,
) -> Result<RunReport, String> {
    let env = env_for(&app, &mount_point).await?;
    let session = app.state::<Session>();
    let cancel = CancelToken::new();
    let plan = {
        let mut run = lock(&session.run);
        if run.is_some() {
            return Err("A plan is already running.".into());
        }
        if session.scans.scanning() {
            return Err("A scan is running. Run the plan when it has finished.".into());
        }
        // Taken out of the drive's state under the run's lock, so a plan can never run twice.
        let plan = session.scans.with_last_mut(&mount_point, |last| {
            let scanned = last.ok_or("There is no plan for this drive.")?;
            match scanned.plan.take() {
                Some(plan) if plan.id() == plan_id => {
                    scanned.spent = true;
                    Ok(plan)
                }
                other => {
                    scanned.plan = other;
                    Err("This plan is no longer the drive's plan. Build it again.")
                }
            }
        })?;
        *run = Some((mount_point.clone(), cancel.clone()));
        plan
    };
    let worker = app.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        let session = worker.state::<Session>();
        let history = session.mode.history(&worker);
        let keep = |report: &RunReport| {
            if let Some(dir) = &history
                && let Err(e) = report::save(dir, report)
            {
                eprintln!(
                    "could not keep the run in the history at {}: {e}",
                    dir.display()
                );
            }
        };
        let journals = session.mode.journals(&worker);
        let report = vacancy_core::execute_with(
            &plan,
            &env,
            journals.as_ref(),
            // Kept before any file moves, so a run the app never finishes still leads to its journals.
            &mut |so_far| keep(so_far),
            &|| session.mode.figures(&mount_point, None),
            &mut |event| {
                // The UI may have gone away mid-run; the run finishes either way, and its report is kept.
                let _ = on_event.send(event);
            },
            &cancel,
        );
        keep(&report);
        report
    })
    .await;
    *lock(&session.run) = None;
    report.map_err(|e| e.to_string())
}

/// Stops the running plan after the file being deleted.
#[tauri::command]
fn cancel_run(session: State<'_, Session>) {
    if let Some((_, cancel)) = lock(&session.run).as_ref() {
        cancel.cancel();
    }
}

/// The run history, newest first, with how many kept runs couldn't be read.
#[tauri::command]
async fn list_runs(app: AppHandle) -> RunHistory {
    tauri::async_runtime::spawn_blocking(move || {
        let mode = &app.state::<Session>().mode;
        mode.history(&app)
            .map(|dir| report::history(&dir))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

/// A run from the history, for its summary and its highlight slides: as it was kept, with each folder it
/// reorganized read again from its journal (what moved, whether Undo can still run, whether it was interrupted).
/// Reading a journal never moves anything and never resumes a run.
#[tauri::command]
async fn open_run(id: String, app: AppHandle) -> Result<RunReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        let mode = &session.mode;
        let dir = mode
            .history(&app)
            .ok_or("The run history can't be found.")?;
        let mut report =
            report::open(&dir, &id).map_err(|e| format!("This run can't be opened: {e}."))?;
        // A running plan holds its own journal; its report isn't final yet, so it is shown as kept.
        if !report.reorganized.is_empty() && !session.running() {
            refresh_tidy(mode, &app, &dir, &mut report);
        }
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reads a kept run's reorganizations again from their journals, and keeps the report if that changed it.
fn refresh_tidy(mode: &Mode, app: &AppHandle, history: &Path, report: &mut RunReport) {
    let (Some(journals), Ok(env)) = (mode.journals(app), mode.env_for(&report.mount_point)) else {
        return;
    };
    if tidy::refresh(&env, &journals, report)
        && let Err(e) = report::save(history, report)
    {
        eprintln!("could not keep the run's journal state in the history: {e}");
    }
}

/// Undoes one folder's Tidy up from a kept run, using its journal (which survives a restart), and keeps what Undo
/// did in the run's report. Files that changed, or whose old names are taken, stay where they are, and the report
/// says which. Waits for no plan: it refuses while one runs, and no plan or scan starts while it runs.
#[tauri::command]
async fn undo_tidy(run_id: String, tidy_id: String, app: AppHandle) -> Result<RunReport, String> {
    let cancel = CancelToken::new();
    {
        let session = app.state::<Session>();
        let mut run = lock(&session.run);
        if run.is_some() {
            return Err("A plan is running. Undo when it has finished.".into());
        }
        if session.scans.scanning() {
            return Err("A scan is running. Undo when it has finished.".into());
        }
        *run = Some((String::new(), cancel.clone()));
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let session = worker.state::<Session>();
        let mode = &session.mode;
        let dir = mode
            .history(&worker)
            .ok_or("The run history can't be found.")?;
        let journals = mode
            .journals(&worker)
            .ok_or("Vacancy can't find where it keeps its records of moves.")?;
        let mut report =
            report::open(&dir, &run_id).map_err(|e| format!("This run can't be opened: {e}."))?;
        let env = mode.env_for(&report.mount_point).map_err(|e| {
            format!(
                "Undo needs the drive this run sorted, {}: {e}",
                report.mount_point
            )
        })?;
        tidy::undo(&env, &journals, &mut report, &tidy_id, &cancel).map_err(|e| e.to_string())?;
        report::save(&dir, &report)
            .map_err(|e| format!("Undo ran, but the history couldn't keep what it did: {e}."))?;
        Ok(report)
    })
    .await;
    *lock(&app.state::<Session>().run) = None;
    result.map_err(|e| e.to_string())?
}

/// Starts the app and blocks until its last window closes.
pub fn run() {
    tauri::Builder::default()
        .manage(Session::new(Mode::from_env()))
        .invoke_handler(tauri::generate_handler![
            machine_name,
            advisor_status,
            list_volumes,
            forget_drive,
            full_disk_access,
            open_full_disk_access_settings,
            open_hint,
            open_guide,
            scan_volume,
            cancel_scan,
            browse,
            build_plan,
            run_plan,
            cancel_run,
            list_runs,
            open_run,
            undo_tidy,
        ])
        .run(tauri::generate_context!())
        .expect("Vacancy failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = r"C:\";
    const D: &str = r"D:\";

    #[test]
    fn fixture_and_refused_fixture_status_never_invoke_host_discovery() {
        for mode in [
            Mode::Fixture {
                root: PathBuf::from("unused-fixture-root"),
                capacity: 0,
            },
            Mode::Refused("invalid fixture".into()),
        ] {
            let status =
                mode.advisor_status_with(|| panic!("fixture mode must never inspect the host"));
            assert_eq!(status, AdvisorStatus::fixture_disabled());
            assert!(!status.enabled);
        }
    }

    fn hint_volume(id: &str) -> VolumeInfo {
        VolumeInfo {
            id: id.into(),
            name: "Fixture volume".into(),
            mount_point: C.into(),
            kind: vacancy_core::DriveKind::Internal,
            system: false,
            total_bytes: 1000,
            used_bytes: 500,
            free_bytes: 500,
        }
    }

    #[test]
    fn real_mode_returns_the_read_only_discovery_report() {
        let expected = AdvisorStatus::fixture(Vec::new());
        let returned = Mode::Real.advisor_status_with(|| expected.clone());
        assert_eq!(returned, expected);
        assert!(!returned.enabled);
    }

    #[test]
    fn hint_fixture_modes_refuse_before_any_lookup_or_launch() {
        for mode in [
            Mode::Fixture {
                root: PathBuf::from("unused"),
                capacity: 0,
            },
            Mode::Refused("invalid".into()),
        ] {
            let result = mode.open_hint_with(
                C,
                || panic!("no scan/host lookup"),
                || panic!("no volume lookup"),
                |_| panic!("no launcher"),
            );
            assert!(result.unwrap_err().contains("fixture mode"));
        }
    }

    #[test]
    fn hint_opener_dispatches_only_the_trusted_nonzero_id_on_the_same_volume() {
        use vacancy_core::hints::HintAction;
        let mut called = false;
        Mode::Real
            .open_hint_with(
                C,
                || Some(("volume-a".into(), HintAction::SteamUninstall { app_id: 42 })),
                || Ok(vec![hint_volume("volume-a")]),
                |app_id| {
                    called = true;
                    assert_eq!(
                        HintAction::SteamUninstall { app_id }.steam_uri().as_deref(),
                        Some("steam://uninstall/42")
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert!(called, "positive fake dispatch must actually occur");
        for action in [
            HintAction::SteamUninstall { app_id: 0 },
            HintAction::LogicInstructions,
        ] {
            assert!(
                Mode::Real
                    .open_hint_with(
                        C,
                        || Some(("volume-a".into(), action)),
                        || panic!("invalid action never looks up volumes"),
                        |_| panic!("never launch")
                    )
                    .is_err()
            );
        }
        let missing = Mode::Real.open_hint_with(
            C,
            || None,
            || panic!("stale history never looks up volumes"),
            |_| panic!("never launch"),
        );
        assert!(missing.unwrap_err().contains("Scan this drive again"));
        for volumes in [vec![], vec![hint_volume("volume-b")], vec![hint_volume("")]] {
            assert!(
                Mode::Real
                    .open_hint_with(
                        C,
                        || Some(("volume-a".into(), HintAction::SteamUninstall { app_id: 42 })),
                        || Ok(volumes),
                        |_| panic!("wrong/missing volume never launches")
                    )
                    .is_err()
            );
        }
        assert!(
            Mode::Real
                .open_hint_with(
                    C,
                    || Some((String::new(), HintAction::SteamUninstall { app_id: 42 })),
                    || Ok(vec![hint_volume("volume-a")]),
                    |_| panic!("unknown scan volume never launches")
                )
                .is_err()
        );
        assert!(
            Mode::Real
                .open_hint_with(
                    C,
                    || Some(("volume-a".into(), HintAction::SteamUninstall { app_id: 42 })),
                    || Err("volume query failed".into()),
                    |_| panic!("failed volume query never launches")
                )
                .is_err()
        );
        assert_eq!(
            Mode::Real
                .open_hint_with(
                    C,
                    || Some(("volume-a".into(), HintAction::SteamUninstall { app_id: 42 })),
                    || Ok(vec![hint_volume("volume-a")]),
                    |_| Err("Steam is not installed".into())
                )
                .unwrap_err(),
            "Steam is not installed"
        );
    }

    #[test]
    fn vendor_references_work_without_a_scan_and_never_dispatch_from_fixture_modes() {
        use vacancy_core::hints::GuideId;
        // A fresh session has no retained scans, just as after reopening a saved report on restart.
        let session = Session::new(Mode::Real);
        for guide in [
            GuideId::WindowsCleanup,
            GuideId::WindowsDelivery,
            GuideId::DockerImages,
            GuideId::DockerCache,
            GuideId::Xcode,
            GuideId::Homebrew,
        ] {
            let expected = guide.action().external_uri().unwrap();
            assert!(expected.starts_with("https://"));
            let mut launched = false;
            session
                .mode
                .open_guide_with(guide, |actual| {
                    launched = true;
                    assert_eq!(actual.external_uri().as_deref(), Some(expected.as_str()));
                    Ok(())
                })
                .unwrap();
            assert!(launched);
            for mode in [
                Mode::Fixture {
                    root: PathBuf::from("fixture"),
                    capacity: 1,
                },
                Mode::Refused("bad fixture".into()),
            ] {
                assert!(
                    mode.open_guide_with(guide, |_| panic!("no host launcher"))
                        .is_err()
                );
            }
        }
    }

    #[test]
    fn hint_opener_refuses_matching_unavailable_identities_but_accepts_a_reported_zero() {
        use vacancy_core::hints::HintAction;
        // Failed Windows queries now map None to an empty id (covered by the pure Windows offering test).
        // Identical failures at scan/click time must never establish that this is the same physical volume.
        for (scanned, current) in [("", ""), ("", "00000000"), ("00000000", "")] {
            assert!(
                Mode::Real
                    .open_hint_with(
                        C,
                        || Some((scanned.into(), HintAction::SteamUninstall { app_id: 42 })),
                        || Ok(vec![hint_volume(current)]),
                        |_| panic!("unavailable volume identity must never dispatch"),
                    )
                    .is_err()
            );
        }
        let mut launched = false;
        Mode::Real
            .open_hint_with(
                C,
                || Some(("00000000".into(), HintAction::SteamUninstall { app_id: 42 })),
                || Ok(vec![hint_volume("00000000")]),
                |id| {
                    assert_eq!(id, 42);
                    launched = true;
                    Ok(())
                },
            )
            .unwrap();
        assert!(
            launched,
            "successful numeric zero is an identity, unlike lookup failure"
        );
    }

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

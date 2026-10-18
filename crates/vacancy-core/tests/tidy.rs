//! Tidy up against a fixture profile on a real file system: what the built-in sorting rules propose for the loose
//! files of Downloads, Desktop and Documents, that a file a cleanup in the same plan deletes is left out of the
//! sort, the engine's cap, and a run of the sorts with its journals: what moved, what stayed and why, and an Undo
//! (also after the app "restarts", from the journal alone) that puts every moved file back. Profiles live under
//! the build's own temp folder, never in a home folder. macOS proposes nothing.

mod support;

use vacancy_core::tidy;
use vacancy_core::{CancelToken, Env};

// Tidy up runs only on Windows; these helpers serve the Windows tests below.
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use vacancy_core::fixture::{self, Profile};
#[cfg(windows)]
use vacancy_core::tidy::{TidyKind, TidyProposal};
#[cfg(windows)]
use vacancy_core::{Moment, ScannedVolume, scan_for_map_with};

#[cfg(windows)]
fn moment() -> Moment {
    Moment {
        now: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
        running_apps: Some(Vec::new()),
        installed_apps: Vec::new(),
    }
}

#[cfg(windows)]
/// A fixture profile under the build's temp folder, changed by `setup`, then scanned in fixture mode.
struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    profile: Profile,
    env: Env,
    scanned: ScannedVolume,
}

#[cfg(windows)]
impl Fx {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(setup: impl FnOnce(&Profile)) -> Self {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let root = support::real_path(dir.path()).join("fixture");
        let profile = fixture::build(&root).expect("build the fixture profile");
        setup(&profile);
        std::thread::sleep(std::time::Duration::from_millis(3200));
        let env = fixture::env(&root);
        let scanned = scan(&root, &env);
        Self {
            _dir: dir,
            root,
            profile,
            env,
            scanned,
        }
    }

    fn sort(&self, id: &str) -> TidyProposal {
        self.scanned
            .tidy
            .proposals()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("{id} is proposed: {:?}", self.scanned.tidy.proposals()))
    }

    fn journals(&self) -> tidy::Journals {
        tidy::Journals {
            dir: fixture::app_data(&self.root).join("tidy"),
        }
    }

    /// The listing of the three known folders, relative to the root.
    fn known(&self) -> Vec<String> {
        fixture::inventory(&self.root)
            .unwrap()
            .into_iter()
            .filter(|line| {
                ["Downloads", "Desktop", "Documents"]
                    .iter()
                    .any(|f| line.starts_with(&format!("Users/fixture/{f}")))
            })
            .collect()
    }

    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }
}

#[cfg(windows)]
fn scan(root: &Path, env: &Env) -> ScannedVolume {
    let figures_root = root.to_path_buf();
    scan_for_map_with(
        root,
        env,
        &moment(),
        &move |snap| Some(fixture::volume(&figures_root, 1 << 30, snap)),
        &mut |_| {},
        &CancelToken::new(),
    )
    .expect("scan the fixture")
}

#[test]
fn macos_proposes_no_tidy_up() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = support::real_path(dir.path());
    std::fs::create_dir_all(root.join("Users/alex/Downloads")).unwrap();
    std::fs::write(root.join("Users/alex/Downloads/photo.jpg"), b"x").unwrap();
    let snap = support::scan_with(&root, 1 << 20);
    let mut env = Env::macos(&root, "alex");
    env.confined_to = Some(root.clone());
    let proposals = vacancy_core::Proposals::default();
    let sorts = tidy::propose(&snap, &proposals, &env, &CancelToken::new());
    assert!(sorts.proposals().is_empty());
    assert!(!sorts.has("tidy-downloads"));
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::collections::BTreeSet;
    use std::os::windows::fs::OpenOptionsExt;
    use vacancy_core::{
        Outcome, ProposalAction, RunEvent, RunReport, SkipKind, StepKind, execute_with, plan,
        report,
    };

    fn ids(fx: &Fx) -> Vec<String> {
        fx.scanned
            .tidy
            .proposals()
            .iter()
            .map(|p| p.id.clone())
            .collect()
    }

    fn year_ago(days: u64) -> String {
        let at = SystemTime::now() - std::time::Duration::from_secs(days * 86_400 + 60);
        tidy::year_of(at.duration_since(UNIX_EPOCH).unwrap().as_nanos() as i128).to_string()
    }

    #[test]
    fn sorts_loose_files_by_kind_and_year_and_says_what_stays() {
        let fx = Fx::new();
        assert_eq!(
            ids(&fx),
            ["tidy-downloads", "tidy-desktop", "tidy-documents"]
        );
        let downloads = fx.sort("tidy-downloads");
        let to = |p: &TidyProposal, name: &str| {
            let m = p
                .moves
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("{name} moves: {:#?}", p.moves));
            (m.dest.clone(), m.to.clone(), m.kind)
        };
        let now = year_ago(20);
        let last = year_ago(400);
        let before = year_ago(760);
        assert_eq!(
            to(&downloads, "IMG_3301.jpg"),
            (
                format!("Images\\{now}"),
                "IMG_3301.jpg".into(),
                TidyKind::Images
            )
        );
        assert_eq!(to(&downloads, "IMG_2041.jpg").0, format!("Images\\{last}"));
        assert_eq!(
            to(&downloads, "floorplan-v3.png").0,
            format!("Images\\{before}")
        );
        assert_eq!(
            to(&downloads, "resume-2025.docx").0,
            format!("Documents\\{last}")
        );
        assert_eq!(to(&downloads, "birthday.mov").0, format!("Videos\\{last}"));
        assert_eq!(to(&downloads, "VSCodeUserSetup-x64.exe").0, "Installers");
        assert_eq!(to(&downloads, "drivers.7z").0, "Archives");
        assert_eq!(
            to(&downloads, "voice-memo.m4a"),
            (
                format!("Audio\\{last}"),
                "voice-memo.m4a".into(),
                TidyKind::Audio
            )
        );
        assert_eq!(to(&downloads, "export_1734").2, TidyKind::Later);
        assert_eq!(to(&downloads, "data.bin").0, "Sort later");
        // A name already taken in its destination gets a numbered suffix; the folder is the user's.
        let fonts = downloads
            .moves
            .iter()
            .find(|m| m.name == "fonts.zip")
            .unwrap();
        assert_eq!((fonts.to.as_str(), fonts.renamed), ("fonts (1).zip", true));
        assert!(downloads.existing.iter().any(|d| d == "Archives"));
        assert!(
            downloads
                .subfolders
                .iter()
                .any(|s| s.name == "Archives" && s.files == 1)
        );
        // An unfinished download stays, with why.
        assert!(
            downloads
                .stays
                .iter()
                .any(|s| s.name == "video.mp4.crdownload" && !s.dropped)
        );
        // In Documents, documents go straight into a year; the collision there is renamed too.
        let documents = fx.sort("tidy-documents");
        let tax = documents
            .moves
            .iter()
            .find(|m| m.name == "tax-return.pdf")
            .unwrap();
        assert_eq!(tax.dest, last);
        assert_eq!(tax.to, "tax-return (1).pdf");
        assert_eq!(to(&documents, "scan001.jpg").0, format!("Images\\{last}"));
        assert_eq!(to(&documents, "Untitled").0, "Sort later");
        // Shortcuts and the folder's settings stay on the Desktop.
        let desktop = fx.sort("tidy-desktop");
        for name in ["Steam.lnk", "desktop.ini"] {
            assert!(desktop.stays.iter().any(|s| s.name == name), "{name} stays");
            assert!(desktop.moves.iter().all(|m| m.name != name));
        }
        for p in fx.scanned.tidy.proposals() {
            assert!(!p.capped && p.later == 0);
            assert_eq!(
                p.loose as usize,
                p.moves.len() + p.stays.len(),
                "{}: every loose file moves or stays",
                p.id
            );
            assert!(p.moves.iter().all(|m| !m.dest.is_empty() && m.bytes > 0));
        }
    }

    #[test]
    fn delete_wins_over_sort() {
        let fx = Fx::new();
        let downloads = fx.sort("tidy-downloads");
        let old = downloads
            .moves
            .iter()
            .find(|m| m.name == "old-setup.exe")
            .expect("the old installer can be sorted");
        assert_eq!(old.deleted_by, ["w-installers"]);
        assert!(
            downloads
                .moves
                .iter()
                .filter(|m| m.name != "old-setup.exe")
                .all(|m| m.deleted_by.is_empty())
        );
        let alone = plan(&fx.scanned, &fx.env, &["tidy-downloads".into()]).unwrap();
        let both = plan(
            &fx.scanned,
            &fx.env,
            &["w-installers".into(), "tidy-downloads".into()],
        )
        .unwrap();
        let moves = |p: &vacancy_core::Plan| p.summary().moves;
        assert_eq!(moves(&alone), downloads.moves.len() as u64);
        assert_eq!(
            moves(&both),
            downloads.moves.len() as u64 - 1,
            "the installer is deleted, not moved"
        );
        let summary = both.summary();
        assert_eq!(summary.steps.last().unwrap().kind, StepKind::Move);
        assert_eq!(
            summary.steps.last().unwrap().bytes,
            0,
            "a sort frees nothing"
        );
        assert!(summary.bytes > 0 && summary.bytes == summary.steps[0].bytes);
        assert!(plan(&fx.scanned, &fx.env, &["tidy-nowhere".into()]).is_err());
    }

    #[test]
    fn a_folder_with_more_files_than_one_run_takes_is_capped_and_says_how_many_wait() {
        let fx = Fx::with(|p| {
            let downloads = p.home.join("Downloads");
            for i in 0..600 {
                std::fs::write(downloads.join(format!("bulk-{i:04}.txt")), b"bulk").unwrap();
            }
        });
        let downloads = fx.sort("tidy-downloads");
        assert!(downloads.capped);
        assert_eq!(downloads.limit as usize, tidy::LIMIT);
        // The cap counts files, not the subfolder listed among them, and the files after it are counted in the
        // same listing: together they are every loose file of the folder.
        assert_eq!(downloads.loose as usize, tidy::LIMIT);
        assert!(downloads.moves.len() <= tidy::LIMIT);
        let files = std::fs::read_dir(fx.profile.home.join("Downloads"))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_file())
            .count() as u64;
        assert_eq!(downloads.loose + downloads.later, files);
        // Deterministic: the first ones in name order.
        let names: Vec<&str> = downloads.moves.iter().map(|m| m.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, sorted);
        let planned = plan(&fx.scanned, &fx.env, &["tidy-downloads".into()]).unwrap();
        assert!(planned.summary().moves as usize <= tidy::LIMIT);
    }

    /// Runs `ids` with journals, keeping the checkpoint the run hands over before the sorts.
    fn run(
        fx: &Fx,
        ids: &[String],
        cancel: &CancelToken,
    ) -> (RunReport, Vec<RunEvent>, Vec<RunReport>) {
        let planned = plan(&fx.scanned, &fx.env, ids).unwrap();
        let journals = fx.journals();
        let mut events = Vec::new();
        let mut checkpoints = Vec::new();
        let report = execute_with(
            &planned,
            &fx.env,
            Some(&journals),
            &mut |r| {
                checkpoints.push(r.clone());
                Ok(())
            },
            &|| None,
            &mut |e| {
                if matches!(e, RunEvent::MoveProgress(_)) && cancel_after_first(&e) {
                    cancel.cancel();
                }
                events.push(e)
            },
            cancel,
        );
        (report, events, checkpoints)
    }

    thread_local! {
        static STOP_AFTER_FIRST_MOVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    fn cancel_after_first(e: &RunEvent) -> bool {
        STOP_AFTER_FIRST_MOVE.with(|s| s.get())
            && matches!(e, RunEvent::MoveProgress(p) if p.done == 1)
    }

    #[test]
    fn runs_every_sort_with_a_journal_and_undo_puts_everything_back_even_after_a_restart() {
        let fx = Fx::new();
        let before = fx.known();
        let mut ids: Vec<String> = fx
            .scanned
            .proposals
            .list()
            .iter()
            .filter(|p| p.action == ProposalAction::Delete)
            .map(|p| p.id.clone())
            .collect();
        ids.extend(super::windows::ids(&fx));
        let planned = plan(&fx.scanned, &fx.env, &ids).unwrap().summary();
        // A file changed after the scan, and one held open while the run goes.
        std::fs::write(
            &fx.profile.tidy.changed_later,
            b"changed after the scan, longer than before",
        )
        .unwrap();
        let changed_line = fx.rel(&fx.profile.tidy.changed_later);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&fx.profile.tidy.held_open)
            .unwrap();
        let (report, events, checkpoints) = run(&fx, &ids, &CancelToken::new());
        drop(held);

        // The checkpoint was kept before any file moved, naming each sort's journal.
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].id, report.id);
        assert_eq!(checkpoints[0].reorganized.len(), 3);
        assert!(
            checkpoints[0]
                .reorganized
                .iter()
                .all(|r| r.interrupted && !r.journal.is_empty())
        );

        // Moves free nothing and never count as deleted.
        let sorts: Vec<_> = report
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Move)
            .collect();
        assert_eq!(sorts.len(), 3);
        assert!(
            sorts
                .iter()
                .all(|s| s.freed_bytes == 0 && s.deleted_items == 0)
        );
        // Moved and left add up to what the review meant to move, plan-time refusals included.
        for s in &sorts {
            let left: u64 = s.skipped.iter().map(|k| k.items).sum();
            assert_eq!(s.moved_items + left, s.planned_items, "{}", s.id);
        }
        let deletes: u64 = report
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Delete)
            .map(|s| s.freed_bytes)
            .sum();
        assert_eq!(report.freed_bytes, deletes);
        assert_eq!(
            report.moved_items,
            report
                .reorganized
                .iter()
                .map(|r| r.moved_items)
                .sum::<u64>()
        );
        assert_eq!(
            report.moved_items + 2,
            planned.moves,
            "all but the open and the changed file moved"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, RunEvent::MoveProgress(_)))
        );
        // The open file and the changed one stayed, each with a reason that names what happened.
        let skips: Vec<_> = sorts.iter().flat_map(|s| &s.skipped).collect();
        let held_skip = skips
            .iter()
            .find(|k| k.path.ends_with("statement-2026-09.pdf"))
            .expect("the open file is reported");
        assert_eq!(held_skip.kind, SkipKind::InUse);
        let changed_skip = skips
            .iter()
            .find(|k| k.path.ends_with("notes-meeting.docx"))
            .expect("the changed file is reported");
        assert_eq!(changed_skip.kind, SkipKind::Changed);
        assert!(sorts.iter().any(|s| s.outcome == Outcome::Partial));
        // Moved files are where the review said; the collision kept the user's file.
        let documents = report
            .reorganized
            .iter()
            .find(|r| r.id == "tidy-documents")
            .unwrap();
        assert!(documents.undoable && documents.folders_made > 0);
        assert_eq!(
            std::fs::read(&fx.profile.tidy.collision_existing)
                .unwrap()
                .len(),
            10 * 1024
        );
        let last = year_ago(400);
        assert!(
            fx.profile
                .home
                .join(format!("Documents\\{last}\\tax-return (1).pdf"))
                .exists()
        );
        assert!(!fx.profile.tidy.collision.exists());
        // The old installer was deleted by its cleanup, not moved.
        assert!(!fx.profile.tidy.deleted_too.exists());
        assert!(
            !fx.profile
                .home
                .join("Downloads\\Installers\\old-setup.exe")
                .exists()
        );

        // Kept, the run reopens after a "restart" from its journals alone, and each folder undoes on its own.
        let history = fixture::app_data(&fx.root).join("runs");
        report::save(&history, &report).unwrap();
        let mut kept = report::open(&history, &report.id).unwrap();
        assert!(
            !tidy::refresh(&fx.env, &fx.journals(), &mut kept),
            "a finished run reads as it was kept"
        );
        for id in ["tidy-downloads", "tidy-desktop", "tidy-documents"] {
            tidy::undo(&fx.env, &fx.journals(), &mut kept, id, &CancelToken::new()).unwrap();
            let r = kept.reorganized.iter().find(|r| r.id == id).unwrap();
            let undone = r.undo.as_ref().unwrap();
            assert_eq!(
                undone.back, r.moved_items,
                "{id}: everything that moved came back"
            );
            assert!(undone.stuck.is_empty(), "{id}: {:?}", undone.stuck);
            assert_eq!(
                undone.removed.len() as u64,
                r.folders_made,
                "{id}: the empty folders it made went"
            );
            assert!(!r.undoable);
        }
        // Undo again changes nothing.
        let again = kept.clone();
        tidy::undo(
            &fx.env,
            &fx.journals(),
            &mut kept,
            "tidy-downloads",
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(
            kept.reorganized[0].undo.as_ref().unwrap().back,
            again.reorganized[0].undo.as_ref().unwrap().back
        );

        // The known folders are exactly as before, but for what the cleanups deleted and the file changed on purpose.
        let after: BTreeSet<String> = fx.known().into_iter().collect();
        let before: BTreeSet<String> = before.into_iter().collect();
        let gone: Vec<_> = before.difference(&after).collect();
        let new: Vec<_> = after.difference(&before).collect();
        assert!(
            gone.iter()
                .all(|l| l.starts_with("Users/fixture/Downloads/old-setup.exe\t")
                    || l.starts_with(&format!("{changed_line}\t"))),
            "only the deleted installer and the changed file differ: {gone:#?}"
        );
        assert!(
            new.iter()
                .all(|l| l.starts_with(&format!("{changed_line}\t"))),
            "nothing new is left behind: {new:#?}"
        );
    }

    #[test]
    fn a_stopped_sort_keeps_its_journal_and_undo_puts_back_what_moved() {
        let fx = Fx::new();
        let before = fx.known();
        STOP_AFTER_FIRST_MOVE.with(|s| s.set(true));
        let cancel = CancelToken::new();
        let (report, _, _) = run(
            &fx,
            &["tidy-downloads".into(), "tidy-desktop".into()],
            &cancel,
        );
        STOP_AFTER_FIRST_MOVE.with(|s| s.set(false));
        assert!(report.cancelled);
        let downloads = &report.reorganized[0];
        assert_eq!(downloads.moved_items, 1);
        assert!(downloads.undoable);
        let step = report
            .steps
            .iter()
            .find(|s| s.id == "tidy-downloads")
            .unwrap();
        assert!(
            step.skipped
                .iter()
                .any(|k| k.kind == SkipKind::Cancelled && k.items > 0)
        );
        // The sort the stop never reached moved nothing and has no journal to undo.
        let desktop = &report.reorganized[1];
        assert_eq!(desktop.moved_items, 0);
        assert!(desktop.journal.is_empty() && !desktop.undoable);
        let mut kept = report.clone();
        tidy::undo(
            &fx.env,
            &fx.journals(),
            &mut kept,
            "tidy-downloads",
            &CancelToken::new(),
        )
        .unwrap();
        assert_eq!(kept.reorganized[0].undo.as_ref().unwrap().back, 1);
        assert!(matches!(
            tidy::undo(
                &fx.env,
                &fx.journals(),
                &mut kept,
                "tidy-desktop",
                &CancelToken::new()
            ),
            Err(tidy::TidyError::NoJournal)
        ));
        assert_eq!(fx.known(), before);
    }

    #[test]
    fn a_report_can_never_name_a_journal_outside_the_journal_folder() {
        let fx = Fx::new();
        let (mut report, _, _) = run(&fx, &["tidy-desktop".into()], &CancelToken::new());
        report.reorganized[0].journal = "..\\runs\\x.journal".into();
        assert!(matches!(
            tidy::undo(
                &fx.env,
                &fx.journals(),
                &mut report,
                "tidy-desktop",
                &CancelToken::new()
            ),
            Err(tidy::TidyError::Unreadable(_))
        ));
        assert!(
            fx.profile.home.join("Desktop\\Images").exists(),
            "nothing was undone"
        );
    }

    /// A plan of `ids`, as the confirm dialog gets it, and the run of it.
    fn planned(fx: &Fx, ids: &[&str]) -> vacancy_core::Plan {
        let ids: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        plan(&fx.scanned, &fx.env, &ids).unwrap()
    }

    #[test]
    fn a_run_whose_report_cant_be_kept_moves_nothing_and_says_why() {
        let fx = Fx::new();
        let before = fx.known();
        let plan = planned(&fx, &["tidy-downloads", "tidy-desktop"]);
        let report = execute_with(
            &plan,
            &fx.env,
            Some(&fx.journals()),
            &mut |_| Err("the disk is full".to_owned()),
            &|| None,
            &mut |_| {},
            &CancelToken::new(),
        );
        assert_eq!(report.moved_items, 0);
        for r in &report.reorganized {
            assert!(
                r.journal.is_empty() && !r.undoable,
                "{}: no journal begun",
                r.id
            );
        }
        for s in report.steps.iter().filter(|s| s.kind == StepKind::Move) {
            assert_eq!(s.outcome, Outcome::Skipped);
            assert!(
                s.skipped.iter().any(|k| k.reason.contains(
                    "couldn't keep this run in Past runs before moving files (the disk is full)"
                )),
                "{:#?}",
                s.skipped
            );
        }
        assert!(
            !fx.journals().dir.exists(),
            "no journal folder or journal was made"
        );
        assert_eq!(fx.known(), before);
    }

    #[test]
    fn reviewed_names_are_used_exactly_or_the_file_stays_with_why() {
        let fx = Fx::new();
        let downloads = fx.sort("tidy-downloads");
        let photo = downloads
            .moves
            .iter()
            .find(|m| m.name == "IMG_3301.jpg")
            .unwrap()
            .clone();
        let documents = fx.sort("tidy-documents");
        let tax = documents
            .moves
            .iter()
            .find(|m| m.name == "tax-return.pdf")
            .unwrap()
            .clone();
        assert_eq!(tax.to, "tax-return (1).pdf");
        // After the review: the file the collision was with goes, and a new file takes a reviewed name.
        std::fs::remove_file(&fx.profile.tidy.collision_existing).unwrap();
        let images = fx.profile.home.join("Downloads").join(&photo.dest);
        std::fs::create_dir_all(&images).unwrap();
        std::fs::write(images.join("IMG_3301.jpg"), b"someone else's photo").unwrap();
        let plan = planned(&fx, &["tidy-downloads", "tidy-documents"]);
        let refused = plan.summary().moves_refused;
        let skip = refused
            .iter()
            .find(|k| k.path.ends_with("\\IMG_3301.jpg"))
            .expect("the photo whose reviewed name is taken is left out");
        assert!(
            skip.reason.contains("won't pick a different one"),
            "{}",
            skip.reason
        );
        let (report, _, _) = run_plan(&fx, &plan);
        assert!(report.moved_items > 0);
        // Still the reviewed name, though the plain one is free now; and the photo stayed, never renumbered.
        assert!(
            fx.profile
                .home
                .join("Documents")
                .join(&tax.dest)
                .join("tax-return (1).pdf")
                .exists()
        );
        assert!(
            !fx.profile
                .home
                .join("Documents")
                .join(&tax.dest)
                .join("tax-return.pdf")
                .exists()
        );
        assert!(fx.profile.home.join("Downloads\\IMG_3301.jpg").exists());
        assert!(!images.join("IMG_3301 (1).jpg").exists());
        assert_eq!(
            std::fs::read(images.join("IMG_3301.jpg")).unwrap(),
            b"someone else's photo"
        );
    }

    /// Runs an already made plan with journals and a checkpoint that always keeps.
    fn run_plan(fx: &Fx, plan: &vacancy_core::Plan) -> (RunReport, Vec<RunEvent>, Vec<RunReport>) {
        let mut events = Vec::new();
        let mut checkpoints = Vec::new();
        let report = execute_with(
            plan,
            &fx.env,
            Some(&fx.journals()),
            &mut |r| {
                checkpoints.push(r.clone());
                Ok(())
            },
            &|| None,
            &mut |e| events.push(e),
            &CancelToken::new(),
        );
        (report, events, checkpoints)
    }

    #[test]
    fn a_delete_whose_target_was_refused_while_planning_no_longer_holds_its_file_out_of_the_sort() {
        let fx = Fx::new();
        // The old installer is replaced after the scan: the cleanup refuses it while planning, so the sort must not
        // quietly leave it out on that cleanup's behalf. It changed for the sort too, which says so.
        std::fs::remove_file(&fx.profile.tidy.deleted_too).unwrap();
        std::fs::write(&fx.profile.tidy.deleted_too, b"another file in its place").unwrap();
        let summary = planned(&fx, &["w-installers", "tidy-downloads"]).summary();
        assert!(
            summary
                .refused
                .iter()
                .any(|k| k.path.ends_with("old-setup.exe")),
            "the cleanup refused it: {:#?}",
            summary.refused
        );
        let left = summary
            .moves_refused
            .iter()
            .find(|k| k.path.ends_with("old-setup.exe"))
            .expect("the sort reports it rather than holding it out silently");
        assert_eq!(left.kind, SkipKind::Changed);
    }

    #[test]
    fn a_journal_another_window_holds_leaves_the_kept_record_as_it_was() {
        let fx = Fx::new();
        let (report, _, _) = run(&fx, &["tidy-desktop".into()], &CancelToken::new());
        let journals = fx.journals();
        let mut env = fx.env.clone();
        env.app_data = journals.dir.clone();
        let held = vacancy_core::reorg::Engine::open(
            &env,
            &journals.dir.join(&report.reorganized[0].journal),
        )
        .unwrap();
        let mut kept = report.clone();
        kept.reorganized[0].note = Some("Something earlier.".into());
        let as_kept = kept.clone();
        assert!(
            !tidy::refresh(&fx.env, &journals, &mut kept),
            "nothing to save"
        );
        assert_eq!(kept, as_kept);
        assert!(matches!(
            tidy::undo(
                &fx.env,
                &journals,
                &mut kept,
                "tidy-desktop",
                &CancelToken::new()
            ),
            Err(tidy::TidyError::Busy)
        ));
        drop(held);
        // A note a failed open left goes once the journal reads again.
        kept.reorganized[0].note = Some(tidy::TidyError::Busy.to_string());
        assert!(tidy::refresh(&fx.env, &journals, &mut kept));
        assert_eq!(kept.reorganized[0].note, None);
        assert_eq!(kept.reorganized[0], report.reorganized[0]);
    }

    #[test]
    fn a_report_kept_only_as_its_checkpoint_reopens_from_the_journals_and_undoes() {
        let fx = Fx::new();
        let before = fx.known();
        let (report, _, checkpoints) = run(
            &fx,
            &["tidy-downloads".into(), "tidy-desktop".into()],
            &CancelToken::new(),
        );
        // The app closed after the moves, before the final report was kept: only the checkpoint is in the history.
        let history = fixture::app_data(&fx.root).join("runs");
        report::save(&history, &checkpoints[0]).unwrap();
        let mut kept = report::open(&history, &report.id).unwrap();
        assert!(
            kept.reorganized
                .iter()
                .all(|r| r.interrupted && r.moved_items == 0 && !r.undoable)
        );
        assert!(tidy::refresh(&fx.env, &fx.journals(), &mut kept));
        assert_eq!(kept.moved_items, report.moved_items);
        for (r, ran) in kept.reorganized.iter().zip(&report.reorganized) {
            assert_eq!(r.moved_items, ran.moved_items, "{}", r.id);
            assert!(
                r.undoable && !r.interrupted,
                "{}: the journal finished",
                r.id
            );
            assert_eq!(r.undo, None);
        }
        // The list Past runs shows offers Undo for it too.
        let listing = vacancy_core::RunListing::from(&kept);
        assert!(listing.tidy.iter().all(|t| t.undoable && !t.interrupted));
        for id in ["tidy-downloads", "tidy-desktop"] {
            tidy::undo(&fx.env, &fx.journals(), &mut kept, id, &CancelToken::new()).unwrap();
            let r = kept.reorganized.iter().find(|r| r.id == id).unwrap();
            let u = r.undo.as_ref().unwrap();
            assert!(u.at.is_some() && u.stuck.is_empty() && u.back == r.moved_items);
        }
        assert_eq!(fx.known(), before);
    }

    #[test]
    fn undo_says_which_files_couldnt_go_back_and_which_folders_it_kept() {
        let fx = Fx::new();
        let (report, _, _) = run(&fx, &["tidy-downloads".into()], &CancelToken::new());
        let home = fx.profile.home.join("Downloads");
        let downloads = fx.sort("tidy-downloads");
        let dest = |name: &str| {
            let m = downloads.moves.iter().find(|m| m.name == name).unwrap();
            home.join(&m.dest).join(&m.to)
        };
        // A new file takes one file's old name; another moved file is replaced where it went; and the user adds a
        // file of their own to a folder the run made.
        std::fs::write(home.join("drivers.7z"), b"a new download with the old name").unwrap();
        let replaced = dest("boarding-pass.png");
        std::fs::remove_file(&replaced).unwrap();
        std::fs::write(&replaced, b"a different file in its place").unwrap();
        let videos = dest("birthday.mov").parent().unwrap().to_path_buf();
        std::fs::write(videos.join("mine.mov"), b"the user's own").unwrap();
        let mut kept = report.clone();
        tidy::undo(
            &fx.env,
            &fx.journals(),
            &mut kept,
            "tidy-downloads",
            &CancelToken::new(),
        )
        .unwrap();
        let r = &kept.reorganized[0];
        let u = r.undo.as_ref().unwrap();
        assert_eq!(u.back + u.stuck.len() as u64, r.moved_items);
        let stuck = |name: &str| {
            u.stuck
                .iter()
                .find(|k| k.path.ends_with(name))
                .unwrap_or_else(|| panic!("{name}: {:#?}", u.stuck))
        };
        assert_eq!(stuck("drivers.7z").reason, "A file now has its old name");
        assert_eq!(stuck("boarding-pass.png").kind, SkipKind::Changed);
        // The made folder the user added to is kept, and so is its parent; neither holds a stuck file.
        let videos_rel = videos.strip_prefix(&home).unwrap().display().to_string();
        assert!(u.kept.contains(&videos_rel), "{:#?}", u.kept);
        assert!(u.kept.contains(&"Videos".to_owned()));
        assert!(!u.removed.is_empty() && u.removed.iter().all(|d| !home.join(d).exists()));
        assert!(videos.join("mine.mov").exists());
        assert_eq!(
            std::fs::read(&replaced).unwrap(),
            b"a different file in its place"
        );
        assert_eq!(
            std::fs::read(home.join("drivers.7z")).unwrap(),
            b"a new download with the old name"
        );
        // Undo can try again for what stayed, and an Undo that tries nothing leaves the last one as it was.
        assert!(r.undoable);
    }
}

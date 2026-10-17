//! The planner and the executor against a fixture profile on a real file system: what a run deletes, what it leaves
//! and why, and what it reports. Each test builds its own profile (`vacancy_core::fixture::build`) in a temp
//! folder, scans it in fixture mode (the Guard confined to it), plans every delete proposal, runs the plan, and
//! compares listings of the whole tree before and after, file contents included.

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use vacancy_core::fixture::{self, Profile};
use vacancy_core::{
    CancelToken, Env, ItemRef, Moment, Outcome, Plan, ProposalAction, RunEvent, RunReport,
    ScannedVolume, SkipKind, execute, plan, scan_for_map_with,
};

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

/// A fixture profile, scanned in fixture mode.
struct Fx {
    _dir: tempfile::TempDir,
    root: PathBuf,
    profile: Profile,
    env: Env,
    scanned: ScannedVolume,
}

impl Fx {
    fn new() -> Self {
        Self::with(|_| {})
    }

    /// A profile changed by `setup` before it is scanned.
    fn with(setup: impl FnOnce(&Profile)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = support::real_path(dir.path()).join("fixture");
        let profile = fixture::build(&root).expect("build the fixture profile");
        setup(&profile);
        // What `setup` changed must look older than the scan, as everything `build` wrote does.
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

    /// Every delete proposal's id.
    fn all_ids(&self) -> Vec<String> {
        self.scanned
            .proposals
            .list()
            .iter()
            .filter(|p| p.action == ProposalAction::Delete)
            .map(|p| p.id.clone())
            .collect()
    }

    fn plan(&self, ids: &[String]) -> Plan {
        plan(&self.scanned, &self.env, ids).expect("a plan")
    }

    /// Runs `plan`, collecting its events.
    fn run(&self, plan: &Plan, cancel: &CancelToken) -> (RunReport, Vec<RunEvent>) {
        let mut events = Vec::new();
        let root = self.root.clone();
        let figures = move || {
            let snap = support::scan_with(&root, 1 << 20);
            Some(fixture::volume(&root, 1 << 30, &snap))
        };
        let report = execute(plan, &self.env, &figures, &mut |e| events.push(e), cancel);
        (report, events)
    }

    fn inventory(&self) -> BTreeSet<String> {
        fixture::inventory(&self.root)
            .expect("list the fixture")
            .into_iter()
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

    /// What the plan for `ids` may remove, relative to the root: folders (with everything below), files, and
    /// folders' loose files (`folder/*`, the names the scan kept individually excepted).
    fn planned(&self, ids: &[String]) -> Vec<Planned> {
        let snap = &self.scanned.snapshot;
        let mut out = Vec::new();
        for id in ids {
            let i = self.scanned.proposals.position(id).unwrap();
            for t in self.scanned.proposals.targets(i) {
                out.push(match *t {
                    ItemRef::Dir(d) => Planned::Tree(self.rel(&snap.path(d))),
                    ItemRef::File(f) => Planned::File(self.rel(&snap.file_path(f))),
                    ItemRef::FilesIn(d) => Planned::Loose(
                        self.rel(&snap.path(d)),
                        snap.files(d)
                            .map(|f| snap.file(f).name.to_string_lossy().into_owned())
                            .collect(),
                    ),
                    ItemRef::Proposal(_) => unreachable!(),
                });
            }
        }
        out
    }
}

#[derive(Debug)]
enum Planned {
    Tree(String),
    File(String),
    Loose(String, Vec<String>),
}

impl Planned {
    /// Whether the inventory line for `rel` is something this target may remove.
    fn covers(&self, rel: &str, line: &str) -> bool {
        match self {
            Planned::Tree(t) => rel == t || rel.starts_with(&format!("{t}/")),
            Planned::File(f) => rel == f,
            Planned::Loose(d, kept) => rel.strip_prefix(&format!("{d}/")).is_some_and(|name| {
                !name.contains('/') && line.contains("\tfile ") && !kept.iter().any(|k| k == name)
            }),
        }
    }
}

fn scan(root: &Path, env: &Env) -> ScannedVolume {
    let root = root.to_path_buf();
    let figures_root = root.clone();
    scan_for_map_with(
        &root,
        env,
        &moment(),
        &move |snap| Some(fixture::volume(&figures_root, 1 << 30, snap)),
        &mut |_| {},
        &CancelToken::new(),
    )
    .expect("scan the fixture")
}

fn path_of(line: &str) -> &str {
    line.split('\t').next().unwrap()
}

/// The lines of `before` that are gone in `after`, and the lines of `after` that are new.
fn diff(before: &BTreeSet<String>, after: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    (
        before.difference(after).cloned().collect(),
        after.difference(before).cloned().collect(),
    )
}

/// Holds a file open the way an app does: on Windows any open handle makes the exclusive open fail; on macOS the
/// executor sees this process's descriptor.
fn hold_open(path: &Path) -> std::fs::File {
    std::fs::File::open(path).expect("hold the file open")
}

#[test]
fn the_fixture_profile_is_really_set_up() {
    let fx = Fx::new();
    let p = &fx.profile;
    let link = std::fs::symlink_metadata(&p.link_to_documents).unwrap();
    assert!(link.file_type().is_symlink(), "a link inside the cache");
    assert!(
        p.long_path.as_os_str().len() > 260,
        "{}",
        p.long_path.display()
    );
    assert!(
        std::fs::metadata(&p.read_only)
            .unwrap()
            .permissions()
            .readonly()
    );
    assert!(
        std::fs::symlink_metadata(p.node_modules.join("left-pad"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let ids = fx.all_ids();
    for want in if cfg!(windows) {
        [
            "w-temp",
            "w-browser",
            "w-pkgcache",
            "w-dumps",
            "w-node",
            "w-installers",
        ]
        .as_slice()
    } else {
        [
            "m-caches", "m-brew", "m-logs", "m-trash", "m-node", "m-dmgs",
        ]
        .as_slice()
    } {
        assert!(ids.iter().any(|i| i == want), "{want} is proposed: {ids:?}");
    }
    let planned = fx.planned(&ids);
    let covered = |path: &Path| {
        let rel = fx.rel(path);
        planned.iter().any(|t| t.covers(&rel, "\tfile "))
    };
    for path in [&p.held_open, &p.modified_later, &p.read_only, &p.long_path] {
        assert!(covered(path), "{} is in a target", path.display());
    }
    assert!(covered(&p.link_to_documents), "the link is inside a target");
    for path in p.untouched.iter().chain([&p.documents, &p.store_file]) {
        assert!(!covered(path), "{} is in no target", path.display());
    }
}

/// Everything proposed is approved and runs: exactly the approved items go, Documents survives behind the link
/// into it, the package store keeps the file `node_modules` had a hard link to, an open file is skipped with a
/// reason, a target modified after the scan is skipped whole, and read-only files and long paths go.
#[test]
fn a_run_deletes_exactly_what_was_approved() {
    let fx = Fx::new();
    let ids = fx.all_ids();
    let plan = fx.plan(&ids);
    let summary = plan.summary();
    assert!(summary.refused.is_empty(), "{:?}", summary.refused);
    assert!(summary.bytes > 0 && summary.items > 0);

    // After the scan: one target modified, one file held open.
    std::fs::write(&fx.profile.modified_later, b"new contents").unwrap();
    let _held = hold_open(&fx.profile.held_open);

    let before = fx.inventory();
    let (report, events) = fx.run(&plan, &CancelToken::new());
    let after = fx.inventory();
    let (gone, added) = diff(&before, &after);

    assert!(added.is_empty(), "nothing appears: {added:#?}");
    let planned = fx.planned(&ids);
    for line in &gone {
        let rel = path_of(line);
        assert!(
            planned.iter().any(|t| t.covers(rel, line)),
            "only approved items go, but {line} went"
        );
    }
    let kept = |path: &Path| {
        let rel = fx.rel(path);
        after.iter().any(|l| path_of(l) == rel)
    };
    let p = &fx.profile;
    assert!(kept(&p.held_open), "the open file stays");
    assert!(kept(&p.modified_later), "the modified target stays");
    assert!(!kept(&p.read_only), "the read-only file goes");
    assert!(!kept(&p.long_path), "the long path goes");
    assert!(!kept(&p.link_to_documents), "the link goes, as a link");
    assert!(!kept(&p.node_modules), "node_modules goes whole");
    assert!(kept(&p.store_file), "the store keeps its file");
    let docs = fx.rel(&p.documents);
    let doc_lines = |set: &BTreeSet<String>| {
        set.iter()
            .filter(|l| path_of(l) == docs || path_of(l).starts_with(&format!("{docs}/")))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(doc_lines(&before).len() >= 5);
    assert_eq!(
        doc_lines(&before),
        doc_lines(&after),
        "Documents is byte for byte the same"
    );
    for path in &p.untouched {
        assert!(kept(path), "{} stays", path.display());
    }

    // Everything else planned is gone.
    let skipped_paths: Vec<String> = report
        .steps
        .iter()
        .flat_map(|s| &s.skipped)
        .map(|k| k.path.clone())
        .collect();
    for line in &before {
        let rel = path_of(line);
        let mine = planned.iter().any(|t| t.covers(rel, line));
        let near_a_skip = skipped_paths.iter().any(|s| {
            let s = fx.rel(Path::new(s));
            rel == s || rel.starts_with(&format!("{s}/")) || s.starts_with(&format!("{rel}/"))
        });
        if mine && !near_a_skip && line.contains("\tfile ") {
            assert!(
                gone.contains(line),
                "{line} was approved and should be gone"
            );
        }
    }

    // The report.
    let in_use: Vec<_> = report
        .steps
        .iter()
        .flat_map(|s| &s.skipped)
        .filter(|k| k.kind == SkipKind::InUse)
        .collect();
    assert_eq!(in_use.len(), 1, "{:#?}", report.steps);
    assert!(in_use[0].path.ends_with("held-open.bin"), "{:?}", in_use[0]);
    let changed: Vec<_> = report
        .steps
        .iter()
        .flat_map(|s| &s.skipped)
        .filter(|k| k.kind == SkipKind::Changed)
        .collect();
    assert_eq!(changed.len(), 1, "{:#?}", report.steps);
    assert!(
        Path::new(&changed[0].path)
            .ends_with(p.modified_later.parent().unwrap().file_name().unwrap()),
        "{:?}",
        changed[0]
    );
    let cache_step = if cfg!(windows) {
        "w-browser"
    } else {
        "m-caches"
    };
    let partial = report.steps.iter().find(|s| s.id == cache_step).unwrap();
    assert_eq!(partial.outcome, Outcome::Partial);
    let node = if cfg!(windows) { "w-node" } else { "m-node" };
    for s in report.steps.iter().filter(|s| s.id != cache_step) {
        assert_eq!(s.outcome, Outcome::Done, "{s:#?}");
        if s.id != node {
            assert_eq!(s.deleted_items, s.planned_items, "{s:#?}");
            assert_eq!(s.freed_bytes, s.planned_bytes, "{s:#?}");
        }
    }
    // The file node_modules shares with the package store: its name there goes, the file stays, and the report
    // says so. (The scan counts it once, wherever it saw it first, so the step's planned figures may or may not
    // hold it.)
    let node = report.steps.iter().find(|s| s.id == node).unwrap();
    let linked: Vec<_> = node
        .skipped
        .iter()
        .filter(|k| k.kind == SkipKind::HardLinked)
        .collect();
    assert_eq!(linked.len(), 1, "{node:#?}");
    assert_eq!(linked[0].items, 1);
    assert!(
        linked[0].path.ends_with("store-linked.js"),
        "{:?}",
        linked[0]
    );
    // Every name in it went, the shared file's too; the scan counted that file here or in the store.
    assert!(
        node.deleted_items == node.planned_items || node.deleted_items == node.planned_items + 1,
        "{node:#?}"
    );
    assert_eq!(
        report.skipped_items,
        report
            .steps
            .iter()
            .flat_map(|s| &s.skipped)
            .filter(|k| k.kind != SkipKind::HardLinked)
            .map(|k| k.items)
            .sum::<u64>(),
        "a name deleted while another keeps the file is not a skip"
    );
    assert!(report.freed_bytes > 0);
    assert!(report.freed_bytes <= report.planned_bytes);
    assert_eq!(
        report.freed_bytes,
        report.steps.iter().map(|s| s.freed_bytes).sum::<u64>()
    );
    assert_eq!(
        report.free_change, report.freed_bytes as i64,
        "the pseudo-drive's free space grew by exactly what was freed"
    );
    assert!(!report.cancelled);
    let biggest = report.steps.iter().max_by_key(|s| s.freed_bytes).unwrap();
    assert_eq!(report.biggest_win.as_deref(), Some(biggest.id.as_str()));
    let by_category: u64 = report.freed_by_category.iter().map(|c| c.bytes).sum();
    assert_eq!(by_category, report.freed_bytes);
    let started = events
        .iter()
        .filter(|e| matches!(e, RunEvent::StepStarted(_)))
        .count();
    assert_eq!(started, plan.summary().steps.len());
    assert!(events.iter().any(|e| matches!(e, RunEvent::Skipped(_))));
    // The run's freed counter only grows, and ends at the report's total.
    let totals: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Progress(p) => Some(p.total_freed_bytes),
            _ => None,
        })
        .collect();
    assert!(totals.windows(2).all(|w| w[0] <= w[1]), "{totals:?}");
    assert_eq!(totals.last().copied(), Some(report.freed_bytes));
}

/// The step whose cache folder holds the file to hold open, for tests that run part of the plan.
fn cache_step() -> &'static str {
    if cfg!(windows) {
        "w-browser"
    } else {
        "m-caches"
    }
}

/// Every skip in a report.
fn skips(report: &RunReport) -> Vec<&vacancy_core::Skip> {
    report.steps.iter().flat_map(|s| &s.skipped).collect()
}

/// Puts a folder link at `link` to `to`: a junction on Windows, a symlink elsewhere.
fn link_dir(link: &Path, to: &Path) {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(to)
            .output()
            .unwrap();
        assert!(out.status.success(), "mklink /J {}", link.display());
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(to, link).unwrap();
}

/// A cancel that lands right after the first place is deleted stops the run there: that place is gone, every
/// other planned place is still there, whole, and the report says exactly that.
#[test]
fn cancelling_mid_run_leaves_a_consistent_report() {
    let fx = Fx::new();
    let ids = fx.all_ids();
    let plan = fx.plan(&ids);
    let before = fx.inventory();
    let cancel = CancelToken::new();
    let stop = cancel.clone();
    let mut events = Vec::new();
    let root = fx.root.clone();
    let figures = move || {
        let snap = support::scan_with(&root, 1 << 20);
        Some(fixture::volume(&root, 1 << 30, &snap))
    };
    let report = execute(
        &plan,
        &fx.env,
        &figures,
        &mut |e| {
            if matches!(e, RunEvent::Deleted(_)) {
                stop.cancel();
            }
            events.push(e);
        },
        &cancel,
    );
    let after = fx.inventory();
    let (gone, added) = diff(&before, &after);
    assert!(report.cancelled);
    assert!(added.is_empty(), "{added:#?}");

    let deleted: Vec<&vacancy_core::Deleted> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Deleted(d) => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(
        deleted.len(),
        1,
        "stopped after the first place: {deleted:#?}"
    );
    let first = fx.rel(Path::new(&deleted[0].path));
    for line in &gone {
        let rel = path_of(line);
        assert!(
            rel == first || rel.starts_with(&format!("{first}/")),
            "only the first place went, but {line} went too"
        );
    }
    assert!(!gone.is_empty(), "the first place went");

    // Every other place is reported as left because of the cancel, and is still there.
    let cancelled: Vec<_> = skips(&report)
        .into_iter()
        .filter(|k| k.kind == SkipKind::Cancelled)
        .collect();
    let places: u64 = report.steps.iter().map(|s| s.places).sum();
    assert_eq!(
        cancelled.len() as u64,
        places - 1,
        "every place after the first: {:#?}",
        report.steps
    );
    for k in &cancelled {
        let rel = fx.rel(Path::new(&k.path));
        assert!(
            after.iter().any(|l| path_of(l) == rel),
            "{rel} is still there"
        );
    }
    assert_eq!(report.deleted_items, deleted[0].items);
    assert_eq!(report.freed_bytes, deleted[0].bytes);
    assert_eq!(report.free_change, report.freed_bytes as i64);
}

/// A cache folder swapped for a link to Documents between the plan and the run is skipped as a link: the walk to
/// it refuses the link, Documents is untouched, and so is the folder that was moved aside.
#[test]
fn a_target_swapped_for_a_link_before_the_run_is_skipped() {
    let fx = Fx::new();
    let ids = vec![cache_step().to_owned()];
    let plan = fx.plan(&ids);
    let cache = &fx.profile.cache;
    let aside = cache.with_extension("aside");
    std::fs::rename(cache, &aside).unwrap();
    link_dir(cache, &fx.profile.documents);
    let before = fx.inventory();

    let (report, _) = fx.run(&plan, &CancelToken::new());
    let after = fx.inventory();
    let (gone, _) = diff(&before, &after);
    let docs = fx.rel(&fx.profile.documents);
    let aside_rel = fx.rel(&aside);
    for line in &gone {
        let rel = path_of(line);
        assert!(
            !rel.starts_with(&docs) && !rel.starts_with(&aside_rel),
            "{line} went"
        );
    }
    let skip = skips(&report)
        .into_iter()
        .find(|k| Path::new(&k.path) == cache)
        .expect("the swapped target is reported");
    assert_eq!(skip.kind, SkipKind::Link, "{skip:?}");
    assert!(
        after.iter().any(|l| path_of(l) == fx.rel(cache)),
        "the link stays"
    );
}

/// The executor acts only on the plan, whatever the Guard is told at run time: with its roots widened to the whole
/// fixture it still deletes nothing outside the plan, and with them narrowed to Documents it deletes nothing.
#[test]
fn a_run_never_deletes_outside_the_plan_whatever_the_guard_allows() {
    let fx = Fx::new();
    let ids = vec![cache_step().to_owned()];
    let plan = fx.plan(&ids);
    let planned = fx.planned(&ids);

    let mut wide = fx.env.clone();
    wide.home = fx.root.clone();
    wide.confined_to = None;
    let before = fx.inventory();
    let report = execute(&plan, &wide, &|| None, &mut |_| {}, &CancelToken::new());
    let after = fx.inventory();
    let (gone, added) = diff(&before, &after);
    assert!(added.is_empty());
    assert!(!gone.is_empty(), "{:#?}", report.steps);
    for line in &gone {
        assert!(
            planned.iter().any(|t| t.covers(path_of(line), line)),
            "{line} was not in the plan"
        );
    }

    // A second plan, run with the Guard confined to Documents: nothing may go.
    let fx = Fx::new();
    let plan = fx.plan(&fx.all_ids());
    let mut narrow = fx.env.clone();
    narrow.confined_to = Some(fx.profile.documents.clone());
    let before = fx.inventory();
    let report = execute(&plan, &narrow, &|| None, &mut |_| {}, &CancelToken::new());
    assert_eq!(before, fx.inventory(), "nothing changed");
    assert_eq!(report.freed_bytes, 0);
    assert_eq!(report.deleted_items, 0);
    assert!(
        skips(&report)
            .iter()
            .all(|k| matches!(k.kind, SkipKind::Outside | SkipKind::Protected)),
        "{:#?}",
        report.steps
    );
}

/// A folder's loose files: one modified after the scan stays, one made after the scan was never approved and
/// stays, the rest go, and the folder itself stays.
#[test]
fn loose_files_new_or_changed_since_the_scan_stay() {
    let fx = Fx::new();
    let loose = &fx.profile.loose;
    let folder = loose.parent().unwrap().to_path_buf();
    let id =
        fx.scanned
            .proposals
            .list()
            .iter()
            .enumerate()
            .find(|(i, _)| {
                fx.scanned.proposals.targets(*i).iter().any(
                    |t| matches!(t, ItemRef::FilesIn(d) if fx.scanned.snapshot.path(*d) == folder),
                )
            })
            .map(|(_, p)| p.id.clone())
            .expect("a proposal takes the folder's loose files");
    let plan = fx.plan(&[id]);
    std::fs::write(loose, b"changed after the scan").unwrap();
    let new = folder.join("made-after-the-scan.tmp");
    std::fs::write(&new, b"new").unwrap();

    let (report, _) = fx.run(&plan, &CancelToken::new());
    assert!(loose.exists(), "the changed file stays");
    assert!(new.exists(), "the new file stays");
    assert!(folder.exists(), "the folder stays");
    let changed: Vec<_> = skips(&report)
        .into_iter()
        .filter(|k| k.kind == SkipKind::Changed)
        .collect();
    assert_eq!(changed.len(), 1, "{:#?}", report.steps);
    assert_eq!(changed[0].items, 1);
    let others: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        others.len(),
        2,
        "only the changed and the new file are left: {others:?}"
    );
}

/// A target replaced since the scan (another folder made under its name) is refused while planning, left out of
/// the numbers the confirm dialog shows, and reported as skipped when the plan runs.
#[test]
fn a_target_replaced_before_planning_is_refused() {
    let fx = Fx::new();
    let cache = &fx.profile.cache;
    std::fs::rename(cache, cache.with_extension("old")).unwrap();
    std::fs::create_dir(cache).unwrap();
    std::fs::write(cache.join("someone-else.bin"), b"not what the scan saw").unwrap();
    let plan = fx.plan(&[cache_step().to_owned()]);
    let summary = plan.summary();
    let refused: Vec<_> = summary
        .refused
        .iter()
        .filter(|k| Path::new(&k.path) == cache)
        .collect();
    assert_eq!(refused.len(), 1, "{:#?}", summary.refused);
    assert_eq!(refused[0].kind, SkipKind::Changed);
    let (report, _) = fx.run(&plan, &CancelToken::new());
    assert!(cache.join("someone-else.bin").exists());
    assert!(
        skips(&report)
            .iter()
            .any(|k| Path::new(&k.path) == cache && k.kind == SkipKind::Changed)
    );
}

/// Fixture mode only ever narrows: its root must be a real folder (no link on the way), not a drive's root, and
/// neither inside nor holding the real home folder.
#[test]
fn fixture_mode_refuses_roots_that_could_widen_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = support::real_path(dir.path());
    let home = base.join("home").join("me");
    let ok = base.join("fixtures").join("one");
    for p in [&home, &ok] {
        std::fs::create_dir_all(p).unwrap();
    }
    assert_eq!(fixture::check_root(&ok, &home), Ok(ok.clone()));
    assert!(!fixture::is_profile(&ok), "an empty folder is no profile");
    let profile = base.join("fixtures").join("built");
    fixture::build(&profile).unwrap();
    assert!(
        fixture::is_profile(&profile),
        "a profile the generator made"
    );
    let file = base.join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let link = base.join("linked");
    link_dir(&link, &ok);
    let drive = if cfg!(windows) {
        PathBuf::from(format!(
            "{}\\",
            base.components()
                .next()
                .unwrap()
                .as_os_str()
                .to_string_lossy()
        ))
    } else {
        PathBuf::from("/")
    };
    let cases = [
        (PathBuf::from("relative"), "relative"),
        (base.join("missing"), "missing"),
        (file, "a file"),
        (link.clone(), "a link"),
        (link.join("inside"), "through a link"),
        (drive, "a drive's root"),
        (home.join("Fixture"), "inside the home"),
        (home.clone(), "the home itself"),
        (base.join("home"), "holding the home"),
    ];
    std::fs::create_dir_all(home.join("Fixture")).unwrap();
    for (root, what) in cases {
        assert!(
            fixture::check_root(&root, &home).is_err(),
            "{what}: {}",
            root.display()
        );
    }
}

/// A volume mounted inside a cache folder is never entered or unmounted: the folder is skipped whole. On Windows the
/// mount is a mount-point reparse point naming a volume (a junction to `\\?\Volume{...}\`, made before the scan); on
/// macOS a disk image mounted on an empty folder of the cache after the plan was made.
#[test]
fn a_volume_mounted_inside_a_target_is_left_alone() {
    #[cfg(windows)]
    {
        let fx = Fx::with(|p| {
            let drive = p
                .root
                .components()
                .next()
                .unwrap()
                .as_os_str()
                .to_string_lossy()
                .into_owned();
            let out = std::process::Command::new("cmd")
                .args(["/C", &format!("mountvol {drive}\\ /L")])
                .output()
                .unwrap();
            let volume = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            assert!(volume.starts_with(r"\\?\Volume{"), "mountvol: {volume}");
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(p.cache.join("mounted"))
                .arg(&volume)
                .output()
                .unwrap();
            assert!(made.status.success(), "mklink /J to {volume}");
        });
        let before = fx.inventory();
        let plan = fx.plan(&[cache_step().to_owned()]);
        let (report, _) = fx.run(&plan, &CancelToken::new());
        let skip = skips(&report)
            .into_iter()
            .find(|k| Path::new(&k.path) == fx.profile.cache)
            .expect("the folder with the volume in it is reported");
        assert_eq!(skip.kind, SkipKind::OtherVolume, "{skip:?}");
        let cache = fx.rel(&fx.profile.cache);
        let after = fx.inventory();
        for line in before.iter().filter(|l| path_of(l).starts_with(&cache)) {
            assert!(after.contains(line), "{line} stays");
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        struct Detach(PathBuf);
        impl Drop for Detach {
            fn drop(&mut self) {
                let _ = Command::new("hdiutil")
                    .args(["detach", "-quiet", "-force"])
                    .arg(&self.0)
                    .status();
            }
        }
        let fx = Fx::with(|p| std::fs::create_dir(p.cache.join("mnt")).unwrap());
        let plan = fx.plan(&[cache_step().to_owned()]);
        let images = tempfile::tempdir().unwrap();
        let image = images.path().join("vacancy-run-test.dmg");
        let mnt = fx.profile.cache.join("mnt");
        let ran = |cmd: &mut Command| match cmd.output() {
            Ok(out) if out.status.success() => true,
            out => {
                assert!(!support::in_ci(), "hdiutil must work in CI: {out:?}");
                eprintln!("skipped: hdiutil: {out:?}");
                false
            }
        };
        if !ran(Command::new("hdiutil")
            .args([
                "create",
                "-quiet",
                "-size",
                "4m",
                "-fs",
                "HFS+",
                "-volname",
                "VacancyRun",
            ])
            .arg(&image))
        {
            return;
        }
        if !ran(Command::new("hdiutil")
            .args(["attach", "-quiet", "-nobrowse", "-mountpoint"])
            .arg(&mnt)
            .arg(&image))
        {
            return;
        }
        let _detach = Detach(mnt.clone());
        let on_volume = mnt.join("keep-me.txt");
        std::fs::write(&on_volume, b"on another volume").unwrap();
        let (report, _) = fx.run(&plan, &CancelToken::new());
        assert!(on_volume.exists(), "nothing on the mounted volume goes");
        let skip = skips(&report)
            .into_iter()
            .find(|k| Path::new(&k.path) == fx.profile.cache)
            .expect("the folder with the volume in it is reported");
        assert!(
            matches!(skip.kind, SkipKind::OtherVolume | SkipKind::Changed),
            "{skip:?}"
        );
        assert!(fx.profile.cache.join("f_000001").exists(), "skipped whole");
    }
}

/// The times of `path` (a file or folder), to put back later with [`set_times_like`].
#[cfg(windows)]
fn times_of(path: &Path) -> windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
    };
    let file = open_attributes(path);
    let mut info = FILE_BASIC_INFO::default();
    // SAFETY: `info` is writable and sized as passed; the handle is open.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&raw mut info).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    assert_ne!(ok, 0, "read the times of {}", path.display());
    info
}

/// Sets all four of `path`'s times, the change time too, as any program may.
#[cfg(windows)]
fn set_times_like(path: &Path, times: &windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
    };
    let file = open_attributes(path);
    let info = FILE_BASIC_INFO {
        FileAttributes: 0,
        ..*times
    };
    // SAFETY: `info` is a valid FILE_BASIC_INFO of the size passed; the handle has FILE_WRITE_ATTRIBUTES.
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileBasicInfo,
            (&raw const info).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    assert_ne!(ok, 0, "set the times of {}", path.display());
}

#[cfg(windows)]
fn open_attributes(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .access_mode(0x0080 | 0x0100) // FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES
        .custom_flags(0x0200_0000 | 0x0020_0000) // FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
        .open(path)
        .unwrap()
}

/// Copies `from` to `to` the way a copy can look old: the copy gets the original's times, and the folder it went
/// into gets its own times back. On Windows all of them, the change time included; elsewhere the kernel sets the
/// change time, and only the others can be put back.
fn copy_with_old_times(from: &Path, to: &Path) {
    let folder = to.parent().unwrap();
    #[cfg(windows)]
    {
        let folder_times = times_of(folder);
        std::fs::copy(from, to).unwrap();
        set_times_like(to, &times_of(from));
        set_times_like(folder, &folder_times);
    }
    #[cfg(not(windows))]
    {
        let old = std::fs::metadata(folder).unwrap().modified().unwrap();
        std::fs::copy(from, to).unwrap();
        let src = std::fs::metadata(from).unwrap();
        let times = std::fs::FileTimes::new()
            .set_modified(src.modified().unwrap())
            .set_accessed(src.accessed().unwrap());
        std::fs::OpenOptions::new()
            .write(true)
            .open(to)
            .unwrap()
            .set_times(times)
            .unwrap();
        std::fs::File::open(folder)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
}

/// A file's times say nothing about whether it is new on Windows: a copy keeps the original's (Explorer,
/// `CopyFile`, `robocopy /COPY:DAT`), and any program can set even the change time. A file copied in after the plan
/// with every time set back to before the scan, its folder's too, is still told apart by its identity: the folder
/// target holding it is skipped whole, and a folder's loose files never include it.
#[test]
fn a_file_copied_in_with_old_times_is_never_deleted() {
    let fx = Fx::new();
    let plan = fx.plan(&fx.all_ids());
    let doc = fx.profile.documents.join("report.docx");
    let deep = fx.profile.long_path.parent().unwrap();
    let into_tree = deep.join("copied-report.docx");
    copy_with_old_times(&doc, &into_tree);
    let into_loose = fx
        .profile
        .loose
        .parent()
        .unwrap()
        .join("copied-report.docx");
    copy_with_old_times(&doc, &into_loose);
    let before = std::fs::read(&doc).unwrap();

    let (report, _) = fx.run(&plan, &CancelToken::new());
    assert_eq!(
        std::fs::read(&into_tree).unwrap(),
        before,
        "the copy in the folder target stays"
    );
    assert_eq!(
        std::fs::read(&into_loose).unwrap(),
        before,
        "the copy among the loose files stays"
    );
    assert!(
        fx.profile.long_path.exists(),
        "the folder target holding the copy is skipped whole"
    );
    let skip = skips(&report)
        .into_iter()
        .find(|k| k.kind == SkipKind::Changed && deep.starts_with(&k.path))
        .unwrap_or_else(|| {
            panic!(
                "the folder target is reported as changed: {:#?}",
                report.steps
            )
        });
    assert!(skip.reason.contains("changed since the scan"), "{skip:?}");
    assert!(
        !fx.profile.loose.exists(),
        "the loose files the scan saw went"
    );
}

/// The same copy made between the scan and the plan: the folder's loose files are no longer the ones the scan saw,
/// so the planner refuses them, and the confirm dialog's numbers leave them out.
#[test]
fn loose_files_that_changed_before_planning_are_refused() {
    let fx = Fx::new();
    let folder = fx.profile.loose.parent().unwrap().to_path_buf();
    copy_with_old_times(
        &fx.profile.documents.join("notes.txt"),
        &folder.join("copied-notes.txt"),
    );
    let summary = fx.plan(&fx.all_ids()).summary();
    let refused = summary
        .refused
        .iter()
        .find(|k| Path::new(&k.path) == folder)
        .unwrap_or_else(|| panic!("the loose files are refused: {:#?}", summary.refused));
    assert_eq!(refused.kind, SkipKind::Changed);
}

/// Rewrites `path` in place (same file, same identity) to `len` bytes, then puts all its times back as they were:
/// on Windows the change time too; elsewhere the kernel sets the change time, and only the others go back.
fn rewrite_keeping_times(path: &Path, len: usize) {
    use std::io::Write;
    #[cfg(windows)]
    let times = times_of(path);
    #[cfg(not(windows))]
    let old = std::fs::metadata(path).unwrap();
    {
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(path, perms).unwrap();
    }
    let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.write_all(&vec![0x5Au8; len]).unwrap();
    f.set_len(len as u64).unwrap();
    drop(f);
    #[cfg(windows)]
    set_times_like(path, &times);
    #[cfg(not(windows))]
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(old.modified().unwrap())
                .set_accessed(old.accessed().unwrap()),
        )
        .unwrap();
}

/// A file rewritten in place to another size after the plan, with every time set back, keeps its identity, but
/// it isn't what the scan measured: the folder target holding it is skipped whole, and among loose files it stays.
#[test]
fn a_file_rewritten_to_another_size_with_old_times_is_never_deleted() {
    let fx = Fx::new();
    let plan = fx.plan(&fx.all_ids());
    let in_tree = fx.profile.cache.join("f_000002");
    rewrite_keeping_times(&in_tree, 300 * 1024);
    rewrite_keeping_times(&fx.profile.loose, 200 * 1024);

    let (report, _) = fx.run(&plan, &CancelToken::new());
    assert_eq!(
        std::fs::metadata(&in_tree).unwrap().len(),
        300 * 1024,
        "the grown file stays"
    );
    assert!(
        fx.profile.cache.join("f_000001").exists(),
        "its folder target is skipped whole"
    );
    assert_eq!(
        std::fs::metadata(&fx.profile.loose).unwrap().len(),
        200 * 1024,
        "the grown loose file stays"
    );
    let changed: Vec<_> = skips(&report)
        .into_iter()
        .filter(|k| k.kind == SkipKind::Changed)
        .map(|k| k.path.clone())
        .collect();
    assert!(
        changed.iter().any(|p| Path::new(p) == fx.profile.cache),
        "{changed:#?}"
    );
    assert!(
        changed.iter().any(|p| Path::new(p) == fx.profile.loose),
        "{changed:#?}"
    );
}

/// Fixture mode is never on when the real home folder can't be established: nothing could then be kept out of it.
#[test]
fn fixture_mode_needs_the_real_home() {
    let dir = tempfile::tempdir().unwrap();
    let base = support::real_path(dir.path());
    let ok = base.join("fixtures").join("one");
    std::fs::create_dir_all(&ok).unwrap();
    assert_eq!(
        fixture::check_root(&ok, &base.join("no-such-home")),
        Err(fixture::FixtureError::HomeUnknown)
    );
    assert_eq!(
        fixture::check_root(&ok, Path::new("")),
        Err(fixture::FixtureError::HomeUnknown)
    );
}

/// A volume mounted in a folder is no fixture root, though nothing on the way to it is a link: its root would put
/// the fixture's own, narrower places over a whole volume. (macOS: a disk image mounted on an empty folder.)
#[cfg(target_os = "macos")]
#[test]
fn fixture_mode_refuses_a_mounted_volume_s_root() {
    use std::process::Command;
    struct Detach(PathBuf);
    impl Drop for Detach {
        fn drop(&mut self) {
            let _ = Command::new("hdiutil")
                .args(["detach", "-quiet", "-force"])
                .arg(&self.0)
                .status();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let base = support::real_path(dir.path());
    let home = base.join("home").join("me");
    let mnt = base.join("mnt");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&mnt).unwrap();
    assert!(
        fixture::check_root(&mnt, &home).is_ok(),
        "the control: an empty folder"
    );
    let images = tempfile::tempdir().unwrap();
    let image = images.path().join("vacancy-fixture-root.dmg");
    let ran = |cmd: &mut Command| match cmd.output() {
        Ok(out) if out.status.success() => true,
        out => {
            assert!(!support::in_ci(), "hdiutil must work in CI: {out:?}");
            eprintln!("skipped: hdiutil: {out:?}");
            false
        }
    };
    if !ran(Command::new("hdiutil")
        .args([
            "create",
            "-quiet",
            "-size",
            "4m",
            "-fs",
            "HFS+",
            "-volname",
            "VacancyRoot",
        ])
        .arg(&image))
    {
        return;
    }
    if !ran(Command::new("hdiutil")
        .args(["attach", "-quiet", "-nobrowse", "-mountpoint"])
        .arg(&mnt)
        .arg(&image))
    {
        return;
    }
    let _detach = Detach(mnt.clone());
    assert_eq!(
        fixture::check_root(&mnt, &home),
        Err(fixture::FixtureError::MountRoot(mnt.clone()))
    );
}

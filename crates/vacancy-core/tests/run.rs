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
        let dir = tempfile::tempdir().unwrap();
        let root = support::real_path(dir.path()).join("fixture");
        let profile = fixture::build(&root).expect("build the fixture profile");
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
    assert!(
        node.deleted_items + 1 >= node.planned_items && node.deleted_items <= node.planned_items
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
    assert_eq!(report.biggest_win.as_deref().is_some(), true);
    let started = events
        .iter()
        .filter(|e| matches!(e, RunEvent::StepStarted(_)))
        .count();
    assert_eq!(started, plan.summary().steps.len());
    assert!(events.iter().any(|e| matches!(e, RunEvent::Skipped(_))));
}

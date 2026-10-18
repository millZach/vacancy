//! The duplicate finder against a fixture profile on a real file system (ticket 09): which files it groups, which
//! copy it keeps, what it leaves alone, how it stops, what it adds to the map, and what a run of it deletes. Each
//! test builds its own profile (`vacancy_core::fixture::build`), which holds known duplicate cases, scans it in
//! fixture mode (the Guard confined to it), and runs the finder.

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use vacancy_core::duplicates::{self, Duplicates, ID};
use vacancy_core::fixture::{self, Profile};
use vacancy_core::{
    CancelToken, Env, ItemKind, ItemRef, Moment, ProposalAction, RunReport, ScannedVolume,
    SkipKind, execute, plan_with, scan_for_map_with,
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
        let figures_root = root.clone();
        let scanned = scan_for_map_with(
            &root,
            &env,
            &moment(),
            &move |snap| Some(fixture::volume(&figures_root, 1 << 30, snap)),
            &mut |_| {},
            &CancelToken::new(),
        )
        .expect("scan the fixture");
        Self {
            _dir: dir,
            root,
            profile,
            env,
            scanned,
        }
    }

    /// Runs the duplicate finder on the scan, and attaches what it finds.
    fn find(&mut self) -> Duplicates {
        let mut last = 0;
        let found = duplicates::find(
            &mut self.scanned,
            &self.env,
            &mut |p| {
                assert!(p.percent >= last, "progress never goes back");
                last = p.percent;
            },
            &CancelToken::new(),
        )
        .expect("the search finishes");
        assert_eq!(last, 100, "the search reports when it is done");
        found
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

    fn run(&self, ids: &[String], skipped: &[String]) -> RunReport {
        let plan = plan_with(&self.scanned, &self.env, ids, skipped).expect("a plan");
        let root = self.root.clone();
        let figures = move || {
            let snap = support::scan_with(&root, 1 << 20);
            Some(fixture::volume(&root, 1 << 30, &snap))
        };
        execute(&plan, &self.env, &figures, &mut |_| {}, &CancelToken::new())
    }
}

/// The groups found, each as its kept copy and its extra copies, by path.
fn groups(found: &Duplicates) -> Vec<(PathBuf, BTreeSet<PathBuf>)> {
    found
        .groups
        .iter()
        .map(|g| {
            (
                PathBuf::from(&g.keep.path),
                g.extras.iter().map(|e| PathBuf::from(&e.path)).collect(),
            )
        })
        .collect()
}

fn every_path(found: &Duplicates) -> BTreeSet<PathBuf> {
    found
        .groups
        .iter()
        .flat_map(|g| std::iter::once(&g.keep).chain(&g.extras))
        .map(|c| PathBuf::from(&c.path))
        .collect()
}

/// Identical files are found, grouped exactly as the fixture made them, and the keeper rule keeps the copy it
/// should: the oldest of the tidiest, never one in Downloads or named like a copy when another isn't.
#[test]
fn finds_the_copies_and_keeps_the_right_one() {
    let mut fx = Fx::new();
    let found = fx.find();
    let mut got = groups(&found);
    got.sort();
    let mut want: Vec<(PathBuf, BTreeSet<PathBuf>)> = fx
        .profile
        .duplicates
        .groups
        .iter()
        .map(|(keep, extras)| (keep.clone(), extras.iter().cloned().collect()))
        .collect();
    want.sort();
    assert_eq!(got, want, "{found:#?}");

    let reason = |name: &str| {
        found
            .groups
            .iter()
            .find(|g| g.name == name)
            .map(|g| g.reason.clone())
            .unwrap()
    };
    assert_eq!(reason("IMG_4471.jpg"), "Kept: oldest, in Pictures");
    let video = if cfg!(windows) {
        ("Wedding highlights.mp4", "Videos")
    } else {
        ("Wedding highlights.mov", "Movies")
    };
    assert_eq!(
        reason(video.0),
        format!("Kept: in {}, not Downloads", video.1)
    );
    assert_eq!(
        reason("Brand guide.pdf"),
        "Kept: in Documents, not Downloads"
    );
    assert_eq!(reason("IMG_5102.jpg"), "Kept: oldest, in Pictures");
    assert_eq!(
        reason("Tax return 2025.pdf"),
        "Kept: original name, not \u{2018}- Copy\u{2019}"
    );
    // Biggest saving first, and every group knows what it frees.
    for pair in found.groups.windows(2) {
        assert!(pair[0].freed >= pair[1].freed);
    }
    for g in &found.groups {
        assert_eq!(g.freed, g.extras.iter().map(|e| e.bytes).sum::<u64>());
        assert!(g.bytes >= duplicates::MIN_BYTES);
    }
}

/// What looks like a copy but isn't one, or is out of bounds, is never proposed: the same length with other
/// contents, the same start with another end (a partial-hash collision), a hard link pair, copies under 1 MiB, a copy
/// in a cloud-sync folder, one in a Photos library, one reached only through a link, and an installer the installers
/// rule already deletes.
#[test]
fn leaves_alone_what_is_not_a_copy_or_out_of_bounds() {
    let mut fx = Fx::new();
    let found = fx.find();
    let paths = every_path(&found);
    for p in &fx.profile.duplicates.not_duplicates {
        assert!(!paths.contains(p), "{} is never in a group", p.display());
    }
    assert!(
        !paths
            .iter()
            .any(|p| p.to_string_lossy().contains("backup-link")),
        "nothing is reached through a link"
    );
    assert!(
        found.compared >= 2,
        "the partial-hash collision was compared in full: {found:#?}"
    );
}

/// A hard-linked file is one file under two names, not a copy: even with a real copy of it beside it, the finder
/// leaves it out (deleting one name would free nothing), and the copy then has no twin.
#[test]
fn hard_links_are_not_duplicates() {
    let mut fx = Fx::with(|p| {
        let a = p.home.join("Pictures").join("hardlink-a.jpg");
        std::fs::copy(&a, p.home.join("Documents").join("hardlink-copy.jpg")).unwrap();
    });
    let found = fx.find();
    let paths = every_path(&found);
    assert!(
        !paths
            .iter()
            .any(|p| p.to_string_lossy().contains("hardlink")),
        "{found:#?}"
    );
    assert!(found.hard_linked >= 1, "{found:#?}");
}

/// Stopping the search ends it at once with nothing attached: before it starts, and part way.
#[test]
fn the_search_can_be_stopped() {
    let mut fx = Fx::new();
    let cancel = CancelToken::new();
    cancel.cancel();
    let search = duplicates::search(&fx.scanned, &fx.env);
    assert!(search.candidates() > 0);
    assert!(search.run(&mut |_| {}, &cancel).is_err());

    let cancel = CancelToken::new();
    let mut heard = 0;
    let stopped = duplicates::search(&fx.scanned, &fx.env).run(
        &mut |_| {
            heard += 1;
            cancel.cancel();
        },
        &cancel,
    );
    assert!(stopped.is_err(), "stopped part way");
    assert!(heard >= 1);
    assert!(
        fx.scanned.proposals.position(ID).is_none(),
        "nothing was attached"
    );
    // And it can still run to the end afterwards.
    assert!(!fx.find().groups.is_empty());
}

/// No byte is counted twice: the duplicates proposal takes no file another proposal takes, the map still holds every
/// byte once, and the duplicates are drawn as one tile per personal folder holding extra copies.
#[test]
fn no_byte_is_counted_twice_and_the_map_gets_a_tile_per_folder() {
    let mut fx = Fx::new();
    let before: u64 = fx.scanned.overview.categories.iter().map(|c| c.bytes).sum();
    let found = fx.find();
    let snap = &fx.scanned.snapshot;
    let proposals = &fx.scanned.proposals;
    let i = proposals.position(ID).expect("the duplicates proposal");
    let dup = &proposals.list()[i];
    assert_eq!(dup.title, "Duplicate files");
    assert_eq!(dup.risk, vacancy_core::Risk::Review);
    assert!(!dup.default_on, "off by default");
    assert!(
        dup.summary.starts_with(
            "3 photos, 1 video and 2 other files have an exact copy elsewhere, in 5 groups."
        ),
        "photos and videos counted first: {}",
        dup.summary
    );
    assert_eq!(dup.bytes, found.groups.iter().map(|g| g.freed).sum::<u64>());

    // Its files, and everything other delete proposals take, never meet.
    let mine: BTreeSet<PathBuf> = proposals
        .targets(i)
        .iter()
        .map(|t| match *t {
            ItemRef::File(f) => snap.file_path(f),
            _ => panic!("the duplicates proposal takes files only"),
        })
        .collect();
    for (j, p) in proposals.list().iter().enumerate() {
        if j == i || p.action != ProposalAction::Delete {
            continue;
        }
        for t in proposals.targets(j) {
            let theirs = match *t {
                ItemRef::Dir(d) | ItemRef::FilesIn(d) => snap.path(d),
                ItemRef::File(f) => snap.file_path(f),
                ItemRef::Proposal(_) => continue,
            };
            for m in &mine {
                assert!(
                    m != &theirs && !m.starts_with(&theirs),
                    "{} is also {}'s",
                    m.display(),
                    p.id
                );
            }
        }
    }
    let installer = fx
        .profile
        .duplicates
        .not_duplicates
        .iter()
        .find(|p| p.to_string_lossy().contains("big-old"))
        .unwrap();
    assert!(!mine.contains(installer));

    // Every byte is still in exactly one tile.
    let after: u64 = fx.scanned.overview.categories.iter().map(|c| c.bytes).sum();
    assert_eq!(before, after);
    let tiles: Vec<_> = fx
        .scanned
        .overview
        .categories
        .iter()
        .flat_map(|c| c.items.iter().map(move |t| (c.id, t)))
        .filter(|(_, t)| t.proposal.as_deref() == Some(ID))
        .collect();
    let folders: BTreeSet<&str> = found
        .groups
        .iter()
        .flat_map(|g| g.extras.iter().map(|e| e.folder.as_str()))
        .collect();
    assert_eq!(
        tiles.len(),
        folders.len(),
        "one tile per folder: {tiles:#?}"
    );
    for (cat, t) in &tiles {
        assert_eq!(t.kind, ItemKind::Proposal);
        assert!(t.id.starts_with("p:duplicates:d"), "{}", t.id);
        let downloads = t.path.ends_with("Downloads");
        assert_eq!(
            *cat,
            if downloads {
                vacancy_core::CategoryId::Downloads
            } else {
                vacancy_core::CategoryId::Media
            }
        );
    }
    // Every copy's tile is on the map: the kept copy's folder, and each extra copy's duplicates tile.
    let ids: BTreeSet<&str> = fx
        .scanned
        .overview
        .categories
        .iter()
        .flat_map(|c| c.items.iter().map(|t| t.id.as_str()))
        .collect();
    for g in &found.groups {
        assert!(ids.contains(g.keep.tile.as_str()), "{}", g.keep.tile);
        for e in &g.extras {
            assert!(ids.contains(e.tile.as_str()), "{}", e.tile);
        }
    }
}

/// A run deletes exactly the extra copies of the groups left ticked: every kept copy stays, an unticked group keeps
/// every copy, and nothing else in the profile changes. The report says one copy of each group stayed.
#[test]
fn a_run_deletes_only_the_extra_copies_of_ticked_groups() {
    let mut fx = Fx::new();
    let found = fx.find();
    let untick = found
        .groups
        .iter()
        .find(|g| Path::new(&g.keep.path) == fx.profile.duplicates.untick)
        .expect("the group to untick")
        .clone();
    let before = fx.inventory();
    let report = fx.run(&[ID.to_owned()], std::slice::from_ref(&untick.id));
    let after = fx.inventory();

    let gone: BTreeSet<String> = before
        .difference(&after)
        .map(|l| l.split('\t').next().unwrap().to_owned())
        .collect();
    let added: Vec<&String> = after.difference(&before).collect();
    let want: BTreeSet<String> = found
        .groups
        .iter()
        .filter(|g| g.id != untick.id)
        .flat_map(|g| g.extras.iter().map(|e| fx.rel(Path::new(&e.path))))
        .collect();
    assert_eq!(gone, want, "exactly the ticked groups' extra copies went");
    assert!(added.is_empty(), "nothing new: {added:#?}");
    for g in &found.groups {
        assert!(Path::new(&g.keep.path).exists(), "{} stays", g.keep.path);
    }
    for e in &untick.extras {
        assert!(
            Path::new(&e.path).exists(),
            "the unticked group keeps {}",
            e.path
        );
    }

    let d = report.duplicates.expect("the report has the duplicates");
    assert_eq!(d.groups, found.groups.len() as u64 - 1);
    assert_eq!(d.cleared_groups, d.groups);
    assert_eq!(d.skipped_groups, 1);
    assert_eq!(d.kept_whole, 0);
    assert_eq!(d.copies_deleted, want.len() as u64);
    let freed: u64 = found
        .groups
        .iter()
        .filter(|g| g.id != untick.id)
        .map(|g| g.freed)
        .sum();
    assert_eq!(d.freed_bytes, freed);
    assert_eq!(report.freed_bytes, freed);
    assert_eq!(
        d.tiles.iter().map(|t| t.freed_bytes).sum::<u64>(),
        freed,
        "the tiles add up"
    );
    assert!(!d.samples.is_empty() && d.samples.len() <= 3);
}

/// Before an extra copy goes, the copy its group keeps is found again: changed since the scan, the whole group
/// stays, with the reason, and the other groups go as planned.
#[test]
fn a_group_whose_kept_copy_changed_keeps_every_copy() {
    let mut fx = Fx::new();
    let found = fx.find();
    let changed = found
        .groups
        .iter()
        .find(|g| Path::new(&g.keep.path) == fx.profile.duplicates.change_kept)
        .expect("the group whose kept copy changes")
        .clone();
    let plan = plan_with(&fx.scanned, &fx.env, &[ID.to_owned()], &[]).expect("a plan");
    // The kept copy is rewritten after the plan was made: same length, other bytes.
    let kept = Path::new(&changed.keep.path);
    let mut data = std::fs::read(kept).unwrap();
    data[0] ^= 0xff;
    std::fs::write(kept, &data).unwrap();

    let root = fx.root.clone();
    let figures = move || {
        let snap = support::scan_with(&root, 1 << 20);
        Some(fixture::volume(&root, 1 << 30, &snap))
    };
    let report = execute(&plan, &fx.env, &figures, &mut |_| {}, &CancelToken::new());
    for e in &changed.extras {
        assert!(Path::new(&e.path).exists(), "{} stays", e.path);
    }
    for g in found.groups.iter().filter(|g| g.id != changed.id) {
        for e in &g.extras {
            assert!(!Path::new(&e.path).exists(), "{} went", e.path);
        }
    }
    let step = report.steps.iter().find(|s| s.id == ID).unwrap();
    let skip = step
        .skipped
        .iter()
        .find(|k| changed.extras.iter().any(|e| e.path == k.path))
        .expect("the extra copy is skipped");
    assert_eq!(skip.kind, SkipKind::Changed);
    assert!(
        skip.reason.contains("copy Vacancy keeps"),
        "{}",
        skip.reason
    );
    let d = report.duplicates.unwrap();
    assert_eq!(d.kept_whole, 1);
    assert_eq!(d.cleared_groups, d.groups - 1);
}

/// Unticking every group leaves nothing to do: no step deletes anything.
#[test]
fn every_group_unticked_deletes_nothing() {
    let mut fx = Fx::new();
    let found = fx.find();
    let all: Vec<String> = found.groups.iter().map(|g| g.id.clone()).collect();
    assert!(
        plan_with(&fx.scanned, &fx.env, &[ID.to_owned()], &all).is_err(),
        "an empty plan"
    );
}

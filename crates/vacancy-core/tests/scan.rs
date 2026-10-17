//! The scanner against real fixture trees on disk, on whichever OS runs the tests (CI runs them on Windows and
//! macOS). Each test builds the tree it needs in a fresh temp dir; none touches a real home folder.

mod support;

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use support::{Fixture, KIB, MIB, Unreadable, in_ci, scan_with};
use vacancy_core::{CancelToken, DirFlags, ScanError, ScanOptions, scan};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn counts_a_nested_tree_with_known_sizes() {
    let fx = Fixture::new();
    fx.file("a/one.bin", 64 * KIB);
    fx.file("a/b/two.bin", 512 * KIB);
    fx.file("a/b/c/three.bin", MIB);
    fx.file("a/b/c/four.bin", 128 * KIB);
    fx.dir("a/empty");

    let snap = fx.scan();
    let root = snap.roots()[0].dir;
    let total = 64 * KIB + 512 * KIB + MIB + 128 * KIB;

    assert_eq!(
        snap.tree(root).bytes,
        total,
        "allocated bytes of every file, once"
    );
    assert_eq!(snap.tree(root).files, 4);
    assert_eq!(snap.tree(root).dirs, 4, "a, a/b, a/b/c and a/empty");
    assert_eq!(snap.stats().files, 4);
    assert_eq!(snap.stats().folders, 5, "the root and its four folders");
    assert_eq!(snap.stats().bytes, total);

    let c = snap
        .find(&fx.path("a/b/c"))
        .expect("a/b/c is in the snapshot");
    assert_eq!(snap.own(c).bytes, MIB + 128 * KIB);
    assert_eq!(snap.own(c).files, 2);
    let b = snap.find(&fx.path("a/b")).unwrap();
    assert_eq!(snap.own(b).bytes, 512 * KIB);
    assert_eq!(snap.tree(b).bytes, 512 * KIB + MIB + 128 * KIB);
    assert_eq!(snap.path(c), fx.path("a/b/c"));
    let empty = snap.find(&fx.path("a/empty")).unwrap();
    assert_eq!(snap.tree(empty).bytes, 0);

    // Files at or above the threshold are kept by name; the rest only count in their folder.
    assert!(snap.find_file(&fx.path("a/b/two.bin")).is_some());
    assert!(snap.find_file(&fx.path("a/b/c/three.bin")).is_some());
    assert!(snap.find_file(&fx.path("a/one.bin")).is_none());
    assert!(snap.find_file(&fx.path("a/b/c/four.bin")).is_none());
    assert_eq!(snap.file_count(), 2);

    // Last used is the newest of modified and accessed, rolled up to every folder above.
    let used = snap.tree(root).last_used;
    assert!(
        (now() - used).abs() < 86_400,
        "last used is about now: {used}"
    );
    assert!(snap.tree(c).last_used > 0);
}

#[test]
fn measures_allocated_size_not_length() {
    let fx = Fixture::new();
    fx.sparse("big-but-empty.bin", 64 * MIB);
    fx.file("real.bin", MIB);

    let snap = fx.scan();
    let root = snap.roots()[0].dir;
    let bytes = snap.tree(root).bytes;
    assert!(
        (MIB..MIB + 256 * KIB).contains(&bytes),
        "a 64 MiB sparse file takes (almost) no space, so the folder holds about 1 MiB, not 65: {bytes}"
    );
    assert_eq!(snap.tree(root).files, 2);
    assert!(
        snap.find_file(&fx.path("big-but-empty.bin")).is_none(),
        "kept or not by its allocated size"
    );
}

#[test]
fn counts_hard_links_once() {
    let fx = Fixture::new();
    let original = fx.file("a/original.bin", MIB);
    fx.dir("b");
    fs::hard_link(&original, fx.path("b/link.bin")).expect("hard link");
    fs::hard_link(&original, fx.path("b/second-link.bin")).expect("hard link");

    let snap = fx.scan();
    let root = snap.roots()[0].dir;
    assert_eq!(
        snap.tree(root).bytes,
        MIB,
        "one file with three names takes its size once"
    );
    assert_eq!(snap.tree(root).files, 1);
    assert_eq!(snap.stats().hard_links_merged, 2);
    let a = snap.find(&fx.path("a")).unwrap();
    let b = snap.find(&fx.path("b")).unwrap();
    assert_eq!(snap.tree(a).bytes + snap.tree(b).bytes, MIB);
}

#[test]
fn keeps_unicode_names_exactly() {
    let fx = Fixture::new();
    let names = [
        "Café ☕",
        "日本語のフォルダ",
        "📁 Ordner mit Ümlaut",
        "e\u{301}cole",
        "Ελληνικά",
    ];
    for name in names {
        fx.file(&format!("{name}/{name}.bin"), 512 * KIB);
    }

    let snap = fx.scan();
    for name in names {
        let path = fx.path(name);
        let dir = snap
            .find(&path)
            .unwrap_or_else(|| panic!("{name} is in the snapshot"));
        assert_eq!(snap.name(dir), std::ffi::OsStr::new(name));
        assert_eq!(snap.path(dir), path);
        assert_eq!(snap.tree(dir).bytes, 512 * KIB, "{name}");
        let file = snap
            .find_file(&path.join(format!("{name}.bin")))
            .unwrap_or_else(|| panic!("{name}.bin is kept"));
        assert_eq!(snap.file_path(file), path.join(format!("{name}.bin")));
    }
}

#[test]
fn reads_paths_longer_than_windows_max_path() {
    let fx = Fixture::new();
    let segment = "a-folder-name-that-is-forty-chars-long-x";
    let rel = std::iter::repeat_n(segment, 9)
        .collect::<Vec<_>>()
        .join("/");
    let deep = fx.file(&format!("{rel}/deep.bin"), MIB);
    assert!(
        deep.as_os_str().len() > 300,
        "the fixture path is long: {}",
        deep.display()
    );

    let snap = fx.scan();
    let file = snap.find_file(&deep).expect("the deep file is found");
    assert_eq!(snap.file(file).bytes, MIB);
    assert_eq!(snap.tree(snap.roots()[0].dir).bytes, MIB);
    assert_eq!(snap.stats().unreadable, 0);
}

#[test]
fn counts_an_unreadable_folder_instead_of_failing() {
    let fx = Fixture::new();
    fx.file("open/visible.bin", MIB);
    let secret = fx.dir("secret");
    fx.file("secret/hidden.bin", MIB);
    let guard = Unreadable::new(&secret);
    if fs::read_dir(&secret).is_ok() {
        assert!(!in_ci(), "CI must be able to make a folder unreadable");
        eprintln!("skipped: this account can read any folder");
        return;
    }

    let snap = fx.scan();
    drop(guard);
    let dir = snap
        .find(&secret)
        .expect("the unreadable folder is still listed");
    assert!(snap.flags(dir).contains(DirFlags::UNREADABLE));
    assert_eq!(snap.stats().unreadable, 1);
    assert_eq!(snap.unreadable().count(), 1);
    assert_eq!(
        snap.tree(snap.roots()[0].dir).bytes,
        MIB,
        "only the readable file counts"
    );
}

/// Creates a folder symlink, or says why it can't (Windows needs Developer Mode or admin for symlinks).
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    return std::os::windows::fs::symlink_dir(target, link);
}

#[test]
fn never_follows_a_symlink_loop() {
    let fx = Fixture::new();
    fx.file("loop/inner/data.bin", MIB);
    let back = fx.path("loop/inner/back");
    if let Err(e) = symlink_dir(&fx.path("loop"), &back) {
        assert!(!in_ci(), "CI can create symlinks: {e}");
        eprintln!("skipped: can't create symlinks here ({e})");
        return;
    }
    // Also a symlink to itself, and a file symlink to a big file.
    let _ = symlink_dir(&fx.path("loop/self"), &fx.path("loop/self"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        fx.path("loop/inner/data.bin"),
        fx.path("loop/data-link.bin"),
    )
    .unwrap();

    let snap = fx.scan();
    let root = snap.roots()[0].dir;
    assert_eq!(
        snap.tree(root).bytes,
        MIB,
        "the data is counted once and the links take nothing"
    );
    assert_eq!(snap.tree(root).files, 1);
    assert!(snap.stats().links_skipped >= 2, "{:?}", snap.stats());
    assert!(
        snap.find(&back.join("inner")).is_none(),
        "nothing is listed through the link"
    );
    // Windows marks folder symlinks as folders, so they are recorded (and flagged); Unix listings can't tell a
    // folder symlink from a file one without looking through it, so there they are only counted.
    #[cfg(windows)]
    {
        let link = snap.find(&back).expect("the link is recorded");
        assert!(snap.flags(link).contains(DirFlags::LINK));
        assert!(!snap.flags(link).entered());
        assert_eq!(snap.children(link).len(), 0, "never listed");
    }
}

/// A cache folder holding a junction to the user's Documents: the classic way a cleaner deletes someone's files.
#[cfg(windows)]
#[test]
fn never_follows_a_junction_from_a_cache_to_documents() {
    let fx = Fixture::new();
    let documents = fx.dir("Users/alex/Documents");
    fx.file("Users/alex/Documents/thesis.bin", 2 * MIB);
    fx.file("Users/alex/AppData/Local/App/Cache/blob.bin", MIB);
    let junction = fx.path("Users/alex/AppData/Local/App/Cache/docs");
    support::win::junction(&junction, &documents);
    assert!(
        fs::read_dir(&junction).unwrap().next().is_some(),
        "the junction works: Windows shows the documents through it"
    );

    let snap = fx.scan();
    let root = snap.roots()[0].dir;
    assert_eq!(
        snap.tree(root).bytes,
        3 * MIB,
        "the documents are counted once, under Documents"
    );
    let link = snap.find(&junction).expect("the junction is recorded");
    assert!(snap.flags(link).contains(DirFlags::LINK));
    assert_eq!(snap.children(link).len(), 0);
    assert_eq!(snap.tree(link).bytes, 0);
    let cache = snap
        .find(&fx.path("Users/alex/AppData/Local/App/Cache"))
        .unwrap();
    assert_eq!(
        snap.tree(cache).bytes,
        MIB,
        "the cache holds only its own blob"
    );
    let docs = snap.find(&documents).unwrap();
    assert_eq!(snap.tree(docs).bytes, 2 * MIB);
    assert!(snap.find_file(&junction.join("thesis.bin")).is_none());
}

/// A disk image mounted inside the tree is another file system: it is recorded and not entered.
#[cfg(target_os = "macos")]
#[test]
fn never_crosses_into_a_mounted_volume() {
    use std::process::Command;
    let fx = Fixture::new();
    fx.file("before.bin", MIB);
    let image = fx.path("disk.dmg");
    let mount = fx.dir("mnt");
    let ok = Command::new("hdiutil")
        .args([
            "create",
            "-size",
            "8m",
            "-fs",
            "HFS+",
            "-volname",
            "VacancyTest",
        ])
        .arg(&image)
        .status()
        .expect("hdiutil create")
        .success();
    assert!(ok, "hdiutil create");
    let ok = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-mountpoint"])
        .arg(&mount)
        .arg(&image)
        .status()
        .expect("hdiutil attach")
        .success();
    assert!(ok, "hdiutil attach");
    struct Detach(std::path::PathBuf);
    impl Drop for Detach {
        fn drop(&mut self) {
            let _ = std::process::Command::new("hdiutil")
                .args(["detach", "-force"])
                .arg(&self.0)
                .status();
        }
    }
    let _detach = Detach(mount.clone());
    support::write_file(&mount.join("inside.bin"), 2 * MIB);

    let snap = fx.scan();
    let dir = snap.find(&mount).expect("the mount point is recorded");
    assert!(snap.flags(dir).contains(DirFlags::OTHER_VOLUME));
    assert_eq!(snap.children(dir).len(), 0);
    assert_eq!(snap.tree(dir).bytes, 0);
    assert!(snap.stats().other_volumes_skipped >= 1);
    assert!(snap.find_file(&mount.join("inside.bin")).is_none());
}

#[test]
fn reports_progress_then_the_final_counts() {
    let fx = Fixture::new();
    for i in 0..60 {
        for j in 0..5 {
            fx.file(&format!("d{i}/e{j}/f.bin"), 4 * KIB);
        }
    }
    let mut updates = Vec::new();
    let mut consistent = true;
    let snap = scan(
        fx.root(),
        &ScanOptions {
            progress_every: Duration::from_millis(1),
            ..ScanOptions::default()
        },
        &mut |p| {
            updates.push((p.files, p.folders, p.bytes, p.current.to_path_buf()));
            // A preview is a consistent snapshot: every folder's own bytes add up to the root's total.
            consistent &= p.preview(|s| {
                let own: u64 = s.dirs().map(|d| s.own(d).bytes).sum();
                own == s.tree(s.roots()[0].dir).bytes
            });
        },
        &CancelToken::new(),
    )
    .expect("scan");

    assert!(consistent, "previews add up");
    let last = updates.last().expect("at least the final update");
    let stats = snap.stats();
    assert_eq!(
        (last.0, last.1, last.2),
        (stats.files, stats.folders, stats.bytes)
    );
    assert_eq!(stats.files, 300);
    assert!(
        updates.windows(2).all(|w| w[0].0 <= w[1].0),
        "counts only grow"
    );
    assert!(
        last.3.starts_with(fx.root()),
        "the current path is inside the scan"
    );
}

#[test]
fn stops_when_cancelled() {
    let fx = Fixture::new();
    for i in 0..40 {
        fx.file(&format!("d{i}/f.bin"), 4 * KIB);
    }
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = scan(fx.root(), &ScanOptions::default(), &mut |_| {}, &cancel);
    assert!(matches!(result, Err(ScanError::Cancelled)), "{result:?}");

    let cancel = CancelToken::new();
    let calls = AtomicUsize::new(0);
    let result = scan(
        fx.root(),
        &ScanOptions {
            progress_every: Duration::from_millis(1),
            ..ScanOptions::default()
        },
        &mut |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            cancel.cancel();
        },
        &cancel,
    );
    assert!(matches!(result, Err(ScanError::Cancelled)), "{result:?}");
    assert!(calls.load(Ordering::Relaxed) >= 1);
}

#[test]
fn a_missing_root_is_an_error() {
    let fx = Fixture::new();
    let missing = fx.path("not-here");
    let result = scan(
        &missing,
        &ScanOptions::default(),
        &mut |_| {},
        &CancelToken::new(),
    );
    match result {
        Err(ScanError::Root { path, source }) => {
            assert_eq!(path, missing);
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected a root error, got {other:?}"),
    }
}

#[test]
fn small_files_are_folded_into_their_folder() {
    let fx = Fixture::new();
    for i in 0..50 {
        fx.file(&format!("many/f{i}.txt"), 4 * KIB);
    }
    fx.file("many/big.bin", 2 * MIB);
    let snap = scan_with(fx.root(), MIB);
    let many = snap.find(&fx.path("many")).unwrap();
    assert_eq!(snap.own(many).files, 51);
    assert_eq!(snap.own(many).bytes, 50 * 4 * KIB + 2 * MIB);
    assert_eq!(
        snap.files(many).len(),
        1,
        "only the big file is kept by name"
    );
}

/// A folder whose entries can be listed but not described (read permission without search permission): its
/// entries' sizes are unknown, so it counts as unreadable instead of silently empty.
#[cfg(unix)]
#[test]
fn counts_a_folder_it_may_list_but_not_search_as_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    fx.file("open/visible.bin", MIB);
    let locked = fx.dir("locked");
    fx.file("locked/inside.bin", MIB);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).expect("chmod 644");
    let listable = fs::read_dir(&locked).is_ok();
    let searchable = fs::metadata(locked.join("inside.bin")).is_ok();
    if !listable || searchable {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            !in_ci(),
            "CI must be able to make a folder listable but not searchable"
        );
        eprintln!("skipped: this account ignores folder permissions");
        return;
    }

    let snap = fx.scan();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let dir = snap.find(&locked).expect("the folder is recorded");
    assert!(snap.flags(dir).contains(DirFlags::UNREADABLE));
    assert_eq!(snap.stats().unreadable, 1);
    assert_eq!(snap.unreadable().count(), 1);
    assert_eq!(
        snap.tree(snap.roots()[0].dir).bytes,
        MIB,
        "only the readable file counts"
    );
}

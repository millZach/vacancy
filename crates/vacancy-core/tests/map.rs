//! The map the UI gets: the overview reconciled with the volume's own numbers, drill-down, folding, the scanning
//! HUD's updates, and the Full Disk Access check. Against fixture trees, with the volume's figures made up so the
//! reconciliation is exact.

mod support;

use support::{Fixture, KIB, MIB};
use vacancy_core::permissions::{FullDiskAccess, probe};
use vacancy_core::{
    CancelToken, CategoryId, DriveKind, Env, ItemKind, MapError, Overview, ScanOptions, VolumeInfo,
    browse, categorize, overview, scan, scan_update,
};

fn volume(used: u64, free: u64) -> VolumeInfo {
    VolumeInfo {
        name: "Test Disk".into(),
        mount_point: "X:\\".into(),
        kind: DriveKind::Internal,
        system: false,
        total_bytes: used + free,
        used_bytes: used,
        free_bytes: free,
    }
}

fn windows_overview(
    fx: &Fixture,
    used: impl Fn(u64) -> u64,
) -> (Overview, vacancy_core::Snapshot, vacancy_core::Categories) {
    fx.file("Windows/System32/kernel.dll", MIB);
    fx.file("Program Files/Adobe/photoshop.exe", 2 * MIB);
    fx.file("Program Files/Adobe/Presets/look.bin", 512 * KIB);
    fx.file("Users/alex/Downloads/setup.exe", MIB);
    let snap = fx.scan();
    let cats = categorize(&snap, &Env::windows(fx.root(), "alex"));
    let scanned = snap.tree(snap.roots()[0].dir).bytes;
    let map = overview(&snap, &cats, volume(used(scanned), 10 * MIB));
    (map, snap, cats)
}

#[test]
fn shows_what_the_scan_could_not_see_as_part_of_system() {
    let fx = Fixture::new();
    let (map, _, _) = windows_overview(&fx, |scanned| scanned + 5 * MIB);

    assert_eq!(map.scan.hidden_bytes, 5 * MIB);
    assert_eq!(map.scan.over_count_bytes, 0);
    assert_eq!(map.free_bytes, 10 * MIB);
    let total: u64 = map.categories.iter().map(|c| c.bytes).sum();
    assert_eq!(
        total, map.volume.used_bytes,
        "the map adds up to what Windows says is used"
    );

    let system = map
        .categories
        .iter()
        .find(|c| c.id == CategoryId::System)
        .unwrap();
    assert!(system.protected);
    assert_eq!(system.bytes, MIB + 5 * MIB);
    let hidden = system
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Hidden)
        .expect("a hidden item");
    assert_eq!(hidden.bytes, 5 * MIB);
    assert!(!hidden.browsable);
    assert!(
        hidden
            .note
            .as_deref()
            .unwrap()
            .starts_with("Windows counts this as used"),
        "{hidden:?}"
    );
    assert_eq!(map.categories[0].id, CategoryId::System, "biggest first");
}

#[test]
fn never_invents_space_when_the_scan_counted_more() {
    let fx = Fixture::new();
    let (map, _, _) = windows_overview(&fx, |scanned| scanned - MIB);
    assert_eq!(map.scan.hidden_bytes, 0);
    assert_eq!(map.scan.over_count_bytes, MIB);
    assert!(
        map.categories
            .iter()
            .flat_map(|c| &c.items)
            .all(|i| i.kind != ItemKind::Hidden)
    );
}

#[test]
fn drills_into_a_folder_and_adds_up() {
    let fx = Fixture::new();
    let (map, snap, cats) = windows_overview(&fx, |scanned| scanned);
    let apps = map
        .categories
        .iter()
        .find(|c| c.id == CategoryId::Apps)
        .unwrap();
    let adobe = apps
        .items
        .iter()
        .find(|i| i.name == "Adobe")
        .expect("Adobe is an item");
    assert_eq!(adobe.kind, ItemKind::Folder);
    assert!(adobe.browsable);
    assert_eq!(adobe.bytes, 2 * MIB + 512 * KIB);
    assert!(adobe.path.ends_with("Adobe"));
    assert!(
        adobe.last_used.unwrap() > 1_600_000_000_000,
        "milliseconds since the epoch"
    );

    let folder = browse(&snap, &cats, &adobe.id).expect("browse Adobe");
    assert_eq!(folder.category, CategoryId::Apps);
    assert_eq!(folder.item.bytes, adobe.bytes);
    let names: Vec<&str> = folder.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["photoshop.exe", "Presets"], "biggest first");
    assert_eq!(
        folder.children.iter().map(|c| c.bytes).sum::<u64>(),
        adobe.bytes
    );
    assert_eq!(folder.children[0].kind, ItemKind::File);

    assert_eq!(
        browse(&snap, &cats, "nonsense"),
        Err(MapError::NoSuchFolder("nonsense".into()))
    );
    assert!(browse(&snap, &cats, "d999999").is_err());
    assert!(browse(&snap, &cats, "hidden").is_err());
}

#[test]
fn folds_the_smallest_items_into_one_tile() {
    let fx = Fixture::new();
    for i in 0..70u64 {
        fx.file(
            &format!("Program Files/App {i:02}/app.bin"),
            (i + 1) * 8 * KIB,
        );
    }
    let snap = fx.scan();
    let cats = categorize(&snap, &Env::windows(fx.root(), "alex"));
    let scanned = snap.tree(snap.roots()[0].dir).bytes;
    let map = overview(&snap, &cats, volume(scanned, MIB));
    let apps = map
        .categories
        .iter()
        .find(|c| c.id == CategoryId::Apps)
        .unwrap();
    assert_eq!(apps.items.len(), 49, "48 items and one tile for the rest");
    let more = apps.items.last().unwrap();
    assert_eq!(more.kind, ItemKind::More);
    assert_eq!(more.count, 22);
    assert_eq!(more.name, "22 smaller items");
    assert_eq!(
        apps.items.iter().map(|i| i.bytes).sum::<u64>(),
        apps.bytes,
        "folding keeps the total"
    );
    assert_eq!(apps.items[0].name, "App 69");
}

#[test]
fn explains_the_hidden_space_in_macos_terms() {
    let fx = Fixture::new();
    fx.file("System/Library/Kernels/kernel", MIB);
    fx.file("Users/alex/Documents/notes.txt", 256 * KIB);
    let snap = fx.scan();
    let cats = categorize(&snap, &Env::macos(fx.root(), "alex"));
    let scanned = snap.tree(snap.roots()[0].dir).bytes;
    let map = overview(&snap, &cats, volume(scanned + 3 * MIB, MIB));
    let system = map
        .categories
        .iter()
        .find(|c| c.id == CategoryId::System)
        .unwrap();
    assert_eq!(system.name, "macOS & System Data");
    let hidden = system
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Hidden)
        .unwrap();
    assert!(
        hidden
            .note
            .as_deref()
            .unwrap()
            .starts_with("macOS counts this as used")
    );
}

#[test]
fn hud_updates_carry_a_preview_of_the_categories() {
    let fx = Fixture::new();
    fx.file("Users/alex/Downloads/big.iso", 2 * MIB);
    fx.file("Windows/System32/kernel.dll", MIB);
    let env = Env::windows(fx.root(), "alex");
    let mut with = Vec::new();
    let mut without = Vec::new();
    scan(
        fx.root(),
        &ScanOptions {
            progress_every: std::time::Duration::from_millis(1),
            ..ScanOptions::default()
        },
        &mut |p| {
            let cats = p.preview(|s| categorize(s, &env));
            with.push(scan_update(p, Some(&cats)));
            without.push(scan_update(p, None));
        },
        &CancelToken::new(),
    )
    .unwrap();
    let last = with.last().unwrap();
    let preview = last.categories.as_ref().expect("a preview");
    let downloads = preview
        .iter()
        .find(|c| c.id == CategoryId::Downloads)
        .unwrap();
    assert_eq!(downloads.bytes, 2 * MIB);
    assert_eq!(downloads.name, "Downloads");
    assert!(preview.iter().all(|c| c.bytes > 0));
    assert_eq!(last.bytes, 3 * MIB);
    assert!(without.iter().all(|u| u.categories.is_none()));
}

#[test]
fn tells_whether_full_disk_access_is_missing() {
    let fx = Fixture::new();
    assert_eq!(
        probe(fx.root()),
        FullDiskAccess::Unknown,
        "no protected folder exists"
    );

    let safari = fx.dir("Library/Safari");
    support::write_file(&safari.join("Bookmarks.plist"), 4 * KIB);
    assert_eq!(probe(fx.root()), FullDiskAccess::Granted);

    // Full Disk Access is a macOS setting, and macOS refuses its protected folders the way `chmod 000` does.
    // On the Windows CI runner, which runs as an administrator, this check could still list a folder denied by
    // ACL; the scanner's own unreadable-folder test covers Windows ACLs.
    #[cfg(unix)]
    {
        use support::{Unreadable, in_ci};
        let guard = Unreadable::new(&safari);
        if std::fs::read_dir(&safari).is_ok() {
            assert!(!in_ci(), "CI must be able to make a folder unreadable");
            eprintln!("skipped: this account can read any folder");
            return;
        }
        assert_eq!(probe(fx.root()), FullDiskAccess::Missing);
        drop(guard);
        assert_eq!(probe(fx.root()), FullDiskAccess::Granted);
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn full_disk_access_is_a_mac_setting() {
    assert_eq!(
        vacancy_core::permissions::full_disk_access(),
        FullDiskAccess::NotApplicable
    );
    assert!(vacancy_core::permissions::open_full_disk_access_settings().is_err());
}

#[test]
fn scans_a_folder_for_the_map_in_one_call() {
    let fx = Fixture::new();
    fx.file("Users/alex/Downloads/big.iso", 2 * MIB);
    fx.file("Program Files/Tool/tool.exe", MIB);
    let env = Env::windows(fx.root(), "alex");
    let mut updates = Vec::new();
    let scanned = vacancy_core::scan_for_map(
        fx.root(),
        &env,
        &mut |u| updates.push(u),
        &CancelToken::new(),
    )
    .expect("scan for the map");
    assert!(
        updates
            .first()
            .and_then(|u| u.categories.as_ref())
            .is_some(),
        "the first update already previews the map"
    );
    assert_eq!(updates.last().unwrap().bytes, 3 * MIB);
    let map = &scanned.overview;
    assert_eq!(map.scan.scanned_bytes, 3 * MIB);
    assert!(
        map.categories
            .iter()
            .any(|c| c.id == CategoryId::Downloads && c.bytes == 2 * MIB)
    );
    let tool = map
        .categories
        .iter()
        .flat_map(|c| &c.items)
        .find(|i| i.name == "Tool")
        .expect("the tool is an item");
    assert!(browse(&scanned.snapshot, &scanned.categories, &tool.id).is_ok());

    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        vacancy_core::scan_for_map(fx.root(), &env, &mut |_| {}, &cancel),
        Err(vacancy_core::ScanError::Cancelled)
    ));
}

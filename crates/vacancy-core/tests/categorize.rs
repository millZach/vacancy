//! The categorizer against fixture trees laid out like a Windows drive and like a Mac, each with its platform's
//! rules. The layouts are injected through [`Env`], so both platforms' catalogs run on both CI runners.

mod support;

use std::collections::BTreeMap;

use support::{Fixture, KIB, MIB};
use vacancy_core::{Categories, CategoryId, CloudRoot, Env, ItemRef, Snapshot, categorize};

/// What a category's items are called, for asserting on.
fn item_names(snap: &Snapshot, cats: &Categories, id: CategoryId) -> Vec<String> {
    cats.get(id)
        .items
        .iter()
        .map(|item| match item.at {
            ItemRef::Dir(d) => snap.name(d).to_string_lossy().into_owned(),
            ItemRef::File(f) => snap.file(f).name.to_string_lossy().into_owned(),
            ItemRef::FilesIn(d) => format!("files in {}", snap.name(d).to_string_lossy()),
        })
        .collect()
}

fn bytes_by_category(cats: &Categories) -> BTreeMap<CategoryId, u64> {
    cats.list()
        .iter()
        .filter(|c| c.bytes > 0)
        .map(|c| (c.id, c.bytes))
        .collect()
}

/// Every byte is in exactly one category, and each item's drill-down adds up to the item.
fn assert_consistent(snap: &Snapshot, cats: &Categories) {
    let total: u64 = cats.list().iter().map(|c| c.bytes).sum();
    let scanned: u64 = snap.roots().iter().map(|r| snap.tree(r.dir).bytes).sum();
    assert_eq!(total, scanned, "the categories add up to the scan");
    for cat in cats.list() {
        let items: u64 = cat.items.iter().map(|i| i.bytes).sum();
        assert_eq!(items, cat.bytes, "{:?}'s items add up", cat.id);
        assert_eq!(cat.protected, cat.id.protected());
        for item in &cat.items {
            if let ItemRef::Dir(d) = item.at {
                let inside: u64 = cats.contents(snap, d).iter().map(|i| i.bytes).sum();
                assert_eq!(
                    inside,
                    item.bytes,
                    "{} drills down to its own size",
                    snap.path(d).display()
                );
                assert_eq!(cats.category_of(d), cat.id);
            }
        }
    }
}

fn windows_drive(fx: &Fixture) {
    fx.file("Windows/System32/kernel.dll", MIB);
    fx.file("Windows/Temp/setup.tmp", 512 * KIB);
    fx.file("Windows/SoftwareDistribution/Download/update.cab", MIB);
    fx.file("pagefile.sys", 2 * MIB);
    fx.file("Program Files/Adobe/photoshop.exe", MIB);
    fx.file("Program Files (x86)/Steam/steam.exe", 512 * KIB);
    fx.file(
        "Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/data.pak",
        2 * MIB,
    );
    fx.file("ProgramData/Microsoft/Windows/WER/report.wer", 256 * KIB);
    fx.file("$Recycle.Bin/S-1-5-21-1000/deleted.bin", 256 * KIB);
    fx.file("Tools/thing.bin", 256 * KIB);
    fx.file("Users/Public/shared.bin", 256 * KIB);
    let home = "Users/alex";
    fx.file(&format!("{home}/Downloads/setup.exe"), MIB);
    fx.file(&format!("{home}/Documents/report.pdf"), 512 * KIB);
    fx.dir(&format!("{home}/Documents/GitHub/site/.git"));
    fx.file(
        &format!("{home}/Documents/GitHub/site/index.html"),
        256 * KIB,
    );
    fx.file(&format!("{home}/Videos/holiday.mp4"), MIB);
    fx.file(&format!("{home}/code/web/package.json"), 4 * KIB);
    fx.file(
        &format!("{home}/code/web/node_modules/react/index.js"),
        512 * KIB,
    );
    fx.file(&format!("{home}/code/tool/Cargo.toml"), 4 * KIB);
    fx.file(&format!("{home}/code/tool/target/debug/tool.exe"), MIB);
    fx.file(
        &format!("{home}/.cargo/registry/cache/serde.crate"),
        256 * KIB,
    );
    fx.file(&format!("{home}/AppData/Local/Temp/t.tmp"), 256 * KIB);
    fx.file(
        &format!("{home}/AppData/Local/npm-cache/_cacache/x"),
        256 * KIB,
    );
    fx.file(
        &format!("{home}/AppData/Local/Google/Chrome/User Data/Default/Cache/Cache_Data/f_0001"),
        512 * KIB,
    );
    fx.file(
        &format!("{home}/AppData/Local/Google/Chrome/User Data/Default/History"),
        256 * KIB,
    );
    fx.file(&format!("{home}/OneDrive/Documents/synced.docx"), 512 * KIB);
    fx.file(
        &format!("{home}/OneDrive/Desktop/node_modules/x.js"),
        256 * KIB,
    );
}

#[test]
fn sorts_a_windows_drive_into_categories() {
    let fx = Fixture::new();
    windows_drive(&fx);
    let mut env = Env::windows(fx.root(), "alex");
    env.cloud.push(CloudRoot {
        provider: "OneDrive".into(),
        path: fx.path("Users/alex/OneDrive"),
    });
    let snap = fx.scan();
    let cats = categorize(&snap, &env);
    assert_consistent(&snap, &cats);

    let bytes = bytes_by_category(&cats);
    use CategoryId::*;
    assert_eq!(bytes[&System], MIB + 2 * MIB, "Windows and the page file");
    assert_eq!(
        bytes[&Caches],
        512 * KIB + MIB + 256 * KIB + 256 * KIB + 256 * KIB + 512 * KIB,
        "Windows temp and update downloads, WER, the Recycle Bin, user temp, Chrome's cache"
    );
    assert_eq!(bytes[&Apps], MIB + 512 * KIB + 2 * MIB);
    assert_eq!(bytes[&Downloads], MIB);
    assert_eq!(
        bytes[&Media],
        512 * KIB + MIB,
        "Documents and Videos, without the repo in Documents"
    );
    assert_eq!(
        bytes[&Dev],
        256 * KIB + 512 * KIB + MIB + 256 * KIB + 256 * KIB,
        "a repo, node_modules, a Rust target, ~/.cargo and the npm cache"
    );
    assert_eq!(
        bytes[&Cloud],
        512 * KIB + 256 * KIB,
        "everything in OneDrive, even a node_modules"
    );

    assert!(cats.get(System).protected && cats.get(Cloud).protected);
    assert!(!cats.get(Caches).protected && !cats.get(Media).protected);
    assert_eq!(cats.get(System).name, "Windows & system");
    assert_eq!(cats.get(Apps).name, "Apps & games");
    assert_eq!(cats.get(Cloud).name, "OneDrive");

    // Program Files' folders and each Steam game are items of their own.
    let apps = item_names(&snap, &cats, Apps);
    for name in ["Adobe", "Steam", "Cyberpunk 2077"] {
        assert!(apps.contains(&name.to_owned()), "{name} in {apps:?}");
    }
    assert!(item_names(&snap, &cats, System).contains(&"pagefile.sys".to_owned()));
    let steam = snap.find(&fx.path("Program Files (x86)/Steam")).unwrap();
    assert_eq!(
        cats.item_of(steam).unwrap().bytes,
        512 * KIB,
        "Steam without its games"
    );

    // The repo inside Documents is Developer, and Documents' drill-down leaves it out.
    let documents = snap.find(&fx.path("Users/alex/Documents")).unwrap();
    assert_eq!(cats.category_of(documents), Media);
    let repo = snap
        .find(&fx.path("Users/alex/Documents/GitHub/site"))
        .unwrap();
    assert_eq!(cats.category_of(repo), Dev);
    assert_eq!(cats.item_of(documents).unwrap().bytes, 512 * KIB);

    // In app data, only the cache-named folder is a cache; Chrome's history stays Other.
    let history = snap
        .find(&fx.path("Users/alex/AppData/Local/Google/Chrome/User Data/Default"))
        .unwrap();
    assert_eq!(cats.category_of(history), Other);
    let chrome_cache = snap
        .find(&fx.path("Users/alex/AppData/Local/Google/Chrome/User Data/Default/Cache"))
        .unwrap();
    assert_eq!(cats.category_of(chrome_cache), Caches);
    let other = item_names(&snap, &cats, Other);
    for name in ["Google", "Tools", "Public", "code"] {
        assert!(other.contains(&name.to_owned()), "{name} in {other:?}");
    }
}

fn mac_disk(fx: &Fixture) {
    fx.file("System/Library/CoreServices/Finder.app/Finder", MIB);
    fx.file("Applications/Xcode.app/Contents/MacOS/Xcode", 2 * MIB);
    fx.file("Library/Application Support/Logic/Sounds/strings.caf", MIB);
    fx.file(
        "Library/Caches/com.apple.iconservices/store.index",
        256 * KIB,
    );
    fx.file("Library/Fonts/Helvetica.ttc", 256 * KIB);
    fx.file("private/var/vm/sleepimage", MIB);
    fx.file("private/var/folders/xy/abc/T/scratch.bin", 256 * KIB);
    fx.file("opt/homebrew/Cellar/ffmpeg/ffmpeg", 512 * KIB);
    let home = "Users/alex";
    fx.file(
        &format!("{home}/Pictures/Photos Library.photoslibrary/originals/A/IMG_0001.heic"),
        2 * MIB,
    );
    fx.file(&format!("{home}/Pictures/screenshot.png"), 256 * KIB);
    fx.file(&format!("{home}/Movies/clip.mov"), MIB);
    fx.file(&format!("{home}/Downloads/Installer.dmg"), MIB);
    fx.file(
        &format!("{home}/Library/Caches/com.spotify.client/Data/blob"),
        512 * KIB,
    );
    fx.file(
        &format!("{home}/Library/Developer/Xcode/DerivedData/App-abc/Build/app"),
        MIB,
    );
    fx.file(
        &format!("{home}/Library/Mobile Documents/com~apple~CloudDocs/Notes.pages"),
        512 * KIB,
    );
    fx.file(
        &format!("{home}/Library/Application Support/Code/User/workspaceStorage/abc/state.vscdb"),
        256 * KIB,
    );
    fx.file(
        &format!("{home}/Library/Application Support/Google/Chrome/Default/Cache/Cache_Data/f_1"),
        256 * KIB,
    );
    fx.file(&format!("{home}/.Trash/old.zip"), 256 * KIB);
    fx.file(&format!("{home}/dev/app/package.json"), 4 * KIB);
    fx.file(
        &format!("{home}/dev/app/node_modules/lodash/lodash.js"),
        256 * KIB,
    );
}

#[test]
fn sorts_a_mac_into_categories() {
    let fx = Fixture::new();
    mac_disk(&fx);
    let env = Env::macos(fx.root(), "alex");
    let snap = fx.scan();
    let cats = categorize(&snap, &env);
    assert_consistent(&snap, &cats);

    let bytes = bytes_by_category(&cats);
    use CategoryId::*;
    assert_eq!(
        bytes[&System],
        MIB + 256 * KIB + MIB,
        "/System, /Library's fonts, swap"
    );
    assert_eq!(
        bytes[&Apps],
        2 * MIB + MIB,
        "Xcode and Logic's sound library"
    );
    assert_eq!(
        bytes[&Photos],
        2 * MIB,
        "the Photos library is its own protected category"
    );
    assert_eq!(bytes[&Media], 256 * KIB + MIB);
    assert_eq!(bytes[&Downloads], MIB);
    assert_eq!(
        bytes[&Caches],
        256 * KIB + 256 * KIB + 512 * KIB + 256 * KIB + 256 * KIB,
        "system and user caches, temp, Chrome's cache, the Trash"
    );
    assert_eq!(
        bytes[&Dev],
        512 * KIB + MIB + 256 * KIB,
        "Homebrew, DerivedData, node_modules"
    );
    assert_eq!(bytes[&Cloud], 512 * KIB, "iCloud Drive");

    assert!(cats.get(Photos).protected && cats.get(System).protected && cats.get(Cloud).protected);
    assert_eq!(cats.get(System).name, "macOS & System Data");
    assert_eq!(cats.get(Apps).name, "Applications");
    assert_eq!(cats.get(Cloud).name, "iCloud Drive");
    assert_eq!(cats.get(Caches).name, "Caches, logs & Trash");
    let apps = item_names(&snap, &cats, Apps);
    assert!(
        apps.contains(&"Xcode.app".to_owned()) && apps.contains(&"Logic".to_owned()),
        "{apps:?}"
    );
    assert!(item_names(&snap, &cats, Other).contains(&"Code".to_owned()));
}

#[test]
fn nothing_inside_a_sync_root_is_re_sorted() {
    let fx = Fixture::new();
    fx.file("Users/alex/Dropbox/plans.dwg", MIB);
    fx.file("Users/alex/Dropbox/node_modules/x.js", 256 * KIB);
    let mut env = Env::windows(fx.root(), "alex");
    env.cloud.push(CloudRoot {
        provider: "Dropbox".into(),
        path: fx.path("Users/alex/Dropbox"),
    });
    env.cloud.push(CloudRoot {
        provider: "OneDrive".into(),
        path: fx.path("Users/alex/OneDrive"),
    });
    let snap = fx.scan();
    let cats = categorize(&snap, &env);
    assert_eq!(cats.get(CategoryId::Cloud).bytes, MIB + 256 * KIB);
    assert_eq!(
        cats.get(CategoryId::Dev).bytes,
        0,
        "nothing inside a sync root is re-sorted"
    );
    assert_eq!(
        cats.get(CategoryId::Cloud).name,
        "Dropbox",
        "only the services that hold files are named"
    );
}

#[test]
fn categorizes_a_scan_preview() {
    let fx = Fixture::new();
    windows_drive(&fx);
    let env = Env::windows(fx.root(), "alex");
    let mut previews = Vec::new();
    let snap = vacancy_core::scan(
        fx.root(),
        &vacancy_core::ScanOptions {
            progress_every: std::time::Duration::from_millis(1),
            ..Default::default()
        },
        &mut |p| {
            previews.push(p.preview(|s| {
                let cats = categorize(s, &env);
                cats.list().iter().map(|c| c.bytes).sum::<u64>()
            }))
        },
        &vacancy_core::CancelToken::new(),
    )
    .unwrap();
    let total = snap.tree(snap.roots()[0].dir).bytes;
    assert!(!previews.is_empty());
    assert!(previews.iter().all(|&b| b <= total));
    assert_eq!(
        *previews.last().unwrap(),
        total,
        "the last preview is the whole scan"
    );
}

#[test]
fn a_photos_library_is_protected_wherever_it_is() {
    let fx = Fixture::new();
    fx.file(
        "Users/alex/.Trash/Old Photos.photoslibrary/originals/IMG_1.heic",
        MIB,
    );
    fx.file(
        "Applications/Demo.app/Contents/Resources/Sample.photoslibrary/originals/IMG_2.heic",
        512 * KIB,
    );
    fx.file(
        "Users/alex/Library/Developer/Fixtures.photoslibrary/originals/IMG_3.heic",
        256 * KIB,
    );
    fx.file(
        "Users/alex/Pictures/Photos Library.photoslibrary/originals/IMG_4.heic",
        2 * MIB,
    );
    let snap = fx.scan();
    let cats = categorize(&snap, &Env::macos(fx.root(), "alex"));

    let photos = cats.get(CategoryId::Photos);
    assert!(photos.protected);
    assert_eq!(
        photos.bytes,
        MIB + 512 * KIB + 256 * KIB + 2 * MIB,
        "in the Trash, inside an app and among developer files too"
    );
    assert_eq!(cats.get(CategoryId::Caches).bytes, 0);
    assert_eq!(cats.get(CategoryId::Dev).bytes, 0);
    let trash = snap
        .find(&fx.path("Users/alex/.Trash/Old Photos.photoslibrary"))
        .unwrap();
    assert_eq!(cats.category_of(trash), CategoryId::Photos);
}

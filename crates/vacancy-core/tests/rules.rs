//! The rules and the Guard against fixture trees laid out like a Windows drive and like a Mac. The layouts are
//! injected through [`Env`] and the clock and running apps through [`Moment`], so both platforms' catalogs run on
//! both CI runners. The last tests use what only a real file system has: links, 8.3 short names, long paths.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use support::{Fixture, KIB, MIB, age};
use vacancy_core::{
    Case, CategoryId, CloudRoot, DriveKind, Env, ItemKind, ItemRef, Moment, Proposal,
    ProposalAction, ProposalSource, Proposals, Risk, Snapshot, VolumeInfo, categorize, overview,
    propose,
};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn moment(running: &[&str]) -> Moment {
    Moment {
        now: now(),
        running_apps: running.iter().map(|s| (*s).to_owned()).collect(),
    }
}

/// Scans the fixture and runs the rules for `env`.
fn run(fx: &Fixture, env: &Env, moment: &Moment) -> (Snapshot, Proposals) {
    let snap = fx.scan();
    let cats = categorize(&snap, env);
    let proposals = propose(&snap, &cats, env, moment);
    (snap, proposals)
}

fn get<'a>(proposals: &'a Proposals, id: &str) -> Option<&'a Proposal> {
    proposals.list().iter().find(|p| p.id == id)
}

/// Every place a proposal would remove, relative to the fixture's root, with `/` separators. A folder's loose files
/// show as `folder/*`.
fn targets(fx: &Fixture, snap: &Snapshot, proposals: &Proposals, id: &str) -> Vec<String> {
    let Some(i) = proposals.position(id) else {
        return Vec::new();
    };
    let mut out: Vec<String> = proposals
        .targets(i)
        .iter()
        .map(|t| match *t {
            ItemRef::Dir(d) => rel(fx, &snap.path(d)),
            ItemRef::File(f) => rel(fx, &snap.file_path(f)),
            ItemRef::FilesIn(d) => format!("{}/*", rel(fx, &snap.path(d))),
            ItemRef::Proposal(_) => unreachable!("targets are places"),
        })
        .collect();
    out.sort();
    out
}

/// Every target of every proposal.
fn all_targets(fx: &Fixture, snap: &Snapshot, proposals: &Proposals) -> Vec<String> {
    let mut out = Vec::new();
    for p in proposals.list() {
        out.extend(targets(fx, snap, proposals, &p.id));
    }
    out
}

fn rel(fx: &Fixture, path: &Path) -> String {
    path.strip_prefix(fx.root())
        .expect("inside the fixture")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a proposal's targets add up to, from the snapshot.
fn bytes_of(snap: &Snapshot, proposals: &Proposals, i: usize) -> u64 {
    proposals
        .targets(i)
        .iter()
        .map(|t| match *t {
            ItemRef::Dir(d) => snap.tree(d).bytes,
            ItemRef::File(f) => snap.file(f).bytes,
            ItemRef::FilesIn(d) => {
                snap.own(d).bytes - snap.files(d).map(|f| snap.file(f).bytes).sum::<u64>()
            }
            ItemRef::Proposal(_) => unreachable!(),
        })
        .sum()
}

/// No target is inside another, and each proposal's numbers are its targets'.
fn assert_sound(fx: &Fixture, snap: &Snapshot, proposals: &Proposals) {
    let all = all_targets(fx, snap, proposals);
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            let (a, b) = (a.trim_end_matches("/*"), b.trim_end_matches("/*"));
            assert!(
                a != b && !b.starts_with(&format!("{a}/")) && !a.starts_with(&format!("{b}/")),
                "{a} and {b} overlap"
            );
        }
    }
    for (i, p) in proposals.list().iter().enumerate() {
        assert_eq!(p.bytes, bytes_of(snap, proposals, i), "{}'s bytes", p.id);
        assert!(p.bytes > 0);
        assert_eq!(p.source, ProposalSource::Rule);
        assert_eq!(p.action, ProposalAction::Delete);
        assert!(!p.sample_paths.is_empty() && p.sample_paths.len() <= 5);
        for s in &p.sample_paths {
            assert!(Path::new(s).exists(), "{} samples a real path: {s}", p.id);
        }
    }
    let sizes: Vec<u64> = proposals.list().iter().map(|p| p.bytes).collect();
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "biggest first");
}

fn windows_home(fx: &Fixture) {
    let h = "Users/alex";
    // Temp: old leftovers go, today's stay.
    fx.file(&format!("{h}/AppData/Local/Temp/old.tmp"), 512 * KIB);
    fx.file(
        &format!("{h}/AppData/Local/Temp/setup-123/payload.cab"),
        MIB,
    );
    age(&fx.path(&format!("{h}/AppData/Local/Temp")), 3);
    fx.file(&format!("{h}/AppData/Local/Temp/fresh.tmp"), 256 * KIB);
    // Browsers.
    let local = format!("{h}/AppData/Local");
    fx.file(
        &format!("{local}/Google/Chrome/User Data/Default/Cache/Cache_Data/f_1"),
        MIB,
    );
    fx.file(
        &format!("{local}/Google/Chrome/User Data/Profile 1/Code Cache/js/x"),
        256 * KIB,
    );
    fx.file(
        &format!("{local}/Google/Chrome/User Data/Default/History"),
        512 * KIB,
    );
    fx.file(
        &format!("{local}/Microsoft/Edge/User Data/Default/Cache/Cache_Data/f_2"),
        512 * KIB,
    );
    fx.file(
        &format!("{local}/Mozilla/Firefox/Profiles/abc.default/cache2/entries/e"),
        256 * KIB,
    );
    // Apps' caches in Roaming, and a library called "cache" that must not be mistaken for one.
    let roaming = format!("{h}/AppData/Roaming");
    fx.file(&format!("{roaming}/discord/Cache/Cache_Data/x"), MIB);
    fx.file(&format!("{roaming}/Slack/Code Cache/js/y"), 512 * KIB);
    fx.file(&format!("{roaming}/Code/CachedData/abc/z"), 256 * KIB);
    fx.file(&format!("{roaming}/Code/User/settings.json"), 4 * KIB);
    fx.file(
        &format!("{roaming}/Tool/node_modules/cache/index.js"),
        256 * KIB,
    );
    fx.file(&format!("{local}/D3DSCache/shader.bin"), 256 * KIB);
    // Crash dumps.
    fx.file(&format!("{local}/CrashDumps/app.exe.4242.dmp"), MIB);
    fx.file(
        &format!("{local}/Microsoft/Windows/WER/ReportArchive/AppCrash_x/Report.wer"),
        256 * KIB,
    );
    // Package managers.
    fx.file(&format!("{local}/npm-cache/_cacache/content/a"), MIB);
    fx.file(&format!("{local}/pip/cache/http/b"), 512 * KIB);
    fx.file(
        &format!("{h}/.cargo/registry/cache/index/serde.crate"),
        256 * KIB,
    );
    fx.file(&format!("{h}/.cargo/bin/cargo.exe"), 256 * KIB);
    // Projects: an idle one, a recent one, one with no lockfile, one in a repo.
    for p in ["old-site", "new-site"] {
        fx.file(&format!("{h}/code/{p}/package.json"), 4 * KIB);
        fx.file(&format!("{h}/code/{p}/package-lock.json"), 4 * KIB);
        fx.file(&format!("{h}/code/{p}/node_modules/react/index.js"), MIB);
        fx.file(
            &format!("{h}/code/{p}/node_modules/react/node_modules/x/y.js"),
            256 * KIB,
        );
    }
    fx.file(&format!("{h}/code/no-lock/package.json"), 4 * KIB);
    fx.file(&format!("{h}/code/no-lock/node_modules/a/a.js"), 512 * KIB);
    fx.dir(&format!("{h}/code/mono/.git"));
    fx.file(&format!("{h}/code/mono/web/package.json"), 4 * KIB);
    fx.file(&format!("{h}/code/mono/web/node_modules/b/b.js"), 512 * KIB);
    // Rust: idle, recent, and a "target" that isn't Cargo's.
    for p in ["ledger", "active"] {
        fx.file(&format!("{h}/code/{p}/Cargo.toml"), 4 * KIB);
        fx.file(&format!("{h}/code/{p}/target/CACHEDIR.TAG"), 4 * KIB);
        fx.file(&format!("{h}/code/{p}/target/debug/app.exe"), MIB);
    }
    fx.file(&format!("{h}/code/fake/Cargo.toml"), 4 * KIB);
    fx.file(&format!("{h}/code/fake/target/notes.md"), 256 * KIB);
    // Apps that ship node_modules, in app data and an editor's extensions.
    let app = format!("{local}/Programs/Tool/resources/app");
    fx.file(&format!("{app}/package.json"), 4 * KIB);
    fx.file(&format!("{app}/package-lock.json"), 4 * KIB);
    fx.file(&format!("{app}/node_modules/electron/e.js"), 512 * KIB);
    let ext = format!("{h}/.vscode/extensions/ms.ext");
    fx.file(&format!("{ext}/package.json"), 4 * KIB);
    fx.file(&format!("{ext}/package-lock.json"), 4 * KIB);
    fx.file(&format!("{ext}/node_modules/dep/d.js"), 256 * KIB);
    // Downloads: old installers go; a new one and a portable app's exe in a subfolder stay.
    fx.file(&format!("{h}/Downloads/SteamSetup.exe"), 2 * MIB);
    fx.file(&format!("{h}/Downloads/Installers/node-v22.msi"), MIB);
    fx.file(&format!("{h}/Downloads/Tools/putty.exe"), MIB);
    fx.file(&format!("{h}/Downloads/report.pdf"), MIB);
    age(&fx.path(&format!("{h}/Downloads")), 60);
    fx.file(&format!("{h}/Downloads/new-setup.exe"), MIB);
    // Everything except the recent things is old.
    for idle in ["old-site", "no-lock", "mono", "ledger", "fake"] {
        age(&fx.path(&format!("{h}/code/{idle}")), 120);
    }
    age(&fx.path(&format!("{local}/Programs")), 400);
    age(&fx.path(&format!("{h}/.vscode")), 400);
}

#[test]
fn the_windows_rules_find_what_the_prototype_proposes() {
    let fx = Fixture::new();
    windows_home(&fx);
    let env = Env::windows(fx.root(), "alex");
    let (snap, proposals) = run(&fx, &env, &moment(&[]));
    assert_sound(&fx, &snap, &proposals);
    let t = |id| targets(&fx, &snap, &proposals, id);
    let local = "Users/alex/AppData/Local";
    let roaming = "Users/alex/AppData/Roaming";

    assert_eq!(
        t("w-temp"),
        vec![
            format!("{local}/Temp/old.tmp"),
            format!("{local}/Temp/setup-123"),
        ],
        "today's temp file stays"
    );
    assert_eq!(
        t("w-browser"),
        vec![
            format!("{local}/Google/Chrome/User Data/Default/Cache/Cache_Data"),
            format!("{local}/Google/Chrome/User Data/Profile 1/Code Cache/js"),
            format!("{local}/Microsoft/Edge/User Data/Default/Cache/Cache_Data"),
            format!("{local}/Mozilla/Firefox/Profiles/abc.default/cache2/entries"),
        ],
        "the caches' contents, never History"
    );
    let browser = get(&proposals, "w-browser").unwrap();
    assert_eq!(
        browser.summary,
        "Pages and images Chrome, Edge and Firefox saved to load sites faster."
    );
    assert_eq!(
        t("w-appcache"),
        vec![
            format!("{roaming}/Code/CachedData/abc"),
            format!("{roaming}/Slack/Code Cache/js"),
            format!("{roaming}/discord/Cache/Cache_Data"),
        ],
        "apps' caches, not a library named cache inside node_modules"
    );
    assert_eq!(
        get(&proposals, "w-appcache").unwrap().summary,
        "Caches that discord, Slack and Code rebuild on their own."
    );
    assert_eq!(
        t("w-gpucache"),
        vec![format!("{local}/D3DSCache/shader.bin")]
    );
    assert_eq!(
        t("w-dumps"),
        vec![
            format!("{local}/CrashDumps/app.exe.4242.dmp"),
            format!("{local}/Microsoft/Windows/WER/ReportArchive/AppCrash_x"),
        ]
    );
    assert_eq!(
        t("w-pkgcache"),
        vec![
            "Users/alex/.cargo/registry/cache/index".to_owned(),
            format!("{local}/npm-cache/_cacache"),
            format!("{local}/pip/cache/http"),
        ]
    );
    assert_eq!(
        get(&proposals, "w-pkgcache").unwrap().summary,
        "Download caches from npm, pip and Cargo."
    );
    assert_eq!(
        t("w-node"),
        vec![
            "Users/alex/code/mono/web/node_modules".to_owned(),
            "Users/alex/code/old-site/node_modules".to_owned(),
        ],
        "idle projects with a lockfile or a repo; not recent ones, apps' or an editor's extensions"
    );
    let node = get(&proposals, "w-node").unwrap();
    assert_eq!(node.title, "node_modules in 2 idle projects");
    assert_eq!(
        node.item_count, 3,
        "every file inside, nested node_modules too"
    );
    assert_eq!(
        t("w-rust"),
        vec!["Users/alex/code/ledger/target".to_owned()]
    );
    assert_eq!(
        get(&proposals, "w-rust").unwrap().title,
        "Rust build output in 1 project"
    );
    assert_eq!(
        t("w-installers"),
        vec![
            "Users/alex/Downloads/Installers/node-v22.msi".to_owned(),
            "Users/alex/Downloads/SteamSetup.exe".to_owned(),
        ],
        "old .msi anywhere and .exe at the top; not new ones or a portable app's"
    );

    // Safe is on; review is off unless the rule says otherwise, as old installers do.
    for p in proposals.list() {
        match p.id.as_str() {
            "w-installers" => {
                assert_eq!(p.risk, Risk::Review);
                assert!(p.default_on);
                assert!(!p.regenerable);
            }
            _ => {
                assert_eq!(p.risk, Risk::Safe, "{}", p.id);
                assert!(p.default_on, "{}", p.id);
            }
        }
    }
    let temp = get(&proposals, "w-temp").unwrap();
    assert_eq!(temp.item_count, 2);
    assert_eq!(temp.bytes, 512 * KIB + MIB);
    assert_eq!(temp.category, CategoryId::Caches);
    assert_eq!(node.category, CategoryId::Dev);
}

fn mac_home(fx: &Fixture) {
    let h = "Users/alex";
    let lib = format!("{h}/Library");
    fx.file(&format!("{lib}/Caches/com.google.Chrome/Default/x"), MIB);
    fx.file(&format!("{lib}/Caches/com.spotify.client/Data/y"), MIB);
    fx.file(&format!("{lib}/Caches/com.apple.bird/session/z"), 256 * KIB);
    fx.file(
        &format!("{lib}/Caches/Homebrew/downloads/ffmpeg.bottle.tar.gz"),
        MIB,
    );
    fx.file(&format!("{h}/.npm/_cacache/content/a"), 256 * KIB);
    fx.file(
        &format!("{lib}/Developer/Xcode/DerivedData/Weatherly-abc/Build/Products/Debug/Weatherly.app/Weatherly"),
        MIB,
    );
    fx.file(
        &format!("{lib}/Developer/Xcode/Archives/2024-03-14/Weatherly 2.1.xcarchive/Info.plist"),
        512 * KIB,
    );
    age(
        &fx.path(&format!("{lib}/Developer/Xcode/Archives/2024-03-14")),
        400,
    );
    fx.file(
        &format!("{lib}/Developer/Xcode/Archives/2026-10-01/Weatherly 3.0.xcarchive/Info.plist"),
        512 * KIB,
    );
    fx.file(
        &format!("{lib}/Logs/DiagnosticReports/Slack-2026.ips"),
        256 * KIB,
    );
    fx.file(
        &format!("{lib}/Containers/com.apple.mail/Data/Library/Mail Downloads/A1/invoice.pdf"),
        512 * KIB,
    );
    fx.file(&format!("{h}/.Trash/old.zip"), MIB);
    fx.file(
        &format!("{lib}/Application Support/discord/Cache/Cache_Data/d"),
        512 * KIB,
    );
    fx.file(
        &format!("{lib}/Application Support/Slack/Cache/Cache_Data/s"),
        512 * KIB,
    );
    fx.file(&format!("{h}/code/weatherly/package.json"), 4 * KIB);
    fx.file(&format!("{h}/code/weatherly/yarn.lock"), 4 * KIB);
    fx.file(&format!("{h}/code/weatherly/node_modules/lodash/l.js"), MIB);
    age(&fx.path(&format!("{h}/code/weatherly")), 100);
    fx.file(&format!("{h}/Downloads/Docker.dmg"), 2 * MIB);
    fx.file(&format!("{h}/Downloads/Old/Figma.pkg"), MIB);
    age(&fx.path(&format!("{h}/Downloads")), 45);
    fx.file(&format!("{h}/Downloads/Today.dmg"), MIB);
}

#[test]
fn the_mac_rules_find_what_the_prototype_proposes() {
    let fx = Fixture::new();
    mac_home(&fx);
    let env = Env::macos(fx.root(), "alex");
    let (snap, proposals) = run(&fx, &env, &moment(&["Slack", "com.spotify.client"]));
    assert_sound(&fx, &snap, &proposals);
    let t = |id| targets(&fx, &snap, &proposals, id);
    let lib = "Users/alex/Library";

    assert_eq!(
        t("m-caches"),
        vec![format!("{lib}/Caches/com.google.Chrome/Default")],
        "not running Spotify's, not iCloud's, not Homebrew's"
    );
    assert_eq!(
        get(&proposals, "m-caches").unwrap().summary,
        "Caches in your Library, from 1 app."
    );
    assert_eq!(
        t("m-brew"),
        vec![
            "Users/alex/.npm/_cacache/content".to_owned(),
            format!("{lib}/Caches/Homebrew/downloads"),
        ]
    );
    assert_eq!(
        t("m-derived"),
        vec![format!("{lib}/Developer/Xcode/DerivedData/Weatherly-abc")]
    );
    assert_eq!(
        t("m-archives"),
        vec![format!(
            "{lib}/Developer/Xcode/Archives/2024-03-14/Weatherly 2.1.xcarchive"
        )],
        "only archives older than a year"
    );
    assert_eq!(t("m-logs"), vec![format!("{lib}/Logs/DiagnosticReports")]);
    assert_eq!(
        t("m-mail"),
        vec![format!(
            "{lib}/Containers/com.apple.mail/Data/Library/Mail Downloads/A1"
        )]
    );
    assert_eq!(t("m-trash"), vec!["Users/alex/.Trash/old.zip".to_owned()]);
    assert_eq!(
        t("m-appcache"),
        vec![format!(
            "{lib}/Application Support/discord/Cache/Cache_Data"
        )],
        "running Slack's cache stays"
    );
    assert_eq!(
        t("m-node"),
        vec!["Users/alex/code/weatherly/node_modules".to_owned()]
    );
    assert_eq!(
        t("m-dmgs"),
        vec![
            "Users/alex/Downloads/Docker.dmg".to_owned(),
            "Users/alex/Downloads/Old/Figma.pkg".to_owned(),
        ]
    );
    let archives = get(&proposals, "m-archives").unwrap();
    assert_eq!(archives.risk, Risk::Review);
    assert!(!archives.default_on, "review is off by default");
    let dmgs = get(&proposals, "m-dmgs").unwrap();
    assert_eq!(dmgs.risk, Risk::Review);
    assert!(dmgs.default_on, "the prototype turns old disk images on");
    assert_eq!(dmgs.category, CategoryId::Downloads);
    assert_eq!(get(&proposals, "m-brew").unwrap().category, CategoryId::Dev);

    // While Xcode is open, DerivedData is left alone.
    let (_, with_xcode) = run(&fx, &env, &moment(&["Xcode"]));
    assert!(get(&with_xcode, "m-derived").is_none());
}

/// JetBrains IDEs keep their local history of every edit in their cache folders. It never goes; the caches beside
/// it may.
#[test]
fn local_history_is_never_swept_up_with_caches() {
    let fx = Fixture::new();
    let mac_ide = "Users/alex/Library/Caches/JetBrains/IntelliJIdea2024.1";
    let win_ide = "Users/alex/AppData/Local/JetBrains/IntelliJIdea2024.1";
    for ide in [mac_ide, win_ide] {
        fx.file(&format!("{ide}/LocalHistory/changes.storageData"), MIB);
        fx.file(&format!("{ide}/caches/content.dat"), MIB);
        fx.file(&format!("{ide}/index/stubs.bin"), MIB);
    }
    fx.file(
        "Users/alex/Library/Caches/pypoetry/virtualenvs/app-py3.12/lib/site.py",
        MIB,
    );
    fx.file("Users/alex/Library/Caches/pypoetry/cache/http/x", MIB);
    for env in [
        Env::windows(fx.root(), "alex"),
        Env::macos(fx.root(), "alex"),
    ] {
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        let all = all_targets(&fx, &snap, &proposals);
        for t in &all {
            assert!(
                !t.contains("LocalHistory") && !t.contains("virtualenvs"),
                "{t} is or holds kept data"
            );
            for kept in ["LocalHistory", "pypoetry/virtualenvs"] {
                assert!(
                    !fx.path(&format!("{mac_ide}/{kept}"))
                        .starts_with(fx.path(t))
                        && !fx
                            .path(&format!("{win_ide}/{kept}"))
                            .starts_with(fx.path(t))
                        && !fx
                            .path("Users/alex/Library/Caches/pypoetry/virtualenvs")
                            .starts_with(fx.path(t)),
                    "{t} holds kept data"
                );
            }
        }
        if env.platform == vacancy_core::Platform::MacOs {
            assert!(
                all.contains(&format!("{mac_ide}/caches"))
                    && all.contains(&format!("{mac_ide}/index")),
                "the caches beside the history still go: {all:?}"
            );
            assert!(
                all.contains(&"Users/alex/Library/Caches/pypoetry/cache".to_owned()),
                "{all:?}"
            );
        }
    }
}

/// Every proposal's tile is on the map, in its category; every byte is still in exactly one tile; and opening a
/// folder part of which was proposed shows only the rest.
#[test]
fn proposals_are_carved_out_of_the_map() {
    let fx = Fixture::new();
    windows_home(&fx);
    let env = Env::windows(fx.root(), "alex");
    let snap = fx.scan();
    let cats = categorize(&snap, &env);
    let before: BTreeMap<CategoryId, u64> = cats.list().iter().map(|c| (c.id, c.bytes)).collect();
    let proposals = propose(&snap, &cats, &env, &moment(&[]));
    let scanned = snap.tree(snap.roots()[0].dir).bytes;
    let volume = VolumeInfo {
        name: "Test".into(),
        mount_point: fx.root().display().to_string(),
        id: String::new(),
        kind: DriveKind::Internal,
        system: false,
        total_bytes: scanned + MIB,
        used_bytes: scanned,
        free_bytes: MIB,
    };
    let scanned_map = vacancy_core::scan_for_map(
        fx.root(),
        &env,
        &moment(&[]),
        &mut |_| {},
        &vacancy_core::CancelToken::new(),
    )
    .expect("scan for the map");
    let carved = &scanned_map.categories;
    let total: u64 = carved.list().iter().map(|c| c.bytes).sum();
    assert_eq!(total, scanned, "every byte in exactly one category");
    for c in carved.list() {
        assert_eq!(c.items.iter().map(|i| i.bytes).sum::<u64>(), c.bytes);
    }
    // Proposals move bytes between categories only when a rule's category differs from the categorizer's; these
    // rules agree with it, so every category keeps its total.
    for c in carved.list() {
        assert_eq!(c.bytes, before[&c.id], "{:?} keeps its total", c.id);
    }

    let map = overview(
        &scanned_map.snapshot,
        carved,
        &scanned_map.proposals,
        volume,
    );
    for p in scanned_map.proposals.list() {
        let tile = map
            .categories
            .iter()
            .find(|c| c.id == p.category)
            .and_then(|c| {
                c.items
                    .iter()
                    .find(|i| i.proposal.as_deref() == Some(&p.id))
            })
            .unwrap_or_else(|| panic!("{} has a tile in its category", p.id));
        assert_eq!(tile.kind, ItemKind::Proposal);
        assert_eq!(tile.bytes, p.bytes);
        assert_eq!(tile.name, p.title);
        assert!(!tile.browsable);
    }
    assert!(get(&proposals, "w-temp").is_some());

    // Temp's tile is what is left of it once the proposal took its part.
    let temp = map
        .categories
        .iter()
        .flat_map(|c| &c.items)
        .find(|i| i.name == "Temp")
        .expect("what's left of Temp");
    let snap = &scanned_map.snapshot;
    let temp_dir = snap
        .find(&fx.path("Users/alex/AppData/Local/Temp"))
        .unwrap();
    let taken = get(&scanned_map.proposals, "w-temp").unwrap().bytes;
    assert!(taken >= MIB, "at least the old setup folder");
    assert_eq!(temp.bytes + taken, snap.tree(temp_dir).bytes);
    // Opening `code` shows the projects without what was proposed, and adds up to its tile.
    let code = map
        .categories
        .iter()
        .flat_map(|c| &c.items)
        .find(|i| i.name == "code")
        .expect("code");
    let opened = vacancy_core::browse(&scanned_map.snapshot, carved, &code.id).expect("open code");
    assert_eq!(
        opened.children.iter().map(|c| c.bytes).sum::<u64>(),
        opened.item.bytes
    );
    assert_eq!(opened.item.bytes, code.bytes);
}

/// The Guard's promise, one protected root at a time: a rule that would match inside it yields nothing there, and
/// the same tree outside it shows the rule does match.
mod guard {
    use super::*;

    /// An idle Node project with a lockfile, and an old installer, under `at`.
    fn bait(fx: &Fixture, at: &str) {
        fx.file(&format!("{at}/proj/package.json"), 4 * KIB);
        fx.file(&format!("{at}/proj/package-lock.json"), 4 * KIB);
        fx.file(&format!("{at}/proj/node_modules/x/x.js"), 512 * KIB);
        fx.file(&format!("{at}/Cache/blob"), 256 * KIB);
        fx.file(&format!("{at}/setup.exe"), MIB);
        fx.file(&format!("{at}/app.dmg"), MIB);
        age(&fx.path(at), 200);
    }

    /// The targets of every proposal that lie in `at`.
    fn inside(fx: &Fixture, snap: &Snapshot, proposals: &Proposals, at: &str) -> Vec<String> {
        all_targets(fx, snap, proposals)
            .into_iter()
            .filter(|t| t == at || t.starts_with(&format!("{at}/")))
            .collect()
    }

    fn win(fx: &Fixture) -> Env {
        Env::windows(fx.root(), "alex")
    }

    fn mac(fx: &Fixture) -> Env {
        Env::macos(fx.root(), "alex")
    }

    #[test]
    fn cloud_sync_roots() {
        for (provider, rel) in [
            ("OneDrive", "Users/alex/OneDrive"),
            ("Dropbox", "Users/alex/Dropbox"),
            ("Google Drive", "Users/alex/Google Drive"),
            ("iCloud Drive", "Users/alex/iCloudDrive"),
        ] {
            let fx = Fixture::new();
            bait(&fx, rel);
            bait(&fx, "Users/alex/code"); // the control
            let mut env = win(&fx);
            env.cloud.push(CloudRoot {
                provider: provider.into(),
                path: fx.path(rel),
            });
            // Even when Temp and Downloads point inside the sync root.
            env.temp = vec![fx.path(rel)];
            env.known.downloads = fx.path(rel);
            let (snap, proposals) = run(&fx, &env, &moment(&[]));
            assert_eq!(
                inside(&fx, &snap, &proposals, rel),
                Vec::<String>::new(),
                "{provider}"
            );
            assert!(
                !inside(&fx, &snap, &proposals, "Users/alex/code").is_empty(),
                "the control: the same project outside {provider} is proposed"
            );
        }
        // iCloud Drive and File Provider roots on a Mac.
        for rel in [
            "Users/alex/Library/Mobile Documents/com~apple~CloudDocs",
            "Users/alex/Library/CloudStorage/Dropbox",
        ] {
            let fx = Fixture::new();
            bait(&fx, rel);
            let mut env = mac(&fx);
            env.known.downloads = fx.path(rel);
            let (snap, proposals) = run(&fx, &env, &moment(&[]));
            assert_eq!(
                inside(&fx, &snap, &proposals, rel),
                Vec::<String>::new(),
                "{rel}"
            );
        }
    }

    /// The cloud root's settings spell `é` as one code point; the folder on disk spells it as `e` plus a combining
    /// accent, as macOS listings can. It is still the cloud root.
    #[test]
    fn a_cloud_root_in_the_other_unicode_form() {
        let fx = Fixture::new();
        let on_disk = "Users/alex/Cafe\u{301} Sync";
        bait(&fx, on_disk);
        for mut env in [win(&fx), mac(&fx)] {
            env.cloud.push(CloudRoot {
                provider: "Dropbox".into(),
                path: fx.path("Users/alex/Caf\u{e9} Sync"),
            });
            let (snap, proposals) = run(&fx, &env, &moment(&[]));
            assert_eq!(
                inside(&fx, &snap, &proposals, on_disk),
                Vec::<String>::new(),
                "{:?}",
                env.platform
            );
        }
    }

    /// Google Drive keeps working folders at the top of every folder it mirrors, wherever the user chose to put
    /// it, and Dropbox keeps a `.dropbox` file and cache at the top of its folder. Those folders are sync roots even
    /// when no setting Vacancy can read names them.
    #[test]
    fn sync_roots_found_by_their_clients_working_files() {
        let fx = Fixture::new();
        fx.dir("Users/alex/Sync/Work/.tmp.drivedownload");
        bait(&fx, "Users/alex/Sync/Work");
        fx.file("Users/alex/Elsewhere/Team Files/.dropbox", 4 * KIB);
        fx.dir("Users/alex/Elsewhere/Team Files/.dropbox.cache");
        bait(&fx, "Users/alex/Elsewhere/Team Files");
        bait(&fx, "Users/alex/code"); // the control
        for env in [win(&fx), mac(&fx)] {
            let (snap, proposals) = run(&fx, &env, &moment(&[]));
            assert_eq!(
                all_targets(&fx, &snap, &proposals),
                vec!["Users/alex/code/proj/node_modules".to_owned()],
                "{:?}",
                env.platform
            );
            let cats = categorize(&snap, &env);
            let work = snap.find(&fx.path("Users/alex/Sync/Work")).unwrap();
            assert_eq!(cats.category_of(work), CategoryId::Cloud);
            assert!(cats.get(CategoryId::Cloud).name.contains("Google Drive"));
        }
    }

    #[test]
    fn os_folders() {
        let fx = Fixture::new();
        bait(&fx, "Windows/Temp");
        bait(&fx, "ProgramData/App");
        let mut env = win(&fx);
        env.temp = vec![fx.path("Windows/Temp")];
        env.known.downloads = fx.path("ProgramData/App");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(all_targets(&fx, &snap, &proposals), Vec::<String>::new());

        let fx = Fixture::new();
        bait(&fx, "Library/Downloads");
        bait(&fx, "System/Library/Caches");
        let mut env = mac(&fx);
        env.known.downloads = fx.path("Library/Downloads");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(all_targets(&fx, &snap, &proposals), Vec::<String>::new());
    }

    #[test]
    fn installed_apps() {
        let fx = Fixture::new();
        let programs = "Users/alex/AppData/Local/Programs/Tool";
        bait(&fx, programs);
        bait(&fx, "Program Files/App");
        bait(
            &fx,
            "Users/alex/AppData/Local/Packages/CanonicalGroupLimited.Ubuntu/LocalState",
        );
        bait(&fx, "Users/alex/Games/steamapps/common/Game");
        let mut env = win(&fx);
        env.temp = vec![fx.path(programs)];
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(all_targets(&fx, &snap, &proposals), Vec::<String>::new());

        let fx = Fixture::new();
        bait(&fx, "Users/alex/Applications/Tool.app/Contents/Resources");
        bait(&fx, "Users/alex/code/Demo.app/Contents");
        bait(&fx, "Users/alex/Downloads/Installer.app/Contents");
        let (snap, proposals) = run(&fx, &mac(&fx), &moment(&[]));
        assert_eq!(all_targets(&fx, &snap, &proposals), Vec::<String>::new());
    }

    #[test]
    fn the_home_folder_itself() {
        // A temp folder (or Downloads) pointed at the home folder must not empty it.
        let fx = Fixture::new();
        bait(&fx, "Users/alex/code");
        fx.file("Users/alex/notes.txt", MIB);
        fx.file("Users/alex/setup.exe", MIB);
        age(&fx.path("Users/alex"), 30);
        let mut env = win(&fx);
        env.temp = vec![fx.path("Users/alex")];
        env.known.downloads = fx.path("Users/alex");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert!(
            get(&proposals, "w-temp").is_none(),
            "nothing in the home folder is temp"
        );
        assert!(
            get(&proposals, "w-installers").is_none(),
            "nor an installer in it"
        );
        for t in all_targets(&fx, &snap, &proposals) {
            assert!(
                t.starts_with("Users/alex/code/"),
                "only what's deep in a project: {t}"
            );
        }
        // A %TEMP% inside the home folder that isn't a temp folder is not emptied either.
        let fx = Fixture::new();
        fx.file("Users/alex/Projects/thesis/draft.docx", MIB);
        fx.file("Users/alex/Projects/old.zip", MIB);
        age(&fx.path("Users/alex/Projects"), 30);
        let mut env = win(&fx);
        env.temp = vec![fx.path("Users/alex/Projects")];
        let (_, proposals) = run(&fx, &env, &moment(&[]));
        assert!(get(&proposals, "w-temp").is_none(), "Projects is not Temp");

        // Downloads redirected to the drive's root doesn't open the whole drive to the installer rule.
        let fx = Fixture::new();
        fx.file("setup.exe", MIB);
        fx.file("Tools/old/package.msi", MIB);
        age(fx.root(), 60);
        let mut env = win(&fx);
        env.home = PathBuf::from(if cfg!(windows) {
            r"Z:\Users\alex"
        } else {
            "/Volumes/Other/Users/alex"
        });
        env.known.downloads = fx.root().to_path_buf();
        let (_, proposals) = run(&fx, &env, &moment(&[]));
        assert!(
            get(&proposals, "w-installers").is_none(),
            "the drive itself is not Downloads"
        );

        // Nor anything that holds a known folder.
        let fx = Fixture::new();
        fx.file("Users/alex/AppData/Local/Temp/x.tmp", MIB);
        fx.file("Users/alex/Documents/thesis.docx", MIB);
        age(&fx.path("Users/alex"), 30);
        let mut env = win(&fx);
        env.temp = vec![fx.path("Users")];
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(all_targets(&fx, &snap, &proposals), Vec::<String>::new());
    }

    /// Wrapping an app in a folder doesn't make the folder fair game: nothing that holds an app bundle is taken
    /// whole, except by the rules for build output and dependencies, which hold the apps they build.
    #[test]
    fn a_folder_holding_an_app() {
        let fx = Fixture::new();
        fx.file(
            "Users/alex/.Trash/wrapper/Tool.app/Contents/MacOS/Tool",
            MIB,
        );
        fx.file("Users/alex/.Trash/old.zip", MIB);
        fx.file("Users/alex/code/app/package.json", 4 * KIB);
        fx.file("Users/alex/code/app/package-lock.json", 4 * KIB);
        fx.file(
            "Users/alex/code/app/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron",
            MIB,
        );
        age(&fx.path("Users/alex/code"), 120);
        let (snap, proposals) = run(&fx, &mac(&fx), &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec![
                "Users/alex/.Trash/old.zip".to_owned(),
                "Users/alex/code/app/node_modules".to_owned(),
            ],
            "not the wrapper; an idle project's node_modules with Electron.app in it, yes"
        );
    }

    #[test]
    fn photos_libraries_and_time_machine() {
        let fx = Fixture::new();
        fx.file(
            "Users/alex/.Trash/Old Photos.photoslibrary/originals/IMG_1.heic",
            MIB,
        );
        fx.file(
            "Users/alex/.Trash/stuff/Lib.photoslibrary/database/p.db",
            MIB,
        );
        fx.file("Users/alex/.Trash/Backups.backupdb/mac/x", MIB);
        fx.file("Users/alex/.Trash/MacBook.sparsebundle/bands/0", MIB);
        fx.file("Users/alex/.Trash/old/Studio.sparsebundle/bands/1", MIB);
        fx.file("Users/alex/.Trash/old.zip", MIB);
        bait(&fx, "Users/alex/Downloads/Mine.photoslibrary/resources");
        bait(&fx, "Users/alex/Downloads/Backups.backupdb");
        bait(&fx, "Users/alex/Downloads/Work.sparsebundle");
        let (snap, proposals) = run(&fx, &mac(&fx), &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec!["Users/alex/.Trash/old.zip".to_owned()],
            "only the zip: not a library, a folder holding one, or a backup"
        );
    }

    /// On a case-sensitive volume a folder spelled differently from the home folder is someone else's: the scan
    /// listing `Users/Alex` is not the home `Users/alex`. (The test volume itself ignores case, so one folder stands
    /// for both spellings; what matters is how the rules compare them.)
    #[test]
    fn a_case_sensitive_volume_matches_only_exact_spellings() {
        let fx = Fixture::new();
        bait(&fx, "Users/Alex/code");
        let mut env = mac(&fx);
        env.case = Case::Sensitive;
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(all_targets(&fx, &snap, &proposals), Vec::<String>::new());
        // The same tree on a case-insensitive volume is the home folder.
        let (snap, proposals) = run(&fx, &mac(&fx), &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec!["Users/Alex/code/proj/node_modules".to_owned()]
        );
    }

    #[test]
    fn outside_the_allowed_roots() {
        // Windows: another user's home is never searched, nor are toolchains' folders at the drive's root
        // (`C:\Tools`, `C:\Python312`), and a %TEMP% pointed at a folder there doesn't empty it. Only a project in
        // a folder people made at the root (`C:\Dev`) is found.
        let fx = Fixture::new();
        bait(&fx, "Dev");
        bait(&fx, "Tools");
        bait(&fx, "Python312/Lib");
        bait(&fx, "Users/bob/code");
        bait(&fx, "Users/alex/code"); // the control
        let mut env = win(&fx);
        env.temp = vec![fx.path("Dev")];
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec![
                "Dev/proj/node_modules".to_owned(),
                "Users/alex/code/proj/node_modules".to_owned(),
            ]
        );

        // Profiles kept somewhere other than `Users`: other users' homes are still off-limits.
        let fx = Fixture::new();
        bait(&fx, "Profiles/bob/code");
        bait(&fx, "Profiles/alex/code");
        let mut env = win(&fx);
        env.home = fx.path("Profiles/alex");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec!["Profiles/alex/code/proj/node_modules".to_owned()]
        );

        // macOS: only the home folder.
        let fx = Fixture::new();
        bait(&fx, "code");
        bait(&fx, "Users/bob/code");
        bait(&fx, "Users/alex/code"); // the control
        let (snap, proposals) = run(&fx, &mac(&fx), &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec!["Users/alex/code/proj/node_modules".to_owned()]
        );
    }
}

/// On a drive that doesn't hold the home folder, the whole drive is fair game for project folders, except its
/// own system folders.
#[test]
fn another_drive_is_searched_for_projects() {
    let fx = Fixture::new();
    fx.file("code/app/package.json", 4 * KIB);
    fx.file("code/app/package-lock.json", 4 * KIB);
    fx.file("code/app/node_modules/x/x.js", MIB);
    fx.file("$Recycle.Bin/S-1-5/app/package.json", 4 * KIB);
    fx.file("$Recycle.Bin/S-1-5/app/package-lock.json", 4 * KIB);
    fx.file("$Recycle.Bin/S-1-5/app/node_modules/y/y.js", MIB);
    age(fx.root(), 200);
    let mut env = Env::windows(fx.root(), "alex");
    env.home = PathBuf::from(if cfg!(windows) {
        r"Z:\Users\alex"
    } else {
        "/Volumes/Other/Users/alex"
    });
    let (snap, proposals) = run(&fx, &env, &moment(&[]));
    assert_eq!(
        all_targets(&fx, &snap, &proposals),
        vec!["code/app/node_modules".to_owned()]
    );
}

/// Real-file-system cases: a link inside a cache is never a target and never counted, and names the OS spells in
/// unusual ways still match.
mod real {
    use super::*;

    #[test]
    fn a_link_in_a_cache_is_left_alone_and_its_target_is_never_counted() {
        let fx = Fixture::new();
        fx.file("Users/alex/Documents/thesis.docx", 2 * MIB);
        fx.file("Users/alex/AppData/Local/Temp/old.tmp", MIB);
        let link = fx.path("Users/alex/AppData/Local/Temp/docs");
        #[cfg(windows)]
        support::win::junction(&link, &fx.path("Users/alex/Documents"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(fx.path("Users/alex/Documents"), &link).unwrap();
        age(&fx.path("Users/alex/AppData/Local/Temp/old.tmp"), 5);
        // The link's own times can't be set through it; the Temp folder is aged without following it.
        let env = Env::windows(fx.root(), "alex");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        let temp = targets(&fx, &snap, &proposals, "w-temp");
        assert_eq!(
            temp,
            vec!["Users/alex/AppData/Local/Temp/old.tmp".to_owned()]
        );
        assert_eq!(
            get(&proposals, "w-temp").unwrap().bytes,
            MIB,
            "Documents isn't counted"
        );
        assert!(fx.path("Users/alex/Documents/thesis.docx").exists());
    }

    /// A folder holding something the scanner couldn't read has an unknown size and unknown contents: it is not
    /// offered. A link deeper inside a folder is fine; it is counted as nothing and the executor deletes the link
    /// itself.
    #[test]
    fn a_folder_holding_an_unread_folder_is_left_alone_and_a_nested_link_counts_as_nothing() {
        let fx = Fixture::new();
        let temp = "Users/alex/AppData/Local/Temp";
        fx.file("Users/alex/Documents/thesis.docx", 2 * MIB);
        fx.file(&format!("{temp}/locked-setup/readable.bin"), MIB);
        fx.file(&format!("{temp}/locked-setup/private/secret.bin"), MIB);
        fx.file(&format!("{temp}/old-setup/payload.cab"), MIB);
        age(&fx.path(temp), 5);
        let link = fx.path(&format!("{temp}/old-setup/docs"));
        #[cfg(windows)]
        support::win::junction(&link, &fx.path("Users/alex/Documents"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(fx.path("Users/alex/Documents"), &link).unwrap();
        // Making the link touched its folder; it is old again.
        age_dir_only(&fx.path(&format!("{temp}/old-setup")), 5);
        let _locked = support::Unreadable::new(&fx.path(&format!("{temp}/locked-setup/private")));
        let env = Env::windows(fx.root(), "alex");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(
            targets(&fx, &snap, &proposals, "w-temp"),
            vec![format!("{temp}/old-setup")],
            "not the folder holding an unreadable one"
        );
        assert_eq!(
            get(&proposals, "w-temp").unwrap().bytes,
            MIB,
            "not Documents through the link"
        );
    }

    /// Sets one folder's own times, without following or touching anything inside it.
    fn age_dir_only(path: &Path, days: u64) {
        let when = SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
        let times = std::fs::FileTimes::new()
            .set_modified(when)
            .set_accessed(when);
        #[cfg(windows)]
        let f = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .access_mode(0x100) // FILE_WRITE_ATTRIBUTES
                .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS
                .open(path)
                .unwrap()
        };
        #[cfg(unix)]
        let f = std::fs::File::open(path).unwrap();
        f.set_times(times).unwrap();
    }

    #[test]
    fn unicode_and_long_paths() {
        let fx = Fixture::new();
        let deep = (0..14)
            .map(|i| format!("folder-with-a-long-name-{i:02}"))
            .collect::<Vec<_>>()
            .join("/");
        fx.file(&format!("Users/alex/AppData/Local/Temp/{deep}/x.bin"), MIB);
        fx.file("Users/alex/AppData/Local/Temp/Ünïcødé 文件/y.bin", MIB);
        age(&fx.path("Users/alex/AppData/Local/Temp"), 5);
        let env = Env::windows(fx.root(), "alex");
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        let temp = get(&proposals, "w-temp").expect("temp");
        assert_eq!(temp.bytes, 2 * MIB);
        assert!(
            targets(&fx, &snap, &proposals, "w-temp")
                .contains(&"Users/alex/AppData/Local/Temp/Ünïcødé 文件".to_owned())
        );
        assert!(
            fx.path(&format!("Users/alex/AppData/Local/Temp/{deep}"))
                .as_os_str()
                .len()
                > 260
        );
    }

    /// `%TEMP%` on CI is `C:\Users\RUNNER~1\...`: a place named with 8.3 short names is the same place.
    #[cfg(windows)]
    #[test]
    fn short_names_name_the_same_place() {
        let fx = Fixture::new();
        fx.file(
            "Users/alex/AppData/Local/Long Named Folder/Temp/old.tmp",
            MIB,
        );
        fx.file("Users/alex/Cloud Storage Folder/proj/package.json", 4 * KIB);
        fx.file(
            "Users/alex/Cloud Storage Folder/proj/package-lock.json",
            4 * KIB,
        );
        fx.file(
            "Users/alex/Cloud Storage Folder/proj/node_modules/x.js",
            MIB,
        );
        age(&fx.path("Users"), 200);
        let short = |rel: &str| short_path(&fx.path(rel));
        let mut env = Env::windows(fx.root(), "alex");
        env.temp = vec![short("Users/alex/AppData/Local/Long Named Folder/Temp")];
        env.cloud.push(CloudRoot {
            provider: "OneDrive".into(),
            path: short("Users/alex/Cloud Storage Folder"),
        });
        let (snap, proposals) = run(&fx, &env, &moment(&[]));
        assert_eq!(
            all_targets(&fx, &snap, &proposals),
            vec!["Users/alex/AppData/Local/Long Named Folder/Temp/old.tmp".to_owned()],
            "temp found through its short name; the cloud root protected through its short name"
        );
    }

    #[cfg(windows)]
    fn short_path(path: &Path) -> PathBuf {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut buf = vec![0u16; 2048];
        // SAFETY: `wide` is NUL-terminated; `buf` has 2048 writable units.
        let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), 2048) } as usize;
        assert!(n > 0 && n < buf.len());
        let short = PathBuf::from(OsString::from_wide(&buf[..n]));
        if short == path {
            eprintln!("8.3 names are off on this volume: {}", path.display());
        }
        short
    }
}

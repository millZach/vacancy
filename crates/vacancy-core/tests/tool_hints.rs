//! Real bounded manifest reads against fresh marked fixtures, both catalog layouts, production scan thresholds.
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use vacancy_core::{CancelToken, Env, Platform, ScanOptions, fixture, hints, scan};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::named("profile")
    }
    fn named(name: &str) -> Self {
        Self::named_with_advice(name, false)
    }
    fn named_with_advice(name: &str, advice: bool) -> Self {
        // Windows' default temp is under the real home. CI's checkout and the local D: worktree are outside it.
        // On macOS use the system temp directory, canonicalized past /var's symlink.
        let base = if cfg!(windows) {
            std::env::current_dir()
                .unwrap()
                .join("target")
                .join("tool-fixtures")
        } else {
            std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join("vacancy-tool-fixtures")
        };
        fs::create_dir_all(&base).unwrap();
        let temp = tempfile::tempdir_in(&base).unwrap();
        let root = temp.path().join(name);
        fs::create_dir_all(&root).unwrap();
        fixture::check_root(&root, &std::env::home_dir().unwrap()).unwrap();
        if advice {
            fixture::build_with_advice(&root, 0).unwrap();
        } else {
            fixture::build(&root).unwrap();
        }
        Self { _temp: temp, root }
    }
    fn env(&self, platform: Platform) -> Env {
        let mut env = match platform {
            Platform::Windows => Env::windows(&self.root, "fixture"),
            Platform::MacOs => Env::macos(&self.root, "fixture"),
        };
        env.confined_to = Some(self.root.clone());
        env
    }
    fn snapshot(&self) -> vacancy_core::Snapshot {
        scan(
            &self.root,
            &ScanOptions::default(),
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap()
    }
}

#[test]
fn generated_fixture_links_work_with_mixed_separators_and_shell_metacharacters() {
    let fx = Fixture::named("owned & (profile)/mixed-separators");
    assert!(fixture::is_profile(&fx.root));
    let before = fixture::inventory(&fx.root).unwrap();
    let snapshot = fx.snapshot();
    assert!(snapshot.stats().files > 0);
    assert_eq!(
        fixture::inventory(&fx.root).unwrap(),
        before,
        "scanner never follows fixture junctions or alters their targets"
    );
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn steam_root(env: &Env) -> PathBuf {
    match env.platform {
        Platform::Windows => env.program_files[0].join("Steam"),
        Platform::MacOs => env.home.join("Library/Application Support/Steam"),
    }
}

fn seed_tool_presence(env: &Env) {
    let paths = match env.platform {
        Platform::Windows => vec![env.system_root.clone(), env.local_app_data.join("Docker")],
        Platform::MacOs => vec![
            env.home.join("Library/Containers/com.docker.docker"),
            env.home.join("Library/Developer/CoreSimulator"),
            env.home.join("Library/Caches/Homebrew"),
        ],
    };
    for path in paths {
        write(&path.join("fixture-presence.txt"), "passive presence only");
    }
}
const MANIFEST: &str = r#""AppState" { "appid" "42" "name" "Fixture game" "LastPlayed" "100000" "SizeOnDisk" "123456789" }"#;

fn moment() -> vacancy_core::Moment {
    vacancy_core::Moment {
        now: std::time::UNIX_EPOCH.elapsed().unwrap().as_secs() as i64,
        running_apps: Some(Vec::new()),
        installed_apps: Vec::new(),
    }
}

fn scan_map(fx: &Fixture, env: &Env) -> vacancy_core::ScannedVolume {
    vacancy_core::scan_for_map_with(
        &fx.root,
        env,
        &moment(),
        &|snap| Some(fixture::volume(&fx.root, 1 << 30, snap)),
        &mut |_| {},
        &CancelToken::new(),
    )
    .unwrap()
}

#[test]
fn scan_advice_makes_zero_byte_reminder_runs_and_round_trips_without_mutation() {
    use vacancy_core::{PlanError, ProposalAction, Risk};
    for platform in [Platform::Windows, Platform::MacOs] {
        let fx = Fixture::new();
        let env = fx.env(platform);
        seed_tool_presence(&env);
        let manifest = steam_root(&env).join("steamapps/appmanifest_42.acf");
        write(&manifest, MANIFEST);
        if platform == Platform::MacOs {
            write(
                &env.home.join("Music/Logic Pro Library.bundle/sound.dat"),
                "keep sound",
            );
        }
        let scanned = scan_map(&fx, &env);
        let hints: Vec<_> = scanned
            .proposals
            .list()
            .iter()
            .filter(|p| p.action == ProposalAction::Hint)
            .collect();
        assert_eq!(hints.len(), if platform == Platform::MacOs { 6 } else { 5 });
        let ids: Vec<_> = hints.iter().map(|p| p.id.clone()).collect();
        for hint in hints {
            assert_eq!((hint.bytes, hint.item_count, hint.place_count), (0, 0, 0));
            assert_eq!(hint.risk, Risk::Review);
            assert!(!hint.default_on);
            assert!(
                scanned
                    .proposals
                    .targets(scanned.proposals.position(&hint.id).unwrap())
                    .is_empty()
            );
            assert!(
                !scanned
                    .overview
                    .categories
                    .iter()
                    .flat_map(|c| &c.items)
                    .any(|tile| tile.proposal.as_ref() == Some(&hint.id))
            );
        }
        let uncarved = vacancy_core::categorize(&scanned.snapshot, &env);
        let baseline = vacancy_core::propose(&scanned.snapshot, &uncarved, &env, &moment());
        assert_eq!(
            scanned.proposals.reclaimable_bytes(),
            baseline.reclaimable_bytes()
        );
        assert_eq!(
            scanned
                .overview
                .categories
                .iter()
                .map(|c| c.bytes)
                .sum::<u64>(),
            scanned.snapshot.stats().bytes
        );
        for category in uncarved.list() {
            assert_eq!(category.bytes, scanned.categories.get(category.id).bytes);
        }
        assert_eq!(
            vacancy_core::plan(&scanned, &env, &[]).unwrap_err(),
            PlanError::Empty
        );
        assert!(matches!(
            vacancy_core::plan(&scanned, &env, &["unknown-hint".into()]),
            Err(PlanError::NoSuchProposal(_))
        ));
        assert!(scanned.proposals.hint_action("unknown-hint").is_none());
        assert_eq!(
            scanned
                .proposals
                .hint_action("hint-steam-42")
                .unwrap()
                .steam_uri()
                .as_deref(),
            Some("steam://uninstall/42")
        );
        let plan = vacancy_core::plan(&scanned, &env, &ids).unwrap();
        let summary = plan.summary();
        assert!(summary.steps.is_empty());
        assert_eq!(
            (summary.bytes, summary.items, summary.places, summary.moves),
            (0, 0, 0, 0)
        );
        assert_eq!(summary.left_for_you, ids);
        let before = fixture::inventory(&fx.root).unwrap();
        let outside = Fixture::new();
        let outside_before = fixture::inventory(&outside.root).unwrap();
        let mut events = Vec::new();
        let report = vacancy_core::execute(
            &plan,
            &env,
            &|| Some(scanned.overview.volume.clone()),
            &mut |event| events.push(event),
            &CancelToken::new(),
        );
        assert!(events.is_empty());
        assert!(report.steps.is_empty());
        assert_eq!(
            (
                report.planned_bytes,
                report.freed_bytes,
                report.deleted_items,
                report.moved_items
            ),
            (0, 0, 0, 0)
        );
        assert!(report.freed_by_category.is_empty());
        assert!(report.biggest_win.is_none());
        assert_eq!(
            report
                .left_for_you
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(
            report
                .left_for_you
                .iter()
                .all(|p| p.bytes == 0 && p.hint_action.is_some())
        );
        assert_eq!(
            fixture::inventory(&fx.root).unwrap(),
            before,
            "advice-only execution never mutates disk"
        );
        let dir = vacancy_core::report::history_dir(&env.app_data.join("Vacancy"));
        // Pre-create only the normal scoped history directories, then prove saving adds one report there only.
        fs::create_dir_all(&dir).unwrap();
        let before_save = fixture::inventory(&fx.root).unwrap();
        let saved = vacancy_core::report::save(&dir, &report).unwrap();
        assert_eq!(
            vacancy_core::report::open(&dir, &report.id).unwrap(),
            report
        );
        let saved_rel = saved
            .strip_prefix(&fx.root)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let mut after_save = fixture::inventory(&fx.root).unwrap();
        after_save.retain(|line| !line.starts_with(&format!("{saved_rel}\t")));
        assert_eq!(after_save, before_save);
        assert_eq!(fixture::inventory(&outside.root).unwrap(), outside_before);
        let mut legacy = serde_json::to_value(&report).unwrap();
        for advice in legacy["leftForYou"].as_array_mut().unwrap() {
            advice.as_object_mut().unwrap().remove("hintAction");
        }
        let legacy: vacancy_core::RunReport = serde_json::from_value(legacy).unwrap();
        assert!(legacy.left_for_you.iter().all(|p| p.hint_action.is_none()));
    }
}

#[test]
fn mixed_cleanup_and_advice_keep_only_real_deletions_in_the_totals() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    seed_tool_presence(&env);
    write(
        &steam_root(&env).join("steamapps/appmanifest_42.acf"),
        MANIFEST,
    );
    let old = env.temp[0].join("removable-advice-test.tmp");
    write(&old, &"old temp".repeat(1024));
    support::age(&env.temp[0], 120);
    // New files' creation/change times must also predate the executor's coarse timestamp margin.
    std::thread::sleep(std::time::Duration::from_millis(3200));
    let scanned = scan_map(&fx, &env);
    let cleanup = scanned
        .proposals
        .list()
        .iter()
        .find(|p| p.id == "w-temp")
        .unwrap();
    assert!(scanned.proposals.hint_action(&cleanup.id).is_none());
    let plan = vacancy_core::plan(
        &scanned,
        &env,
        &[
            cleanup.id.clone(),
            "hint-steam-42".into(),
            "hint-windows-cleanup".into(),
            "hint-docker-cache".into(),
        ],
    )
    .unwrap();
    assert_eq!(plan.summary().bytes, cleanup.bytes);
    let report = vacancy_core::execute(
        &plan,
        &env,
        &|| Some(scanned.overview.volume.clone()),
        &mut |_| {},
        &CancelToken::new(),
    );
    assert_eq!(report.freed_bytes, cleanup.bytes);
    assert_eq!(report.deleted_items, cleanup.item_count);
    assert_eq!(report.left_for_you.len(), 3);
    assert_eq!(report.left_for_you[0].bytes, 0);
    assert!(!old.exists());
    assert!(
        steam_root(&env)
            .join("steamapps/appmanifest_42.acf")
            .exists()
    );
}

#[test]
fn optional_advice_generator_yields_native_hints_and_refuses_existing_profiles() {
    let fx = Fixture::named_with_advice("advice-profile", true);
    let env = fixture::env(&fx.root);
    let scanned = scan_map(&fx, &env);
    assert!(scanned.proposals.hint_action("hint-steam-42").is_some());
    assert_eq!(
        scanned
            .proposals
            .hint_action("hint-logic-library")
            .is_some(),
        env.platform == Platform::MacOs
    );
    let before = fixture::inventory(&fx.root).unwrap();
    assert!(fixture::build_with_advice(&fx.root, 0).is_err());
    assert_eq!(fixture::inventory(&fx.root).unwrap(), before);
}

#[test]
fn discovery_is_capped_and_cancels_without_publishing_partial_advice() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    for id in 1..=520 {
        write(
            &steam_root(&env).join(format!("steamapps/appmanifest_{id}.acf")),
            &MANIFEST.replace("\"42\"", &format!("\"{id}\"")),
        );
    }
    let snap = fx.snapshot();
    let before = fixture::inventory(&fx.root).unwrap();
    assert_eq!(
        hints::discover_until(&env, &snap, moment().now, &|| false)
            .unwrap()
            .len(),
        512
    );
    let checks = std::cell::Cell::new(0);
    let token = CancelToken::new();
    let result = hints::discover_until(&env, &snap, moment().now, &|| {
        checks.set(checks.get() + 1);
        if checks.get() >= 50 {
            token.cancel();
        }
        token.is_cancelled()
    });
    assert!(result.is_none());
    assert!(
        checks.get() >= 50,
        "cancellation happened during discovery, not before it"
    );
    assert_eq!(fixture::inventory(&fx.root).unwrap(), before);
}

#[test]
fn changed_missing_and_replaced_manifests_are_not_new_advice_for_an_old_scan() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    let path = steam_root(&env).join("steamapps/appmanifest_42.acf");
    write(&path, MANIFEST);
    let snap = fx.snapshot();
    // Same length and identity, but an explicit after-scan timestamp.
    write(&path, &MANIFEST.replace("Fixture game", "Changed game"));
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
        .unwrap();
    assert!(hints::steam(&env, &snap, moment().now).is_empty());
    drop(file);
    fs::rename(&path, path.with_extension("old")).unwrap();
    write(&path, MANIFEST);
    assert!(hints::steam(&env, &snap, moment().now).is_empty());
    fs::remove_file(&path).unwrap();
    assert!(hints::steam(&env, &snap, moment().now).is_empty());
}

#[test]
#[ignore = "diagnostic: run alone under the shared heavy-job helper for uncontended path timings"]
fn advice_path_timing() {
    let plain = Fixture::new();
    let start = std::time::Instant::now();
    let normal = scan_map(&plain, &fixture::env(&plain.root));
    let normal_ms = start.elapsed().as_secs_f64() * 1000.0;
    let seeded = Fixture::named_with_advice("timed-advice-profile", true);
    let env = fixture::env(&seeded.root);
    let start = std::time::Instant::now();
    let advised = scan_map(&seeded, &env);
    let advised_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "advice scan path: normal_ms={normal_ms:.3} normal_files={} seeded_ms={advised_ms:.3} seeded_files={} hints={}",
        normal.snapshot.stats().files,
        advised.snapshot.stats().files,
        advised
            .proposals
            .list()
            .iter()
            .filter(|p| p.action == vacancy_core::ProposalAction::Hint)
            .count()
    );
    for id in 1..=520 {
        write(
            &steam_root(&env).join(format!("steamapps/appmanifest_{id}.acf")),
            &MANIFEST.replace("\"42\"", &format!("\"{id}\"")),
        );
    }
    let snapshot = seeded.snapshot();
    let start = std::time::Instant::now();
    let capped = hints::steam(&env, &snapshot, moment().now);
    let capped_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(capped.len(), 512);
    let checks = std::cell::Cell::new(0);
    let start = std::time::Instant::now();
    let cancelled = hints::discover_until(&env, &snapshot, moment().now, &|| {
        checks.set(checks.get() + 1);
        checks.get() >= 50
    });
    let cancel_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(cancelled.is_none());
    println!(
        "advice cap path: inputs=520 accepted={} discovery_ms={capped_ms:.3} cancelled={} cancel_checks={} cancel_ms={cancel_ms:.3}",
        capped.len(),
        cancelled.is_none(),
        checks.get()
    );
}

#[test]
fn production_scan_retains_tiny_manifests_but_not_general_small_files_for_both_layouts() {
    for platform in [Platform::Windows, Platform::MacOs] {
        let fx = Fixture::new();
        let env = fx.env(platform);
        let manifest = steam_root(&env).join("steamapps/appmanifest_42.acf");
        write(&manifest, MANIFEST);
        let unrelated = env.home.join("Documents/tiny-unrelated.txt");
        write(&unrelated, "keep aggregated");
        let snap = fx.snapshot();
        assert!(
            snap.find_file(&manifest).is_some(),
            "small production Steam manifest retained"
        );
        assert!(
            snap.find_file(&unrelated).is_none(),
            "ordinary small files stay aggregated"
        );
        let hints = hints::steam(&env, &snap, i64::MAX);
        assert_eq!(hints.len(), 1, "{platform:?}");
        assert_eq!(
            hints[0].action.steam_uri().as_deref(),
            Some("steam://uninstall/42")
        );
        let proposal = hints[0].proposal();
        assert_eq!(proposal.bytes, 0);
        assert_eq!(proposal.item_count, 0);
        assert_eq!(fs::read_to_string(&manifest).unwrap(), MANIFEST);
    }
}

#[test]
fn secondary_libraries_are_bounded_by_fixture_identity() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    let outside = Fixture::new();
    let secondary = fx.root.join("Games");
    write(&secondary.join("steamapps/appmanifest_42.acf"), MANIFEST);
    write(&outside.root.join("steamapps/appmanifest_42.acf"), MANIFEST);
    let vdf = format!(
        "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} }}",
        secondary.display().to_string().replace('\\', "\\\\"),
        outside.root.display().to_string().replace('\\', "\\\\")
    );
    write(&steam_root(&env).join("steamapps/libraryfolders.vdf"), &vdf);
    let hints = hints::steam(&env, &fx.snapshot(), i64::MAX);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].manifest.starts_with(&fx.root));
}

#[test]
fn custom_install_roots_can_be_injected_without_host_registry_or_other_volume_hints() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    let custom = fx.root.join("CustomSteam");
    write(&custom.join("steamapps/appmanifest_42.acf"), MANIFEST);
    let snapshot = fx.snapshot();
    assert!(hints::steam(&env, &snapshot, i64::MAX).is_empty());
    assert_eq!(
        hints::steam_with_roots(&env, &snapshot, i64::MAX, &[custom]).len(),
        1
    );
    let outside = Fixture::new();
    let install = outside.root.join("Steam");
    write(&install.join("steamapps/appmanifest_42.acf"), MANIFEST);
    assert!(hints::steam_with_roots(&env, &snapshot, i64::MAX, &[install]).is_empty());
}

#[test]
fn manifest_growing_after_scan_is_omitted_and_never_changes_disk() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    let manifest = steam_root(&env).join("steamapps/appmanifest_42.acf");
    write(&manifest, MANIFEST);
    let snap = fx.snapshot();
    fs::write(&manifest, "x".repeat(1024 * 1024 + 1)).unwrap();
    assert!(hints::steam(&env, &snap, i64::MAX).is_empty());
    assert_eq!(fs::metadata(&manifest).unwrap().len(), 1024 * 1024 + 1);
}

#[test]
fn new_and_legacy_logic_libraries_are_advice_with_no_reclaim_promise() {
    for new_layout in [false, true] {
        let fx = Fixture::new();
        let env = fx.env(Platform::MacOs);
        let library = if new_layout {
            env.home.join("Music/Logic Pro Library.bundle")
        } else {
            fx.root.join("Library/Application Support/Logic")
        };
        write(&library.join("samples/sound.dat"), "fixture sound");
        let hint = hints::logic(&env, &fx.snapshot()).unwrap();
        assert_eq!(hint.proposal().bytes, 0);
        assert!(hint.why.contains("Shared or cloned"));
        assert!(hint.action.steam_uri().is_none());
        assert_eq!(
            fs::read_to_string(library.join("samples/sound.dat")).unwrap(),
            "fixture sound"
        );
    }
}

#[test]
fn library_replaced_by_junction_or_symlink_after_scan_is_not_read() {
    let fx = Fixture::new();
    let env = fx.env(Platform::Windows);
    let apps = steam_root(&env).join("steamapps");
    write(&apps.join("appmanifest_42.acf"), MANIFEST);
    let snap = fx.snapshot();
    let old = apps.with_file_name("steamapps-old");
    fs::rename(&apps, &old).unwrap();
    #[cfg(windows)]
    support::win::junction(&apps, &old);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&old, &apps).unwrap();
    assert!(hints::steam(&env, &snap, i64::MAX).is_empty());
    assert_eq!(
        fs::read_to_string(old.join("appmanifest_42.acf")).unwrap(),
        MANIFEST
    );
    // Remove only the link itself; never recursively traverse it during fixture teardown.
    #[cfg(windows)]
    fs::remove_dir(&apps).unwrap();
    #[cfg(unix)]
    fs::remove_file(&apps).unwrap();
}

#[test]
fn passive_guides_require_current_scanned_nofollow_presence_and_never_fall_back_to_host() {
    for platform in [Platform::Windows, Platform::MacOs] {
        let fx = Fixture::new();
        let env = fx.env(platform);
        let empty = hints::discover_until(&env, &fx.snapshot(), moment().now, &|| false).unwrap();
        assert!(empty.is_empty(), "missing tools are not invented");
        seed_tool_presence(&env);
        let snap = fx.snapshot();
        let hints = hints::discover_until(&env, &snap, moment().now, &|| false).unwrap();
        assert_eq!(hints.len(), 4);
        assert!(hints.iter().all(|h| {
            h.reported_bytes.is_none()
                && h.action
                    .external_uri()
                    .is_some_and(|u| u.starts_with("https://"))
        }));
        let outside = Fixture::new();
        let outside_env = outside.env(platform);
        seed_tool_presence(&outside_env);
        let before = fixture::inventory(&outside.root).unwrap();
        let mut wrong_env = outside_env.clone();
        wrong_env.confined_to = Some(fx.root.clone());
        assert!(
            hints::discover_until(&wrong_env, &snap, moment().now, &|| false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(fixture::inventory(&outside.root).unwrap(), before);

        let evidence = if platform == Platform::Windows {
            env.local_app_data.join("Docker")
        } else {
            env.home.join("Library/Containers/com.docker.docker")
        };
        let moved = evidence.with_file_name("docker-old");
        fs::rename(&evidence, &moved).unwrap();
        #[cfg(windows)]
        support::win::junction(&evidence, &moved);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&moved, &evidence).unwrap();
        let found = hints::discover_until(&env, &snap, moment().now, &|| false).unwrap();
        assert!(
            found.iter().all(|h| !h.id.starts_with("hint-docker")),
            "replaced/link evidence is omitted"
        );
        assert_eq!(
            fs::read_to_string(moved.join("fixture-presence.txt")).unwrap(),
            "passive presence only"
        );
        #[cfg(windows)]
        fs::remove_dir(&evidence).unwrap();
        #[cfg(unix)]
        fs::remove_file(&evidence).unwrap();

        // Even a snapshot listing of a different profile cannot bypass positive confinement.
        assert!(
            hints::discover_until(&wrong_env, &outside.snapshot(), moment().now, &|| false)
                .unwrap()
                .is_empty()
        );
    }
}

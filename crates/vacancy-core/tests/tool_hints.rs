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
        // Windows' default temp is under the real home. CI's checkout and the local D: worktree are outside it.
        // On macOS use the system temp directory, canonicalized past /var's symlink.
        let base = if cfg!(windows) {
            std::env::current_dir()
                .unwrap()
                .join("target/tool-fixtures")
        } else {
            std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join("vacancy-tool-fixtures")
        };
        fs::create_dir_all(&base).unwrap();
        let temp = tempfile::tempdir_in(&base).unwrap();
        let root = temp.path().join("profile");
        fs::create_dir(&root).unwrap();
        fixture::check_root(&root, &std::env::home_dir().unwrap()).unwrap();
        fixture::build(&root).unwrap();
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
const MANIFEST: &str = r#""AppState" { "appid" "42" "name" "Fixture game" "LastPlayed" "100000" "SizeOnDisk" "123456789" }"#;

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

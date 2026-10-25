//! The check of Claude Code before privacy mode off lets Claude open files (ADR 0008, `claude::canary`), with a fake
//! Claude Code: each outcome, that the check is a privacy-off call from the real settings builder, and that its kept
//! result holds for one executable only. `live_claude_canary_check` asks the real Claude Code (opt-in, never in CI).

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};
use vacancy_core::claude::canary::{self, Canary, Check, Identity, Record, Scope};
use vacancy_core::claude::runner::{self, Drive, Style};
use vacancy_core::claude::{Access, ClaudeCode, Failure, FakeClaude, Reply, hidden};
use vacancy_core::{CancelToken, Env, ScanOptions};

/// A fresh app data folder, as the app's own.
fn home() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = support::real_path(dir.path()).join("app-data");
    std::fs::create_dir_all(&home).unwrap();
    (dir, home)
}

fn new_check(home: &Path) -> Canary {
    Canary::new(
        &home.join(canary::FOLDER),
        Scope::Folder { mounts: Vec::new() },
    )
    .expect("the check's folder")
}

/// What a Claude Code that keeps its settings answers: the control file's text, the canary's read refused.
fn honest(check: &Canary, list_refusal: bool) -> FakeClaude {
    let (control, hidden_file) = check.files();
    let (control, hidden_file) = (control.to_path_buf(), hidden_file.to_path_buf());
    FakeClaude::new(move |_| {
        Ok(Reply {
            structured: json!({
                "first": std::fs::read_to_string(&control).unwrap(),
                "second": "Error: File is in a directory that is denied by your permission settings.",
            }),
            refused: if list_refusal {
                vec![hidden_file.display().to_string()]
            } else {
                Vec::new()
            },
            model: None,
        })
    })
}

#[test]
fn the_check_passes_only_when_the_canary_stays_hidden_and_the_control_comes_back() {
    let (_dir, home) = home();
    let cancel = CancelToken::new();

    // Passed: the control came back, the canary's read was refused, and Claude Code said so.
    let check = new_check(&home);
    let fake = honest(&check, true);
    let verdict = canary::run(&fake, &check, &cancel);
    assert!(
        verdict.passed && verdict.refusal_seen && verdict.keep,
        "{verdict:?}"
    );
    assert_eq!(
        fake.calls(),
        vec![check.call().clone()],
        "the check's own call, once"
    );
    // The prompt names the files, never what they hold.
    let (control, hidden_file) = check.files();
    let words = |p: &Path| std::fs::read_to_string(p).unwrap().trim().to_owned();
    assert!(!check.call().prompt.contains(&words(control)));
    assert!(!check.call().prompt.contains(&words(hidden_file)));
    assert!(
        check
            .call()
            .prompt
            .contains(&hidden_file.display().to_string())
    );

    // With no refusal recorded, it doesn't pass: Claude may simply have skipped the canary's read.
    let verdict = canary::run(&honest(&check, false), &check, &cancel);
    assert!(
        !verdict.passed && !verdict.refusal_seen && verdict.keep,
        "{verdict:?}"
    );
    assert!(
        verdict
            .why
            .unwrap()
            .contains("didn't record refusing the hidden file")
    );
    // A refusal recorded with the path spelled another way still counts.
    let skipped = check.files().1.display().to_string().replace('\\', "/");
    let control_path = check.files().0.to_path_buf();
    let fake = FakeClaude::new(move |_| {
        Ok(Reply {
            structured: json!({"first": std::fs::read_to_string(&control_path).unwrap(), "second": "denied"}),
            refused: vec![skipped.clone()],
            model: None,
        })
    });
    assert!(canary::run(&fake, &check, &cancel).passed);

    // Each other outcome fails, and privacy mode off stays unavailable.
    let control_text = words(control);
    let canary_text = words(hidden_file);
    let replies: Vec<(&str, Result<Reply, Failure>, &str)> = vec![
        (
            "the canary read",
            Ok(Reply {
                structured: json!({"first": control_text, "second": canary_text}),
                refused: Vec::new(),
                model: None,
            }),
            "let Claude read a file",
        ),
        (
            "the canary read though a refusal is listed",
            Ok(Reply {
                structured: json!({"first": control_text, "second": format!("x {canary_text} y")}),
                refused: vec![hidden_file.display().to_string()],
                model: None,
            }),
            "let Claude read a file",
        ),
        (
            "the control missing",
            Ok(Reply {
                structured: json!({"first": "Error: denied", "second": "Error: denied"}),
                refused: vec![hidden_file.display().to_string()],
                model: None,
            }),
            "didn't return the check's readable file",
        ),
        (
            "only another read refused",
            Ok(Reply {
                structured: json!({"first": control_text, "second": "Error"}),
                refused: vec!["/elsewhere/x".into()],
                model: None,
            }),
            "didn't record refusing the hidden file",
        ),
        (
            "the canary skipped, nothing refused",
            Ok(Reply {
                structured: json!({"first": control_text, "second": "I didn't read it."}),
                refused: Vec::new(),
                model: None,
            }),
            "didn't record refusing the hidden file",
        ),
        (
            "an error",
            Err(Failure::error("API Error: 429 rate limit reached")),
            "didn't finish: API Error: 429",
        ),
        ("a timeout", Err(Failure::TimedOut), "took too long"),
        (
            "an answer that doesn't parse",
            runner::parse("garbage", ""),
            "didn't finish",
        ),
        (
            "an answer outside the schema",
            runner::parse(
                r#"{"type":"result","subtype":"success","is_error":false,"result":"hi"}"#,
                "",
            ),
            "didn't come in the form",
        ),
    ];
    for (what, reply, says) in replies {
        let fake = FakeClaude::new(move |_| reply.clone());
        let verdict = canary::run(&fake, &check, &cancel);
        assert!(!verdict.passed, "{what}: {verdict:?}");
        assert!(
            verdict.keep,
            "{what}: kept, so it isn't asked again until Check again"
        );
        let why = verdict.why.unwrap();
        assert!(why.contains(says), "{what}: {why}");
    }
    // Cancelled, or Claude Code gone or signed out: nothing said about this Claude Code, so nothing kept.
    for failure in [Failure::Cancelled, Failure::NotFound, Failure::SignedOut] {
        let fake = FakeClaude::new(move |_| Err(failure.clone()));
        let verdict = canary::run(&fake, &check, &cancel);
        assert!(!verdict.passed && !verdict.keep, "{verdict:?}");
    }
    // The folder is Vacancy's own and goes with the check.
    let dir = check.dir().to_path_buf();
    assert!(dir.starts_with(home.join(canary::FOLDER)));
    drop(check);
    assert!(!dir.exists());
    // Each check's words are fresh.
    let a = new_check(&home);
    let b = new_check(&home);
    assert_ne!(words(a.files().0), words(b.files().0));
    assert_ne!(words(a.files().1), words(b.files().1));
}

/// The check is made exactly as a privacy-off call: the same builder, the settings in a file, the same flags and
/// environment, `blockReadsOutsideWorkingDirectories`, its folder as the read scope, and its dummy profile hidden by
/// the same machinery that hides a real one.
#[test]
fn the_check_is_a_privacy_off_call_from_the_real_settings_builder() {
    let (_dir, home) = home();
    let (other, inside) = if cfg!(windows) {
        (PathBuf::from(r"Q:\"), PathBuf::from(r"Q:\Mount\Disk2\"))
    } else {
        (
            PathBuf::from("/Volumes/USB"),
            PathBuf::from("/Users/alex/Archive"),
        )
    };
    let check = Canary::new(
        &home.join(canary::FOLDER),
        Scope::Drive {
            others: vec![other.clone()],
            mounts: vec![inside.clone()],
        },
    )
    .unwrap();
    let dir = check.dir().to_path_buf();
    assert_eq!(
        check.access(),
        &Access::Drive(Drive {
            root: dir.clone(),
            others: vec![other],
            mounts: vec![inside],
        })
    );
    // The dummy profile's rules are the ones a scan of the folder makes, as for any real scan.
    let snap = vacancy_core::scan(
        &dir,
        &ScanOptions::default(),
        &mut |_| {},
        &CancelToken::new(),
    )
    .unwrap();
    let mut env = if cfg!(windows) {
        Env::windows(&dir, "check")
    } else {
        Env::macos(&dir, "check")
    };
    env.confined_to = Some(dir.clone());
    assert_eq!(
        check.call().deny,
        hidden::Hider::of(&snap, &env).deny_rules()
    );
    let profile = runner::rule_path(&dir.join("BrowserData").join("Work")).unwrap();
    assert!(
        check
            .call()
            .deny
            .iter()
            .any(|r| r.starts_with(&format!("Read(/{profile}/"))),
        "the profile is walled: {:?}",
        check.call().deny
    );
    let file = home.join("claude-work").join("call-1.settings.json");
    let inv = runner::invocation(Path::new("claude"), check.call(), check.access(), &file);
    let at = |flag: &str| inv.args[inv.args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(at("--tools"), "Read,Glob,Grep");
    assert_eq!(at("--add-dir"), dir.display().to_string());
    assert_eq!(at("--settings"), file.display().to_string());
    assert_eq!(at("--output-format"), "json");
    // Never the person's model or reasoning: the check is kept per executable, not per model.
    assert!(
        !inv.args.iter().any(|a| a == "--model" || a == "--effort"),
        "no --model and no --effort, whatever the person picked"
    );
    assert_eq!(
        (check.call().model, check.call().effort),
        (
            vacancy_core::claude::Model::Default,
            vacancy_core::claude::Effort::Default
        )
    );
    let set: Vec<(String, String)> = runner::ENV_SET
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    assert_eq!(inv.env_set, set);
    assert_eq!(inv.env_remove, vec!["ANTHROPIC_API_KEY".to_owned()]);
    let (_, json) = inv.settings.as_ref().unwrap();
    runner::check_settings(json).expect("the settings a real call passes");
    let settings: Value = serde_json::from_str(json).unwrap();
    assert_eq!(
        settings["permissions"]["blockReadsOutsideWorkingDirectories"],
        Value::Bool(true)
    );
    assert_eq!(
        settings["permissions"]["allow"],
        json!([runner::allow_rule_in(dir.to_str().unwrap(), Style::HOST).unwrap()])
    );
    let deny: Vec<String> =
        serde_json::from_value(settings["permissions"]["deny"].clone()).unwrap();
    for rule in &check.call().deny {
        assert!(deny.contains(rule), "{rule}");
    }
    for rule in hidden::deny_rules() {
        assert!(deny.contains(&rule), "the always-hidden places: {rule}");
    }
    let mount = if cfg!(windows) {
        "Read(//q/Mount/Disk2/**)"
    } else {
        "Read(//Users/alex/Archive/**)"
    };
    assert!(
        deny.contains(&mount.to_owned()),
        "the mounts, as a real call's"
    );
    // In fixture mode, as the fixture folder is read, with every mount point: one nested in the folder is denied,
    // and the volume holding it isn't.
    let probe = new_check(&home);
    let volume = probe.dir().ancestors().last().unwrap().to_path_buf();
    drop(probe);
    let folder = Canary::new(
        &home.join(canary::FOLDER),
        Scope::Folder {
            mounts: vec![volume.clone()],
        },
    )
    .unwrap();
    assert_eq!(
        folder.access(),
        &Access::Fixture {
            root: folder.dir().to_path_buf(),
            mounts: vec![volume.clone()],
        }
    );
    let access = folder
        .access()
        .with_mounts(vec![volume, folder.dir().join("Mount").join("Image")]);
    let inv = runner::invocation(Path::new("claude"), folder.call(), &access, &file);
    let settings: Value = serde_json::from_str(&inv.settings.unwrap().1).unwrap();
    let deny: Vec<String> =
        serde_json::from_value(settings["permissions"]["deny"].clone()).unwrap();
    let nested = folder.dir().join("Mount").join("Image");
    let rule = format!("Read(/{}/**)", runner::rule_path(&nested).unwrap());
    assert!(deny.contains(&rule), "{rule}");
    assert!(
        !deny
            .iter()
            .any(|r| r == "Read(//**)" || r == "Read(//*/**)")
    );
    assert!(runner::access_mounts_unavailable(&access).is_none());
}

/// The kept result holds for exactly one Claude Code: its resolved path, size, modified time and version. Any change
/// makes the check due again.
#[test]
fn a_kept_result_holds_for_one_executable_only() {
    let (dir, home) = home();
    let exe = dir.path().join("claude.exe");
    std::fs::write(&exe, b"one").unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let version = |v: &'static str| {
        let asked = asked.clone();
        move |_: &Path| {
            asked.fetch_add(1, Ordering::SeqCst);
            Ok::<_, Failure>(v.to_owned())
        }
    };
    let first = Identity::of(&exe, None, version("2.1.309 (Claude Code)")).unwrap();
    assert_eq!(first.path, canary::resolve(&exe).unwrap().to_string_lossy());
    assert!(
        !first.path.starts_with(r"\\?\"),
        "a path that can be started as it is"
    );
    assert_eq!(first.size, 3);
    assert!(first.modified.is_some());
    // The SHA-256 of "one".
    assert_eq!(
        first.sha256,
        "7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert!(first.is(&exe));
    // The same file, as this process found it: its hash and version stand, nothing is read or started.
    assert_eq!(
        Identity::of(&exe, Some(&first), version("x")).unwrap(),
        first
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    // Another executable of the same size with the same modified time (a copy that kept its timestamps): a fresh
    // look (each process's first, and Check again) hashes it, so it doesn't inherit the pass.
    let twin = dir.path().join("twin.exe");
    std::fs::write(&twin, b"two").unwrap();
    let stamp = std::fs::metadata(&exe).unwrap().modified().unwrap();
    std::fs::File::options()
        .write(true)
        .open(&twin)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    std::fs::copy(&twin, &exe).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&exe)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    let swapped = Identity::of(&exe, None, version("2.1.309 (Claude Code)")).unwrap();
    assert_eq!(
        (swapped.size, swapped.modified),
        (first.size, first.modified)
    );
    assert_ne!(swapped.sha256, first.sha256);
    std::fs::write(&exe, b"one").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&exe)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    asked.store(1, Ordering::SeqCst);

    canary::save(
        &home,
        &Record {
            identity: first.clone(),
            passed: true,
            why: None,
        },
    )
    .unwrap();
    let kept = canary::load(&home);
    assert_eq!(canary::state(kept.as_ref(), &first), Check::Passed);
    assert_eq!(canary::state(None, &first), Check::Unchecked);

    // A new version, the same file (asked again with Check again): due.
    let newer = Identity::of(&exe, None, version("2.1.310 (Claude Code)")).unwrap();
    assert_eq!(canary::state(kept.as_ref(), &newer), Check::Unchecked);
    // Another size: its version is asked again, and the check is due.
    std::fs::write(&exe, b"longer").unwrap();
    let f = std::fs::File::options().write(true).open(&exe).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();
    drop(f);
    let resized = Identity::of(&exe, Some(&first), version("2.1.309 (Claude Code)")).unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 3);
    assert_eq!(resized.size, 6);
    assert_eq!(canary::state(kept.as_ref(), &resized), Check::Unchecked);
    // Another modified time only.
    let f = std::fs::File::options().write(true).open(&exe).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000))
        .unwrap();
    drop(f);
    let touched = Identity::of(&exe, Some(&resized), version("2.1.309 (Claude Code)")).unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 4);
    assert_ne!(touched.modified, resized.modified);
    // Another path (a second install), and each field on its own.
    for changed in [
        Identity {
            path: format!("{}2", first.path),
            ..first.clone()
        },
        Identity {
            size: first.size + 1,
            ..first.clone()
        },
        Identity {
            modified: Some(1),
            ..first.clone()
        },
        Identity {
            version: "2.1.310".into(),
            ..first.clone()
        },
        swapped.clone(),
        Identity {
            sha256: swapped.sha256.clone(),
            ..first.clone()
        },
    ] {
        assert_eq!(
            canary::state(kept.as_ref(), &changed),
            Check::Unchecked,
            "{changed:?}"
        );
    }
    // A failed check holds for its Claude Code too, saying why.
    canary::save(
        &home,
        &Record {
            identity: first.clone(),
            passed: false,
            why: Some("It leaked.".into()),
        },
    )
    .unwrap();
    match canary::state(canary::load(&home).as_ref(), &first) {
        Check::Failed { message } => {
            assert!(
                message.contains("couldn't verify this version of Claude Code"),
                "{message}"
            );
            assert!(message.contains("It leaked."), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // A damaged file is no result: due.
    std::fs::write(canary::path(&home), b"{").unwrap();
    assert_eq!(
        canary::state(canary::load(&home).as_ref(), &first),
        Check::Unchecked
    );
    // A Claude Code that can't be read, or won't say its version, is an error in words.
    let gone = Identity::of(&dir.path().join("missing.exe"), None, version("x")).unwrap_err();
    assert!(gone.contains("couldn't read"), "{gone}");
    let silent = Identity::of(&exe, None, |_: &Path| Err(Failure::TimedOut)).unwrap_err();
    assert!(silent.contains("which version"), "{silent}");
}

fn record(identity: &Identity, passed: bool) -> Record {
    Record {
        identity: identity.clone(),
        passed,
        why: (!passed).then(|| "It leaked.".to_owned()),
    }
}

fn some_identity() -> Identity {
    Identity {
        path: "claude".into(),
        size: 1,
        modified: Some(1),
        sha256: "00".into(),
        version: "2.1.309 (Claude Code)".into(),
    }
}

/// A newer result is kept so that no older one is trusted again after a restart, even when the kept file is
/// read-only: Vacancy takes the flag off and replaces it.
#[test]
fn a_newer_failure_replaces_a_read_only_pass() {
    let (_dir, home) = home();
    let id = some_identity();
    canary::save(&home, &record(&id, true)).unwrap();
    let file = canary::path(&home);
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions).unwrap();
    canary::keep(&home, &record(&id, false)).unwrap();
    // What a restart reads: never the old pass.
    assert_ne!(
        canary::state(canary::load(&home).as_ref(), &id),
        Check::Passed
    );
    // A later pass is kept again, and trusted.
    canary::keep(&home, &record(&id, true)).unwrap();
    assert_eq!(
        canary::state(canary::load(&home).as_ref(), &id),
        Check::Passed
    );
    assert!(!home.join(canary::REVOKED).exists());
}

/// When the kept file can neither be replaced nor removed (here a folder stands in its place), it is revoked: a
/// restart trusts no result until a newer one is kept, which takes the revocation away.
#[test]
fn a_kept_result_that_cant_be_replaced_is_revoked() {
    let (_dir, home) = home();
    let id = some_identity();
    let file = canary::path(&home);
    std::fs::create_dir_all(file.join("in the way")).unwrap();
    canary::keep(&home, &record(&id, false)).unwrap();
    assert!(home.join(canary::REVOKED).exists());
    assert_eq!(canary::load(&home), None);
    std::fs::remove_dir_all(&file).unwrap();
    // The old pass, written back by something else, still isn't trusted while revoked.
    canary::save(&home, &record(&id, true)).unwrap();
    assert_eq!(canary::load(&home), None);
    canary::keep(&home, &record(&id, true)).unwrap();
    assert_eq!(
        canary::state(canary::load(&home).as_ref(), &id),
        Check::Passed
    );
}

/// Windows: a kept pass held open by another program (so it can't be replaced or removed) is revoked too.
#[cfg(windows)]
#[test]
fn a_locked_pass_is_revoked() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_dir, home) = home();
    let id = some_identity();
    canary::save(&home, &record(&id, true)).unwrap();
    // Shared for reading only: no rename over it, no delete.
    let held = std::fs::File::options()
        .read(true)
        .share_mode(1)
        .open(canary::path(&home))
        .unwrap();
    canary::keep(&home, &record(&id, false)).unwrap();
    assert!(home.join(canary::REVOKED).exists());
    assert_eq!(canary::load(&home), None, "a restart trusts no pass");
    drop(held);
    canary::keep(&home, &record(&id, true)).unwrap();
    assert_eq!(
        canary::state(canary::load(&home).as_ref(), &id),
        Check::Passed
    );
}

/// What Claude Code resolves to must be a regular file, and on Windows an `.exe`: a link named `claude.exe` that
/// leads to a `.bat` would run through `cmd.exe`.
#[test]
fn only_a_real_executable_is_resolved() {
    let (dir, _home) = home();
    let folder = dir.path().join("claude-folder.exe");
    std::fs::create_dir(&folder).unwrap();
    assert!(canary::resolve(&folder).is_err(), "a folder");
    if cfg!(windows) {
        let bat = dir.path().join("claude.bat");
        std::fs::write(&bat, b"@echo off\r\n").unwrap();
        assert!(canary::resolve(&bat).is_err(), "a .bat");
        let link = dir.path().join("claude.exe");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&bat, &link);
        #[cfg(not(windows))]
        let made: std::io::Result<()> = Ok(());
        match made {
            Ok(()) => {
                let why = canary::resolve(&link).unwrap_err().to_string();
                assert!(why.contains("isn't an .exe"), "{why}");
                assert!(Identity::of(&link, None, |_| Ok("x".into())).is_err());
            }
            Err(e) => assert!(!support::in_ci(), "CI can make symlinks: {e}"),
        }
    }
    let real = dir.path().join("real.exe");
    std::fs::write(&real, b"x").unwrap();
    assert!(canary::resolve(&real).is_ok());
}

/// Windows: a `\\?\` path whose plain form names another place (`Tools.` is `Tools` once Windows normalizes it) is
/// refused, never quietly started from the other place.
#[cfg(windows)]
#[test]
fn a_verbatim_path_that_means_another_place_plainly_is_refused() {
    let (dir, _home) = home();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let dotted = PathBuf::from(format!("{}\\Tools.", base.display()));
    std::fs::create_dir(&dotted).unwrap();
    std::fs::write(dotted.join("claude.exe"), b"the one that was asked for").unwrap();
    // The place its plain spelling names.
    let plain = PathBuf::from(format!("{}\\Tools", base.display()));
    std::fs::create_dir(&plain).unwrap();
    std::fs::write(plain.join("claude.exe"), b"another one").unwrap();
    let why = canary::resolve(&dotted.join("claude.exe"))
        .unwrap_err()
        .to_string();
    assert!(why.contains("names another place"), "{why}");
    // An ordinary path still resolves, without the prefix.
    let ok = canary::resolve(&plain.join("claude.exe")).unwrap();
    assert!(!ok.to_string_lossy().starts_with(r"\\?\"), "{ok:?}");
    // Only a verbatim path reaches the dotted folder again.
    std::fs::remove_dir_all(&dotted).unwrap();
}

/// An opt-in check (never in CI) with the real Claude Code, exactly as the app makes it: privacy mode off's settings
/// and flags, reading a fresh folder Vacancy owns. `VACANCY_LIVE_CANARY=1` runs it (once, with
/// `ANTHROPIC_MODEL=haiku` set for that run only); `VACANCY_PROBE_OUT` is where to keep what came back (no session
/// ids, costs or account details). It must pass.
#[test]
fn live_claude_canary_check() {
    if std::env::var("VACANCY_LIVE_CANARY").as_deref() != Ok("1") {
        eprintln!("skipped: set VACANCY_LIVE_CANARY=1 to check the real Claude Code");
        return;
    }
    let program = vacancy_core::claude::discovery::locate().expect("Claude Code is installed");
    let (_dir, home) = home();
    let work = home.join("claude-work");
    let identity = Identity::of(&program, None, |p| runner::version(p, &work)).unwrap();
    let mounts = vacancy_core::claude::mounts::list().expect("the mounts");
    let others: Vec<PathBuf> = vacancy_core::list_volumes()
        .unwrap()
        .into_iter()
        .map(|v| PathBuf::from(v.mount_point))
        .collect();
    let check = Canary::new(
        &home.join(canary::FOLDER),
        Scope::Drive {
            others,
            mounts: mounts.clone(),
        },
    )
    .unwrap();
    let cli = ClaudeCode {
        program: program.clone(),
        access: check.access().clone(),
        work: work.clone(),
    };
    let invocation = runner::invocation(
        &program,
        check.call(),
        check.access(),
        &work.join("call.settings.json"),
    );
    let control = runner::Control {
        cancel: CancelToken::new(),
        timeout: canary::TIMEOUT,
        slow_after: Duration::MAX,
    };
    let (result, stdout) = cli.call_recorded(check.call(), &control, &mut |_| {});
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    let verdict = check.judge(result);
    let (control_file, canary_file) = check.files();
    let canary_words = std::fs::read_to_string(canary_file)
        .unwrap()
        .trim()
        .to_owned();
    let control_words = std::fs::read_to_string(control_file)
        .unwrap()
        .trim()
        .to_owned();
    let settings: Value = invocation
        .settings
        .as_ref()
        .and_then(|(_, j)| serde_json::from_str(j).ok())
        .unwrap_or(Value::Null);
    let deny = settings["permissions"]["deny"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let kept = json!({
        "claudeCode": identity.version,
        "passed": verdict.passed,
        "why": verdict.why,
        "refusalSeen": verdict.refusal_seen,
        "canaryInOutput": stdout.contains(&canary_words),
        "controlInOutput": stdout.contains(&control_words),
        "checkFolder": check.dir().display().to_string(),
        "files": [control_file.display().to_string(), canary_file.display().to_string()],
        "argvAfterSystemPrompt": invocation.args.iter().skip(14).collect::<Vec<_>>(),
        "envSet": invocation.env_set,
        "envRemove": invocation.env_remove,
        "allow": settings["permissions"]["allow"],
        "blockReadsOutsideWorkingDirectories": settings["permissions"]["blockReadsOutsideWorkingDirectories"],
        "denyRules": deny.len(),
        "profileDenyRules": deny.iter().filter(|r| r.as_str().is_some_and(|r| r.contains("BrowserData"))).count(),
        "mountsListed": mounts.len(),
        "subtype": envelope["subtype"],
        "is_error": envelope["is_error"],
        "structured_output": envelope["structured_output"],
        "permission_denials": envelope["permission_denials"],
        "models": envelope["modelUsage"].as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()),
    });
    eprintln!("{}", serde_json::to_string_pretty(&kept).unwrap());
    if let Ok(out) = std::env::var("VACANCY_PROBE_OUT") {
        std::fs::write(out, serde_json::to_string_pretty(&kept).unwrap() + "\n").unwrap();
    }
    assert!(verdict.passed, "{verdict:?}");
}

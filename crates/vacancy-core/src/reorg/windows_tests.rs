use super::*;
use crate::reorg::journal::tests::{self as faults, Fault};
use std::cell::RefCell;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;

type Hook = Box<dyn FnMut(&str)>;
thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
pub(super) fn hook(name: &str) {
    HOOK.with(|slot| {
        if let Some(callback) = slot.borrow_mut().as_mut() {
            callback(name);
        }
    });
}
fn set_hook(callback: impl FnMut(&str) + 'static) {
    HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(callback)));
}
fn clear_hook() {
    HOOK.with(|slot| *slot.borrow_mut() = None);
}

fn fixture() -> (tempfile::TempDir, Env) {
    let base = std::env::var_os("VACANCY_T08_TEST_ROOT")
        .or_else(|| std::env::var_os("RUNNER_TEMP"))
        .expect("T08 tests require an outside-home wrapper root or CI RUNNER_TEMP");
    let base = PathBuf::from(base);
    assert!(native::plain(&base));
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let home = PathBuf::from(home);
        assert!(!base.starts_with(&home) && !home.starts_with(&base));
    }
    let dir = tempfile::Builder::new()
        .prefix("vacancy-t08-case-")
        .tempdir_in(base)
        .unwrap();
    let mut env = Env::windows(dir.path(), "fixture");
    env.confined_to = Some(dir.path().to_path_buf());
    for path in [
        &env.known.downloads,
        &env.known.desktop,
        &env.known.documents,
        &env.app_data,
        &env.system_root,
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(
        dir.path().join("t08-test-marker"),
        b"isolated synthetic Env only",
    )
    .unwrap();
    (dir, env)
}
fn put(env: &Env, name: &str) -> PathBuf {
    let path = env.known.downloads.join(name);
    fs::write(&path, format!("payload:{name}")).unwrap();
    path
}
fn approved(env: &Env, names: &[&str], directories: &[&str]) -> ValidatedPlan {
    let inventory = Inventory::capture(env, &CancelToken::new()).unwrap();
    let drafts = names
        .iter()
        .map(|name| Draft {
            entry_id: inventory
                .entries()
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap()
                .id
                .clone(),
            destination: Folder::Documents,
            directories: directories.iter().map(|name| (*name).into()).collect(),
            filename: None,
        })
        .collect::<Vec<_>>();
    inventory.validate(&drafts, &CancelToken::new()).unwrap()
}
fn journal(env: &Env) -> PathBuf {
    env.app_data.join("run.vjr")
}

#[test]
fn windows_move_undo_restores_exact_bytes_names_and_hardlink_relation() {
    let (_dir, env) = fixture();
    let source = put(&env, "Résumé.bin");
    let sibling = env.known.desktop.join("another-name.bin");
    fs::hard_link(&source, &sibling).unwrap();
    let original = fs::read(&source).unwrap();
    let mut engine = Engine::begin(
        &env,
        &journal(&env),
        approved(&env, &["Résumé.bin"], &["Sorted", "Documents"]),
    )
    .unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 1);
    let moved = env.known.documents.join("Sorted/Documents/Résumé.bin");
    assert!(!source.exists());
    assert_eq!(fs::read(&moved).unwrap(), original);
    assert_eq!(
        crate::fsid::identity(&moved),
        crate::fsid::identity(&sibling)
    );
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(!env.known.documents.join("Sorted").exists());
    assert_eq!(
        crate::fsid::identity(&source),
        crate::fsid::identity(&sibling)
    );
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
}

#[test]
fn reviewed_collision_name_is_fixed_and_a_later_collision_never_overwrites() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    fs::write(env.known.documents.join("a.txt"), b"existing").unwrap();
    let plan = approved(&env, &["a.txt"], &[]);
    assert!(plan.review()[0].to.ends_with("a (1).txt"));
    fs::write(
        env.known.documents.join("a (1).txt"),
        b"arrived after review",
    )
    .unwrap();
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0);
    assert_eq!(
        fs::read(env.known.documents.join("a (1).txt")).unwrap(),
        b"arrived after review"
    );
    assert!(env.known.downloads.join("a.txt").exists());
    assert!(!env.known.documents.join("a (2).txt").exists());
}

#[test]
fn kernel_collision_after_intent_is_atomic_and_no_replace() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    let target = env.known.documents.join("a.txt");
    let observed = target.clone();
    let mut engine = Engine::begin(&env, &journal(&env), approved(&env, &["a.txt"], &[])).unwrap();
    set_hook(move |at| {
        if at == "move-intent" {
            fs::write(&target, b"race sentinel").unwrap();
        }
    });
    let result = engine.run(&CancelToken::new());
    clear_hook();
    assert_eq!(result.unwrap().applied, 0);
    assert_eq!(fs::read(observed).unwrap(), b"race sentinel");
    assert!(env.known.downloads.join("a.txt").exists());
}

#[test]
fn changed_and_held_open_files_stay_in_place() {
    let (_dir, env) = fixture();
    let source = put(&env, "a.txt");
    let plan = approved(&env, &["a.txt"], &[]);
    fs::write(&source, b"new differently sized contents").unwrap();
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0);
    drop(engine);
    let plan = approved(&env, &["a.txt"], &[]);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(&source)
        .unwrap();
    let mut engine = Engine::begin(&env, &env.app_data.join("held.vjr"), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0);
    assert!(source.exists());
    drop(held);
}

#[test]
fn edited_same_identity_is_undone_but_original_name_conflicts_and_replacements_are_preserved() {
    let (_dir, env) = fixture();
    let source = put(&env, "a.txt");
    let mut engine = Engine::begin(&env, &journal(&env), approved(&env, &["a.txt"], &[])).unwrap();
    engine.run(&CancelToken::new()).unwrap();
    let target = env.known.documents.join("a.txt");
    fs::write(&target, b"user edit is retained").unwrap();
    fs::write(&source, b"new original-name file").unwrap();
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 0);
    assert_eq!(fs::read(&source).unwrap(), b"new original-name file");
    fs::remove_file(&source).unwrap();
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
    assert_eq!(fs::read(&source).unwrap(), b"user edit is retained");
}

#[test]
fn no_first_move_without_durable_intent_and_no_second_move_after_result_failure() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    put(&env, "b.txt");
    let mut engine = Engine::begin(
        &env,
        &journal(&env),
        approved(&env, &["a.txt", "b.txt"], &[]),
    )
    .unwrap();
    faults::set(3, Fault::Before);
    assert!(engine.run(&CancelToken::new()).is_err());
    assert!(env.known.downloads.join("a.txt").exists());
    assert!(env.known.downloads.join("b.txt").exists());
    drop(engine);
    let mut engine = Engine::begin(
        &env,
        &env.app_data.join("second.vjr"),
        approved(&env, &["a.txt", "b.txt"], &[]),
    )
    .unwrap();
    faults::set(4, Fault::Partial(24));
    assert!(matches!(
        engine.run(&CancelToken::new()),
        Err(Error::StoppedUnresolved)
    ));
    assert!(env.known.documents.join("a.txt").exists());
    assert!(env.known.downloads.join("b.txt").exists());
    assert!(matches!(
        engine.undo(&CancelToken::new()),
        Err(Error::StoppedUnresolved)
    ));
}

#[test]
fn torn_result_reopens_twice_reconciles_once_and_allows_durable_explicit_undo() {
    let (_dir, env) = fixture();
    let source = put(&env, "a.txt");
    let location = journal(&env);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    faults::set(4, Fault::Partial(27));
    assert!(engine.run(&CancelToken::new()).is_err());
    drop(engine);
    let preserved = fs::read(&location).unwrap();
    let mut engine = Engine::open(&env, &location).unwrap();
    assert_eq!(engine.status().unresolved, 1);
    assert!(!source.exists());
    assert_eq!(engine.reconcile(&CancelToken::new()).unwrap().applied, 1);
    assert_eq!(engine.status().unresolved, 0);
    drop(engine);
    assert_eq!(fs::read(&location).unwrap(), preserved);
    let mut engine = Engine::open(&env, &location).unwrap();
    assert_eq!(engine.reconcile(&CancelToken::new()).unwrap().applied, 1);
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
    assert!(source.exists());
    drop(engine);
    let mut engine = Engine::open(&env, &location).unwrap();
    assert_eq!(engine.reconcile(&CancelToken::new()).unwrap().undone, 1);
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
}

#[test]
fn failed_flush_never_allows_the_first_namespace_change() {
    let (_dir, env) = fixture();
    let source = put(&env, "a.txt");
    let mut engine = Engine::begin(&env, &journal(&env), approved(&env, &["a.txt"], &[])).unwrap();
    faults::set(3, Fault::Flush);
    assert!(engine.run(&CancelToken::new()).is_err());
    assert!(source.exists());
    assert!(!env.known.documents.join("a.txt").exists());
}

#[test]
fn corrupt_complete_frame_refuses_while_a_live_stable_lease_excludes_another_engine() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    let location = journal(&env);
    let engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    assert!(matches!(Engine::open(&env, &location), Err(Error::Busy)));
    drop(engine);
    let mut bytes = fs::read(&location).unwrap();
    let at = bytes.len() - 9;
    bytes[at] ^= 1;
    fs::write(&location, bytes).unwrap();
    assert!(matches!(
        Engine::open(&env, &location),
        Err(Error::CorruptJournal)
    ));
    assert!(env.known.downloads.join("a.txt").exists());
}

#[test]
fn malicious_components_unknown_inventory_ids_and_protected_absent_destinations_refuse() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    for name in ["..", "x:y", "NUL.txt", "a/b", "\\\\server\\share"] {
        let draft = Draft {
            entry_id: inventory.entries()[0].id.clone(),
            destination: Folder::Documents,
            directories: vec![name.into()],
            filename: None,
        };
        assert!(inventory.validate(&[draft], &CancelToken::new()).is_err());
    }
    let draft = Draft {
        entry_id: "display-string-not-an-inventory-id".into(),
        destination: Folder::Documents,
        directories: vec![],
        filename: None,
    };
    assert!(matches!(
        inventory.validate(&[draft], &CancelToken::new()),
        Err(Error::UnknownEntry)
    ));
    let draft = Draft {
        entry_id: inventory.entries()[0].id.clone(),
        destination: Folder::Documents,
        directories: vec!["WindowsApps".into()],
        filename: None,
    };
    assert!(matches!(
        inventory.validate(&[draft], &CancelToken::new()),
        Err(Error::Denied(_))
    ));
}

// Prepared while check1 source is frozen; append to reorg/windows_tests.rs after its actual outcome.
fn retag_in_place(link: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
    };
    use windows_sys::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT};
    let file = fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(7)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(link)?;
    let substitute = format!(r"\??\{}", to.display())
        .encode_utf16()
        .collect::<Vec<_>>();
    let print = to.as_os_str().encode_wide().collect::<Vec<_>>();
    let data_len = 8 + (substitute.len() + print.len() + 2) * 2;
    let mut data = Vec::new();
    data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
    data.extend_from_slice(&(data_len as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    for value in [
        0,
        substitute.len() * 2,
        (substitute.len() + 1) * 2,
        print.len() * 2,
    ] {
        data.extend_from_slice(&(value as u16).to_le_bytes());
    }
    for value in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        data.extend_from_slice(&value.to_le_bytes());
    }
    let mut returned = 0;
    // SAFETY: owned fixture handle, complete bounded mount-point buffer and writable result pointer.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[test]
fn actual_attribute_only_retag_after_intent_cannot_redirect_a_move_or_journal_write() {
    let (_dir, env) = fixture();
    let source = put(&env, "a.txt");
    let target = env.known.documents.join("Sorted");
    fs::create_dir(&target).unwrap();
    let protected = env.system_root.join("protected");
    fs::create_dir(&protected).unwrap();
    fs::write(protected.join("sentinel"), b"protected bytes").unwrap();
    let control = env.known.desktop.join("control");
    fs::create_dir(&control).unwrap();
    retag_in_place(&control, &protected).unwrap();
    assert_eq!(
        fs::read(control.join("sentinel")).unwrap(),
        b"protected bytes"
    );
    fs::remove_dir(control).unwrap();
    let mut engine = Engine::begin(
        &env,
        &journal(&env),
        approved(&env, &["a.txt"], &["Sorted"]),
    )
    .unwrap();
    let (attacked, outside) = (target.clone(), protected.clone());
    set_hook(move |point| {
        if point == "move-intent" {
            retag_in_place(&attacked, &outside).unwrap();
            assert_eq!(
                fs::read(attacked.join("sentinel")).unwrap(),
                b"protected bytes"
            );
        }
    });
    let result = engine.run(&CancelToken::new());
    clear_hook();
    assert_eq!(result.unwrap().applied, 0);
    assert!(source.exists());
    assert!(!protected.join("a.txt").exists());
    assert_eq!(
        fs::read(protected.join("sentinel")).unwrap(),
        b"protected bytes"
    );
    drop(engine);
    // Empty journal storage is retagged after its parent is retained. Native relative creation must either refuse
    // or create in that exact original object; it must never reach the newly redirected protected namespace.
    let empty = env.known.desktop.join("journal-parent");
    fs::create_dir(&empty).unwrap();
    let pin = native::directory(&empty).unwrap();
    retag_in_place(&empty, &protected).unwrap();
    assert_eq!(
        fs::read(empty.join("sentinel")).unwrap(),
        b"protected bytes"
    );
    let name = Name::text("relative-journal-check").unwrap();
    if let Ok((mut file, _)) = native::journal_file(&pin.dir, &name, native::JournalOpen::Create) {
        use std::io::Write;
        file.write_all(b"journal-only fixture check").unwrap();
        file.sync_all().unwrap();
    }
    assert!(!protected.join("relative-journal-check").exists());
    assert!(native::revalidate(&pin).is_err());
}

#[test]
fn cancellation_finishes_current_intent_and_result_then_stops() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    put(&env, "b.txt");
    let mut engine = Engine::begin(
        &env,
        &journal(&env),
        approved(&env, &["a.txt", "b.txt"], &[]),
    )
    .unwrap();
    let cancel = CancelToken::new();
    let signal = cancel.clone();
    set_hook(move |point| {
        if point == "move-intent" {
            signal.cancel();
        }
    });
    let status = engine.run(&cancel);
    clear_hook();
    let status = status.unwrap();
    assert_eq!(status.applied, 1);
    assert_eq!(status.unresolved, 0);
    assert!(status.cancelled);
    assert!(env.known.downloads.join("b.txt").exists());
    engine.undo(&CancelToken::new()).unwrap();
    assert!(env.known.downloads.join("a.txt").exists());
}

#[test]
fn inventory_is_metadata_only_bounded_cancellable_and_foreign_ids_do_not_bind() {
    let (_dir, env) = fixture();
    let a = put(&env, "a.txt");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&a)
        .unwrap();
    let first = Inventory::capture(&env, &CancelToken::new()).unwrap();
    assert_eq!(first.entries().len(), 1);
    drop(held);
    let second = Inventory::capture(&env, &CancelToken::new()).unwrap();
    let foreign = Draft {
        entry_id: first.entries()[0].id.clone(),
        destination: Folder::Documents,
        directories: vec![],
        filename: None,
    };
    assert!(matches!(
        second.validate(&[foreign], &CancelToken::new()),
        Err(Error::UnknownEntry)
    ));
    fs::create_dir(env.known.documents.join("nested")).unwrap();
    fs::write(
        env.known.documents.join("nested/not-loose.txt"),
        b"not inventoried",
    )
    .unwrap();
    for index in 0..600 {
        fs::write(env.known.downloads.join(format!("{index:04}.dat")), b"x").unwrap();
    }
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    assert!(inventory.truncated());
    assert!(inventory.entries().len() <= MAX_ENTRIES);
    assert!(
        !inventory
            .entries()
            .iter()
            .any(|entry| entry.name == "not-loose.txt")
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        Inventory::capture(&env, &cancel),
        Err(Error::Cancelled)
    ));
}

fn run_child(env: &Env, point: &str, mode: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "reorg::windows::tests::crash_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("VACANCY_T08_CHILD_ROOT", &env.root)
        .env("VACANCY_T08_CRASH_AT", point)
        .env("VACANCY_T08_CHILD_MODE", mode)
        .output()
        .unwrap()
}

#[test]
#[ignore = "subprocess boundary helper, only run with a fresh parent-created fixture"]
fn crash_child() {
    let root =
        PathBuf::from(std::env::var_os("VACANCY_T08_CHILD_ROOT").expect("parent fixture root"));
    let base = PathBuf::from(
        std::env::var_os("VACANCY_T08_TEST_ROOT").expect("outside-home test wrapper"),
    );
    assert!(
        root.starts_with(&base)
            && root != base
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("vacancy-t08-case-")
    );
    assert_eq!(
        fs::read(root.join("t08-test-marker")).unwrap(),
        b"isolated synthetic Env only"
    );
    let mut env = Env::windows(&root, "fixture");
    env.confined_to = Some(root);
    let mode = std::env::var("VACANCY_T08_CHILD_MODE").unwrap();
    if mode == "lease" {
        assert!(matches!(
            Engine::open(&env, &journal(&env)),
            Err(Error::Busy)
        ));
        return;
    }
    let mut engine = Engine::open(&env, &journal(&env)).unwrap();
    match mode.as_str() {
        "run" => {
            engine.run(&CancelToken::new()).unwrap();
        }
        "undo" => {
            engine.undo(&CancelToken::new()).unwrap();
        }
        "reconcile" => {
            engine.reconcile(&CancelToken::new()).unwrap();
        }
        _ => panic!("unknown child mode"),
    }
    panic!("requested crash boundary was not reached");
}

#[test]
fn process_crashes_at_namespace_and_durable_receipts_reconcile_honestly_then_undo() {
    for point in [
        "mkdir-intent",
        "mkdir-namespace",
        "mkdir-result",
        "move-intent",
        "move-namespace",
        "move-result",
        "undo-intent",
        "undo-namespace",
        "undo-result",
        "remove-intent",
        "remove-namespace",
        "remove-result",
        "segment-create",
        "segment-bound",
        "reconcile-result",
    ] {
        let (_dir, env) = fixture();
        let source = put(&env, "a.txt");
        let location = journal(&env);
        let mut engine = Engine::begin(
            &env,
            &location,
            approved(
                &env,
                &["a.txt"],
                if point.starts_with("segment") || point == "reconcile-result" {
                    &[]
                } else {
                    &["Sorted"]
                },
            ),
        )
        .unwrap();
        let mode = if point.starts_with("undo") || point.starts_with("remove") {
            engine.run(&CancelToken::new()).unwrap();
            "undo"
        } else if point.starts_with("segment") || point == "reconcile-result" {
            faults::set(4, Fault::Partial(25));
            assert!(engine.run(&CancelToken::new()).is_err());
            "reconcile"
        } else {
            "run"
        };
        drop(engine);
        let result = run_child(&env, point, mode);
        assert_eq!(
            result.status.code(),
            Some(73),
            "{point}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let before = source.exists();
        let mut engine = Engine::open(&env, &location).unwrap();
        let status = engine.reconcile(&CancelToken::new()).unwrap();
        assert_eq!(
            source.exists(),
            before,
            "reconciliation never moves at {point}"
        );
        if point == "mkdir-namespace" {
            assert_eq!(status.unresolved, 1);
            assert!(matches!(
                engine.undo(&CancelToken::new()),
                Err(Error::Unresolved)
            ));
            assert!(env.known.documents.join("Sorted").exists());
        } else {
            assert_eq!(status.unresolved, 0, "{point}");
            engine.undo(&CancelToken::new()).unwrap();
            assert!(source.exists(), "{point}");
        }
        let first = engine.status();
        drop(engine);
        let mut engine = Engine::open(&env, &location).unwrap();
        let again = engine.reconcile(&CancelToken::new()).unwrap();
        assert_eq!(again, first, "{point}: repeated recovery changes counts");
    }
}

#[test]
fn two_consecutive_continuation_create_crashes_preserve_the_committed_prefix_and_undo() {
    let (_dir, env) = fixture();
    let source = put(&env, "a.txt");
    let location = journal(&env);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    faults::set(4, Fault::Partial(25));
    assert!(engine.run(&CancelToken::new()).is_err());
    drop(engine);
    let preserved = fs::read(&location).unwrap();
    for _ in 0..2 {
        let result = run_child(&env, "segment-create", "reconcile");
        assert_eq!(result.status.code(), Some(73));
    }
    let mut engine = Engine::open(&env, &location).unwrap();
    assert_eq!(engine.reconcile(&CancelToken::new()).unwrap().applied, 1);
    drop(engine);
    assert_eq!(fs::read(&location).unwrap(), preserved);
    let mut engine = Engine::open(&env, &location).unwrap();
    engine.undo(&CancelToken::new()).unwrap();
    assert!(source.exists());
}

#[test]
fn stable_lease_excludes_a_second_process_for_the_whole_engine_session() {
    let (_dir, env) = fixture();
    put(&env, "a.txt");
    let mut engine = Engine::begin(&env, &journal(&env), approved(&env, &["a.txt"], &[])).unwrap();
    let result = run_child(&env, "", "lease");
    assert!(
        result.status.success(),
        "{} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    engine.run(&CancelToken::new()).unwrap();
    engine.undo(&CancelToken::new()).unwrap();
}

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

// Preparation only while focused attempt2 source is frozen. Not compiled/imported yet.

#[test]
fn native_unpaired_utf16_filename_roundtrips_without_display_string_authority() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let (_dir, env) = fixture();
    let name = OsString::from_wide(&[0xD800, 46, 98, 105, 110]);
    let source = env.known.downloads.join(&name);
    fs::write(&source, b"native name payload").unwrap();
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    assert_eq!(inventory.entries().len(), 1);
    let draft = Draft { entry_id: inventory.entries()[0].id.clone(), destination: Folder::Documents, directories: vec![], filename: None };
    let plan = inventory.validate(&[draft], &CancelToken::new()).unwrap();
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 1);
    assert_eq!(fs::read(env.known.documents.join(&name)).unwrap(), b"native name payload");
    drop(engine);
    let mut engine = Engine::open(&env, &journal(&env)).unwrap();
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
    assert_eq!(fs::read(&source).unwrap(), b"native name payload");
}

#[test]
fn replacing_a_reviewed_source_or_known_folder_never_moves_the_replacement() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt");
    let plan = approved(&env, &["a.txt"], &[]);
    fs::rename(&source, env.known.downloads.join("preserved-original.txt")).unwrap();
    fs::write(&source, b"replacement identity").unwrap();
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0);
    assert_eq!(fs::read(&source).unwrap(), b"replacement identity"); drop(engine);

    let plan = approved(&env, &["a.txt"], &[]);
    let old_parent = env.known.documents.with_file_name("preserved-documents");
    fs::rename(&env.known.documents, &old_parent).unwrap();
    fs::create_dir(&env.known.documents).unwrap();
    let mut engine = Engine::begin(&env, &env.app_data.join("parent.vjr"), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0);
    assert!(source.exists()); assert!(!env.known.documents.join("a.txt").exists());
}

#[test]
fn cloud_roots_unknown_sync_and_junctions_do_not_become_move_authority() {
    let (_dir, mut env) = fixture(); put(&env, "a.txt");
    env.cloud.push(crate::CloudRoot { provider: "fixture-sync".into(), path: env.known.downloads.clone() });
    assert!(Inventory::capture(&env, &CancelToken::new()).unwrap().entries().is_empty());
    env.cloud.clear(); env.unknown_clouds.push("fixture-unknown-sync".into());
    assert!(Inventory::capture(&env, &CancelToken::new()).unwrap().entries().is_empty());
    env.unknown_clouds.clear();
    let target = env.known.documents.join("Sorted"); fs::create_dir(&target).unwrap();
    let protected = env.system_root.join("protected"); fs::create_dir(&protected).unwrap();
    fs::write(protected.join("sentinel"), b"unchanged").unwrap();
    retag_in_place(&target, &protected).unwrap();
    assert_eq!(fs::read(target.join("sentinel")).unwrap(), b"unchanged");
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    let draft = Draft { entry_id: inventory.entries()[0].id.clone(), destination: Folder::Documents, directories: vec!["Sorted".into()], filename: None };
    assert!(inventory.validate(&[draft], &CancelToken::new()).is_err());
    assert!(!protected.join("a.txt").exists());
}

#[test]
fn an_extra_hardlink_after_review_or_move_is_preserved_as_uncertain_authority() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt");
    let plan = approved(&env, &["a.txt"], &[]);
    let alias = env.known.desktop.join("extra-link.txt"); fs::hard_link(&source, &alias).unwrap();
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0); drop(engine);
    fs::remove_file(&alias).unwrap();
    let mut engine = Engine::begin(&env, &env.app_data.join("second.vjr"), approved(&env, &["a.txt"], &[])).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 1);
    let moved = env.known.documents.join("a.txt"); fs::hard_link(&moved, &alias).unwrap();
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 0);
    assert!(!source.exists()); assert!(moved.exists()); assert!(alias.exists());
}

#[test]
fn undo_never_adopts_replacement_files_or_removes_nonempty_owned_directories() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt");
    let mut engine = Engine::begin(&env, &journal(&env), approved(&env, &["a.txt"], &["Sorted"])).unwrap();
    engine.run(&CancelToken::new()).unwrap();
    let moved = env.known.documents.join("Sorted/a.txt");
    let retained = env.known.desktop.join("retained-original.txt");
    fs::rename(&moved, &retained).unwrap(); fs::write(&moved, b"replacement preserved").unwrap();
    let user_added = env.known.documents.join("Sorted/user-added.txt"); fs::write(&user_added, b"user addition").unwrap();
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 0);
    assert!(!source.exists()); assert_eq!(fs::read(&moved).unwrap(), b"replacement preserved");
    assert_eq!(fs::read(&user_added).unwrap(), b"user addition"); assert!(retained.exists());
}

#[test]
fn case_alias_after_approval_never_overwrites_or_chooses_an_unreviewed_suffix() {
    let (_dir, env) = fixture(); let source = put(&env, "report.txt");
    let plan = approved(&env, &["report.txt"], &[]);
    let alias = env.known.documents.join("REPORT.TXT"); fs::write(&alias, b"case alias sentinel").unwrap();
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 0);
    assert!(source.exists()); assert_eq!(fs::read(&alias).unwrap(), b"case alias sentinel");
    assert!(!env.known.documents.join("report (1).txt").exists());
}

#[test]
fn a_case_alias_journal_name_cannot_split_the_exclusive_lease() {
    let (_dir, env) = fixture(); put(&env, "a.txt");
    let _engine = Engine::begin(&env, &journal(&env), approved(&env, &["a.txt"], &[])).unwrap();
    assert!(matches!(Engine::open(&env, &env.app_data.join("RUN.VJR")), Err(Error::Busy)));
}

// Preparation only; not imported/compiled while attempt2 source remains frozen.

#[test]
fn complete_checksummed_unknown_schema_or_illegal_transition_refuses_before_mutation() {
    for invalid in [
        serde_json::json!({"kind":"started", "extra":"not ignored"}),
        serde_json::json!({"kind":"unknownFutureEvent"}),
        serde_json::json!({"kind":"finished", "cancelled":false}),
        serde_json::json!({"kind":"intent", "operation":"move", "index":0, "identity":null}),
    ] {
        let (_dir, env) = fixture(); let source = put(&env, "a.txt");
        let location = journal(&env);
        drop(Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap());
        // This test intentionally bypasses Engine's state validator to represent a complete, correctly
        // checksummed but semantically invalid file on disk. No user-data primitive is called here.
        let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
        store.append(&invalid).unwrap(); drop(store);
        assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)));
        assert!(source.exists()); assert!(!env.known.documents.join("a.txt").exists());
    }
}

#[test]
fn journal_hardlink_alias_never_authorizes_first_move() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt");
    let location = journal(&env);
    drop(Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap());
    fs::hard_link(&location, env.known.desktop.join("journal-alias")).unwrap();
    assert!(Engine::open(&env, &location).is_err());
    assert!(source.exists()); assert!(!env.known.documents.join("a.txt").exists());
}

#[test]
fn metadata_reconciliation_preserves_same_identity_at_both_names_as_ambiguous() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    faults::set(4, Fault::Partial(25)); assert!(engine.run(&CancelToken::new()).is_err()); drop(engine);
    let moved = env.known.documents.join("a.txt"); fs::hard_link(&moved, &source).unwrap();
    let mut engine = Engine::open(&env, &location).unwrap();
    let status = engine.reconcile(&CancelToken::new()).unwrap();
    assert_eq!(status.applied, 0); assert_eq!(status.unresolved, 1);
    assert!(matches!(engine.undo(&CancelToken::new()), Err(Error::Unresolved)));
    assert!(source.exists()); assert!(moved.exists());
    let status2 = engine.reconcile(&CancelToken::new()).unwrap(); assert_eq!(status2, status);
}

#[test]
fn corrupted_interior_length_cannot_hide_a_completed_move_from_recovery() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 1); drop(engine);
    let moved = env.known.documents.join("a.txt"); let original = fs::read(&moved).unwrap();
    let mut bytes = fs::read(&location).unwrap();
    let intent = bytes.windows(b"\"kind\":\"intent\"".len()).position(|part| part == b"\"kind\":\"intent\"").unwrap();
    let header = bytes[..intent].windows(4).rposition(|part| part == b"VJR2").unwrap();
    assert!(bytes.len() - header < 1024 * 1024);
    bytes[header + 4..header + 8].copy_from_slice(&(1024u32 * 1024).to_le_bytes());
    fs::write(&location, bytes).unwrap();
    assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)));
    assert!(!source.exists()); assert_eq!(fs::read(&moved).unwrap(), original);
}

#[test]
fn proposed_case_alias_directories_are_rejected_before_any_journal_or_mkdir() {
    let (_dir, env) = fixture(); put(&env, "a.txt"); put(&env, "b.txt");
    // Prove the alias under this actual fixture filesystem, rather than assuming case behavior.
    let control = env.known.desktop.join("CaseControl"); fs::create_dir(&control).unwrap();
    fs::write(env.known.desktop.join("casecontrol/sentinel"), b"actual native alias").unwrap();
    assert_eq!(fs::read(control.join("sentinel")).unwrap(), b"actual native alias");
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    let drafts = ["Sorted", "sorted"].iter().enumerate().map(|(index, dir)| Draft {
        entry_id: inventory.entries()[index].id.clone(), destination: Folder::Documents,
        directories: vec![(*dir).into()], filename: None,
    }).collect::<Vec<_>>();
    assert!(matches!(inventory.validate(&drafts, &CancelToken::new()), Err(Error::InvalidName)));
    assert!(!env.known.documents.join("Sorted").exists()); assert!(!journal(&env).exists());
    assert_eq!(fs::read_dir(&env.app_data).unwrap().count(), 0);
}

#[test]
fn oversized_draft_names_and_complete_approved_manifest_refuse_before_store_creation() {
    let (_dir, env) = fixture();
    for index in 0..MAX_ENTRIES { put(&env, &format!("file-{index:03}.txt")); }
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    assert_eq!(inventory.entries().len(), MAX_ENTRIES);
    let huge = Draft { entry_id: inventory.entries()[0].id.clone(), destination: Folder::Documents,
        directories: vec!["x".repeat(8 * 1024 * 1024)], filename: None };
    assert!(matches!(inventory.validate(&[huge], &CancelToken::new()), Err(Error::InvalidName)));
    let drafts = inventory.entries().iter().map(|entry| Draft { entry_id: entry.id.clone(),
        destination: Folder::Documents, directories: (0..11).map(|index| format!("{index}{}", "x".repeat(199))).collect(), filename: None }).collect::<Vec<_>>();
    assert!(matches!(inventory.validate(&drafts, &CancelToken::new()), Err(Error::Bounds)));
    assert_eq!(fs::read_dir(&env.app_data).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&env.known.documents).unwrap().count(), 0);
}

// Not imported until the frozen attempt5 finishes. Requires typed native Collision receipt.
#[test]
fn directory_collision_after_durable_intent_skips_without_adoption_and_other_moves_stay_undoable() {
    for after_intent in [false, true] {
        let (_dir, env) = fixture(); put(&env, "a.txt"); put(&env, "b.txt");
        let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
        let drafts = [("a.txt", "ZExternal"), ("b.txt", "AOwned")].into_iter().map(|(name, dir)| Draft {
            entry_id: inventory.entries().iter().find(|entry| entry.name == name).unwrap().id.clone(),
            destination: Folder::Documents, directories: vec![dir.into()], filename: None,
        }).collect::<Vec<_>>();
        let plan = inventory.validate(&drafts, &CancelToken::new()).unwrap();
        let external = env.known.documents.join("ZExternal");
        if !after_intent { fs::create_dir(&external).unwrap(); fs::write(external.join("sentinel"), b"external ownership").unwrap(); }
        let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
        if after_intent {
            let attacked = external.clone(); let mut attempts = 0;
            set_hook(move |point| {
                if point == "mkdir-intent" { attempts += 1; } if point == "mkdir-intent" && attempts == 2 {
                    fs::create_dir(&attacked).unwrap();
                    fs::write(attacked.join("sentinel"), b"external ownership").unwrap();
                }
            });
        }
        let result = engine.run(&CancelToken::new()); clear_hook(); let status = result.unwrap();
        assert_eq!(status.applied, 1); assert_eq!(status.unresolved, 0);
        assert!(env.known.downloads.join("a.txt").exists()); assert!(env.known.documents.join("AOwned/b.txt").exists());
        assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"external ownership");
        assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
        assert!(env.known.downloads.join("b.txt").exists()); assert!(!env.known.documents.join("AOwned").exists());
        assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"external ownership");
    }
}

// Preparation only. Exercise the actual bounded continuation path, not a relaxed production budget.
#[test]
fn exhausted_continuation_capacity_cannot_commit_a_new_move_intent() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    // Stand at the same private loop boundary as run(), after its durable Started record and before an intent.
    engine.record(Event::Started).unwrap(); drop(engine);
    let preserved = fs::read(&location).unwrap();
    // Empty preserved segments represent repeated process death immediately after no-replace segment create.
    // They carry no approval/events. Opening them may inspect/bind evidence but must not grant move authority.
    for index in 1..32 { fs::write(env.app_data.join(format!("run.vjr.part{index:04}")), b"").unwrap(); }
    let mut engine = Engine::open(&env, &location).unwrap();
    let status = engine.status();
    assert!(matches!(engine.move_one(0, false), Err(Error::Bounds)));
    assert_eq!(engine.status(), status); assert!(source.exists()); assert!(!env.known.documents.join("a.txt").exists());
    assert!(matches!(engine.move_one(0, false), Err(Error::StoppedUnresolved)));
    drop(engine); assert_eq!(fs::read(&location).unwrap(), preserved);
}

#[test]
fn invalid_native_name_and_literal_replacement_character_have_distinct_inventory_and_review_text() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let (_dir, env) = fixture();
    let invalid = OsString::from_wide(&[0xd800, 46, 116, 120, 116]);
    let replacement = OsString::from_wide(&[0xfffd, 46, 116, 120, 116]);
    fs::write(env.known.downloads.join(&invalid), b"invalid unit").unwrap();
    fs::write(env.known.downloads.join(&replacement), b"replacement character").unwrap();
    let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
    assert_eq!(inventory.entries().len(), 2);
    let invalid_entry = inventory.entries().iter().find(|entry| entry.name == "\\u{D800}.txt").unwrap();
    let replacement_entry = inventory.entries().iter().find(|entry| entry.name == "\u{fffd}.txt").unwrap();
    assert_ne!(invalid_entry.id, replacement_entry.id);
    let drafts = [invalid_entry, replacement_entry].iter().map(|entry| Draft { entry_id: entry.id.clone(), destination: Folder::Documents, directories: vec![], filename: None }).collect::<Vec<_>>();
    let plan = inventory.validate(&drafts, &CancelToken::new()).unwrap();
    assert_ne!(plan.review()[0].from, plan.review()[1].from); assert_ne!(plan.review()[0].to, plan.review()[1].to);
    let mut engine = Engine::begin(&env, &journal(&env), plan).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 2);
    assert_eq!(fs::read(env.known.documents.join(&invalid)).unwrap(), b"invalid unit");
    assert_eq!(fs::read(env.known.documents.join(&replacement)).unwrap(), b"replacement character");
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 2);
}

// Not imported while attempt5 is frozen. Requires terminal forward receipts for successful Finish.
#[test]
fn premature_successful_finish_is_corrupt_but_explicit_cancellation_can_leave_unattempted_moves() {
    for cancelled in [false, true] {
        let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
        drop(Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap());
        let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
        store.append(&Event::Started).unwrap(); store.append(&Event::Finished { cancelled }).unwrap(); drop(store);
        let result = Engine::open(&env, &location);
        if cancelled {
            let engine = result.unwrap(); assert!(engine.status().finished); assert!(engine.status().cancelled);
            assert_eq!(engine.status().applied, 0);
        } else { assert!(matches!(result, Err(Error::CorruptJournal))); }
        assert!(source.exists()); assert!(!env.known.documents.join("a.txt").exists());
    }
}

#[test]
fn exclusive_unavailable_lease_refuses_a_fresh_store_then_release_restores_writes() {
    let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
    let held = fs::OpenOptions::new().read(true).write(true).create_new(true).share_mode(0)
        .open(env.app_data.join("run.vjr.lease")).unwrap();
    assert!(matches!(Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])), Err(Error::Busy)));
    assert!(!location.exists()); assert!(source.exists()); assert!(!env.known.documents.join("a.txt").exists());
    drop(held);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
    assert_eq!(engine.run(&CancelToken::new()).unwrap().applied, 1);
    assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1); assert!(source.exists());
}

#[test]
fn authenticated_header_and_torn_payload_checksum_footer_preserve_intent_and_explicit_undo() {
    for cut in 0..6 {
        let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
        let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &[])).unwrap();
        let applied = Event::Applied { operation: Operation::Move, index: 0, identity: engine.state.plan.moves[0].fingerprint.id };
        let payload_length = serde_json::to_vec(&serde_json::to_value(applied).unwrap()).unwrap().len();
        let counts = [48, 49, 48 + payload_length / 2, 48 + payload_length, 48 + payload_length + 31, 48 + payload_length + 32 + 7];
        faults::set(4, Fault::Partial(counts[cut]));
        assert!(matches!(engine.run(&CancelToken::new()), Err(Error::StoppedUnresolved))); drop(engine);
        let preserved = fs::read(&location).unwrap(); let target = env.known.documents.join("a.txt");
        assert!(!source.exists()); assert!(target.exists());
        let mut engine = Engine::open(&env, &location).unwrap();
        assert_eq!(engine.status().unresolved, 1); assert_eq!(engine.status().applied, 0);
        assert!(!source.exists()); assert!(target.exists()); // load never resumes namespace actions
        assert_eq!(engine.reconcile(&CancelToken::new()).unwrap().applied, 1); drop(engine);
        assert_eq!(fs::read(&location).unwrap(), preserved);
        let mut engine = Engine::open(&env, &location).unwrap();
        assert_eq!(engine.reconcile(&CancelToken::new()).unwrap().applied, 1);
        assert_eq!(engine.undo(&CancelToken::new()).unwrap().undone, 1);
        assert!(source.exists()); assert!(!target.exists());
    }
}

#[test]
fn aliased_or_nested_known_folder_roots_refuse_before_approval_or_store() {
    for nested in [false, true] {
        let (_dir, mut env) = fixture(); put(&env, "a.txt");
        env.known.desktop = if nested { let child = env.known.downloads.join("NestedDesktop"); fs::create_dir(&child).unwrap(); child }
            else { env.known.downloads.clone() };
        assert!(matches!(Inventory::capture(&env, &CancelToken::new()), Err(Error::Unsupported)));
        assert!(!journal(&env).exists()); assert_eq!(fs::read_dir(&env.app_data).unwrap().count(), 0);
    }
    let (_dir, env) = fixture(); let source = put(&env, "a.txt");
    let mut plan = approved(&env, &["a.txt"], &[]).data;
    plan.roots[1].chain = plan.roots[0].chain.clone();
    let location = journal(&env);
    let mut store = Store::open(&location, true, &journal_parent(&env, &location).unwrap()).unwrap();
    store.append(&Event::Approved { version: 1, plan }).unwrap(); drop(store);
    assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)));
    assert!(source.exists());
}

#[test]
fn correctly_framed_unreachable_not_attempted_transitions_refuse() {
    for scenario in 0..5 {
        let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
        drop(Engine::begin(&env, &location, approved(&env, &["a.txt"], &["Sorted"])).unwrap());
        let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
        store.append(&Event::Started).unwrap();
        let operation = match scenario { 0 => Operation::Undo, 1 => Operation::RemoveDirectory, 2 | 4 => Operation::Move, _ => Operation::Mkdir };
        let event = Event::NotAttempted { operation, index: 0, reason: "semantic regression".into() };
        if scenario == 2 || scenario == 3 { store.append(&Event::Finished { cancelled: true }).unwrap(); }
        if scenario == 4 { store.append(&event).unwrap(); }
        store.append(&event).unwrap(); drop(store);
        assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)), "scenario {scenario}");
        assert!(source.exists()); assert!(!env.known.documents.join("Sorted").exists());
    }
}

#[test]
fn move_and_nested_mkdir_intents_require_owned_manifest_parents() {
    for skipped_parent in [false, true] {
        let (_dir, env) = fixture(); let source = put(&env, "a.txt"); let location = journal(&env);
        let plan = approved(&env, &["a.txt"], &["Sorted"]); let file_id = plan.data.moves[0].fingerprint.id;
        drop(Engine::begin(&env, &location, plan).unwrap());
        let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
        store.append(&Event::Started).unwrap();
        let skipped = Event::NotAttempted { operation: Operation::Mkdir, index: 0, reason: "parent not created".into() };
        if skipped_parent { store.append(&skipped).unwrap(); }
        store.append(&Event::Intent { operation: Operation::Move, index: 0, identity: Some(file_id) }).unwrap();
        store.append(&Event::Applied { operation: Operation::Move, index: 0, identity: file_id }).unwrap();
        if !skipped_parent { store.append(&skipped).unwrap(); }
        store.append(&Event::Finished { cancelled: false }).unwrap(); drop(store);
        assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)));
        assert!(source.exists()); assert!(!env.known.documents.join("Sorted").exists());
    }
    for skipped_parent in [false, true] {
        let (_dir, env) = fixture(); put(&env, "a.txt"); let location = journal(&env);
        drop(Engine::begin(&env, &location, approved(&env, &["a.txt"], &["Outer", "Inner"])).unwrap());
        let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
        store.append(&Event::Started).unwrap();
        if skipped_parent { store.append(&Event::NotAttempted { operation: Operation::Mkdir, index: 0, reason: "no parent".into() }).unwrap(); }
        store.append(&Event::Intent { operation: Operation::Mkdir, index: 1, identity: None }).unwrap(); drop(store);
        assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)));
        assert!(!env.known.documents.join("Outer").exists());
    }
}

#[test]
fn rollback_phase_never_returns_to_forward_moves_mkdirs_or_finish() {
    for scenario in 0..4 {
        let (_dir, env) = fixture(); put(&env, "a.txt"); put(&env, "b.txt"); let location = journal(&env);
        let plan = if scenario == 3 {
            let inventory = Inventory::capture(&env, &CancelToken::new()).unwrap();
            let drafts = [("a.txt", "AOwned"), ("b.txt", "ZFuture")].into_iter().map(|(name, dir)| Draft {
                entry_id: inventory.entries().iter().find(|entry| entry.name == name).unwrap().id.clone(),
                destination: Folder::Documents, directories: vec![dir.into()], filename: None,
            }).collect::<Vec<_>>();
            inventory.validate(&drafts, &CancelToken::new()).unwrap()
        } else { approved(&env, &["a.txt", "b.txt"], &[]) };
        let mut engine = Engine::begin(&env, &location, plan).unwrap();
        engine.record(Event::Started).unwrap();
        if scenario == 3 {
            engine.record(Event::Intent { operation: Operation::Mkdir, index: 0, identity: None }).unwrap();
            let parent = native::directory(&env.known.documents).unwrap();
            let (dir, step) = native::mkdir(&parent.dir, &Name::text("AOwned").unwrap()).unwrap();
            engine.record(Event::Applied { operation: Operation::Mkdir, index: 0, identity: native::Id::step(&step) }).unwrap(); drop(dir); drop(parent);
        } else { engine.move_one(0, false).unwrap(); }
        let second_identity = engine.state.plan.moves[1].fingerprint.id;
        engine.undo(&CancelToken::new()).unwrap(); drop(engine);
        let bad = match scenario {
            0 => Event::Intent { operation: Operation::Move, index: 1, identity: Some(second_identity) },
            1 => Event::NotAttempted { operation: Operation::Move, index: 1, reason: "forward after reverse".into() },
            2 => Event::Finished { cancelled: true },
            _ => Event::Intent { operation: Operation::Mkdir, index: 1, identity: None },
        };
        let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
        store.append(&bad).unwrap(); drop(store);
        assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)), "scenario {scenario}");
        assert!(env.known.downloads.join("a.txt").exists()); assert!(env.known.downloads.join("b.txt").exists());
        assert!(!env.known.documents.join("ZFuture").exists());
    }
}

#[test]
fn removed_owned_parent_cannot_authorize_a_later_undo_intent() {
    let (_dir, env) = fixture(); put(&env, "a.txt"); let location = journal(&env);
    let mut engine = Engine::begin(&env, &location, approved(&env, &["a.txt"], &["Sorted"])).unwrap();
    engine.run(&CancelToken::new()).unwrap();
    let identity = engine.state.plan.moves[0].fingerprint.id;
    fs::rename(env.known.documents.join("Sorted/a.txt"), env.known.desktop.join("user-moved.txt")).unwrap();
    engine.undo(&CancelToken::new()).unwrap(); assert!(!env.known.documents.join("Sorted").exists()); drop(engine);
    let mut store = Store::open(&location, false, &journal_parent(&env, &location).unwrap()).unwrap();
    store.append(&Event::Intent { operation: Operation::Undo, index: 0, identity: Some(identity) }).unwrap(); drop(store);
    assert!(matches!(Engine::open(&env, &location), Err(Error::CorruptJournal)));
    assert!(env.known.desktop.join("user-moved.txt").exists()); assert!(!env.known.downloads.join("a.txt").exists());
}

#[test]
fn unused_or_structurally_impossible_directory_authority_refuses_before_store_creation() {
    for scenario in 0..3 {
        let (_dir, env) = fixture(); let source = put(&env, "a.txt");
        let mut plan = approved(&env, &["a.txt"], &["Outer", "Inner"]);
        match scenario {
            0 => plan.data.directories.push(Directory { at: Place { folder: Folder::Documents, names: vec![Name::text("Unreviewed").unwrap()] }, expected: None }),
            1 => { plan.data.directories.remove(0); },
            _ => { plan.data.directories[1].expected = Some(plan.data.root); },
        }
        assert!(matches!(Engine::begin(&env, &journal(&env), plan), Err(Error::CorruptJournal)));
        assert_eq!(fs::read_dir(&env.app_data).unwrap().count(), 0);
        assert!(!env.known.documents.join("Outer").exists()); assert!(!env.known.documents.join("Unreviewed").exists());
        assert!(source.exists());
    }
}

// Opt-in native seam proof; import after frozen6, execute only via helper with fresh D TEMP/TMP.
#[test]
#[ignore = "requires explicit fresh second-volume fixture admission; never a drive or home plan"]
fn cross_volume_native_rename_refuses_without_copy_delete_or_overwrite() {
    use std::io::Write;
    let cross_base = PathBuf::from(std::env::var_os("VACANCY_T08_CROSS_BASE").expect("explicit C fixture base required"));
    assert_eq!(cross_base, PathBuf::from(r"C:\tmp"));
    let real_home = PathBuf::from(std::env::var_os("USERPROFILE").expect("real home required to deny it"));
    let base_pin = native::directory(&cross_base).expect("second fixture base must already be an ordinary no-follow directory");
    let cross_name = Name::text(&format!("vacancy-t08-cross-{}", native::nonce().unwrap())).unwrap();
    let cross = cross_base.join(cross_name.os());
    assert!(!cross.exists()); // evidence/control only; FILE_CREATE below is the actual no-replace gate
    let (destination, destination_step) = native::mkdir(&base_pin.dir, &cross_name).unwrap();
    crate::fixture::check_root(&cross, &real_home).unwrap();
    assert_eq!(fs::read_dir(&cross).unwrap().count(), 0);
    let (mut sentinel, _) = native::journal_file(&destination, &Name::text("sentinel.txt").unwrap(), native::JournalOpen::Create).unwrap();
    sentinel.write_all(b"second volume sentinel").unwrap(); sentinel.sync_all().unwrap(); drop(sentinel);
    let (dir, env) = fixture(); let source = put(&env, "a.txt");
    crate::fixture::check_root(dir.path(), &real_home).unwrap();
    let before_source = crate::fixture::inventory(dir.path()).unwrap();
    let before_destination = crate::fixture::inventory(&cross).unwrap();
    let source_parent = native::directory(&env.known.downloads).unwrap();
    let (source_file, fingerprint) = native::file(&source_parent.dir, &Name::text("a.txt").unwrap(), true).unwrap();
    let destination_identity = native::Id::step(&destination_step);
    if fingerprint.id.volume == destination_identity.volume {
        let kept = dir.keep();
        panic!("cross-volume proof UNAVAILABLE: native volume identities are equal; fixtures retained at {} and {}", kept.display(), cross.display());
    }
    assert!(matches!(native::rename(&source_file, &destination, &Name::text("a.txt").unwrap()), Err(Fail::OtherVolume)));
    drop(source_file); drop(source_parent);
    let after_source = crate::fixture::inventory(dir.path()).unwrap();
    let after_destination = crate::fixture::inventory(&cross).unwrap();
    assert_eq!(after_source, before_source); assert_eq!(after_destination, before_destination);
    assert!(source.exists()); assert!(!cross.join("a.txt").exists());
    let kept = dir.keep();
    println!("T08_CROSS_VOLUME_RECEIPT {}", serde_json::json!({
        "sourceRoot": kept, "destinationRoot": cross,
        "sourceVolume": fingerprint.id.volume, "destinationVolume": destination_identity.volume,
        "sourceBefore": before_source, "sourceAfter": after_source,
        "destinationBefore": before_destination, "destinationAfter": after_destination,
        "renameOutcome": "OtherVolume", "userDataMutations": 0,
        "fixturePreparationOnly": true, "retainedForAudit": true
    }));
}

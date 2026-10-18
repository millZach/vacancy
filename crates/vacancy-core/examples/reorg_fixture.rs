//! PREPARED ONLY: not executed, compiled, or approved for admission.
//! Root may copy this to examples/reorg_fixture.rs only after final source review.
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("Unsupported: this fixture proof is Windows-only");
    std::process::ExitCode::FAILURE
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match proof::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAILED; preserve partial evidence; do not retry these directories: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
mod proof {
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    use std::path::{Component, Path, PathBuf, Prefix};
    use std::time::Instant;
    use vacancy_core::{
        CancelToken, fixture,
        reorg::{Draft, Engine, Folder, Inventory, Status},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    const DEST: &str = "T08-reviewed-moves";
    const JOURNAL: &str = "t08-core-proof.vjr";
    const NAMES: [&str; 4] = [
        "t08-alpha.txt",
        "t08-Résumé.txt",
        "t08-changed.txt",
        "t08-held.txt",
    ];
    const PEER: &str = "t08-alpha-peer.txt";
    const CHANGED: &[u8] = b"intentional edit after review; this content must survive Undo\n";

    fn require(ok: bool, message: &str) -> Result<()> {
        if !ok {
            return Err(io::Error::other(message).into());
        }
        Ok(())
    }
    fn write_new(path: &Path, data: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(data)?;
        file.sync_all()?;
        Ok(())
    }
    fn save(out: &Path, name: &str, data: &Value) -> Result<()> {
        write_new(&out.join(name), &serde_json::to_vec_pretty(data)?)
    }
    fn relative(root: &Path, path: &Path) -> Result<String> {
        Ok(path
            .strip_prefix(root)?
            .components()
            .map(|c| {
                c.as_os_str()
                    .to_str()
                    .ok_or_else(|| io::Error::other("non-Unicode evidence path"))
            })
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("/"))
    }
    fn units(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().collect()
    }
    fn reviewed(path: &Path) -> Result<String> {
        Ok(path
            .to_str()
            .ok_or_else(|| io::Error::other("non-Unicode path"))?
            .replace('\\', "\\\\"))
    }
    fn conservative_spelling(path: &Path) -> Result<()> {
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::other("ASCII proof directory required"))?;
        let bytes = text.as_bytes();
        // build_with inherits cmd /C mklink: admit no shell syntax, whitespace, Unicode or alternate prefixes.
        require(
            bytes.len() > 3
                && bytes.len() < 2048
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
                && bytes[3..].split(|b| *b == b'\\').all(|part| {
                    !part.is_empty()
                        && part
                            .iter()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'-'))
                }),
            "proof directories require exact drive-letter:\\ASCII_alphanumeric-hyphen_components; no shell syntax",
        )
    }
    fn admitted(path: &Path, home: &Path) -> Result<PathBuf> {
        // Validate exact input before even the read-only admission walk, and again after normalization.
        conservative_spelling(path)?;
        require(path.is_absolute(), "absolute directory required")?;
        require(
            path.components()
                .all(|c| !matches!(c, Component::CurDir | Component::ParentDir)),
            "dot component",
        )?;
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::other("Unicode directory required"))?;
        require(
            text.len() < 2048 && !text.chars().any(char::is_control),
            "directory text bound/control character",
        )?;
        require(
            matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_))),
            "ordinary local drive path required",
        )?;
        // Existing directories only. This also refuses home/ancestors, mounted roots and linked ancestors.
        fixture::check_root(path, home)?;
        let canonical = fs::canonicalize(path)?;
        // Windows canonicalize uses a verbatim prefix; the engine admits only ordinary Disk paths.
        // Rebuild components losslessly, then re-run identity/link/home admission on that spelling.
        let mut parts = canonical.components();
        let drive = match parts.next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::VerbatimDisk(drive) | Prefix::Disk(drive) => drive,
                _ => return Err(io::Error::other("canonical path is not local drive").into()),
            },
            _ => return Err(io::Error::other("canonical path lacks drive").into()),
        };
        require(
            parts.next() == Some(Component::RootDir),
            "canonical path lacks root",
        )?;
        let mut normalized = PathBuf::from(format!("{}:\\", char::from(drive)));
        for part in parts {
            require(
                matches!(part, Component::Normal(_)),
                "ambiguous canonical component",
            )?;
            normalized.push(part.as_os_str());
        }
        conservative_spelling(&normalized)?;
        fixture::check_root(&normalized, home)?;
        require(
            fs::read_dir(&normalized)?.next().is_none(),
            "directory must be empty; never reuse",
        )?;
        Ok(normalized)
    }
    fn id(path: &Path) -> Result<String> {
        require(
            fs::symlink_metadata(path)?.is_file(),
            "identity requires ordinary regular file",
        )?;
        let file = OpenOptions::new()
            .access_mode(0x80)
            .share_mode(7)
            .custom_flags(0x0020_0000)
            .open(path)?; // attributes only; OPEN_REPARSE_POINT
        // SAFETY: FILE_ID_INFO is plain output storage; handle is owned/live and buffer has its stated size.
        let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: live read-only handle and writable FILE_ID_INFO buffer, with the correct class/size.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(format!(
            "{:016x}:{}",
            info.VolumeSerialNumber,
            info.FileId
                .Identifier
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
    }
    fn manifest(root: &Path, out: &Path, name: &str) -> Result<BTreeMap<String, String>> {
        let lines = fixture::inventory(root)?;
        require(lines.len() < 2048, "fixture entry bound")?;
        let mut map = BTreeMap::new();
        for line in &lines {
            require(line.len() < 16384, "manifest line bound")?;
            let (path, what) = line
                .split_once('\t')
                .ok_or_else(|| io::Error::other("bad fixture inventory"))?;
            require(
                !path.is_empty()
                    && !what.contains(['\t', '\r', '\n'])
                    && !path.contains(['\r', '\n']),
                "unsafe TSV",
            )?;
            require(
                what == "dir"
                    || what.starts_with("link -> ")
                    || (what.starts_with("file ") && !what.contains("unread")),
                "unread or unknown manifest entry",
            )?;
            require(
                map.insert(path.to_owned(), what.to_owned()).is_none(),
                "duplicate inventory path",
            )?;
        }
        write_new(&out.join(name), (lines.join("\n") + "\n").as_bytes())?;
        Ok(map)
    }
    fn status(value: &Status) -> Value {
        json!({"run_id":value.run_id, "applied":value.applied, "undone":value.undone,
            "skipped":value.skipped,"unresolved":value.unresolved,"cancelled":value.cancelled,
            "finished":value.finished,"reasons":value.reasons})
    }
    fn counts(s: &Status, undone: u32) -> Result<()> {
        require(
            s.applied == 2
                && s.undone == undone
                && s.skipped == 2
                && s.unresolved == 0
                && s.finished
                && !s.cancelled,
            "unexpected engine counts/state",
        )?;
        // Both native source-open sharing refusal and fingerprint mismatch are public Error::Changed.
        require(
            s.reasons == vec!["a file, parent or volume identity changed"; 2],
            "unexpected skip reasons",
        )
    }
    fn metadata(path: &str, journal: &str) -> bool {
        path == journal
            || path == format!("{journal}.lease")
            || (1..32).any(|i| path == format!("{journal}.part{i:04}"))
    }
    fn data(map: &BTreeMap<String, String>, journal: &str) -> BTreeMap<String, String> {
        map.iter()
            .filter(|(p, _)| !metadata(p, journal))
            .map(|(p, w)| (p.clone(), w.clone()))
            .collect()
    }
    fn identities(root: &Path, paths: &[PathBuf]) -> Result<Value> {
        let mut result = serde_json::Map::new();
        for path in paths {
            result.insert(relative(root, path)?, json!(id(path)?));
        }
        Ok(Value::Object(result))
    }
    fn observe(
        timings: &mut serde_json::Map<String, Value>,
        name: &str,
        started: Instant,
        flushes_before: Option<u64>,
        flushes_after: Option<u64>,
        result_status: Option<&Status>,
    ) -> Result<()> {
        // Elapsed time is observational only; no timing threshold or durability inference.
        let elapsed_micros = u64::try_from(started.elapsed().as_micros())?;
        require(
            timings
                .insert(
                    name.into(),
                    json!({"calls":1,"elapsed_micros":elapsed_micros,
            "journal_flushes_before":flushes_before,"journal_flushes_after":flushes_after,
            "result_status":result_status.map(status)}),
                )
                .is_none(),
            "duplicate operation observation",
        )
    }

    pub fn run() -> Result<()> {
        let args = std::env::args_os().skip(1).take(4).collect::<Vec<_>>();
        require(
            args.len() == 3 && args[0] == "--run",
            "usage: reorg_fixture --run <existing EMPTY profile> <existing EMPTY evidence>",
        )?;
        let requested = PathBuf::from(&args[1]);
        // Exact OsString equality, before normalization: never silently substitute another profile.
        require(
            std::env::var_os(fixture::ROOT_VAR).as_deref() == Some(requested.as_os_str()),
            "VACANCY_FIXTURE_ROOT must exactly match profile argument",
        )?;
        let home = std::env::home_dir().ok_or_else(|| io::Error::other("real home unknown"))?;
        let root = admitted(&requested, &home)?;
        let out = admitted(Path::new(&args[2]), &home)?;
        require(
            !root.starts_with(&out) && !out.starts_with(&root),
            "profile/evidence overlap",
        )?;
        // These were independently validated and still must be empty at first mutation.
        require(!fixture::is_profile(&root), "previous profile refused")?;
        require(
            fs::read_dir(&root)?.next().is_none() && fs::read_dir(&out)?.next().is_none(),
            "admission changed",
        )?;
        let profile = fixture::build_with(&root, 0)?;
        require(fixture::is_profile(&root), "generator marker missing")?;
        let marker = fs::symlink_metadata(root.join(fixture::MARKER))?;
        require(
            marker.is_file() && !marker.file_type().is_symlink(),
            "ordinary generated marker required",
        )?;
        let env = fixture::env(&root);
        require(
            env.confined_to.as_deref() == Some(root.as_path())
                && env.root == root
                && env.home == profile.home,
            "fixture confinement mismatch",
        )?;
        require(
            fixture::app_data(&root) == env.app_data.join("Vacancy"),
            "fixture app data mismatch",
        )?;
        // Missing known folders are created once, inside this newly generated profile, before the baseline.
        fs::create_dir(&env.known.desktop)?;
        fs::create_dir(&env.app_data)?;
        let seeds = NAMES.map(|name| env.known.downloads.join(name));
        for (i, path) in seeds.iter().enumerate() {
            write_new(
                path,
                format!("synthetic t08 seed {i}; no real user content\n").as_bytes(),
            )?;
        }
        let peer = env.known.desktop.join(PEER);
        fs::hard_link(&seeds[0], &peer)?;
        require(id(&seeds[0])? == id(&peer)?, "hardlink setup failed")?;
        require(
            fs::symlink_metadata(&profile.link_to_documents)?
                .file_type()
                .is_symlink(),
            "fixture junction missing",
        )?;
        let destination = env.known.documents.join(DEST);
        require(
            !destination.try_exists()?,
            "destination must be new; no overwrites",
        )?;
        let journal = env.app_data.join(JOURNAL);
        let journal_rel = relative(&root, &journal)?;
        let cancel = CancelToken::default();
        let mut timings = serde_json::Map::new();
        let started = Instant::now();
        let inventory = Inventory::capture(&env, &cancel)?;
        observe(&mut timings, "inventory", started, None, None, None)?;
        require(!inventory.truncated(), "truncated scan")?;
        save(&out, "scan-inventory.json", &json!(inventory.entries().iter().map(|e| json!({
            "id":e.id,"folder":format!("{:?}",e.folder),"name":e.name,"logical_bytes":e.logical_bytes,
            "allocated_bytes":e.allocated_bytes,"changed_ns":e.changed_ns.to_string()})).collect::<Vec<_>>()))?;
        let original = manifest(&root, &out, "after-scan.tsv")?;
        let drafts = NAMES
            .iter()
            .map(|name| {
                let candidates = inventory
                    .entries()
                    .iter()
                    .filter(|e| e.folder == Folder::Downloads && e.name == *name)
                    .collect::<Vec<_>>();
                require(candidates.len() == 1, "seed missing/ambiguous in scan")?;
                Ok(Draft {
                    entry_id: candidates[0].id.clone(),
                    destination: Folder::Documents,
                    directories: vec![DEST.into()],
                    filename: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let started = Instant::now();
        let plan = inventory.validate(&drafts, &cancel)?;
        observe(&mut timings, "validate", started, None, None, None)?;
        let reviewed_moves = plan.review().len();
        require(reviewed_moves == 4, "review must select exactly four seeds")?;
        let mut expected = Vec::new();
        for (i, mv) in plan.review().iter().enumerate() {
            let to = destination.join(NAMES[i]);
            require(
                mv.entry_id == drafts[i].entry_id
                    && mv.from == reviewed(&seeds[i])?
                    && mv.to == reviewed(&to)?,
                "reviewed native names mismatch",
            )?;
            expected.push(json!({"entry_id":mv.entry_id,"from":relative(&root,&seeds[i])?,"to":relative(&root,&to)?,
                "native_from_utf16":units(seeds[i].strip_prefix(&root)?),"native_to_utf16":units(to.strip_prefix(&root)?),
                "outcome":if i < 2 {"applied"} else {"skipped"}}));
        }
        save(&out, "expected-moves.json", &json!(expected))?;
        save(
            &out,
            "reviewed-plan.json",
            &json!(
                plan.review()
                    .iter()
                    .map(|mv| json!({
            "entry_id":mv.entry_id,"from":mv.from,"to":mv.to}))
                    .collect::<Vec<_>>()
            ),
        )?;
        // Only deliberate post-review mutation, before begin/run. Never rewrite any other seed.
        let mut changed = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&seeds[2])?;
        changed.write_all(CHANGED)?;
        changed.sync_all()?;
        drop(changed);
        // Genuine reader allows content/attribute reads but denies DELETE sharing; native exclusive open refuses it.
        let mut held: File = OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x0020_0000)
            .open(&seeds[3])?;
        let mut first_byte = [0];
        held.read_exact(&mut first_byte)?;
        require(
            first_byte == [b's'],
            "held reader did not read expected synthetic seed",
        )?;
        let before = manifest(&root, &out, "before-run.tsv")?;
        let mut deliberate = original.clone();
        let changed_rel = relative(&root, &seeds[2])?;
        require(
            original.get(&changed_rel) != before.get(&changed_rel),
            "deliberate change absent",
        )?;
        deliberate.insert(changed_rel, before[&relative(&root, &seeds[2])?].clone());
        require(deliberate == before, "unexpected pre-run mutation")?;
        let mut before_paths = seeds.to_vec();
        before_paths.push(peer.clone());
        let before_ids = identities(&root, &before_paths)?;
        let started = Instant::now();
        let mut engine = Engine::begin(&env, &journal, plan)?;
        observe(
            &mut timings,
            "begin",
            started,
            None,
            Some(engine.durability_flushes()),
            Some(&engine.status()),
        )?;
        let flushes_before = engine.durability_flushes();
        let started = Instant::now();
        let ran = engine.run(&cancel)?;
        observe(
            &mut timings,
            "run",
            started,
            Some(flushes_before),
            Some(engine.durability_flushes()),
            Some(&ran),
        )?;
        counts(&ran, 0)?;
        require(engine.status() == ran, "run status mismatch")?;
        drop(engine); // No journal reads or listing while the exclusive native store handles live.
        save(&out, "run-status.json", &status(&ran))?;
        let after = manifest(&root, &out, "after-run.tsv")?;
        let after_paths = [
            destination.join(NAMES[0]),
            destination.join(NAMES[1]),
            seeds[2].clone(),
            seeds[3].clone(),
            peer.clone(),
        ];
        let after_ids = identities(&root, &after_paths)?;
        for i in 0..4 {
            require(
                after_ids[&relative(&root, &after_paths[i])?]
                    == before_ids[&relative(&root, &seeds[i])?],
                "moved or skipped seed native identity changed",
            )?;
        }
        let mut expected_after = before.clone();
        expected_after.insert(relative(&root, &destination)?, "dir".into());
        for i in 0..2 {
            let from = relative(&root, &seeds[i])?;
            let value = expected_after
                .remove(&from)
                .ok_or_else(|| io::Error::other("missing move source"))?;
            require(
                expected_after
                    .insert(relative(&root, &destination.join(NAMES[i]))?, value)
                    .is_none(),
                "overwrite",
            )?;
            require(
                id(&destination.join(NAMES[i]))? == before_ids[&from].as_str().unwrap_or(""),
                "native identity changed during move",
            )?;
        }
        require(
            data(&after, &journal_rel) == expected_after,
            "unexpected run namespace/content change",
        )?;
        require(
            id(&destination.join(NAMES[0]))? == id(&peer)?,
            "hardlink relation lost",
        )?;
        drop(held);
        let started = Instant::now();
        let mut engine = Engine::open(&env, &journal)?;
        observe(
            &mut timings,
            "open_reconcile",
            started,
            None,
            Some(engine.durability_flushes()),
            Some(&engine.status()),
        )?;
        require(engine.status() == ran, "reopen changed status")?;
        let flushes_before = engine.durability_flushes();
        let started = Instant::now();
        let reconciled = engine.reconcile(&cancel)?;
        observe(
            &mut timings,
            "reconcile",
            started,
            Some(flushes_before),
            Some(engine.durability_flushes()),
            Some(&reconciled),
        )?;
        require(reconciled == ran, "reconciliation changed completed run")?;
        drop(engine);
        save(&out, "reconcile-status.json", &status(&reconciled))?;
        let reopened = manifest(&root, &out, "after-reconcile.tsv")?;
        require(
            reopened == after,
            "reopen/reconcile performed unapproved work",
        )?;
        let reconcile_ids = identities(&root, &after_paths)?;
        require(
            reconcile_ids == after_ids,
            "reopen/reconcile changed native identities",
        )?;
        let started = Instant::now();
        let mut engine = Engine::open(&env, &journal)?;
        observe(
            &mut timings,
            "open_undo",
            started,
            None,
            Some(engine.durability_flushes()),
            Some(&engine.status()),
        )?;
        let flushes_before = engine.durability_flushes();
        let started = Instant::now();
        let undone = engine.undo(&cancel)?;
        observe(
            &mut timings,
            "undo",
            started,
            Some(flushes_before),
            Some(engine.durability_flushes()),
            Some(&undone),
        )?;
        counts(&undone, 2)?;
        require(
            undone.run_id == ran.run_id,
            "Undo journal identity mismatch",
        )?;
        drop(engine);
        save(&out, "undo-status.json", &status(&undone))?;
        let restored = manifest(&root, &out, "after-undo.tsv")?;
        require(
            data(&restored, &journal_rel) == before,
            "Undo did not restore data names/content",
        )?;
        let undo_ids = identities(&root, &before_paths)?;
        require(
            before_ids == undo_ids && id(&seeds[0])? == id(&peer)?,
            "Undo identities/hardlink mismatch",
        )?;
        require(
            fs::read(&seeds[2])? == CHANGED,
            "Undo lost intentional edit",
        )?;
        let artifacts = restored
            .keys()
            .filter(|p| metadata(p, &journal_rel))
            .cloned()
            .collect::<BTreeSet<_>>();
        require(
            artifacts.contains(&journal_rel) && artifacts.contains(&format!("{journal_rel}.lease")),
            "persistent journal/lease missing",
        )?;
        // Completion appears once, only after all local assertions. External audit remains separate.
        save(
            &out,
            "completion.json",
            &json!({"schema":1,"scope":"native-windows-core-fixture",
            "profile":root.to_str(),"evidence":out.to_str(),"run_id":ran.run_id,"journal_relative":journal_rel,
            "data_directories":[relative(&root,&destination)?],"deleted_files":0,"recovered_bytes":0,
            "run":status(&ran),"reconcile":status(&reconciled),"undo":status(&undone),
            "before_ids":before_ids,"after_ids":after_ids,"reconcile_ids":reconcile_ids,"undo_ids":undo_ids,
            "timings":timings,"inventory_entries":inventory.entries().len(),"reviewed_moves":reviewed_moves,
            "journal_artifacts":artifacts,"run_calls":1,"undo_calls":1}),
        )?;
        println!(
            "Core fixture assertions completed; retain all data and run the independent saved-evidence audit."
        );
        Ok(())
    }
}

//! Fixture mode and fixture profiles: how Vacancy is proven against a fake profile, never a real one.
//!
//! **Fixture mode** runs the app against a folder instead of a drive: the drive list holds one pseudo-drive whose
//! root is the folder, the places the rules and the Guard use (home, app data, temp) are laid out inside it
//! ([`env`]), and the Guard denies everything outside it by identity ([`Env::confined_to`]). It only ever narrows
//! what Vacancy can touch. [`check_root`] refuses a folder that could widen it: one that isn't an existing folder,
//! is reached through a link, is a drive's root, or is in, or holds, the real home folder, and the desktop app also
//! wants the [`MARKER`] file that [`build`] writes, so the mode only ever runs on a folder the generator made. The
//! app enters the mode when `VACANCY_FIXTURE_ROOT` names such a folder (see AGENTS.md, "Prove an app change").
//!
//! **A fixture profile** ([`build`]) is the fake home those proofs and the executor's tests run against: caches,
//! temp files, an idle project whose `node_modules` is laid out the way pnpm lays it out (links into `.pnpm`, a
//! hard link into a package store outside it), a link inside a cache pointing at the profile's `Documents`, a
//! read-only file, a file for a test to hold open, a target for a test to modify after the scan, a path longer
//! than 260 characters, and a `Documents` folder that nothing may touch. It also holds known duplicate files for
//! the duplicate finder (see [`DuplicateCases`]). `cargo run -p vacancy-core --example fixture -- <folder>` writes
//! one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::categorize::Env;
use crate::fsid;
use crate::snapshot::Snapshot;
use crate::volume::{DriveKind, VolumeInfo};

/// The fixture profile's user name: its home is `<root>/Users/fixture`.
pub const USER: &str = "fixture";

/// The environment variable that puts the desktop app in fixture mode.
pub const ROOT_VAR: &str = "VACANCY_FIXTURE_ROOT";

/// The file [`build`] writes at a profile's root. Fixture mode runs only on a folder that has it: a folder the
/// generator made holds nothing real, so the fixture's own, narrower places protect all there is.
pub const MARKER: &str = "vacancy-fixture-profile.txt";

/// Whether `root` is a fixture profile [`build`] made: it has the [`MARKER`] file, a file and not a link.
pub fn is_profile(root: &Path) -> bool {
    fs::symlink_metadata(root.join(MARKER)).is_ok_and(|m| m.is_file())
}

/// Why a folder can't be a fixture root.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FixtureError {
    #[error("{0} is not an absolute path")]
    Relative(PathBuf),
    #[error("{0} is not an existing folder")]
    NotAFolder(PathBuf),
    #[error("{0} is reached through a link, or can't be opened")]
    Linked(PathBuf),
    #[error("{0} is a drive's root")]
    DriveRoot(PathBuf),
    #[error("{0} is the root of a mounted volume")]
    MountRoot(PathBuf),
    #[error("your home folder can't be found, so it can't be kept out")]
    HomeUnknown,
    #[error("{0} is in, or holds, your home folder")]
    RealHome(PathBuf),
    #[error("{0} is not empty")]
    NotEmpty(PathBuf),
}

/// Checks that `root` may be a fixture root, given the real home folder `home`: an existing folder, every folder
/// on the way to it a real folder (no link, junction or mount point), not a drive's root, and neither in nor
/// holding `home`, by identity. Gives the root as given.
pub fn check_root(root: &Path, home: &Path) -> Result<PathBuf, FixtureError> {
    if !root.is_absolute() {
        return Err(FixtureError::Relative(root.to_path_buf()));
    }
    let meta =
        fs::symlink_metadata(root).map_err(|_| FixtureError::NotAFolder(root.to_path_buf()))?;
    if !meta.is_dir() {
        return Err(FixtureError::NotAFolder(root.to_path_buf()));
    }
    let chain = fsid::chain(root).ok_or_else(|| FixtureError::Linked(root.to_path_buf()))?;
    if chain.len() < 2 {
        return Err(FixtureError::DriveRoot(root.to_path_buf()));
    }
    // The root of a volume mounted in a folder (a disk image at /Volumes/Test, a drive in a folder): its device
    // or volume isn't its parent's, or the OS says a volume is mounted right there.
    let (parent, last) = (&chain[chain.len() - 2], &chain[chain.len() - 1]);
    if last.id.volume() != parent.id.volume() || is_mount_point(root) {
        return Err(FixtureError::MountRoot(root.to_path_buf()));
    }
    let root_id = last.id;
    // The real home as it really is: through whatever links lead to it, so no spelling of it slips by. A home
    // that can't be found can't be kept out, so then nothing is a fixture root.
    let home_chain = fsid::resolve(home)
        .or_else(|| fsid::chain(home))
        .ok_or(FixtureError::HomeUnknown)?;
    let home_id = home_chain.last().map(|s| s.id);
    let inside_home = chain.iter().any(|s| Some(s.id) == home_id);
    let holds_home = home_chain.iter().any(|s| s.id == root_id);
    if inside_home || holds_home {
        return Err(FixtureError::RealHome(root.to_path_buf()));
    }
    Ok(root.to_path_buf())
}

/// Whether a volume is mounted at `path` itself, as the OS says: on macOS the mount `statfs` names is `path`; on
/// Windows the volume path that holds it is `path` (a volume mounted in a folder, or a drive's root).
fn is_mount_point(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return true;
        };
        // SAFETY: an all-zero statfs is a valid value of a plain C struct.
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is NUL-terminated and `fs` is writable.
        if unsafe { libc::statfs(c.as_ptr(), &mut fs) } != 0 {
            return true;
        }
        let mount: Vec<u8> = fs
            .f_mntonname
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect();
        mount == path.as_os_str().as_bytes()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut out = vec![0u16; 32_768];
        // SAFETY: `wide` is NUL-terminated; `out` holds as many writable units as passed.
        if unsafe { GetVolumePathNameW(wide.as_ptr(), out.as_mut_ptr(), out.len() as u32) } == 0 {
            return true;
        }
        let len = out.iter().position(|&c| c == 0).unwrap_or(out.len());
        let volume = std::ffi::OsString::from_wide(&out[..len]);
        let trim = |p: &std::ffi::OsStr| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
        trim(&volume) == trim(path.as_os_str())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        false
    }
}

/// The places on the fixture root `root`, laid out the way this computer's OS lays out a drive, for the user
/// [`USER`], with the Guard confined to `root`.
pub fn env(root: &Path) -> Env {
    let mut env = if cfg!(windows) {
        Env::windows(root, USER)
    } else {
        Env::macos(root, USER)
    };
    env.confined_to = Some(root.to_path_buf());
    env
}

/// The fixture's app data folder, where the app keeps its run history in fixture mode.
pub fn app_data(root: &Path) -> PathBuf {
    env(root).app_data.join("Vacancy")
}

/// The pseudo-drive's figures in fixture mode: its used bytes are what the fixture holds (as `snapshot`, a scan of
/// it, counts them), its capacity is `capacity`, fixed when the app starts, and the rest is free.
pub fn volume(root: &Path, capacity: u64, snapshot: &Snapshot) -> VolumeInfo {
    let used = snapshot.stats().bytes.min(capacity);
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string());
    VolumeInfo {
        name: format!("Fixture ({name})"),
        mount_point: root.display().to_string(),
        id: "fixture".into(),
        kind: DriveKind::Internal,
        system: true,
        total_bytes: capacity,
        used_bytes: used,
        free_bytes: capacity - used,
    }
}

/// The pseudo-drive's capacity for a fixture holding `used` bytes: twice that, at least 64 MiB, in whole MiB. Only
/// so the map has room to show it; no file is written.
pub fn capacity(used: u64) -> u64 {
    const MIB: u64 = 1 << 20;
    (used.saturating_mul(2).max(64 * MIB)).div_ceil(MIB) * MIB
}

/// What a fixture profile holds, by role, for tests and proofs to point at.
#[derive(Clone, Debug)]
pub struct Profile {
    /// The fixture's root: the pseudo-drive.
    pub root: PathBuf,
    /// The profile's home folder.
    pub home: PathBuf,
    /// The folder that must survive whole, and the link to it inside a cache.
    pub documents: PathBuf,
    pub link_to_documents: PathBuf,
    /// A cache folder deleted whole, and a file in it for a test to hold open during the run.
    pub cache: PathBuf,
    pub held_open: PathBuf,
    /// A loose file: one of a folder's small files, which the plan takes as that folder's loose files.
    pub loose: PathBuf,
    /// A file in a target of its own, for a test to modify after the scan.
    pub modified_later: PathBuf,
    /// A read-only file inside an approved cache folder.
    pub read_only: PathBuf,
    /// A file whose path is longer than 260 characters.
    pub long_path: PathBuf,
    /// The idle project's `node_modules`, with pnpm's links and a hard link into the package store.
    pub node_modules: PathBuf,
    /// The package store file `node_modules` has a hard link to: it must survive.
    pub store_file: PathBuf,
    /// Things no rule may propose: a project in use, a fresh installer.
    pub untouched: Vec<PathBuf>,
    /// The duplicate finder's known cases.
    pub duplicates: DuplicateCases,
}

/// The duplicate files a profile holds, for the duplicate finder's tests and proofs (ticket 09).
#[derive(Clone, Debug, Default)]
pub struct DuplicateCases {
    /// Each group of identical files: the copy the keeper rule keeps, then the extra copies, in the order the
    /// rule ranks them.
    pub groups: Vec<(PathBuf, Vec<PathBuf>)>,
    /// The keeper of the group a proof unticks, to see that an unticked group keeps every copy.
    pub untick: PathBuf,
    /// The kept copy a test changes after the scan, to see that its group's extra copies then stay.
    pub change_kept: PathBuf,
    /// Files that look like copies but must never be proposed: same size with other contents, the same start
    /// with another end, a hard link pair, copies under 1 MiB, a copy in a cloud-sync folder, one in a Photos
    /// library, one only reachable through a link, and an installer another rule already deletes.
    pub not_duplicates: Vec<PathBuf>,
}

/// The profile's files, relative to its home, `/`-separated, with their sizes, and how old they are in days.
struct Spec {
    rel: String,
    len: usize,
    days: u64,
    /// What is in it: varied bytes of its own, or (for the duplicate cases) the same bytes as every file with
    /// the same seed, its last `tail` bytes flipped.
    seed: Option<(u64, usize)>,
}

/// How long [`build`] waits at the end, so that everything it wrote is older than a scan started after it by more
/// than the executor's margin for coarse timestamps.
const SETTLE: Duration = Duration::from_millis(3200);

/// Writes a fixture profile into `root` (made if missing; it must be empty) for this computer's OS. Every file and
/// folder gets an old modified time, so the rules find what is idle; then it waits a moment, so that a scan right
/// after sees nothing as changed since.
pub fn build(root: &Path) -> io::Result<Profile> {
    build_with(root, 0)
}

/// [`build`], with `bulk` more small files in the npm cache, spread over folders of a hundred: enough of them and a
/// run takes long enough to watch.
pub fn build_with(root: &Path, bulk: usize) -> io::Result<Profile> {
    build_options(root, bulk, false)
}

/// A fresh profile with deterministic native-layout Steam advice and, on macOS, Logic library instructions.
/// Existing profiles are refused, just as in [`build`]; this is never a seed operation on arbitrary user data.
pub fn build_with_advice(root: &Path, bulk: usize) -> io::Result<Profile> {
    build_options(root, bulk, true)
}

fn build_options(root: &Path, bulk: usize, advice: bool) -> io::Result<Profile> {
    fs::create_dir_all(root)?;
    if fs::read_dir(root)?.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            FixtureError::NotEmpty(root.to_path_buf()).to_string(),
        ));
    }
    fs::write(
        root.join(MARKER),
        "A fake profile Vacancy's tests and proofs run against. Nothing here is real.
",
    )?;
    let home = sub(root, &format!("Users/{USER}"));
    let windows = cfg!(windows);
    let mut specs: Vec<Spec> = Vec::new();
    let mut file = |rel: &str, len: usize, days: u64| {
        specs.push(Spec {
            rel: rel.to_owned(),
            len,
            days,
            seed: None,
        })
    };
    const OLD: u64 = 200;
    const KIB: usize = 1024;

    // What must survive.
    file("Documents/report.docx", 48 * KIB, OLD);
    file("Documents/notes.txt", 3 * KIB, OLD);
    file("Documents/photos/beach.jpg", 160 * KIB, OLD);
    file("Downloads/new-setup.exe", 90 * KIB, 0);
    file("code/live-app/package.json", KIB, 0);
    file("code/live-app/package-lock.json", 2 * KIB, 0);
    file("code/live-app/node_modules/kleur/index.js", 6 * KIB, 0);

    // The idle project, pnpm style.
    let pnpm = "code/old-app/node_modules/.pnpm";
    file("code/old-app/package.json", KIB, OLD);
    file("code/old-app/pnpm-lock.yaml", 4 * KIB, OLD);
    file("code/old-app/src/index.js", 2 * KIB, OLD);
    file("code/old-app/node_modules/.modules.yaml", KIB, OLD);
    file(
        &format!("{pnpm}/left-pad@1.3.0/node_modules/left-pad/index.js"),
        2 * KIB,
        OLD,
    );
    file(
        &format!("{pnpm}/left-pad@1.3.0/node_modules/left-pad/package.json"),
        KIB,
        OLD,
    );
    file(
        &format!("{pnpm}/lodash@4.17.21/node_modules/lodash/lodash.js"),
        180 * KIB,
        OLD,
    );
    file(
        &format!("{pnpm}/lodash@4.17.21/node_modules/lodash/package.json"),
        KIB,
        OLD,
    );

    // The two cache folders deleted whole, the npm cache, the package store, and the files only this OS has.
    type Layout<'a> = (
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        Vec<(&'a str, usize, u64)>,
    );
    let (cache_a, cache_b, npm, store, extra): Layout<'_> = if windows {
        (
            "AppData/Local/Google/Chrome/User Data/Default/Cache/Cache_Data",
            "AppData/Local/Google/Chrome/User Data/Profile 1/Cache/Cache_Data",
            "AppData/Local/npm-cache/_cacache",
            "AppData/Local/pnpm/store/v3/files/5e",
            vec![
                ("Downloads/old-setup.exe", 220 * KIB, 60),
                (
                    "AppData/Local/Temp/vacancy-old-install/setup.cab",
                    120 * KIB,
                    3,
                ),
                (
                    "AppData/Local/Temp/vacancy-old-install/setup.log",
                    4 * KIB,
                    3,
                ),
                ("AppData/Local/Temp/old-1.tmp", 24 * KIB, 3),
                ("AppData/Local/Temp/old-2.log", 9 * KIB, 3),
                ("AppData/Local/CrashDumps/app.exe.4410.dmp", 1536 * KIB, 20),
                ("AppData/Local/CrashDumps/tool.exe.112.dmp", 12 * KIB, 20),
                (
                    "AppData/Local/npm-cache/_update-notifier-last-checked",
                    1,
                    20,
                ),
            ],
        )
    } else {
        (
            "Library/Caches/com.example.fixtureapp/fsCachedData",
            "Library/Caches/com.example.otherapp/fsCachedData",
            ".npm/_cacache",
            "Library/pnpm/store/v3/files/5e",
            vec![
                ("Downloads/old-installer.dmg", 220 * KIB, 60),
                (
                    "Library/Caches/com.example.fixtureapp/Cache.db",
                    40 * KIB,
                    20,
                ),
                ("Library/Logs/fixtureapp/old.log", 30 * KIB, 20),
                (".Trash/old-archive.zip", 70 * KIB, 20),
            ],
        )
    };
    for (rel, len, days) in extra {
        file(rel, len, days);
    }
    for i in 1..=6 {
        file(&format!("{cache_a}/f_{i:06}"), 32 * KIB, 20);
    }
    file(&format!("{cache_a}/index"), 8 * KIB, 20);
    file(&format!("{cache_a}/held-open.bin"), 16 * KIB, 20);
    file(&format!("{cache_a}/read-only.bin"), 12 * KIB, 20);
    file(&format!("{cache_b}/f_000001"), 28 * KIB, 20);
    file(&format!("{cache_b}/modified-later.bin"), 20 * KIB, 20);
    for (i, hash) in ["ab/cd/0f1e", "ab/ef/77aa", "c3/01/9b2d"]
        .iter()
        .enumerate()
    {
        file(
            &format!("{npm}/content-v2/sha512/{hash}"),
            (40 + 10 * i) * KIB,
            20,
        );
    }
    file(
        &format!("{npm}/content-v2/read-only-folder/blob"),
        6 * KIB,
        20,
    );
    // A path longer than 260 characters, inside the npm cache.
    let deep: Vec<String> = (0..7)
        .map(|i| format!("{i:02}-index-bucket-with-a-long-name-to-pass-max-path"))
        .collect();
    let long_rel = format!("{npm}/index-v5/{}/entry.json", deep.join("/"));
    file(&long_rel, 2 * KIB, 20);
    file(&format!("{store}/store-file.js"), 64 * KIB, OLD);
    for i in 0..bulk {
        file(
            &format!("{npm}/content-v2/bulk/{:03}/{i:06}", i / 100),
            4 * KIB,
            20,
        );
    }

    let dups = duplicate_specs(windows);
    specs.extend(dups.specs);

    let now = SystemTime::now();
    for spec in &specs {
        let path = sub(&home, &spec.rel);
        match spec.seed {
            None => write_file(&path, spec.len)?,
            Some((seed, tail)) => write_seeded(&path, spec.len, seed, tail)?,
        }
    }

    // pnpm's links, the link into Documents, and the hard link into the store.
    let node_modules = sub(&home, "code/old-app/node_modules");
    link_dir(
        root,
        &node_modules.join("left-pad"),
        &sub(
            &home,
            &format!("{pnpm}/left-pad@1.3.0/node_modules/left-pad"),
        ),
    )?;
    link_dir(
        root,
        &node_modules.join("lodash"),
        &sub(&home, &format!("{pnpm}/lodash@4.17.21/node_modules/lodash")),
    )?;
    let documents = home.join("Documents");
    let link_to_documents = sub(&home, &format!("{npm}/content-v2/linked-docs"));
    link_dir(root, &link_to_documents, &documents)?;
    let store_file = sub(&home, &format!("{store}/store-file.js"));
    let store_link = sub(
        &home,
        &format!("{pnpm}/lodash@4.17.21/node_modules/lodash/store-linked.js"),
    );
    fs::hard_link(&store_file, &store_link)?;
    // The duplicate cases' hard link pair, and a link from Pictures to a folder holding another copy.
    let hard_b = sub(&home, dups.hard_link.1);
    fs::hard_link(sub(&home, dups.hard_link.0), &hard_b)?;
    link_dir(root, &sub(&home, dups.link.0), &sub(&home, dups.link.1))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        "../.pnpm/left-pad@1.3.0/node_modules/left-pad/index.js",
        sub(&home, "code/old-app/node_modules/.bin-left-pad.js"),
    )?;

    let read_only = sub(&home, &format!("{cache_a}/read-only.bin"));
    let ro_folder = sub(&home, &format!("{npm}/content-v2/read-only-folder"));

    // Old times on everything, deepest first, so that writing a child doesn't make its folder new again. Links
    // keep the time they were made, which is before any scan.
    // A folder is as old as the youngest thing in it.
    let mut dirs: std::collections::HashMap<PathBuf, u64> = std::collections::HashMap::new();
    for spec in &specs {
        let p = sub(&home, &spec.rel);
        set_age(&p, now, spec.days)?;
        let mut at = p.parent();
        while let Some(d) = at {
            if !d.starts_with(root) || d == root {
                break;
            }
            let days = dirs.entry(d.to_path_buf()).or_insert(spec.days);
            *days = (*days).min(spec.days);
            at = d.parent();
        }
    }
    // NTFS keeps a copy of a file's times in each folder that lists one of its names, refreshed only through that
    // name: set them through the hard link's other name too, or its folder lists it as new.
    set_age(&store_link, now, OLD)?;
    set_age(&hard_b, now, OLD)?;
    let mut dirs: Vec<(PathBuf, u64)> = dirs.into_iter().collect();
    dirs.sort_by_key(|(d, _)| std::cmp::Reverse(d.components().count()));
    for (d, days) in &dirs {
        set_age(d, now, *days)?;
    }

    let mut perms = fs::metadata(&read_only)?.permissions();
    perms.set_readonly(true);
    fs::set_permissions(&read_only, perms)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ro_folder, fs::Permissions::from_mode(0o555))?;
    }
    #[cfg(windows)]
    {
        let mut perms = fs::metadata(&ro_folder)?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&ro_folder, perms)?;
    }

    let mut advice_files = Vec::new();
    if advice {
        let env = env(root);
        let steam = match env.platform {
            crate::Platform::Windows => env.program_files[0].join("Steam"),
            crate::Platform::MacOs => sub(&env.home, "Library/Application Support/Steam"),
        };
        let manifest = sub(&steam, "steamapps/appmanifest_42.acf");
        fs::create_dir_all(manifest.parent().unwrap())?;
        fs::write(
            &manifest,
            r#""AppState" { "appid" "42" "name" "Vacancy fixture game" "LastPlayed" "100000" "SizeOnDisk" "123456789" }"#,
        )?;
        advice_files.push(manifest);
        // Passive instruction evidence only. No installed tools or host app data are consulted.
        let docker = if windows {
            env.local_app_data.join("Docker")
        } else {
            env.home.join("Library/Containers/com.docker.docker")
        };
        let marker = docker.join("fixture-presence.txt");
        write_file(&marker, KIB)?;
        advice_files.push(marker);
        if windows {
            let marker = env.system_root.join("fixture-presence.txt");
            write_file(&marker, KIB)?;
            advice_files.push(marker);
        } else {
            for folder in ["Library/Developer/CoreSimulator", "Library/Caches/Homebrew"] {
                let marker = env.home.join(folder).join("fixture-presence.txt");
                write_file(&marker, KIB)?;
                advice_files.push(marker);
            }
        }
        if !windows {
            let sample = sub(
                &env.home,
                "Music/Logic Pro Library.bundle/fixture-sound.dat",
            );
            write_file(&sample, KIB)?;
            advice_files.push(sample);
        }
    }
    let mut untouched = vec![
        sub(&home, "Downloads/new-setup.exe"),
        sub(&home, "code/live-app/node_modules"),
    ];
    untouched.extend(advice_files);
    let at = |rel: &str| sub(&home, rel);
    let duplicates = DuplicateCases {
        groups: dups
            .groups
            .iter()
            .map(|(keep, extras)| (at(keep), extras.iter().map(|e| at(e)).collect()))
            .collect(),
        untick: at(dups.untick),
        change_kept: at(dups.change_kept),
        not_duplicates: dups.not_duplicates.iter().map(|r| at(r)).collect(),
    };
    std::thread::sleep(SETTLE);
    Ok(Profile {
        duplicates,
        root: root.to_path_buf(),
        documents,
        link_to_documents,
        cache: sub(&home, cache_a),
        held_open: sub(&home, &format!("{cache_a}/held-open.bin")),
        loose: sub(
            &home,
            if windows {
                "AppData/Local/Temp/old-1.tmp"
            } else {
                "Library/Caches/com.example.fixtureapp/Cache.db"
            },
        ),
        modified_later: sub(&home, &format!("{cache_b}/modified-later.bin")),
        read_only,
        long_path: sub(&home, &long_rel),
        node_modules,
        store_file,
        untouched,
        home,
    })
}

/// The duplicate cases' files, relative to the home folder, and their roles.
struct DuplicateSpecs {
    specs: Vec<Spec>,
    groups: Vec<(&'static str, Vec<&'static str>)>,
    untick: &'static str,
    change_kept: &'static str,
    not_duplicates: Vec<&'static str>,
    /// The hard link pair: the file written, and its second name.
    hard_link: (&'static str, &'static str),
    /// A folder link inside Pictures, and the folder it leads to (outside the personal folders).
    link: (&'static str, &'static str),
}

/// The duplicate finder's known cases (ticket 09), kept apart from the rest of the profile. Every copy is a little
/// over 1 MiB (the finder's threshold) so the profile stays small. Group by group, the keeper rule keeps the first:
/// - the oldest photo, over a copy in Downloads and a newer one named "(1)" on the Desktop;
/// - a video in Videos (Movies) over an older copy in Downloads;
/// - a document with its own name over an older "- Copy";
/// - a guide in Documents over a copy in Downloads (the group a proof unticks);
/// - the older of two photos in Pictures.
fn duplicate_specs(windows: bool) -> DuplicateSpecs {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    let mut specs = Vec::new();
    let mut copy = |rel: &'static str, len: usize, days: u64, seed: u64, tail: usize| {
        specs.push(Spec {
            rel: rel.to_owned(),
            len,
            days,
            seed: Some((seed, tail)),
        });
        rel
    };
    let (videos, wedding, clip, installer, installer_copy) = if windows {
        (
            "Videos/2024/Wedding highlights.mp4",
            "Downloads/Wedding highlights.mp4",
            "Videos/clip-a.mp4",
            "Downloads/big-old-setup.exe",
            "Desktop/big-old-setup.exe",
        )
    } else {
        (
            "Movies/2024/Wedding highlights.mov",
            "Downloads/Wedding highlights.mov",
            "Movies/clip-a.mov",
            "Downloads/big-old-installer.dmg",
            "Desktop/big-old-installer.dmg",
        )
    };
    let photo = MIB + 512 * KIB;
    let groups = vec![
        (
            copy("Pictures/2025/Lisbon/IMG_4471.jpg", photo, 400, 1, 0),
            vec![
                copy("Downloads/IMG_4471.jpg", photo, 390, 1, 0),
                copy("Desktop/IMG_4471 (1).jpg", photo, 120, 1, 0),
            ],
        ),
        (
            copy(videos, 2 * MIB + 7 * KIB, 300, 2, 0),
            vec![copy(wedding, 2 * MIB + 7 * KIB, 610, 2, 0)],
        ),
        (
            copy(
                "Documents/Taxes/Tax return 2025.pdf",
                MIB + 100 * KIB,
                100,
                3,
                0,
            ),
            vec![copy(
                "Documents/Taxes/Tax return 2025 - Copy.pdf",
                MIB + 100 * KIB,
                300,
                3,
                0,
            )],
        ),
        (
            copy("Documents/Work/Brand guide.pdf", MIB + 300 * KIB, 75, 4, 0),
            vec![copy("Downloads/Brand guide.pdf", MIB + 300 * KIB, 76, 4, 0)],
        ),
        (
            copy(
                "Pictures/Camera Roll/IMG_5102.jpg",
                MIB + 50 * KIB,
                300,
                5,
                0,
            ),
            vec![copy(
                "Pictures/Phone import/IMG_5102.jpg",
                MIB + 50 * KIB,
                280,
                5,
                0,
            )],
        ),
    ];
    let not_duplicates = vec![
        // The same length, other contents.
        copy(
            "Pictures/Camera Roll/IMG_6000.jpg",
            MIB + 70 * KIB,
            300,
            6,
            0,
        ),
        copy(
            "Pictures/Phone import/IMG_6001.jpg",
            MIB + 70 * KIB,
            280,
            7,
            0,
        ),
        // The same first 64 KiB and more, another end.
        copy(clip, MIB + 200 * KIB, 200, 8, 0),
        copy("Downloads/clip-b.mp4", MIB + 200 * KIB, 190, 8, 16),
        // One file, two names.
        copy("Pictures/hardlink-a.jpg", MIB + 120 * KIB, 200, 9, 0),
        "Desktop/hardlink-b.jpg",
        // Copies, but under 1 MiB.
        copy("Pictures/small.png", 512 * KIB, 200, 10, 0),
        copy("Downloads/small.png", 512 * KIB, 190, 10, 0),
        // A copy of the first group in a Dropbox folder, older than all of them: never kept, never deleted.
        copy("Documents/Dropbox/IMG_4471.jpg", photo, 500, 1, 0),
        copy("Documents/Dropbox/.dropbox", 1, 500, 12, 0),
        // And in a Photos library.
        copy(
            "Pictures/Photos Library.photoslibrary/originals/IMG_4471.jpg",
            photo,
            600,
            1,
            0,
        ),
        // A copy of the last group behind a link from Pictures: the scan never follows it.
        copy("code/photo-backup/IMG_5102.jpg", MIB + 50 * KIB, 350, 5, 0),
        // An old installer the installers rule deletes, and its copy on the Desktop: no byte counted twice.
        copy(installer, MIB + 10 * KIB, 60, 11, 0),
        copy(installer_copy, MIB + 10 * KIB, 60, 11, 0),
    ];
    DuplicateSpecs {
        specs,
        groups,
        untick: "Documents/Work/Brand guide.pdf",
        change_kept: "Documents/Taxes/Tax return 2025.pdf",
        not_duplicates,
        hard_link: ("Pictures/hardlink-a.jpg", "Desktop/hardlink-b.jpg"),
        link: ("Pictures/backup-link", "code/photo-backup"),
    }
}

/// Writes `len` bytes that are the same for every file with `seed`, the last `tail` of them flipped, making the
/// folders on the way.
fn write_seeded(path: &Path, len: usize, seed: u64, tail: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut data = vec![0u8; len];
    for b in &mut data {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = state as u8;
    }
    let from = len.saturating_sub(tail);
    for b in &mut data[from..] {
        *b = !*b;
    }
    fs::write(path, data)
}

/// `base` joined with `/`-separated names.
fn sub(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|p| !p.is_empty())
        .fold(base.to_path_buf(), |p, part| p.join(part))
}

/// Writes `len` bytes of varied content (so nothing compresses it away), making the folders on the way.
fn write_file(path: &Path, len: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut seed =
        (len as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ path.as_os_str().len() as u64 | 1;
    let mut data = vec![0u8; len];
    for b in &mut data {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *b = seed as u8;
    }
    fs::write(path, data)
}

/// Sets a file's or folder's modified and accessed times to `days` days before `now`.
fn set_age(path: &Path, now: SystemTime, days: u64) -> io::Result<()> {
    let at = now - Duration::from_secs(days * 86_400 + 60);
    let times = fs::FileTimes::new().set_accessed(at).set_modified(at);
    let file = open_for_times(path)?;
    file.set_times(times)
}

#[cfg(windows)]
fn open_for_times(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

#[cfg(not(windows))]
fn open_for_times(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

/// A folder link at `link` to `to`: a junction on Windows (no privilege needed), a symlink elsewhere.
fn link_dir(root: &Path, link: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        link_dir_with(root, link, to, || {})
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        std::os::unix::fs::symlink(to, link)
    }
}

#[cfg(windows)]
fn junction_data(to: &Path) -> io::Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    // The generator owns both locations. Canonicalization supplies native separators and a verbatim
    // Windows target; no cmd filename parsing, quoting, metacharacters, or inherited shell settings.
    let target: Vec<u16> = fs::canonicalize(to)?.as_os_str().encode_wide().collect();
    let prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    if !target.starts_with(&prefix) {
        return Err(io::Error::other(
            "fixture target has no native verbatim prefix",
        ));
    }
    let substitute: Vec<u16> = r"\??\"
        .encode_utf16()
        .chain(target[4..].iter().copied())
        .collect();
    let data_len = 8 + (substitute.len() + target.len() + 2) * 2;
    if data_len + 8 > 16 * 1024 {
        return Err(io::Error::other(
            "fixture junction target exceeds reparse buffer limit",
        ));
    }
    let mut data = Vec::new();
    data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
    data.extend_from_slice(&(data_len as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    for n in [
        0,
        substitute.len() * 2,
        (substitute.len() + 1) * 2,
        target.len() * 2,
    ] {
        data.extend_from_slice(&(n as u16).to_le_bytes());
    }
    for n in substitute.into_iter().chain([0]).chain(target).chain([0]) {
        data.extend_from_slice(&n.to_le_bytes());
    }
    Ok(data)
}

#[cfg(windows)]
fn set_junction(file: &fs::File, data: &[u8]) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
    let mut returned = 0;
    // SAFETY: owned directory handle, initialized bounded mount-point buffer, writable byte count.
    if unsafe {
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
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn link_dir_with(
    root: &Path,
    link: &Path,
    to: &Path,
    after_parent: impl FnOnce(),
) -> io::Result<()> {
    if !is_profile(root) {
        return Err(io::Error::other("generated fixture marker required"));
    }
    let data = junction_data(to)?;
    fsid::with_fresh_directory(root, link, after_parent, |file| set_junction(file, &data))
}

#[cfg(all(test, windows))]
mod junction_tests {
    use super::*;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_WRITE_ATTRIBUTES,
    };

    fn retag_owned_test_directory(path: &Path, to: &Path) {
        let file = fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .share_mode(7)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap();
        set_junction(&file, &junction_data(to).unwrap()).unwrap();
    }

    #[test]
    fn atomic_junction_creation_never_retags_an_outside_or_existing_directory() {
        let base = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("tool-fixtures");
        fs::create_dir_all(&base).unwrap();
        let temp = tempfile::tempdir_in(base).unwrap();
        let root = temp.path().join("profile");
        let outside = temp.path().join("outside-profile");
        for profile in [&root, &outside] {
            fs::create_dir(profile).unwrap();
            check_root(profile, &std::env::home_dir().unwrap()).unwrap();
            fs::write(profile.join(MARKER), b"fresh fixture proof profile").unwrap();
        }
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        let outside_leaf = outside.join("leaf");
        fs::create_dir(&outside_leaf).unwrap();
        fs::write(outside.join("sentinel.bin"), b"outside fixture sentinel").unwrap();
        let before = inventory(&outside).unwrap();

        // Valid attack control for the create-then-absolute-open pattern: replace the ancestor after creating
        // a fresh leaf. An absolute lookup now names the preexisting outside ordinary directory. This makes
        // no claim about mklink's internal implementation, and never retags the outside leaf itself.
        let control = root.join("control");
        fs::create_dir(&control).unwrap();
        fs::create_dir(control.join("leaf")).unwrap();
        fs::rename(control.join("leaf"), root.join("moved-control-leaf")).unwrap();
        retag_owned_test_directory(&control, &outside);
        assert_eq!(
            fs::canonicalize(control.join("leaf")).unwrap(),
            fs::canonicalize(&outside_leaf).unwrap()
        );
        fs::remove_dir(&control).unwrap();

        let parent = root.join("attack");
        fs::create_dir(&parent).unwrap();
        let mut attacked = false;
        let result = link_dir_with(&root, &parent.join("leaf"), &target, || {
            retag_owned_test_directory(&parent, &outside);
            attacked = true;
            assert_eq!(
                fs::canonicalize(parent.join("leaf")).unwrap(),
                fs::canonicalize(&outside_leaf).unwrap(),
                "the retained parent was successfully retagged"
            );
        });
        assert!(attacked);
        assert!(result.is_err(), "a redirected parent must fail closed");
        assert_eq!(
            fs::symlink_metadata(&outside_leaf)
                .unwrap()
                .file_attributes()
                & FILE_ATTRIBUTE_REPARSE_POINT,
            0
        );
        assert_eq!(inventory(&outside).unwrap(), before);
        fs::remove_dir(&parent).unwrap();

        let existing = root.join("existing");
        fs::create_dir(&existing).unwrap();
        assert!(
            link_dir(&root, &existing, &target).is_err(),
            "FILE_CREATE must refuse an existing ordinary leaf"
        );
        assert_eq!(
            fs::symlink_metadata(&existing).unwrap().file_attributes()
                & FILE_ATTRIBUTE_REPARSE_POINT,
            0
        );
        let fresh = root.join("fresh");
        link_dir(&root, &fresh, &target).unwrap();
        assert_ne!(
            fs::symlink_metadata(&fresh).unwrap().file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT,
            0
        );
        fs::remove_dir(fresh).unwrap();
    }
}

/// Everything under `root`, without following links: one line per file, folder and link, `path<TAB>what`, where the
/// path is relative to `root` with `/` separators and `what` is `dir`, `link -> <where it points>`, or
/// `file <bytes> <hash>` (64-bit FNV-1a of the contents), sorted. Two inventories of the same tree are equal exactly
/// when nothing in it changed. For before-and-after proofs. On Windows reading a file leaves its last-access time
/// alone, so listing a fixture before a scan doesn't make it look just used; elsewhere, list it after the scan.
pub fn inventory(root: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let meta = fs::symlink_metadata(&path)?;
            let what = if meta.file_type().is_symlink() {
                let to = fs::read_link(&path)
                    .map(|t| t.display().to_string())
                    .unwrap_or_default();
                format!("link -> {to}")
            } else if meta.is_dir() {
                stack.push(path.clone());
                "dir".to_owned()
            } else {
                match read_quietly(&path) {
                    Ok(data) => format!("file {} {:016x}", data.len(), fnv1a(&data)),
                    // Held open by someone with no sharing: listed, unread.
                    Err(e) => format!("file {} unread ({e})", meta.len()),
                }
            };
            out.push(format!("{rel}\t{what}"));
        }
    }
    out.sort();
    Ok(out)
}

/// A file's contents, read without changing its last-access time on Windows (where reading would refresh it, and
/// the rules would then see the file as just used). Elsewhere a plain read.
fn read_quietly(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(windows)]
    {
        use std::io::Read;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::Storage::FileSystem::SetFileTime;
        const GENERIC_READ: u32 = 0x8000_0000;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        let mut file = fs::OpenOptions::new()
            .access_mode(GENERIC_READ | FILE_WRITE_ATTRIBUTES)
            .share_mode(0x7)
            .open(path)?;
        // All ones: don't update the last-access time for anything done through this handle.
        let keep = FILETIME {
            dwLowDateTime: u32::MAX,
            dwHighDateTime: u32::MAX,
        };
        // SAFETY: the handle is open with FILE_WRITE_ATTRIBUTES; `keep` outlives the call; null leaves a time alone.
        unsafe {
            SetFileTime(
                file.as_raw_handle(),
                std::ptr::null(),
                &keep,
                std::ptr::null(),
            )
        };
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Ok(data)
    }
    #[cfg(not(windows))]
    {
        fs::read(path)
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

//! The places the categorizer needs to know: the user's home, app data and temp folders, the known folders
//! (Downloads, Documents...) and cloud-sync roots. [`Env::detect`] reads them from the OS; tests build one with
//! [`Env::windows`] or [`Env::macos`] pointing into a fixture tree, so both platforms' rules run on either OS.

use std::path::{Path, PathBuf};

/// Which platform's rules apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
}

/// The user's known folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownFolders {
    /// Downloads.
    pub downloads: PathBuf,
    /// Documents.
    pub documents: PathBuf,
    /// Desktop.
    pub desktop: PathBuf,
    /// Pictures.
    pub pictures: PathBuf,
    /// Videos (Movies on macOS).
    pub videos: PathBuf,
    /// Music.
    pub music: PathBuf,
}

/// A folder a sync client keeps in step with the cloud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudRoot {
    /// The service: "OneDrive", "Dropbox", "iCloud Drive".
    pub provider: String,
    /// The folder.
    pub path: PathBuf,
}

/// Where things are on the scanned volume. See the module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Env {
    /// Whose rules apply.
    pub platform: Platform,
    /// The volume's root: `C:\`, or `/`.
    pub root: PathBuf,
    /// The user's home folder.
    pub home: PathBuf,
    /// Downloads, Documents and the rest.
    pub known: KnownFolders,
    /// Temp folders: `%TEMP%` on Windows; `$TMPDIR` on macOS.
    pub temp: Vec<PathBuf>,
    /// Cloud-sync roots.
    pub cloud: Vec<CloudRoot>,
    /// Sync clients that are installed but whose folders can't be known: Google Drive, when its record of them
    /// can't be read consistently. Their folders could be anywhere, so the rules that search where people keep
    /// their own files (projects, Downloads) propose nothing while this is not empty.
    pub unknown_clouds: Vec<String>,
    /// Windows only: `C:\Windows`.
    pub system_root: PathBuf,
    /// Windows only: `C:\Program Files` and `C:\Program Files (x86)`.
    pub program_files: Vec<PathBuf>,
    /// Windows only: `C:\ProgramData`.
    pub program_data: PathBuf,
    /// Windows only: `%LOCALAPPDATA%`.
    pub local_app_data: PathBuf,
    /// Windows only: `%APPDATA%` (Roaming).
    pub app_data: PathBuf,
}

impl Env {
    /// The standard Windows layout on the volume at `root`, for user `user`: `{root}\Windows`,
    /// `{root}\Users\{user}\AppData\Local` and so on, no cloud roots.
    pub fn windows(root: &Path, user: &str) -> Self {
        let home = root.join("Users").join(user);
        let local = home.join("AppData").join("Local");
        Self {
            platform: Platform::Windows,
            root: root.to_path_buf(),
            known: KnownFolders {
                downloads: home.join("Downloads"),
                documents: home.join("Documents"),
                desktop: home.join("Desktop"),
                pictures: home.join("Pictures"),
                videos: home.join("Videos"),
                music: home.join("Music"),
            },
            temp: vec![local.join("Temp")],
            cloud: Vec::new(),
            unknown_clouds: Vec::new(),
            system_root: root.join("Windows"),
            program_files: vec![root.join("Program Files"), root.join("Program Files (x86)")],
            program_data: root.join("ProgramData"),
            app_data: home.join("AppData").join("Roaming"),
            local_app_data: local,
            home,
        }
    }

    /// The standard macOS layout on the volume at `root`, for user `user`: `{root}/Users/{user}`, its Library
    /// and known folders, iCloud Drive in `~/Library/Mobile Documents`.
    pub fn macos(root: &Path, user: &str) -> Self {
        let home = root.join("Users").join(user);
        let library = home.join("Library");
        Self {
            platform: Platform::MacOs,
            root: root.to_path_buf(),
            known: KnownFolders {
                downloads: home.join("Downloads"),
                documents: home.join("Documents"),
                desktop: home.join("Desktop"),
                pictures: home.join("Pictures"),
                videos: home.join("Movies"),
                music: home.join("Music"),
            },
            temp: vec![root.join("private").join("var").join("folders")],
            cloud: vec![CloudRoot {
                provider: "iCloud Drive".into(),
                path: library.join("Mobile Documents"),
            }],
            unknown_clouds: Vec::new(),
            system_root: root.join("System"),
            program_files: vec![root.join("Applications")],
            program_data: root.join("Library"),
            local_app_data: library.join("Application Support"),
            app_data: library.join("Application Support"),
            home,
        }
    }

    /// The real places on this computer, for the volume at `root`. On a volume other than the one the user's
    /// home is on, the home-based rules simply match nothing.
    pub fn detect(root: &Path) -> Self {
        detect::detect(root)
    }
}

/// The sync service a folder directly in the home folder belongs to, going by its name: `OneDrive`,
/// `OneDrive - Contoso`, `Dropbox`, `Dropbox (Acme)`, `Google Drive`, `My Drive`, `iCloudDrive`, `Box`.
pub(crate) fn sync_provider(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let named =
        |p: &str| n == p || n.starts_with(&format!("{p} - ")) || n.starts_with(&format!("{p} ("));
    if named("onedrive") {
        Some("OneDrive")
    } else if named("dropbox") {
        Some("Dropbox")
    } else if named("google drive") || n == "my drive" {
        Some("Google Drive")
    } else if n == "iclouddrive" || n == "icloud drive" {
        Some("iCloud Drive")
    } else if n == "box" || n == "box sync" {
        Some("Box")
    } else {
        None
    }
}

/// Sync folders directly in `home`, found by name (see [`sync_provider`]).
pub(crate) fn clouds_in(home: &Path) -> Vec<CloudRoot> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if let Some(provider) = sync_provider(&entry.file_name().to_string_lossy())
            && entry.file_type().is_ok_and(|t| t.is_dir())
        {
            out.push(CloudRoot {
                provider: provider.into(),
                path: entry.path(),
            });
        }
    }
    out
}

/// The folders Dropbox's `info.json` names, one per account (`personal`, `business`): Dropbox can live anywhere,
/// on any drive.
pub(crate) fn dropbox_paths(info: &str) -> Vec<PathBuf> {
    let Ok(serde_json::Value::Object(accounts)) = serde_json::from_str(info) else {
        return Vec::new();
    };
    accounts
        .values()
        .filter_map(|a| a.get("path")?.as_str())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path under the temp folder in the running OS's syntax.
    fn native(base: &Path, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(base.to_path_buf(), |p, part| p.join(part))
    }

    /// A `root_preference_sqlite.db` in `dir` laid out as Drive lays it out, listing `mirrors` and the drive
    /// mounted at `mount`.
    fn drive_db(dir: &Path, mirrors: &[Option<&str>], mount: &str) {
        let conn = rusqlite::Connection::open(dir.join("root_preference_sqlite.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE roots (root_id INTEGER PRIMARY KEY, media_id INTEGER, title TEXT,
                 last_seen_absolute_path TEXT, sync_type INTEGER);
             CREATE TABLE media (media_id INTEGER PRIMARY KEY, fs_type INTEGER, last_mount_point TEXT);",
        )
        .unwrap();
        for (i, path) in mirrors.iter().enumerate() {
            conn.execute(
                "INSERT INTO roots (media_id, title, last_seen_absolute_path, sync_type)
                 VALUES (1, ?1, ?2, 1)",
                rusqlite::params![format!("folder {i}"), path],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO media (fs_type, last_mount_point) VALUES (10, ?1), (3, ?2)",
            rusqlite::params![mount, "/Volumes/USB"],
        )
        .unwrap();
    }

    /// The cloud roots Drive's record names, when it can be read.
    fn roots(dir: &Path) -> Vec<CloudRoot> {
        match google_drive_mirrors(dir) {
            DriveMirrors::Known(roots) => roots,
            DriveMirrors::Unknown => panic!("Drive's record in {} is unknown", dir.display()),
        }
    }

    fn found(dir: &Path) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = roots(dir).into_iter().map(|c| c.path).collect();
        out.sort();
        out
    }

    /// The folders Drive mirrors are wherever the user picked, with no name to go by: Drive's own record says
    /// where. Blank, missing and relative entries, and media other than Drive's own, are left out.
    #[test]
    fn reads_google_drive_s_record_of_the_folders_it_mirrors() {
        let fx = tempfile::tempdir().unwrap();
        let work = native(fx.path(), "Users/alex/Projects/Work");
        let mount = native(fx.path(), "GoogleDrive");
        let work_text = work.to_str().unwrap().to_owned();
        let verbatim = format!(r"\\?\{work_text}");
        let mirrors = [
            Some(work_text.as_str()),
            cfg!(windows).then_some(verbatim.as_str()), // the same folder, as Drive writes it on Windows
            None,
            Some("  "),
            Some("relative/folder"),
        ];
        drive_db(fx.path(), &mirrors, mount.to_str().unwrap());
        assert_eq!(found(fx.path()), {
            let mut want = vec![work.clone(), mount];
            want.sort();
            want
        });
        assert!(
            roots(fx.path())
                .iter()
                .all(|c| c.provider == "Google Drive")
        );
        // Listed, the mirror is a cloud root: nothing in it may go, though it has no working folders to go by.
        let mut env = if cfg!(windows) {
            Env::windows(fx.path(), "alex")
        } else {
            Env::macos(fx.path(), "alex")
        };
        env.cloud.extend(roots(fx.path()));
        let guard = crate::Guard::new(&env, fx.path());
        assert!(matches!(
            guard.check(&work.join("app").join("node_modules")),
            Err(crate::Denied::Protected(_))
        ));
    }

    /// A folder's name may end in a space. The path Drive stored is used exactly as stored, never trimmed: the
    /// mirror `Work ` is the cloud root and is protected, not some `Work` that may or may not be beside it.
    #[test]
    fn a_mirror_whose_name_ends_in_a_space() {
        let fx = tempfile::tempdir().unwrap();
        let home = native(fx.path(), "Users/alex");
        let stored = if cfg!(windows) {
            format!(r"\\?\{}\Work ", home.display())
        } else {
            format!("{}/Work ", home.display())
        };
        let mirror = PathBuf::from(&stored);
        let target = mirror.join("app").join("node_modules");
        std::fs::create_dir_all(&target).unwrap();
        assert!(
            std::fs::read_dir(&home)
                .unwrap()
                .any(|e| e.unwrap().file_name() == "Work "),
            "the folder on disk ends in a space"
        );
        drive_db(fx.path(), &[Some(stored.as_str())], "/nowhere");
        assert_eq!(found(fx.path()), vec![mirror.clone()], "exactly as stored");
        let mut env = if cfg!(windows) {
            Env::windows(fx.path(), "alex")
        } else {
            Env::macos(fx.path(), "alex")
        };
        env.cloud.extend(roots(fx.path()));
        let guard = crate::Guard::new(&env, fx.path());
        assert!(matches!(
            guard.check(&target),
            Err(crate::Denied::Protected(_))
        ));
    }

    /// Reading the database never writes beside it (no journal, no copy) and never changes it.
    #[test]
    fn reads_drive_s_database_in_place_without_writing() {
        let fx = tempfile::tempdir().unwrap();
        let work = native(fx.path(), "Work");
        drive_db(fx.path(), &[work.to_str()], "/Volumes/GoogleDrive");
        let db = fx.path().join("root_preference_sqlite.db");
        let before = std::fs::read(&db).unwrap();
        let listing = |dir: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let files = listing(fx.path());
        assert!(found(fx.path()).contains(&work));
        assert_eq!(listing(fx.path()), files, "nothing new beside it");
        assert_eq!(std::fs::read(&db).unwrap(), before, "unchanged");
    }

    /// Drive holds its database open while it runs, in write-ahead-log mode. A mirror it added lives in the log
    /// until Drive checkpoints it. Shared, the log is read with the database. Locked by Drive, it can't be: the
    /// mirrors are then unknown, never the stale list in the database file, which leaves the new mirror out.
    #[test]
    fn a_mirror_only_in_drive_s_log_is_read_or_left_unknown() {
        let fx = tempfile::tempdir().unwrap();
        let work = native(fx.path(), "Work");
        let drive =
            rusqlite::Connection::open(fx.path().join("root_preference_sqlite.db")).unwrap();
        drive
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA wal_autocheckpoint = 0;
                 CREATE TABLE roots (root_id INTEGER PRIMARY KEY, last_seen_absolute_path TEXT);
                 CREATE TABLE media (media_id INTEGER PRIMARY KEY, fs_type INTEGER,
                     last_mount_point TEXT);
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        drive
            .execute(
                "INSERT INTO roots (last_seen_absolute_path) VALUES (?1)",
                [work.to_str().unwrap()],
            )
            .unwrap();
        let wal = fx.path().join("root_preference_sqlite.db-wal");
        assert!(
            std::fs::metadata(&wal).unwrap().len() > 0,
            "the mirror is in the log"
        );
        assert_eq!(
            found(fx.path()),
            vec![work.clone()],
            "shared: read with the log"
        );

        drive
            .execute_batch(
                "PRAGMA locking_mode = EXCLUSIVE;
                 BEGIN IMMEDIATE; UPDATE media SET fs_type = fs_type; COMMIT;",
            )
            .unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            google_drive_mirrors(fx.path()),
            DriveMirrors::Unknown,
            "locked: unknown, not the database file without the log"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        drop(drive);
        assert_eq!(found(fx.path()), vec![work], "readable again");
    }

    /// Drive installed (its folder is there) but its record missing, malformed or not Drive's: unknown. Not
    /// installed: nothing to say.
    #[test]
    fn drive_s_record_missing_or_broken_is_unknown() {
        let fx = tempfile::tempdir().unwrap();
        assert_eq!(
            google_drive_mirrors(&fx.path().join("nowhere")),
            DriveMirrors::Known(Vec::new()),
            "not installed"
        );
        assert_eq!(
            google_drive_mirrors(fx.path()),
            DriveMirrors::Unknown,
            "missing"
        );
        let broken = tempfile::tempdir().unwrap();
        std::fs::write(broken.path().join("root_preference_sqlite.db"), b"not a db").unwrap();
        assert_eq!(
            google_drive_mirrors(broken.path()),
            DriveMirrors::Unknown,
            "not a database"
        );
        let other = tempfile::tempdir().unwrap();
        rusqlite::Connection::open(other.path().join("root_preference_sqlite.db"))
            .unwrap()
            .execute_batch("CREATE TABLE settings (k TEXT, v TEXT);")
            .unwrap();
        assert_eq!(
            google_drive_mirrors(other.path()),
            DriveMirrors::Unknown,
            "a layout without Drive's tables"
        );
    }

    #[test]
    fn names_odd_paths_in_sqlite_uris() {
        let path = if cfg!(windows) {
            PathBuf::from(r"C:\Users\Jo #1\Drive?%\root.db")
        } else {
            PathBuf::from("/Users/Jo #1/Drive?%/root.db")
        };
        let uri = sqlite_uri(&path, "mode=ro").unwrap();
        let want = if cfg!(windows) {
            "file:///C:/Users/Jo%20%231/Drive%3F%25/root.db?mode=ro"
        } else {
            "file:///Users/Jo%20%231/Drive%3F%25/root.db?mode=ro"
        };
        assert_eq!(uri, want);
    }

    #[test]
    fn names_sync_folders_by_service() {
        for (name, provider) in [
            ("OneDrive", "OneDrive"),
            ("OneDrive - Contoso", "OneDrive"),
            ("Dropbox", "Dropbox"),
            ("Dropbox (Acme)", "Dropbox"),
            ("Google Drive", "Google Drive"),
            ("My Drive", "Google Drive"),
            ("iCloudDrive", "iCloud Drive"),
            ("Box", "Box"),
        ] {
            assert_eq!(sync_provider(name), Some(provider), "{name}");
        }
        for name in ["OneDriveBackupNotes", "Dropboxes", "Boxes", "Drive", "code"] {
            assert_eq!(sync_provider(name), None, "{name}");
        }
    }

    #[test]
    fn finds_sync_folders_in_the_home_folder() {
        let home = tempfile::tempdir().unwrap();
        for dir in ["OneDrive - Contoso", "Dropbox (Acme)", "My Drive", "code"] {
            std::fs::create_dir(home.path().join(dir)).unwrap();
        }
        std::fs::write(home.path().join("Dropbox"), b"a file, not a sync root").unwrap();
        let mut found: Vec<(String, String)> = clouds_in(home.path())
            .into_iter()
            .map(|c| {
                (
                    c.provider,
                    c.path.file_name().unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![
                ("Dropbox".to_owned(), "Dropbox (Acme)".to_owned()),
                ("Google Drive".to_owned(), "My Drive".to_owned()),
                ("OneDrive".to_owned(), "OneDrive - Contoso".to_owned()),
            ]
        );
    }

    #[test]
    fn reads_dropbox_s_own_record_of_where_it_lives() {
        let root = if cfg!(windows) {
            r"D:\\Dropbox"
        } else {
            "/Volumes/Data/Dropbox"
        };
        let business = if cfg!(windows) {
            r"D:\\Dropbox (Acme)"
        } else {
            "/Volumes/Data/Dropbox (Acme)"
        };
        let info = format!(
            r#"{{"personal": {{"path": "{root}", "host": 1, "is_team": false}}, "business": {{"path": "{business}"}}}}"#
        );
        let mut paths = dropbox_paths(&info);
        paths.sort();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths.iter().all(|p| p.is_absolute()));
        assert!(dropbox_paths("not json").is_empty());
        assert!(dropbox_paths(r#"{"personal": {"path": "relative/Dropbox"}}"#).is_empty());
    }
}

/// Adds `root` unless a root for the same place is there already.
fn add_cloud(cloud: &mut Vec<CloudRoot>, root: CloudRoot) {
    let key = crate::pathkey::PathKey::of(&root.path);
    if key.is_some()
        && !cloud
            .iter()
            .any(|c| crate::pathkey::PathKey::of(&c.path) == key)
    {
        cloud.push(root);
    }
}

/// Dropbox's folders, from the `info.json` files in `dirs`.
fn dropbox_roots(dirs: &[PathBuf]) -> Vec<CloudRoot> {
    let mut out = Vec::new();
    for dir in dirs {
        if let Ok(info) = std::fs::read_to_string(dir.join("info.json")) {
            for path in dropbox_paths(&info) {
                out.push(CloudRoot {
                    provider: "Dropbox".into(),
                    path,
                });
            }
        }
    }
    out
}

/// What Google Drive for desktop's own record says about the folders it keeps in sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DriveMirrors {
    /// The folders it mirrors or backs up and the drive it mounts; none when Drive isn't installed.
    Known(Vec<CloudRoot>),
    /// Drive is installed, but its record can't be read consistently: it is locked, its write-ahead log can't be
    /// used, or it is missing or malformed. Where its mirrors are is unknown.
    Unknown,
}

/// Google Drive for desktop's own record of the folders it keeps in sync, from `root_preference_sqlite.db` in
/// `dir` (`%LOCALAPPDATA%\Google\DriveFS` on Windows, `~/Library/Application Support/Google/DriveFS` on a Mac):
/// every folder of this computer it mirrors or backs up (`roots`), and the drive it mounts for My Drive (`media`).
///
/// Drive keeps the database open while it runs. It is read in place, never copied, through a read-only
/// connection that sees the database as SQLite itself does, write-ahead log included, in one read transaction,
/// waiting a moment at most for Drive's locks. Anything short of that consistent read leaves the mirrors
/// [`DriveMirrors::Unknown`]: a stale reading that skipped the log would leave out mirrors added since.
pub(crate) fn google_drive_mirrors(dir: &Path) -> DriveMirrors {
    match std::fs::metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return DriveMirrors::Known(Vec::new());
        }
        Ok(meta) if !meta.is_dir() => return DriveMirrors::Known(Vec::new()),
        _ => {}
    }
    let Ok(paths) = drive_db_paths(&dir.join("root_preference_sqlite.db")) else {
        return DriveMirrors::Unknown;
    };
    let mut out: Vec<CloudRoot> = Vec::new();
    for path in paths {
        add_cloud(
            &mut out,
            CloudRoot {
                provider: "Google Drive".into(),
                path,
            },
        );
    }
    DriveMirrors::Known(out)
}

/// What Drive's database lists, read consistently, or the reason it couldn't be: a lock held past a moment, a
/// database that isn't there or can't be read, or one without the tables and columns Drive keeps.
fn drive_db_paths(db: &Path) -> rusqlite::Result<Vec<PathBuf>> {
    use rusqlite::{Connection, OpenFlags, TransactionBehavior};
    let uri = sqlite_uri(db, "mode=ro").ok_or_else(|| rusqlite::Error::InvalidPath(db.into()))?;
    let mut conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let read = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut out = Vec::new();
    for sql in [
        // Folders of this computer that Drive mirrors or backs up.
        "SELECT last_seen_absolute_path FROM roots",
        // The drive (`G:\`, or a folder on a Mac) that holds My Drive and shared drives.
        "SELECT last_mount_point FROM media WHERE fs_type = 10",
    ] {
        let mut stmt = read.prepare(sql)?;
        let found = stmt
            .query_map([], |row| row.get::<_, Option<String>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out.extend(found.into_iter().flatten().filter_map(drive_path));
    }
    Ok(out)
}

/// A path exactly as Drive stored it, if it is one. Nothing is trimmed or rewritten: a folder's name may end in a
/// space, and on Windows Drive stores `\\?\C:\...`, the only form that names such a folder exactly (`C:\...` loses
/// trailing spaces and dots on the way to the file system).
fn drive_path(text: String) -> Option<PathBuf> {
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

/// A `file:` URI for SQLite naming `path`, with `params` as its query.
fn sqlite_uri(path: &Path, params: &str) -> Option<String> {
    let text = path.to_str()?.replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/'); // `C:/x` is `file:///C:/x`
    }
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push('?');
    uri.push_str(params);
    Some(uri)
}

#[cfg(windows)]
mod detect {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
        FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_ProgramData,
        FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_RoamingAppData, FOLDERID_Videos,
        FOLDERID_Windows, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    use windows_sys::core::GUID;

    use super::{
        CloudRoot, DriveMirrors, Env, KnownFolders, add_cloud, clouds_in, dropbox_roots,
        google_drive_mirrors,
    };

    /// An environment variable naming an absolute path, with 8.3 short names expanded: `%TEMP%` is often spelled
    /// `C:\Users\RUNNER~1\...`, while the scan sees the long names.
    fn var(name: &str) -> Option<PathBuf> {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| long(&p))
    }

    fn long(path: &Path) -> PathBuf {
        crate::pathkey::long_form(path).into_owned()
    }

    fn known(id: &GUID) -> Option<PathBuf> {
        let mut out = std::ptr::null_mut();
        // SAFETY: `id` is a valid folder id; on success `out` is a NUL-terminated string we must free.
        let hr = unsafe {
            SHGetKnownFolderPath(id, KF_FLAG_DEFAULT as u32, std::ptr::null_mut(), &mut out)
        };
        if hr < 0 || out.is_null() {
            if !out.is_null() {
                // SAFETY: allocated by the shell, freed once.
                unsafe { CoTaskMemFree(out.cast()) };
            }
            return None;
        }
        // SAFETY: `out` is NUL-terminated.
        let len = (0..).take_while(|&i| unsafe { *out.add(i) } != 0).count();
        // SAFETY: `out` holds `len` units before the NUL.
        let path = OsString::from_wide(unsafe { std::slice::from_raw_parts(out, len) });
        // SAFETY: allocated by the shell, freed once.
        unsafe { CoTaskMemFree(out.cast()) };
        Some(long(Path::new(&path)))
    }

    /// A known folder from the shell, else an environment variable, else the standard layout. The shell is the
    /// authority (it follows redirection); the variables can be missing in a stripped environment.
    fn place(id: &GUID, variable: &str, standard: &Path) -> PathBuf {
        known(id)
            .or_else(|| var(variable))
            .unwrap_or_else(|| standard.to_path_buf())
    }

    pub(super) fn detect(root: &Path) -> Env {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let base = Env::windows(root, &user);
        let home = place(&FOLDERID_Profile, "USERPROFILE", &base.home);
        let local = place(&FOLDERID_LocalAppData, "LOCALAPPDATA", &base.local_app_data);
        let roaming = place(&FOLDERID_RoamingAppData, "APPDATA", &base.app_data);
        // Sync roots from every source that names them: the cloud-files registrations (OneDrive, Dropbox, Google
        // Drive, iCloud, Box wherever they live), OneDrive's list of its accounts and its variables, Dropbox's own
        // settings, Google Drive's record of the folders it mirrors, and folders in the home folder named after a
        // service. The scan also recognises Google Drive and Dropbox folders by the working folders the clients
        // keep inside them, and cloud-files placeholders by their mark (see the categorizer).
        let mut cloud = Vec::new();
        for (provider, path) in sync_roots::roots() {
            add_cloud(
                &mut cloud,
                CloudRoot {
                    provider,
                    path: long(&path),
                },
            );
        }
        for path in sync_roots::onedrive_accounts() {
            add_cloud(
                &mut cloud,
                CloudRoot {
                    provider: "OneDrive".into(),
                    path: long(&path),
                },
            );
        }
        for name in ["OneDriveConsumer", "OneDriveCommercial", "OneDrive"] {
            if let Some(path) = var(name) {
                add_cloud(
                    &mut cloud,
                    CloudRoot {
                        provider: "OneDrive".into(),
                        path,
                    },
                );
            }
        }
        let mut unknown_clouds = Vec::new();
        let drive = match google_drive_mirrors(&local.join("Google").join("DriveFS")) {
            DriveMirrors::Known(roots) => roots,
            DriveMirrors::Unknown => {
                unknown_clouds.push("Google Drive".to_owned());
                Vec::new()
            }
        };
        for root in dropbox_roots(&[roaming.join("Dropbox"), local.join("Dropbox")])
            .into_iter()
            .chain(drive)
        {
            add_cloud(
                &mut cloud,
                CloudRoot {
                    path: long(&root.path),
                    ..root
                },
            );
        }
        for root in clouds_in(&home) {
            add_cloud(&mut cloud, root);
        }
        Env {
            known: KnownFolders {
                downloads: known(&FOLDERID_Downloads).unwrap_or(base.known.downloads),
                documents: known(&FOLDERID_Documents).unwrap_or(base.known.documents),
                desktop: known(&FOLDERID_Desktop).unwrap_or(base.known.desktop),
                pictures: known(&FOLDERID_Pictures).unwrap_or(base.known.pictures),
                videos: known(&FOLDERID_Videos).unwrap_or(base.known.videos),
                music: known(&FOLDERID_Music).unwrap_or(base.known.music),
            },
            temp: {
                let mut temp = vec![local.join("Temp")];
                if let Some(t) = var("TEMP")
                    && !temp.contains(&t)
                {
                    temp.push(t);
                }
                temp
            },
            cloud,
            unknown_clouds,
            system_root: place(&FOLDERID_Windows, "SystemRoot", &base.system_root),
            program_files: vec![
                place(
                    &FOLDERID_ProgramFiles,
                    "ProgramFiles",
                    &base.program_files[0],
                ),
                place(
                    &FOLDERID_ProgramFilesX86,
                    "ProgramFiles(x86)",
                    &base.program_files[1],
                ),
            ],
            program_data: place(&FOLDERID_ProgramData, "ProgramData", &base.program_data),
            app_data: roaming,
            local_app_data: local,
            home,
            ..base
        }
    }

    /// The sync roots registered with Windows' cloud-files API, for every user:
    /// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager\<provider>!<sid>!<account>\UserSyncRoots`.
    mod sync_roots {
        use std::path::PathBuf;

        use windows_sys::Win32::Foundation::ERROR_SUCCESS;
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ,
            RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
        };

        struct Key(HKEY);

        impl Drop for Key {
            fn drop(&mut self) {
                // SAFETY: opened by `open`, closed once.
                unsafe { RegCloseKey(self.0) };
            }
        }

        fn open(parent: HKEY, sub: &str) -> Option<Key> {
            let wide: Vec<u16> = sub.encode_utf16().chain([0]).collect();
            let mut key: HKEY = std::ptr::null_mut();
            // SAFETY: `wide` is NUL-terminated; `key` is written on success.
            let r = unsafe { RegOpenKeyExW(parent, wide.as_ptr(), 0, KEY_READ, &mut key) };
            (r == ERROR_SUCCESS).then_some(Key(key))
        }

        fn subkeys(key: &Key) -> Vec<String> {
            let mut out = Vec::new();
            for i in 0.. {
                let mut name = [0u16; 512];
                let mut len = name.len() as u32;
                // SAFETY: `name` holds `len` units; the optional outputs are null.
                let r = unsafe {
                    RegEnumKeyExW(
                        key.0,
                        i,
                        name.as_mut_ptr(),
                        &mut len,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if r != ERROR_SUCCESS {
                    break;
                }
                out.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            out
        }

        /// The string values of a key, with their names.
        fn strings(key: &Key) -> Vec<(String, String)> {
            let mut out = Vec::new();
            for i in 0.. {
                let mut name = vec![0u16; 16_384];
                let mut name_len = name.len() as u32;
                let mut data = vec![0u16; 32_768];
                let mut data_len = (data.len() * 2) as u32;
                let mut kind = 0u32;
                // SAFETY: `name` holds `name_len` units and `data` holds `data_len` bytes.
                let r = unsafe {
                    RegEnumValueW(
                        key.0,
                        i,
                        name.as_mut_ptr(),
                        &mut name_len,
                        std::ptr::null(),
                        &mut kind,
                        data.as_mut_ptr().cast(),
                        &mut data_len,
                    )
                };
                if r != ERROR_SUCCESS {
                    break; // no more values (or one too long to be a path, which ends the list early)
                }
                if kind == REG_SZ || kind == REG_EXPAND_SZ {
                    let units = &data[..(data_len as usize / 2).min(data.len())];
                    let text = String::from_utf16_lossy(units);
                    let text = text.trim_end_matches('\0').to_owned();
                    if !text.is_empty() {
                        out.push((String::from_utf16_lossy(&name[..name_len as usize]), text));
                    }
                }
            }
            out
        }

        /// "OneDrive!S-1-5-21-...!Personal" is OneDrive's.
        fn provider(registration: &str) -> String {
            match registration.split('!').next().unwrap_or(registration) {
                "GoogleDriveFS" => "Google Drive".into(),
                "iCloudDrive" => "iCloud Drive".into(),
                p if p.to_ascii_lowercase().starts_with("onedrive") => "OneDrive".into(),
                p => p.to_owned(),
            }
        }

        pub(super) fn roots() -> Vec<(String, PathBuf)> {
            let Some(base) = open(
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager",
            ) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for registration in subkeys(&base) {
                if let Some(users) = open(base.0, &format!(r"{registration}\UserSyncRoots")) {
                    for (_, path) in strings(&users) {
                        let path = PathBuf::from(path);
                        if path.is_absolute() {
                            out.push((provider(&registration), path));
                        }
                    }
                }
            }
            out
        }

        /// OneDrive's own list of its accounts' folders (personal and every work or school account):
        /// `HKCU\Software\Microsoft\OneDrive\Accounts\<account>\UserFolder`.
        pub(super) fn onedrive_accounts() -> Vec<PathBuf> {
            let Some(accounts) = open(HKEY_CURRENT_USER, r"Software\Microsoft\OneDrive\Accounts")
            else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for account in subkeys(&accounts) {
                if let Some(key) = open(accounts.0, &account) {
                    for (name, path) in strings(&key) {
                        let path = PathBuf::from(path);
                        if name.eq_ignore_ascii_case("UserFolder") && path.is_absolute() {
                            out.push(path);
                        }
                    }
                }
            }
            out
        }
    }
}

#[cfg(not(windows))]
mod detect {
    use std::path::{Path, PathBuf};

    use super::{
        CloudRoot, DriveMirrors, Env, add_cloud, clouds_in, dropbox_roots, google_drive_mirrors,
    };

    pub(super) fn detect(root: &Path) -> Env {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let user = home
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut env = Env::macos(root, &user);
        env.known = super::KnownFolders {
            downloads: home.join("Downloads"),
            documents: home.join("Documents"),
            desktop: home.join("Desktop"),
            pictures: home.join("Pictures"),
            videos: home.join("Movies"),
            music: home.join("Music"),
        };
        let library = home.join("Library");
        env.local_app_data = library.join("Application Support");
        env.app_data = library.join("Application Support");
        env.cloud = vec![CloudRoot {
            provider: "iCloud Drive".into(),
            path: library.join("Mobile Documents"),
        }];
        // Dropbox, OneDrive, Google Drive and Box sync through File Provider into ~/Library/CloudStorage.
        if let Ok(entries) = std::fs::read_dir(library.join("CloudStorage")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let provider = name.split(['-', ' ']).next().unwrap_or(&name);
                let provider = match provider {
                    "GoogleDrive" => "Google Drive",
                    p => p,
                };
                env.cloud.push(CloudRoot {
                    provider: provider.to_owned(),
                    path: entry.path(),
                });
            }
        }
        // Dropbox wherever its settings say it is, the folders Google Drive says it mirrors, and sync folders in the
        // home folder found by name.
        let drive = library
            .join("Application Support")
            .join("Google")
            .join("DriveFS");
        let drive = match google_drive_mirrors(&drive) {
            DriveMirrors::Known(roots) => roots,
            DriveMirrors::Unknown => {
                env.unknown_clouds.push("Google Drive".to_owned());
                Vec::new()
            }
        };
        for root in dropbox_roots(&[home.join(".dropbox")])
            .into_iter()
            .chain(drive)
        {
            add_cloud(&mut env.cloud, root);
        }
        for root in clouds_in(&home) {
            add_cloud(&mut env.cloud, root);
        }
        if let Some(tmp) = std::env::var_os("TMPDIR") {
            env.temp.push(PathBuf::from(tmp));
        }
        env.home = home;
        env
    }
}

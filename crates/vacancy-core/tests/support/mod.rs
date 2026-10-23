//! Fixture trees for the core's tests: real folders in a fresh temp dir, built the way each test needs, on whichever
//! OS runs the tests. Never a real home folder.

#![allow(dead_code, reason = "each test file uses its own share of the helpers")]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use vacancy_core::{CancelToken, ScanOptions, Snapshot, scan};

pub const KIB: u64 = 1024;
pub const MIB: u64 = 1024 * 1024;

/// A fixture tree in a temp dir, deleted when dropped.
pub struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = real_path(dir.path());
        Self { _dir: dir, root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(self.root().to_path_buf(), |p, part| p.join(part))
    }

    /// Creates a folder and its parents.
    pub fn dir(&self, rel: &str) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(&p).expect("create dir");
        p
    }

    /// Writes a file of `len` bytes of incompressible-enough data, creating its folder.
    pub fn file(&self, rel: &str, len: u64) -> PathBuf {
        let p = self.path(rel);
        write_file(&p, len);
        p
    }

    /// A file whose length is `len` but which has (almost) nothing allocated.
    pub fn sparse(&self, rel: &str, len: u64) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let f = fs::File::create(&p).expect("create sparse file");
        #[cfg(windows)]
        win::set_sparse(&p);
        f.set_len(len).expect("extend sparse file");
        drop(f);
        p
    }

    /// Scans the fixture with small files folded at 256 KiB.
    pub fn scan(&self) -> Snapshot {
        scan_with(self.root(), 256 * KIB)
    }
}

pub fn write_file(p: &Path, len: u64) {
    fs::create_dir_all(p.parent().unwrap()).expect("create parent");
    let mut f = fs::File::create(p).expect("create file");
    let mut left = len;
    let mut seed = len.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut chunk = vec![0u8; 64 * 1024];
    while left > 0 {
        for b in chunk.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *b = seed as u8;
        }
        let n = left.min(chunk.len() as u64) as usize;
        f.write_all(&chunk[..n]).expect("write");
        left -= n as u64;
    }
    f.sync_all().expect("sync");
}

/// `path` as the file system has it, links resolved. A Mac's temp folder is under `/var`, a link to `/private/var`;
/// what a scan reports is never reached through a link, so fixtures are built at the real path. (Windows' temp folder
/// has no links on the way; its path is kept as it is, 8.3 names and all.)
pub fn real_path(path: &Path) -> PathBuf {
    if cfg!(windows) {
        path.to_path_buf()
    } else {
        path.canonicalize().expect("canonicalize")
    }
}

#[cfg(windows)]
mod windows_case;
#[cfg(windows)]
pub fn make_case_sensitive(dir: &Path) -> bool {
    windows_case::make_case_sensitive(dir)
}

/// Makes everything at and below `path` look last used `days` ago: the modified and accessed times of every file
/// and folder. Call it once the tree is built: creating anything inside afterwards makes its folder new again.
pub fn age(path: &Path, days: u64) {
    let when = std::time::SystemTime::now() - Duration::from_secs(days * 86_400);
    let times = fs::FileTimes::new().set_modified(when).set_accessed(when);
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let dir = fs::symlink_metadata(&p).expect("stat").is_dir();
        if dir {
            for entry in fs::read_dir(&p).expect("list") {
                stack.push(entry.expect("entry").path());
            }
        }
        open_for_times(&p, dir)
            .set_times(times)
            .unwrap_or_else(|e| panic!("set times on {}: {e}", p.display()));
    }
}

#[cfg(windows)]
fn open_for_times(path: &Path, _dir: bool) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
    };
    fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .expect("open for its times")
}

#[cfg(unix)]
fn open_for_times(path: &Path, dir: bool) -> fs::File {
    if dir {
        fs::File::open(path)
    } else {
        fs::OpenOptions::new().write(true).open(path)
    }
    .expect("open for its times")
}

pub fn scan_with(root: &Path, keep_files_from: u64) -> Snapshot {
    scan(
        root,
        &ScanOptions {
            keep_files_from,
            progress_every: Duration::from_millis(5),
            ..ScanOptions::default()
        },
        &mut |_| {},
        &CancelToken::new(),
    )
    .expect("scan the fixture")
}

/// Whether the tests run in CI, where every platform feature they need is available (CI runners are admin on
/// Windows, so symlinks can be created there).
pub fn in_ci() -> bool {
    std::env::var_os("CI").is_some()
}

/// Makes a folder unlistable until the guard drops.
///
/// On Unix: `chmod 000`. On Windows: an ACE denying everyone the right to list it, plus a handle held open with no
/// sharing. CI runs as an administrator, and a deny ACE alone did not always stop it listing the folder; no
/// privilege gets past another handle's sharing mode.
pub struct Unreadable {
    path: PathBuf,
    #[cfg(windows)]
    held: win::Exclusive,
}

impl Unreadable {
    pub fn new(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("chmod 000");
        }
        #[cfg(windows)]
        let held = {
            let held = win::Exclusive::open(path);
            win::deny_listing(path, true);
            held
        };
        Self {
            path: path.to_path_buf(),
            #[cfg(windows)]
            held,
        }
    }
}

impl Drop for Unreadable {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o755));
        }
        #[cfg(windows)]
        {
            self.held.close();
            win::deny_listing(&self.path, false);
        }
    }
}

#[cfg(windows)]
pub mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::process::Command;
    use std::ptr;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FSCTL_SET_REPARSE_POINT, FSCTL_SET_SPARSE};

    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    fn ioctl(path: &Path, code: u32, input: &[u8]) {
        let w = wide(path.as_os_str());
        // SAFETY: `w` is NUL-terminated.
        let h = unsafe {
            CreateFileW(
                w.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert!(
            h != INVALID_HANDLE_VALUE,
            "open {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        let mut returned = 0u32;
        // SAFETY: `input` is readable for its length; no output buffer.
        let ok = unsafe {
            DeviceIoControl(
                h,
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            )
        };
        let err = std::io::Error::last_os_error();
        // SAFETY: `h` is open and not used again.
        unsafe { CloseHandle(h) };
        assert!(ok != 0, "ioctl on {}: {err}", path.display());
    }

    /// A folder held open for reading with no sharing: until it closes, nothing else can list the folder.
    pub struct Exclusive(isize);

    impl Exclusive {
        pub fn open(path: &Path) -> Self {
            let w = wide(path.as_os_str());
            // SAFETY: `w` is NUL-terminated; share mode 0 refuses every other open that wants to read.
            let h = unsafe {
                CreateFileW(
                    w.as_ptr(),
                    GENERIC_READ,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS,
                    ptr::null_mut(),
                )
            };
            assert!(
                h != INVALID_HANDLE_VALUE,
                "hold {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
            Self(h as isize)
        }

        pub fn close(&mut self) {
            if self.0 != 0 {
                // SAFETY: the handle is open and closed only here.
                unsafe { CloseHandle(self.0 as _) };
                self.0 = 0;
            }
        }
    }

    impl Drop for Exclusive {
        fn drop(&mut self) {
            self.close();
        }
    }

    pub fn set_sparse(path: &Path) {
        ioctl(path, FSCTL_SET_SPARSE, &[]);
    }

    /// What `mklink /J link target` does: an empty folder whose mount-point reparse data names `target`.
    pub fn junction(link: &Path, target: &Path) {
        std::fs::create_dir(link).expect("create the junction's folder");
        let target = std::path::absolute(target).unwrap();
        let print: Vec<u16> = target.as_os_str().encode_wide().collect();
        let substitute: Vec<u16> = OsStr::new(r"\??\")
            .encode_wide()
            .chain(print.iter().copied())
            .collect();
        let mut path_buffer = Vec::new();
        path_buffer.extend_from_slice(&substitute);
        path_buffer.push(0);
        path_buffer.extend_from_slice(&print);
        path_buffer.push(0);
        let sub_len = (substitute.len() * 2) as u16;
        let print_len = (print.len() * 2) as u16;
        let data_len = 8 + path_buffer.len() * 2;
        let mut buf = Vec::new();
        buf.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buf.extend_from_slice(&(data_len as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // substitute name offset
        buf.extend_from_slice(&sub_len.to_le_bytes());
        buf.extend_from_slice(&(sub_len + 2).to_le_bytes()); // print name offset
        buf.extend_from_slice(&print_len.to_le_bytes());
        for c in path_buffer {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        ioctl(link, FSCTL_SET_REPARSE_POINT, &buf);
    }

    /// Adds (or removes) an ACE denying everyone the right to list `path`.
    pub fn deny_listing(path: &Path, deny: bool) {
        let args: &[&str] = if deny {
            &["/deny", "*S-1-1-0:(RD)"]
        } else {
            &["/remove:d", "*S-1-1-0"]
        };
        let out = Command::new("icacls")
            .arg(path)
            .args(args)
            .output()
            .expect("run icacls");
        assert!(
            out.status.success(),
            "icacls failed: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

/// Made-up browser profiles for the tests of ADR 0008's browser wall, from dummy files only (never a real
/// browser's): a Chromium user data folder of its own (`--user-data-dir`, with a `Local State`) holding a profile
/// named `Work`, `Default` in the browser's own root under `env`'s home, Opera's root that is its own profile, a
/// Firefox profile kept in a folder of its own, and a game's settings folder with a `Preferences` file and nothing
/// else a browser keeps.
pub struct DummyBrowsers {
    /// Everything that must stay hidden: files and folders in the profiles and roots that aren't caches.
    pub hidden: Vec<PathBuf>,
    /// The cache folders and files in them, which must stay readable.
    pub caches: Vec<PathBuf>,
    /// The profiles and roots themselves.
    pub profiles: Vec<PathBuf>,
    /// The game's settings folder and its files, which are no profile.
    pub ordinary: Vec<PathBuf>,
}

/// The browser roots under `env`'s home: Chrome's own (in Windows' `%LOCALAPPDATA%`, or a Mac's `Application
/// Support`) and Opera's, which is its own profile.
pub fn browser_roots(env: &vacancy_core::Env) -> (PathBuf, PathBuf) {
    let sub = |base: &Path, rel: &str| rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s));
    match env.platform {
        vacancy_core::Platform::Windows => (
            sub(&env.local_app_data, "Google/Chrome/User Data"),
            sub(&env.app_data, "Opera Software/Opera Stable"),
        ),
        vacancy_core::Platform::MacOs => {
            let support = sub(&env.home, "Library/Application Support");
            (
                sub(&support, "Google/Chrome"),
                sub(&support, "com.operasoftware.Opera"),
            )
        }
    }
}

/// What a Chromium profile keeps beside its markers that the by-name rules don't all name (the review of #29's
/// list), and a folder made after any scan would be.
const CHROMIUM_HIDDEN: &[&str] = &[
    "Preferences",
    "History",
    "Sessions/Session_123",
    "Shortcuts",
    "Favicons",
    "DIPS",
    "File System/000/t/00/00000000",
    "WebStorage/1/IndexedDB/x.sqlite3",
    "databases/Databases.db",
    "Sync Data/LevelDB/000003.log",
    "Storage/ext/nkbihfbeogaeaoehlefnkodbefgpgknn/def/Local Storage/x.log",
    "Service Worker/CacheStorage/abc/index",
    "Service Worker/ScriptCache/x",
    "Network Persistent State",
    "TransportSecurity",
    "Made After/x.txt",
];

const CHROMIUM_CACHES: &[&str] = &[
    "Cache/Cache_Data/f_000001",
    "Code Cache/js/index",
    "GPUCache/data_0",
    "DawnCache/data_0",
    "GraphiteDawnCache/data_0",
];

fn sub(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s))
}

impl DummyBrowsers {
    fn put(&mut self, at: PathBuf, hidden: bool, len: u64) {
        write_file(&at, len);
        if hidden {
            self.hidden.push(at);
        } else {
            self.caches.push(at);
        }
    }

    fn chromium(&mut self, profile: &Path, len: u64) {
        self.profiles.push(profile.to_path_buf());
        for f in CHROMIUM_HIDDEN {
            self.put(sub(profile, f), true, len);
        }
        for f in CHROMIUM_CACHES {
            self.put(sub(profile, f), false, len);
        }
        for folder in [
            "Sessions",
            "File System",
            "WebStorage",
            "databases",
            "Sync Data",
            "Storage",
            "Service Worker",
        ] {
            self.hidden.push(profile.join(folder));
        }
        for folder in [
            "Cache",
            "Code Cache",
            "GPUCache",
            "DawnCache",
            "GraphiteDawnCache",
        ] {
            self.caches.push(profile.join(folder));
        }
    }

    /// The hidden places under `base`, which Claude may be asked about.
    pub fn hidden_under(&self, base: &Path) -> Vec<PathBuf> {
        self.hidden
            .iter()
            .filter(|p| p.starts_with(base))
            .cloned()
            .collect()
    }
}

/// Builds [`DummyBrowsers`] under `base` and `env`'s home, each file `len` bytes.
pub fn dummy_browsers(base: &Path, env: &vacancy_core::Env, len: u64) -> DummyBrowsers {
    let mut d = DummyBrowsers {
        hidden: Vec::new(),
        caches: Vec::new(),
        profiles: Vec::new(),
        ordinary: Vec::new(),
    };
    // A user data folder of its own, with the profile `Work`.
    let data = base.join("BrowserData");
    d.profiles.push(data.clone());
    d.put(data.join("Local State"), true, len);
    d.put(data.join("First Run"), true, len);
    d.put(sub(&data, "Crashpad/reports/x.dmp"), true, len);
    d.put(sub(&data, "ShaderCache/data_0"), false, len);
    d.put(sub(&data, "GrShaderCache/data_0"), false, len);
    d.chromium(&data.join("Work"), len);
    // The browser's own root, with `Default`: by its markers, and by its name in a root.
    let (own, opera) = browser_roots(env);
    d.profiles.push(own.clone());
    d.put(own.join("Local State"), true, len);
    d.put(sub(&own, "ShaderCache/data_1"), false, len);
    d.chromium(&own.join("Default"), len);
    // Opera's root, which is its own profile.
    d.put(opera.join("Local State"), true, len);
    d.chromium(&opera, len);
    // A Firefox profile kept in a folder of its own.
    let firefox = sub(base, "FirefoxData/abc.custom");
    d.profiles.push(firefox.clone());
    for f in [
        "prefs.js",
        "times.json",
        "extensions.json",
        "xulstore.json",
        "addonStartup.json.lz4",
        "storage/default/https+++mail.example.com/ls/data.sqlite",
        "datareporting/state.json",
    ] {
        d.put(sub(&firefox, f), true, len);
    }
    for f in [
        "cache2/entries/ABC",
        "startupCache/scriptCache.bin",
        "thumbnails/x.png",
    ] {
        d.put(sub(&firefox, f), false, len);
    }
    // A game's settings: a `Preferences` file, and nothing else a browser keeps.
    let game = base.join("Game Settings");
    for f in ["Preferences", "savegame.dat"] {
        let at = game.join(f);
        write_file(&at, len);
        d.ordinary.push(at);
    }
    d.ordinary.push(game);
    d
}

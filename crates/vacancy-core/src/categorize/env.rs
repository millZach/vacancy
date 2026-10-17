//! The places the categorizer needs to know: the user's home, app data and temp folders, the known folders
//! (Downloads, Documents...) and cloud-sync roots. [`Env::detect`] reads them from the OS; tests build one with
//! [`Env::windows`] or [`Env::macos`] pointing into a fixture tree, so both platforms' rules run on either OS.

use std::path::{Path, PathBuf};

use crate::pathkey::Case;

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
    /// How the volume compares names. Allowed roots and known places are matched this way; deny entries always
    /// ignore case.
    pub case: Case,
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
            system_root: root.join("Windows"),
            program_files: vec![root.join("Program Files"), root.join("Program Files (x86)")],
            program_data: root.join("ProgramData"),
            app_data: home.join("AppData").join("Roaming"),
            case: Case::Insensitive,
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
            system_root: root.join("System"),
            program_files: vec![root.join("Applications")],
            program_data: root.join("Library"),
            local_app_data: library.join("Application Support"),
            app_data: library.join("Application Support"),
            case: Case::Insensitive,
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

    use super::{CloudRoot, Env, KnownFolders, add_cloud, clouds_in, dropbox_roots};

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
        // Drive, iCloud, Box wherever they live), OneDrive's variables, Dropbox's own settings, and folders in the
        // home folder named after a service. One that is missed is still protected by its cloud-files mark when
        // the scan sees it.
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
        for root in dropbox_roots(&[roaming.join("Dropbox"), local.join("Dropbox")]) {
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
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegEnumKeyExW,
            RegEnumValueW, RegOpenKeyExW,
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

        /// The string values of a key: each user's sync root path.
        fn strings(key: &Key) -> Vec<String> {
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
                        out.push(text);
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
                    for path in strings(&users) {
                        let path = PathBuf::from(path);
                        if path.is_absolute() {
                            out.push((provider(&registration), path));
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

    use super::{CloudRoot, Env, add_cloud, clouds_in, dropbox_roots};

    pub(super) fn detect(root: &Path) -> Env {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let user = home
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut env = Env::macos(root, &user);
        env.case = case_of(root);
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
        // Dropbox wherever its settings say it is, and sync folders in the home folder found by name.
        for root in dropbox_roots(&[home.join(".dropbox")]) {
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

    /// Whether the volume at `root` tells names apart by case. If it can't say, it is taken to: allowed roots then
    /// match only exact spellings, which allows less.
    fn case_of(root: &Path) -> super::Case {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(root.as_os_str().as_bytes()) else {
            return super::Case::Sensitive;
        };
        #[cfg(target_os = "macos")]
        // SAFETY: `path` is NUL-terminated; pathconf only reads it.
        let answer = unsafe { libc::pathconf(path.as_ptr(), libc::_PC_CASE_SENSITIVE) };
        #[cfg(not(target_os = "macos"))]
        let answer = {
            let _ = path;
            1
        };
        if answer == 0 {
            super::Case::Insensitive
        } else {
            super::Case::Sensitive
        }
    }
}

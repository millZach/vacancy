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

    use super::{CloudRoot, Env, KnownFolders};

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
        let mut cloud = Vec::new();
        for (name, provider) in [
            ("OneDriveConsumer", "OneDrive"),
            ("OneDriveCommercial", "OneDrive"),
            ("OneDrive", "OneDrive"),
        ] {
            if let Some(path) = var(name)
                && !cloud.iter().any(|c: &CloudRoot| c.path == path)
            {
                cloud.push(CloudRoot {
                    provider: provider.into(),
                    path,
                });
            }
        }
        for (folder, provider) in [
            ("Dropbox", "Dropbox"),
            ("iCloudDrive", "iCloud Drive"),
            ("Google Drive", "Google Drive"),
            ("Box", "Box"),
        ] {
            let path = home.join(folder);
            if path.is_dir() {
                cloud.push(CloudRoot {
                    provider: provider.into(),
                    path,
                });
            }
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
            app_data: place(&FOLDERID_RoamingAppData, "APPDATA", &base.app_data),
            local_app_data: local,
            home,
            ..base
        }
    }
}

#[cfg(not(windows))]
mod detect {
    use std::path::{Path, PathBuf};

    use super::{CloudRoot, Env};

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
        for (folder, provider) in [("Dropbox", "Dropbox"), ("Google Drive", "Google Drive")] {
            let path = home.join(folder);
            if path.is_dir() {
                env.cloud.push(CloudRoot {
                    provider: provider.into(),
                    path,
                });
            }
        }
        if let Some(tmp) = std::env::var_os("TMPDIR") {
            env.temp.push(PathBuf::from(tmp));
        }
        env.home = home;
        env
    }
}

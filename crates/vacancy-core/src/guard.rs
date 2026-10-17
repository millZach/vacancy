//! The Guard: the deny-list every cleanup passes through, whichever rule or AI proposed it (ADR 0002). Nothing can
//! override it; there is no setting, flag or proposal field that turns an entry off.
//!
//! A place passes only if it is strictly inside an allowed root and none of these apply:
//!
//! - **Exact places**: the volume's root, the home folder, its known folders (Downloads, Documents...), app data and
//!   temp roots. The place itself, and anything holding it, is denied; what is inside them may pass.
//! - **Protected places**: the OS, installed apps, cloud-sync roots, other apps' private stores, VM disks. The place,
//!   anything inside it and anything holding it is denied.
//! - **Protected names**, wherever they appear on the path: Photos libraries, Time Machine backups, macOS app bundles,
//!   Windows' Store and game install folders.
//!
//! The allowed roots are the user's home and temp folders on the volume that holds the home folder, and the whole
//! volume (below its root) on any other drive.
//!
//! Places are compared as [`PathKey`]s, so case, `\\?\` prefixes, trailing separators and 8.3 short names don't
//! change the answer. A path that can't be pinned to one place (relative, with `..`, with a stream, or with a short
//! name the file system couldn't expand) is denied.
//!
//! [`Guard::check`] judges a path on its own. A protected place deeper inside a folder (a Photos library somewhere in
//! the Trash) is only visible to whoever has the folder's contents: the rules use the scan for that (see
//! `rules::Guarded`), and the executor (ticket 04) must look again on disk.

use std::path::Path;

use crate::categorize::{Env, Platform};
use crate::pathkey::{PathKey, name_key};

/// Why the Guard refused a place.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    /// The path can't be pinned to one place: relative, with `..`, with a stream, or with a short name that could
    /// not be expanded.
    #[error("the path is not clear enough to act on")]
    Unclear,
    /// It is not inside a place Vacancy may clean.
    #[error("it is outside the places Vacancy may clean")]
    OutsideAllowedRoots,
    /// It is, is inside, or holds a protected place.
    #[error("it is or holds {0}, which Vacancy never touches")]
    Protected(String),
}

/// How far a deny entry reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// The place itself, and anything holding it.
    Exact,
    /// The place, anything inside it, and anything holding it.
    Subtree,
}

#[derive(Clone, Debug)]
struct Entry {
    key: PathKey,
    reach: Reach,
    what: String,
}

/// The deny-list for one volume, built from the [`Env`]. See the module docs.
#[derive(Clone, Debug)]
pub struct Guard {
    platform: Platform,
    allowed: Vec<PathKey>,
    entries: Vec<Entry>,
}

impl Guard {
    /// The Guard for the volume at `volume_root`, with the places `env` names.
    pub fn new(env: &Env, volume_root: &Path) -> Self {
        let mut g = Builder {
            allowed: Vec::new(),
            entries: Vec::new(),
        };
        let root = PathKey::of(volume_root);
        let home = PathKey::of(&env.home);
        let home_here = match (&home, &root) {
            (Some(home), Some(root)) => home.starts_with(root),
            _ => false,
        };
        if home_here {
            g.allow(&env.home);
            // A temp folder outside the home folder (`C:\Temp`) is allowed only if it is called that: `%TEMP%`
            // pointed at some other folder must not open it up.
            for t in &env.temp {
                let named_temp = PathKey::of(t)
                    .and_then(|k| {
                        k.names_from_last()
                            .next()
                            .map(|n| n == "temp" || n == "tmp")
                    })
                    .unwrap_or(false);
                if named_temp {
                    g.allow(t);
                }
            }
        } else {
            g.allow(volume_root);
        }

        g.exact(volume_root, "the drive itself");
        g.exact(&env.home, "your home folder");
        if let Some(users) = env.home.parent() {
            g.exact(users, "the folder of user accounts");
        }
        let k = &env.known;
        g.exact(&k.downloads, "your Downloads folder");
        g.exact(&k.documents, "your Documents folder");
        g.exact(&k.desktop, "your Desktop");
        g.exact(&k.pictures, "your Pictures folder");
        g.exact(&k.videos, "your Videos folder");
        g.exact(&k.music, "your Music folder");
        for t in &env.temp {
            g.exact(t, "a temp folder");
        }
        for c in &env.cloud {
            g.subtree(&c.path, &c.provider);
        }
        match env.platform {
            Platform::Windows => windows(env, volume_root, &mut g),
            Platform::MacOs => macos(env, volume_root, &mut g),
        }
        Self {
            platform: env.platform,
            allowed: g.allowed,
            entries: g.entries,
        }
    }

    /// Whether Vacancy may delete the folder or file at `path`. See the module docs.
    pub fn check(&self, path: &Path) -> Result<(), Denied> {
        self.check_key(&PathKey::of(path).ok_or(Denied::Unclear)?)
    }

    /// [`check`](Self::check) for a path already turned into a key.
    pub(crate) fn check_key(&self, key: &PathKey) -> Result<(), Denied> {
        if key.has_short_alias() {
            return Err(Denied::Unclear);
        }
        for e in &self.entries {
            let hit = key == &e.key
                || e.key.is_inside(key)
                || (e.reach == Reach::Subtree && key.is_inside(&e.key));
            if hit {
                return Err(Denied::Protected(e.what.clone()));
            }
        }
        if let Some(what) = key.names_from_last().find_map(|n| self.protected_name(n)) {
            return Err(Denied::Protected(what.to_owned()));
        }
        if !self.allowed.iter().any(|a| key.is_inside(a)) {
            return Err(Denied::OutsideAllowedRoots);
        }
        Ok(())
    }

    /// What a folder of this name is, if the name alone makes it protected wherever it is. `name` is a key (see
    /// [`name_key`]).
    pub(crate) fn protected_name(&self, name: &str) -> Option<&'static str> {
        let ends = |suffix: &str| name.len() > suffix.len() && name.ends_with(suffix);
        if ends(".photoslibrary") || ends(".photolibrary") || ends(".aplibrary") {
            return Some("a Photos library");
        }
        if matches!(
            name,
            "backups.backupdb" | ".timemachine" | "com.apple.timemachine.localsnapshots"
        ) || ends(".backupbundle")
        {
            return Some("Time Machine backups");
        }
        match self.platform {
            Platform::MacOs if ends(".app") => Some("an app"),
            Platform::Windows if matches!(name, "windowsapps" | "steamapps" | "xboxgames") => {
                Some("installed apps and games")
            }
            _ => None,
        }
    }

    /// [`protected_name`](Self::protected_name) for a name as the OS spells it, and whether a folder that holds
    /// one is protected too. A Photos library or a backup somewhere inside makes the whole folder off-limits; an
    /// app bundle doesn't, because Xcode's build output and archives hold the apps they build, and they may go.
    pub(crate) fn protected_os_name(&self, name: &std::ffi::OsStr) -> Option<(&'static str, bool)> {
        let key = name_key(name);
        let what = self.protected_name(&key)?;
        Some((what, what != "an app"))
    }
}

struct Builder {
    allowed: Vec<PathKey>,
    entries: Vec<Entry>,
}

impl Builder {
    fn allow(&mut self, path: &Path) {
        if let Some(key) = PathKey::of(path) {
            self.allowed.push(key);
        }
    }

    fn exact(&mut self, path: &Path, what: &str) {
        self.add(path, Reach::Exact, what);
    }

    fn subtree(&mut self, path: &Path, what: &str) {
        self.add(path, Reach::Subtree, what);
    }

    fn add(&mut self, path: &Path, reach: Reach, what: &str) {
        // A place the Env names with an unclear path (relative, or with `..`) has no key and can't be compared. The
        // places built here all come from absolute OS answers or the standard layout, so in practice all have one.
        if let Some(key) = PathKey::of(path) {
            self.entries.push(Entry {
                key,
                reach,
                what: what.to_owned(),
            });
        }
    }
}

/// `base` joined with `/`-separated names.
fn sub(base: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/')
        .filter(|p| !p.is_empty())
        .fold(base.to_path_buf(), |p, part| p.join(part))
}

fn windows(env: &Env, volume_root: &Path, g: &mut Builder) {
    let local = &env.local_app_data;
    let roaming = &env.app_data;
    g.exact(&env.home.join("AppData"), "app data");
    g.exact(local, "app data");
    g.exact(roaming, "app data");
    g.exact(&sub(&env.home, "AppData/LocalLow"), "app data");

    g.subtree(&env.system_root, "Windows");
    for pf in &env.program_files {
        g.subtree(pf, "installed programs");
    }
    g.subtree(&env.program_data, "programs' shared data");
    g.subtree(&local.join("Programs"), "installed programs");
    g.subtree(&sub(local, "Microsoft/WindowsApps"), "installed apps");
    g.subtree(&local.join("Packages"), "Store apps and their data");
    g.subtree(&local.join("wsl"), "WSL's Linux disks");
    g.subtree(&local.join("Docker"), "Docker's disks");
    g.subtree(&sub(roaming, "Microsoft/Windows"), "Windows settings");
    g.subtree(&sub(local, "Microsoft/Credentials"), "saved sign-ins");
    g.subtree(&sub(roaming, "Microsoft/Credentials"), "saved sign-ins");

    // Every drive's own system folders, wherever Windows is installed.
    for name in [
        "Windows",
        "Windows.old",
        "Program Files",
        "Program Files (x86)",
        "ProgramData",
        "System Volume Information",
        "$Recycle.Bin",
        "Recovery",
        "$WinREAgent",
        "$Windows.~BT",
        "$Windows.~WS",
        "$SysReset",
        "$GetCurrent",
        "Config.Msi",
        "Boot",
        "EFI",
        "MSOCache",
        "PerfLogs",
        "pagefile.sys",
        "hiberfil.sys",
        "swapfile.sys",
    ] {
        g.subtree(&volume_root.join(name), "Windows");
    }
    g.exact(&volume_root.join("Users"), "the folder of user accounts");
}

fn macos(env: &Env, volume_root: &Path, g: &mut Builder) {
    let home = &env.home;
    let lib = home.join("Library");
    for (rel, what) in [
        ("Library", "your Library"),
        ("Library/Caches", "your Library"),
        ("Library/Logs", "your Library"),
        ("Library/Application Support", "your Library"),
        ("Library/Containers", "your Library"),
        ("Library/Developer", "your Library"),
        ("Library/Developer/Xcode", "your Library"),
        ("Library/Developer/Xcode/DerivedData", "Xcode's folders"),
        ("Library/Developer/Xcode/Archives", "Xcode's folders"),
        (".Trash", "the Trash"),
    ] {
        g.exact(&sub(home, rel), what);
    }
    g.subtree(&home.join("Applications"), "installed apps");
    g.subtree(&lib.join("Mobile Documents"), "iCloud Drive");
    g.subtree(&lib.join("CloudStorage"), "cloud-sync folders");
    for (rel, what) in [
        ("Keychains", "your keychains"),
        ("Mail", "your mail"),
        ("Messages", "your messages"),
        ("Accounts", "your accounts"),
        ("Calendars", "your calendars"),
        ("Group Containers", "apps' shared data"),
        ("Containers/com.docker.docker", "Docker's disks"),
        ("Containers/com.apple.mail/Data/Library/Mail", "your mail"),
    ] {
        g.subtree(&lib.join(sub(Path::new(""), rel)), what);
    }

    for name in [
        "System",
        "Library",
        "Applications",
        "usr",
        "bin",
        "sbin",
        "private",
        "opt",
        "cores",
        "dev",
        "etc",
        "var",
        "tmp",
        "Volumes",
        ".Spotlight-V100",
        ".fseventsd",
        ".Trashes",
        ".TemporaryItems",
        ".DocumentRevisions-V100",
        ".PKInstallSandboxManager",
        ".vol",
    ] {
        g.subtree(&volume_root.join(name), "macOS");
    }
    g.subtree(&sub(volume_root, "Users/Shared"), "shared files");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::categorize::CloudRoot;

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" })
    }

    fn at(rel: &str) -> PathBuf {
        sub(&root(), rel)
    }

    fn win() -> (Env, Guard) {
        let mut env = Env::windows(&root(), "me");
        env.cloud.push(CloudRoot {
            provider: "OneDrive".into(),
            path: at("Users/me/OneDrive"),
        });
        let guard = Guard::new(&env, &root());
        (env, guard)
    }

    fn mac() -> (Env, Guard) {
        let env = Env::macos(&root(), "me");
        let guard = Guard::new(&env, &root());
        (env, guard)
    }

    fn protected(r: Result<(), Denied>) -> bool {
        matches!(r, Err(Denied::Protected(_)))
    }

    #[test]
    fn lets_through_what_is_inside_the_home_and_nothing_else() {
        let (_, g) = win();
        assert_eq!(g.check(&at("Users/me/AppData/Local/Temp/x.tmp")), Ok(()));
        assert_eq!(
            g.check(&at("Users/me/code/app/node_modules")),
            Ok(()),
            "a project inside the home"
        );
        assert_eq!(
            g.check(&at("Users/other/AppData/Local/Temp/x")),
            Err(Denied::OutsideAllowedRoots),
            "another user's home"
        );
        assert_eq!(
            g.check(&at("Tools/cache")),
            Err(Denied::OutsideAllowedRoots)
        );
        assert_eq!(g.check(Path::new("relative/x")), Err(Denied::Unclear));
        assert_eq!(
            g.check(&at("Users/me/Downloads/../OneDrive/x")),
            Err(Denied::Unclear)
        );
    }

    #[test]
    fn never_lets_through_a_known_place_itself_or_what_holds_it() {
        let (_, g) = win();
        for rel in [
            "Users/me",
            "Users/me/Downloads",
            "Users/me/Documents",
            "Users/me/AppData",
            "Users/me/AppData/Local",
            "Users/me/AppData/Local/Temp",
        ] {
            assert!(protected(g.check(&at(rel))), "{rel}");
        }
        assert!(
            g.check(&at("Users/me/Downloads/setup.exe")).is_ok(),
            "inside Downloads is fine"
        );
        assert!(g.check(&at("Users/me/Documents/app/node_modules")).is_ok());
    }

    #[test]
    fn denies_protected_places_inside_and_around() {
        let (_, g) = win();
        for rel in [
            "Users/me/OneDrive",
            "Users/me/OneDrive/Documents/node_modules",
            "Users/me/onedrive/x",
            "Users/me/AppData/Local/Programs/Microsoft VS Code/resources/app/node_modules",
            "Users/me/AppData/Local/Packages/CanonicalGroupLimited.Ubuntu/LocalState/ext4.vhdx",
            "Users/me/AppData/Local/wsl/x/ext4.vhdx",
            "Users/me/AppData/Local/Docker/wsl/disk.vhdx",
            "Users/me/AppData/Local/Microsoft/WindowsApps/python.exe",
        ] {
            assert!(protected(g.check(&at(rel))), "{rel}");
        }
    }

    #[test]
    fn holding_a_protected_place_is_as_bad_as_being_one() {
        let mut env = Env::windows(&root(), "me");
        env.cloud.push(CloudRoot {
            provider: "Dropbox".into(),
            path: at("Users/me/Stuff/Dropbox"),
        });
        let g = Guard::new(&env, &root());
        assert_eq!(
            g.check(&at("Users/me/Stuff")),
            Err(Denied::Protected("Dropbox".into()))
        );
        assert_eq!(g.check(&at("Users/me/Stuff/notes.txt")), Ok(()));
        assert!(protected(g.check(&at("Users/me/Stuff/Dropbox/notes.txt"))));
    }

    #[test]
    fn whole_names_only() {
        let (_, g) = win();
        assert!(
            g.check(&at("Users/me/OneDriveX/cache")).is_ok(),
            "OneDriveX is not OneDrive"
        );
        assert!(g.check(&at("Users/me/AppDataX/thing")).is_ok());
        assert!(protected(g.check(&at("Users/me/AppData"))));
    }

    #[test]
    fn protected_names_anywhere() {
        let (_, g) = mac();
        for rel in [
            "Users/me/.Trash/Old.photoslibrary",
            "Users/me/.Trash/Old.photoslibrary/originals/A/1.heic",
            "Users/me/Downloads/Backups.backupdb/2026",
            "Users/me/Applications/Slack.app",
            "Users/me/code/Demo.app/Contents/node_modules",
        ] {
            assert!(protected(g.check(&at(rel))), "{rel}");
        }
        assert!(
            g.check(&at("Users/me/Library/Developer/Xcode/DerivedData/App-abc"))
                .is_ok()
        );
        let (_, g) = win();
        for rel in [
            "Users/me/Games/steamapps/common/x",
            "Users/me/WindowsApps/x",
        ] {
            assert!(protected(g.check(&at(rel))), "{rel}");
        }
    }

    #[test]
    fn mac_places() {
        let (_, g) = mac();
        for rel in [
            "Users/me/Library",
            "Users/me/Library/Caches",
            "Users/me/Library/Mobile Documents/com~apple~CloudDocs/x",
            "Users/me/Library/CloudStorage/Dropbox/x",
            "Users/me/Library/Mail/V10",
            "Users/me/Library/Containers/com.docker.docker/Data/vms",
            "Users/me/.Trash",
        ] {
            assert!(protected(g.check(&at(rel))), "{rel}");
        }
        for rel in [
            "System/Library/Caches/x",
            "Applications/Xcode.app",
            "private/var/folders/x",
        ] {
            assert!(g.check(&at(rel)).is_err(), "{rel}");
        }
        for rel in [
            "Users/me/Library/Caches/com.spotify.client",
            "Users/me/Library/Containers/com.apple.mail/Data/Library/Mail Downloads/x.pdf",
            "Users/me/.Trash/old.zip",
        ] {
            assert_eq!(g.check(&at(rel)), Ok(()), "{rel}");
        }
    }

    #[test]
    fn another_drive_is_allowed_below_its_root_but_not_its_system_folders() {
        let env = Env::windows(&root(), "me");
        let d = if cfg!(windows) {
            PathBuf::from(r"D:\")
        } else {
            PathBuf::from("/Volumes/Data")
        };
        let g = Guard::new(&env, &d);
        assert_eq!(g.check(&d.join("code").join("node_modules")), Ok(()));
        assert!(protected(g.check(&d)), "the drive itself");
        assert!(protected(
            g.check(&d.join("System Volume Information").join("x"))
        ));
        assert!(protected(g.check(&d.join("Program Files").join("x"))));
        assert_eq!(
            g.check(&at("Users/me/AppData/Local/Temp/x")),
            Err(Denied::OutsideAllowedRoots),
            "C:'s places are not D:'s"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_spellings_dont_get_past() {
        let (_, g) = win();
        for p in [
            r"\\?\C:\Users\me\OneDrive\x",
            r"c:\users\ME\onedrive\x",
            r"C:\Users\me\OneDrive.\x",
            r"C:\Users\me\OneDrive \x",
            r"C:/Users/me/OneDrive/x",
            r"\\.\C:\Users\me\OneDrive\x",
        ] {
            assert!(protected(g.check(Path::new(p))), "{p}");
        }
        assert_eq!(
            g.check(Path::new(r"C:\Users\me\ONEDRI~1\x")),
            Err(Denied::Unclear),
            "a short name that names nothing on this disk"
        );
    }
}

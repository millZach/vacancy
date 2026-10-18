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
//! The allowed roots are, on the drive that holds the home folder, the home folder, temp folders (outside the home
//! folder only if they are called `Temp` or `tmp`) and, on Windows, the folders people make at the drive's root
//! (`C:\Dev`; never `C:\Users`); on any other drive, the whole drive below its root. While a sync client's folders
//! can't be known ([`Env::unknown_clouds`]), they could be anywhere people keep files, so the allowed roots shrink
//! to app data (the Library on a Mac) and the package managers' cache folders in the home folder, for every rule
//! alike.
//!
//! **Authorization is by file-system identity** (see [`crate::fsid`]). The allowed roots and the exact and
//! protected places are resolved once, when the Guard is built, to the identities of the folders they are and of
//! every folder that holds them. A path is judged by its real chain: its volume root and each folder below, opened
//! by name inside the one before and never through a link, compared by identity. Two spellings that fold to the
//! same string (`İpek` and `i` + U+0307 + `pek`, `alex` and `Alex` in a case-sensitive folder) are two places when
//! the file system keeps them apart. A path that can't be walked (relative, with `..`, through a link, or not
//! there) is denied.
//!
//! Names are compared too, as keys that fold case and Unicode form ([`PathKey`]), but only ever to deny: a path
//! spelled like a protected place, or with a protected name on it, is denied whatever its identity. A name never
//! decides that a place may go.
//!
//! [`Guard::check`] judges a path on its own. A protected place deeper inside a folder (a Photos library somewhere in
//! the Trash) is only visible to whoever has the folder's contents: the rules use the scan for that (see
//! `rules::Guarded`), and the executor (ticket 04) must look again on disk.

use std::collections::HashMap;
use std::path::Path;

use crate::categorize::{Env, Platform};
use crate::fsid::{self, Identity, Step, Walker};
use crate::pathkey::{PathKey, name_key};

/// Why the Guard refused a place.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    /// The path can't be pinned to one place: relative, with `..`, with a stream, through a link, not there, or
    /// with a short name that could not be expanded.
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

/// The name a folder's loose files are judged under: a file directly inside it, which no real name can be.
const LOOSE_FILES: &str = "\0";

/// The deny-list for one volume, built from the [`Env`]. See the module docs.
#[derive(Clone, Debug)]
pub struct Guard {
    platform: Platform,
    /// The exact and protected places by name, to deny what is spelled like one.
    entries: Vec<Entry>,
    /// The allowed roots' identities.
    allowed: Vec<Identity>,
    /// Protected places' identities: they and everything inside them are denied.
    inside: HashMap<Identity, String>,
    /// The identities of every exact and protected place and of every folder that holds one: none may go.
    held: HashMap<Identity, String>,
    /// On the Windows drive that holds the home folder, the identities of the drive's root and of the folder of
    /// user accounts: what is inside the folders people make at the root (`C:\Dev`) is allowed too, but never
    /// other users' homes.
    drive_folders: Option<(Identity, Option<Identity>)>,
    /// Fixture mode ([`Env::confined_to`]): nothing passes unless this folder is strictly above it, on its volume.
    /// `Some(None)` when the folder can't be walked to without a link: then nothing passes at all.
    confine: Option<Option<Identity>>,
}

impl Guard {
    /// The Guard for the volume at `volume_root`, with the places `env` names. Reads the file system once, to
    /// resolve those places' identities.
    pub fn new(env: &Env, volume_root: &Path) -> Self {
        let mut g = Builder::default();
        // What grants anything is resolved without following links (see `fsid::identity`): the volume root, whether
        // the home folder is on it, the allowed roots and the folder of user accounts.
        let root = fsid::identity(volume_root);
        let home_here = match fsid::chain(&env.home) {
            Some(home) => root.is_some_and(|r| home.iter().any(|s| s.id == r)),
            // A home folder that can't be walked without a link, or isn't there, is taken to be here, which allows
            // less than another drive would.
            None => true,
        };
        let mut drive_folders = None;
        if !env.unknown_clouds.is_empty() {
            for zone in cache_zones(env) {
                g.allow(&zone);
            }
        } else if home_here {
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
            // Folders at the drive's root are allowed only when the folder of user accounts can be told apart from
            // them.
            if env.platform == Platform::Windows
                && let Some(root) = root
                && let Some(users) = env.home.parent().and_then(fsid::identity)
            {
                drive_folders = Some((root, Some(users)));
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
            entries: g.entries,
            allowed: g.allowed,
            inside: g.inside,
            held: g.held,
            drive_folders,
            // Resolved like an allowed root, without following links: a confining folder reached through a link
            // confines to nothing, and nothing passes.
            confine: env.confined_to.as_deref().map(fsid::identity),
        }
    }

    /// Whether Vacancy may delete the folder or file at `path`. See the module docs.
    pub fn check(&self, path: &Path) -> Result<(), Denied> {
        self.judge(&mut Walker::default(), path, false)
    }

    /// Whether Vacancy may delete the loose files directly inside the folder at `dir` (not the folder itself, which
    /// may be a place that must stay, like a temp folder).
    pub fn check_contents(&self, dir: &Path) -> Result<(), Denied> {
        self.judge(&mut Walker::default(), dir, true)
    }

    /// [`check`](Self::check), or with `contents` [`check_contents`](Self::check_contents), walking with `walker`,
    /// which keeps recently opened folders open for the next path in them. Gives the chain it judged.
    pub(crate) fn check_with(
        &self,
        walker: &mut Walker,
        path: &Path,
        contents: bool,
    ) -> Result<Vec<Step>, Denied> {
        let key = PathKey::of(path).ok_or(Denied::Unclear)?;
        let key = if contents {
            key.child(LOOSE_FILES)
        } else {
            key
        };
        self.deny_by_name(&key)?;
        let chain = walker.chain(path).ok_or(Denied::Unclear)?;
        self.judge_chain(&chain, contents)?;
        Ok(chain)
    }

    /// Judges `path` by a chain someone else walked to it just now (the executor, which keeps the chain's folders
    /// open to delete relative to them): its name, then the chain's identities. With `contents`, what is judged is a
    /// loose file directly inside the folder the chain ends at. The chain must be the path's: its volume root and
    /// one step per name, each opened inside the one before without following a link.
    pub(crate) fn judge_walked(
        &self,
        path: &Path,
        chain: &[Step],
        contents: bool,
    ) -> Result<(), Denied> {
        let key = PathKey::of(path).ok_or(Denied::Unclear)?;
        let key = if contents {
            key.child(LOOSE_FILES)
        } else {
            key
        };
        self.deny_by_name(&key)?;
        let names = path
            .components()
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .count();
        if chain.len() != names + 1 {
            return Err(Denied::Unclear);
        }
        self.judge_chain(chain, contents)
    }

    /// What protected place a folder with this identity is or holds, if any: a folder inside something being
    /// deleted that is one must stay, and so must everything holding it.
    pub(crate) fn protects(&self, id: Identity) -> Option<&str> {
        self.held.get(&id).map(String::as_str)
    }

    /// A nonexistent descendant has no identity to fabricate. Its existing, retained parent authorizes only
    /// contents; all deny-by-name rules still apply to the full proposed relative path.
    #[cfg(windows)]
    pub(crate) fn judge_absent(
        &self,
        parent: &Path,
        chain: &[Step],
        relative: &Path,
    ) -> Result<(), Denied> {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(Denied::Unclear);
        }
        self.judge_walked(parent, chain, true)?;
        self.deny_by_name(&PathKey::of(&parent.join(relative)).ok_or(Denied::Unclear)?)
    }

    fn judge(&self, walker: &mut Walker, path: &Path, contents: bool) -> Result<(), Denied> {
        self.check_with(walker, path, contents).map(|_| ())
    }

    /// Denies what is spelled like an exact or protected place, holds one, or (protected) is inside one, and what
    /// has a protected name on its path. Names only ever deny.
    fn deny_by_name(&self, key: &PathKey) -> Result<(), Denied> {
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
        Ok(())
    }

    /// Judges a path by its chain's identities. With `contents`, the chain is a folder's and what is judged is a
    /// loose file directly inside it. In fixture mode, what passes must also be inside the fixture.
    fn judge_chain(&self, chain: &[Step], contents: bool) -> Result<(), Denied> {
        self.judge_allowed(chain, contents)?;
        let Some(confine) = self.confine else {
            return Ok(());
        };
        let confine = confine.ok_or(Denied::OutsideAllowedRoots)?;
        let above = if contents {
            chain
        } else {
            &chain[..chain.len().saturating_sub(1)]
        };
        let inside = above
            .iter()
            .position(|s| s.id == confine)
            .is_some_and(|at| fsid::one_volume(&chain[at..]));
        if inside {
            Ok(())
        } else {
            Err(Denied::OutsideAllowedRoots)
        }
    }

    /// [`judge_chain`](Self::judge_chain) without fixture mode's confinement.
    fn judge_allowed(&self, chain: &[Step], contents: bool) -> Result<(), Denied> {
        let target = chain.last().ok_or(Denied::Unclear)?;
        if !contents && let Some(what) = self.held.get(&target.id) {
            return Err(Denied::Protected(what.clone()));
        }
        if let Some(what) = chain.iter().find_map(|s| self.inside.get(&s.id)) {
            return Err(Denied::Protected(what.clone()));
        }
        if let Some(what) = chain[1..]
            .iter()
            .find_map(|s| self.protected_name(&name_key(&s.name)))
        {
            return Err(Denied::Protected(what.to_owned()));
        }
        // The folders strictly above what is judged: all of a folder's chain, for its loose files.
        let above = if contents {
            chain
        } else {
            &chain[..chain.len() - 1]
        };
        // An allowed root strictly above, with everything from it down on its volume: a volume mounted inside the
        // home folder is not the home folder.
        let from_allowed = above
            .iter()
            .enumerate()
            .any(|(at, s)| self.allowed.contains(&s.id) && fsid::one_volume(&chain[at..]));
        if from_allowed {
            return Ok(());
        }
        if let Some((root, users)) = self.drive_folders
            && let Some(at) = above.iter().position(|s| s.id == root)
            && fsid::one_volume(&chain[at..])
        {
            // Inside a folder at the drive's root, not the folder itself: at least two names below the root.
            let below = chain.len() - 1 - at + usize::from(contents);
            if below >= 2 {
                let first = &chain[at + 1];
                let name = name_key(&first.name);
                if users.is_none_or(|u| first.id != u)
                    && !TOOLS_AT_ROOT.contains(&name.as_str())
                    && !name.starts_with("python")
                    && !name.starts_with("ruby")
                {
                    return Ok(());
                }
            }
        }
        Err(Denied::OutsideAllowedRoots)
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
            || ends(".sparsebundle")
        {
            // A network Time Machine backup is a `.sparsebundle`; so are some disk images, which are kept too.
            return Some("Time Machine backups");
        }
        match self.platform {
            Platform::MacOs if ends(".app") => Some(APP_BUNDLE),
            Platform::Windows if matches!(name, "windowsapps" | "steamapps" | "xboxgames") => {
                Some("installed apps and games")
            }
            _ => None,
        }
    }

    /// [`protected_name`](Self::protected_name) for a name as the OS spells it, and whether it is an app bundle
    /// (only the DerivedData rule may take a folder holding one: Xcode's build products are the apps it builds).
    pub(crate) fn protected_os_name(&self, name: &std::ffi::OsStr) -> Option<(&'static str, bool)> {
        let key = name_key(name);
        let what = self.protected_name(&key)?;
        Some((what, what == APP_BUNDLE))
    }
}

#[derive(Default)]
struct Builder {
    entries: Vec<Entry>,
    allowed: Vec<Identity>,
    inside: HashMap<Identity, String>,
    held: HashMap<Identity, String>,
}

impl Builder {
    /// Allows what is inside `path`, if `path` is a real folder reached without a link: a root that is a link, or
    /// has one among the folders above it, is not allowed at all, so a cache folder linked to some other folder
    /// never opens that folder up.
    fn allow(&mut self, path: &Path) {
        if let Some(id) = fsid::identity(path) {
            self.allowed.push(id);
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
        // A place that isn't there has no identity: nothing on disk is it, inside it or holding it. Its name still
        // denies. A place to protect is followed through links (`fsid::resolve`): that only protects more.
        let Some(chain) = fsid::resolve(path) else {
            return;
        };
        let Some((place, holders)) = chain.split_last() else {
            return;
        };
        self.held.insert(place.id, what.to_owned());
        if reach == Reach::Subtree {
            self.inside
                .entry(place.id)
                .or_insert_with(|| what.to_owned());
        }
        for h in holders {
            self.held.entry(h.id).or_insert_with(|| what.to_owned());
        }
    }
}

/// Where Vacancy may still clean while a sync client's folders are unknown: app data (the Library on a Mac), which
/// no sync client mirrors, and the package managers' cache folders in the home folder that the rules empty.
fn cache_zones(env: &Env) -> Vec<std::path::PathBuf> {
    let home = &env.home;
    let mut zones = match env.platform {
        Platform::Windows => vec![
            home.join("AppData"),
            env.local_app_data.clone(),
            env.app_data.clone(),
        ],
        Platform::MacOs => vec![home.join("Library")],
    };
    for rel in [".npm/_cacache", ".nuget/packages", ".cargo/registry/cache"] {
        zones.push(sub(home, rel));
    }
    zones
}

/// `base` joined with `/`-separated names.
fn sub(base: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/')
        .filter(|p| !p.is_empty())
        .fold(base.to_path_buf(), |p, part| p.join(part))
}

/// Folders that toolchains and drivers install at the root of the system drive (keys, so lowercase). Not folders
/// people made, so not open to the rules. Python and Ruby installs (`C:\Python312`) are told by their prefix.
const TOOLS_AT_ROOT: &[&str] = &[
    "msys64",
    "msys32",
    "cygwin",
    "cygwin64",
    "mingw64",
    "strawberry",
    "tools",
    "inetpub",
    "intel",
    "amd",
    "nvidia",
    "drivers",
    "android",
    "flutter",
    "xampp",
    "wamp64",
];

/// What the Guard calls a macOS app bundle.
const APP_BUNDLE: &str = "an app";

/// What Windows and installers keep at the root of a drive. Protected on every drive; the rules never look there.
pub(crate) const WINDOWS_DRIVE_SYSTEM: &[&str] = &[
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
    "OneDriveTemp",
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
];

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
    g.subtree(&sub(roaming, "Microsoft/Protect"), "encryption keys");
    g.subtree(&sub(roaming, "Microsoft/Crypto"), "encryption keys");
    g.subtree(
        &sub(roaming, "Microsoft/SystemCertificates"),
        "certificates",
    );
    g.subtree(&sub(local, "Microsoft/Vault"), "saved sign-ins");
    g.subtree(&sub(local, "Microsoft/Outlook"), "your mail");
    g.subtree(&env.home.join(".ssh"), "SSH keys");
    g.subtree(&env.home.join(".gnupg"), "GnuPG keys");

    // Every drive's own system folders, wherever Windows is installed.
    for name in WINDOWS_DRIVE_SYSTEM {
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
    g.subtree(&home.join(".ssh"), "SSH keys");
    g.subtree(&home.join(".gnupg"), "GnuPG keys");
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

    /// A fixture tree standing for a drive: the Guard reads identities, so the places it judges are real folders.
    struct Fx {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Fx {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            // A Mac's temp folder is reached through the `/var` link; a scan never reports such a path.
            let root = if cfg!(windows) {
                dir.path().to_path_buf()
            } else {
                dir.path().canonicalize().unwrap()
            };
            Self { _dir: dir, root }
        }

        /// The folder at `rel` (`/`-separated) below the root, made if it isn't there.
        fn at(&self, rel: &str) -> PathBuf {
            let p = sub(&self.root, rel);
            if !rel.contains("..") {
                std::fs::create_dir_all(&p).unwrap();
            }
            p
        }

        /// The Guard for `env` on the fixture's drive, with the places `env` names made first.
        fn guard(&self, env: &Env) -> Guard {
            self.guard_for(env, &self.root.clone())
        }

        fn guard_for(&self, env: &Env, volume_root: &Path) -> Guard {
            let k = &env.known;
            let mut places = vec![
                &env.home,
                &env.local_app_data,
                &env.app_data,
                &k.downloads,
                &k.documents,
                &k.desktop,
                &k.pictures,
                &k.videos,
                &k.music,
            ];
            places.extend(&env.temp);
            places.extend(env.cloud.iter().map(|c| &c.path));
            for p in places {
                std::fs::create_dir_all(p).unwrap();
            }
            Guard::new(env, volume_root)
        }

        fn win(&self) -> Guard {
            let mut env = Env::windows(&self.root, "me");
            env.cloud.push(CloudRoot {
                provider: "OneDrive".into(),
                path: sub(&self.root, "Users/me/OneDrive"),
            });
            self.guard(&env)
        }

        fn mac(&self) -> Guard {
            self.guard(&Env::macos(&self.root, "me"))
        }
    }

    fn protected(r: Result<(), Denied>) -> bool {
        matches!(r, Err(Denied::Protected(_)))
    }

    #[test]
    fn lets_through_what_is_inside_the_home_and_nothing_else() {
        let fx = Fx::new();
        let g = fx.win();
        assert_eq!(g.check(&fx.at("Users/me/AppData/Local/Temp/x.tmp")), Ok(()));
        assert_eq!(
            g.check(&fx.at("Users/me/code/app/node_modules")),
            Ok(()),
            "a project inside the home"
        );
        assert_eq!(
            g.check(&fx.at("Users/other/AppData/Local/Temp/x")),
            Err(Denied::OutsideAllowedRoots),
            "another user's home"
        );
        assert_eq!(
            g.check(&fx.at("Tools")),
            Err(Denied::OutsideAllowedRoots),
            "a folder at the drive's root is never removed whole"
        );
        assert_eq!(
            g.check(&fx.at("Tools/x")),
            Err(Denied::OutsideAllowedRoots),
            "nor is what toolchains keep there"
        );
        assert_eq!(
            g.check(&fx.at("Dev/app/node_modules")),
            Ok(()),
            "but what is inside a folder people made there may be"
        );
        assert_eq!(g.check(Path::new("relative/x")), Err(Denied::Unclear));
        assert_eq!(
            g.check(&fx.at("Users/me/Downloads/../OneDrive/x")),
            Err(Denied::Unclear)
        );
        assert_eq!(
            g.check(&sub(&fx.root, "Users/me/code/not-there")),
            Err(Denied::Unclear),
            "what isn't there can't be vouched for"
        );
    }

    #[test]
    fn never_lets_through_a_known_place_itself_or_what_holds_it() {
        let fx = Fx::new();
        let g = fx.win();
        for rel in [
            "Users/me",
            "Users/me/Downloads",
            "Users/me/Documents",
            "Users/me/AppData",
            "Users/me/AppData/Local",
            "Users/me/AppData/Local/Temp",
        ] {
            assert!(protected(g.check(&fx.at(rel))), "{rel}");
        }
        assert!(
            g.check(&fx.at("Users/me/Downloads/setup.exe")).is_ok(),
            "inside Downloads is fine"
        );
        assert!(
            g.check(&fx.at("Users/me/Documents/app/node_modules"))
                .is_ok()
        );
        assert_eq!(
            g.check_contents(&fx.at("Users/me/AppData/Local/Temp")),
            Ok(()),
            "a temp folder's loose files, not the folder"
        );
    }

    #[test]
    fn denies_protected_places_inside_and_around() {
        let fx = Fx::new();
        let g = fx.win();
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
            assert!(protected(g.check(&fx.at(rel))), "{rel}");
        }
    }

    #[test]
    fn holding_a_protected_place_is_as_bad_as_being_one() {
        let fx = Fx::new();
        let mut env = Env::windows(&fx.root, "me");
        env.cloud.push(CloudRoot {
            provider: "Dropbox".into(),
            path: fx.at("Users/me/Stuff/Dropbox"),
        });
        let g = fx.guard(&env);
        assert_eq!(
            g.check(&fx.at("Users/me/Stuff")),
            Err(Denied::Protected("Dropbox".into()))
        );
        assert_eq!(g.check(&fx.at("Users/me/Stuff/notes.txt")), Ok(()));
        assert!(protected(
            g.check(&fx.at("Users/me/Stuff/Dropbox/notes.txt"))
        ));
    }

    /// A protected place's identity protects it under any name: here a cloud root the Env names through a link,
    /// whose real folder is spelled nothing like it.
    #[test]
    fn a_protected_place_is_protected_by_identity_under_any_name() {
        let fx = Fx::new();
        let real = fx.at("Users/me/Sync Area/Box Files");
        let link = sub(&fx.root, "Users/me/Box");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&real)
                .output()
                .unwrap();
            assert!(made.status.success(), "mklink /J");
        }
        let mut env = Env::windows(&fx.root, "me");
        env.cloud.push(CloudRoot {
            provider: "Box".into(),
            path: link,
        });
        let g = fx.guard(&env);
        assert_eq!(
            g.check(&fx.at("Users/me/Sync Area/Box Files/code/node_modules")),
            Err(Denied::Protected("Box".into()))
        );
        assert_eq!(
            g.check(&fx.at("Users/me/Sync Area")),
            Err(Denied::Protected("Box".into())),
            "and what holds it"
        );
    }

    /// An allowed root reached through a link allows nothing: here `$TMPDIR` names a Temp folder through a link
    /// (a junction on Windows, a symlink on macOS) in the home folder to a folder outside it, which must not open
    /// that folder up.
    #[test]
    fn an_allowed_root_behind_a_link_allows_nothing() {
        let fx = Fx::new();
        let real = fx.at("Elsewhere");
        let old = fx.at("Elsewhere/Temp/old");
        let link = sub(&fx.root, "Users/me/Linked");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&real)
                .output()
                .unwrap();
            assert!(made.status.success(), "mklink /J");
        }
        let mut env = Env::macos(&fx.root, "me");
        env.temp = vec![link.join("Temp")];
        let g = fx.guard(&env);
        assert_eq!(g.check(&old), Err(Denied::OutsideAllowedRoots));
        assert_eq!(
            g.check(&fx.at("Users/me/code/node_modules")),
            Ok(()),
            "the control"
        );
    }

    #[test]
    fn whole_names_only() {
        let fx = Fx::new();
        let g = fx.win();
        assert!(
            g.check(&fx.at("Users/me/OneDriveX/cache")).is_ok(),
            "OneDriveX is not OneDrive"
        );
        assert!(g.check(&fx.at("Users/me/AppDataX/thing")).is_ok());
        assert!(protected(g.check(&fx.at("Users/me/AppData"))));
    }

    #[test]
    fn protected_names_anywhere() {
        let fx = Fx::new();
        let g = fx.mac();
        for rel in [
            "Users/me/.Trash/Old.photoslibrary",
            "Users/me/.Trash/Old.photoslibrary/originals/A/1.heic",
            "Users/me/Downloads/Backups.backupdb/2026",
            "Users/me/.Trash/MacBook.sparsebundle/bands/0",
            "Users/me/Applications/Slack.app",
            "Users/me/code/Demo.app/Contents/node_modules",
        ] {
            assert!(protected(g.check(&fx.at(rel))), "{rel}");
        }
        assert!(
            g.check(&fx.at("Users/me/Library/Developer/Xcode/DerivedData/App-abc"))
                .is_ok()
        );
        let fx = Fx::new();
        let g = fx.win();
        for rel in [
            "Users/me/Games/steamapps/common/x",
            "Users/me/WindowsApps/x",
        ] {
            assert!(protected(g.check(&fx.at(rel))), "{rel}");
        }
    }

    #[test]
    fn mac_places() {
        let fx = Fx::new();
        let g = fx.mac();
        for rel in [
            "Users/me/Library",
            "Users/me/Library/Caches",
            "Users/me/Library/Mobile Documents/com~apple~CloudDocs/x",
            "Users/me/Library/CloudStorage/Dropbox/x",
            "Users/me/Library/Mail/V10",
            "Users/me/Library/Containers/com.docker.docker/Data/vms",
            "Users/me/.Trash",
        ] {
            assert!(protected(g.check(&fx.at(rel))), "{rel}");
        }
        for rel in [
            "System/Library/Caches/x",
            "Applications/Xcode.app",
            "private/var/folders/x",
        ] {
            assert!(g.check(&fx.at(rel)).is_err(), "{rel}");
        }
        for rel in [
            "Users/me/Library/Caches/com.spotify.client",
            "Users/me/Library/Containers/com.apple.mail/Data/Library/Mail Downloads/x.pdf",
            "Users/me/.Trash/old.zip",
        ] {
            assert_eq!(g.check(&fx.at(rel)), Ok(()), "{rel}");
        }
    }

    /// A cloud root recorded with a precomposed `é` (as Dropbox's `info.json` writes it) protects the folder spelled
    /// with `e` and a combining accent, by name, whether or not the file system treats them as one.
    #[test]
    fn one_name_in_either_unicode_form() {
        let fx = Fx::new();
        let mut env = Env::macos(&fx.root, "alex");
        env.cloud.push(CloudRoot {
            provider: "Dropbox".into(),
            path: sub(&fx.root, "Users/alex/Caf\u{e9} Sync"),
        });
        let g = fx.guard(&env);
        assert!(protected(
            g.check(&fx.at("Users/alex/Cafe\u{301} Sync/node_modules"))
        ));
        assert!(protected(
            g.check(&fx.at("Users/alex/Caf\u{e9} Sync/node_modules"))
        ));
    }

    /// Whether the file system keeps two spellings of a folder apart: two folders made under them are two.
    fn kept_apart(a: &Path, b: &Path) -> bool {
        fsid::identity(a).unwrap() != fsid::identity(b).unwrap()
    }

    /// `İpek` and `i` + U+0307 + `pek` lowercase to the same string, but NTFS keeps them apart: two users' homes.
    /// Only the home folder the file system means is allowed; where the file system treats them as one name, both
    /// spellings are that folder.
    #[test]
    fn turkish_dotted_i_names_another_home_where_the_file_system_says_so() {
        let fx = Fx::new();
        let env = Env::windows(&fx.root, "\u{130}pek");
        let g = fx.guard(&env);
        let mine = fx.at("Users/\u{130}pek/code/app/node_modules");
        let other = fx.at("Users/i\u{307}pek/code/app/node_modules");
        assert_eq!(g.check(&mine), Ok(()));
        let apart = kept_apart(
            &sub(&fx.root, "Users/\u{130}pek"),
            &sub(&fx.root, "Users/i\u{307}pek"),
        );
        assert_eq!(
            g.check(&other),
            if apart {
                Err(Denied::OutsideAllowedRoots)
            } else {
                Ok(())
            },
            "kept apart: {apart}"
        );
        if cfg!(windows) {
            assert!(apart, "NTFS keeps them apart");
        }
    }

    /// `Jos` + `é` and `Jose` + U+0301: NTFS keeps the two Unicode forms apart, APFS doesn't. The Guard agrees with
    /// whichever file system it is on.
    #[test]
    fn unicode_forms_name_another_home_where_the_file_system_says_so() {
        let fx = Fx::new();
        let g = fx.guard(&Env::windows(&fx.root, "Jos\u{e9}"));
        let other = fx.at("Users/Jose\u{301}/code/node_modules");
        let apart = kept_apart(
            &sub(&fx.root, "Users/Jos\u{e9}"),
            &sub(&fx.root, "Users/Jose\u{301}"),
        );
        assert_eq!(
            g.check(&other),
            if apart {
                Err(Denied::OutsideAllowedRoots)
            } else {
                Ok(())
            },
            "kept apart: {apart}"
        );
        if cfg!(windows) {
            assert!(apart, "NTFS keeps them apart");
        }
    }

    /// Windows can make one folder case-sensitive: `alex` and `Alex` in it are two users' homes.
    /// Explicitly unavailable where the system doesn't allow the native per-directory flag.
    #[cfg(windows)]
    #[test]
    fn a_case_sensitive_folder_on_windows_keeps_homes_apart() {
        let fx = Fx::new();
        let users = fx.at("Users");
        if !make_case_sensitive(&users) {
            return;
        }
        let mine = fx.at("Users/alex/code/app/node_modules");
        let other = fx.at("Users/Alex/code/app/node_modules");
        assert!(
            kept_apart(&sub(&fx.root, "Users/alex"), &sub(&fx.root, "Users/Alex")),
            "two folders"
        );
        let g = fx.guard(&Env::windows(&fx.root, "alex"));
        assert_eq!(g.check(&mine), Ok(()));
        assert_eq!(g.check(&other), Err(Denied::OutsideAllowedRoots));
        eprintln!(
            "EXERCISED: case-sensitive Guard scenario, native flag verified and two distinct homes checked"
        );
    }

    /// Verifies the native flag on this test-owned directory, without a shell or global policy changes.
    /// Ancestors belong to the fresh Fx fixture and are not raced; this is not an adversarial path sandbox.
    #[cfg(windows)]
    pub(crate) fn make_case_sensitive(dir: &Path) -> bool {
        use std::fs::{File, OpenOptions};
        use std::io;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, FileAttributeTagInfo,
            FileCaseSensitiveInfo, FileIdInfo, GetFileInformationByHandleEx,
            SetFileInformationByHandle,
        };
        // FILE_CS_FLAG_CASE_SENSITIVE_DIR, defined by the Windows SDK for FILE_CASE_SENSITIVE_INFO.
        const CASE_SENSITIVE: u32 = 1;

        fn flags(file: &File) -> io::Result<u32> {
            let mut info = FILE_CASE_SENSITIVE_INFO::default();
            // SAFETY: the live handle and matching writable information buffer outlive the call.
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileCaseSensitiveInfo,
                    (&raw mut info).cast(),
                    std::mem::size_of_val(&info) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(info.Flags)
        }
        let enable = || -> io::Result<()> {
            let chain = crate::fsid::chain(dir)
                .ok_or_else(|| io::Error::other("fixture ancestry is missing or linked"))?;
            let expected = chain
                .last()
                .ok_or_else(|| io::Error::other("empty fixture ancestry"))?
                .id;
            let file = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(dir)?;
            let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
            // SAFETY: the live handle and correctly sized writable native buffer outlive the query.
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileAttributeTagInfo,
                    (&raw mut tag).cast(),
                    std::mem::size_of_val(&tag) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
                || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(io::Error::other(
                    "fixture leaf is not an ordinary directory",
                ));
            }
            let mut identity = FILE_ID_INFO::default();
            // SAFETY: the live handle and correctly sized writable native buffer outlive the query.
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileIdInfo,
                    (&raw mut identity).cast(),
                    std::mem::size_of_val(&identity) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if crate::fsid::Identity::new(
                identity.VolumeSerialNumber,
                u128::from_le_bytes(identity.FileId.Identifier),
            ) != expected
            {
                return Err(io::Error::other(
                    "opened fixture directory identity changed",
                ));
            }
            let before = flags(&file)?;
            if before & CASE_SENSITIVE != 0 {
                eprintln!(
                    "VERIFIED: native case-sensitive directory flag already enabled ({before:#x})"
                );
                return Ok(());
            }
            let info = FILE_CASE_SENSITIVE_INFO {
                Flags: before | CASE_SENSITIVE,
            };
            // SAFETY: this ordinary fixture handle has WRITE_ATTRIBUTES; the matching buffer is live.
            let set = unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileCaseSensitiveInfo,
                    (&raw const info).cast(),
                    std::mem::size_of_val(&info) as u32,
                )
            };
            let failure = if set == 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
            let after = flags(&file);
            if let Some(error) = failure {
                return Err(io::Error::other(format!(
                    "native set failed: {error}; before={before:#x}; after={after:?}"
                )));
            }
            let after = after?;
            if after & CASE_SENSITIVE == 0 {
                return Err(io::Error::other(format!(
                    "native set reported success but flag is absent: before={before:#x}; after={after:#x}"
                )));
            }
            eprintln!(
                "VERIFIED: native case-sensitive directory flag before={before:#x}, after={after:#x}"
            );
            Ok(())
        };
        match enable() {
            Ok(()) => true,
            Err(error) => {
                eprintln!("UNAVAILABLE: optional Windows case-sensitive Guard scenario: {error}");
                false
            }
        }
    }

    /// A disk image mounted inside the home folder is another volume: nothing on it inherits the home folder's
    /// standing. Skipped where `hdiutil` can't make or mount one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_volume_mounted_inside_the_home_authorizes_nothing() {
        use std::process::Command;

        struct Detach(PathBuf);
        impl Drop for Detach {
            fn drop(&mut self) {
                let _ = Command::new("hdiutil")
                    .args(["detach", "-quiet", "-force"])
                    .arg(&self.0)
                    .status();
            }
        }

        let fx = Fx::new();
        let g = fx.guard(&Env::macos(&fx.root, "alex"));
        let mnt = fx.at("Users/alex/code/mnt");
        let images = tempfile::tempdir().unwrap();
        let image = images.path().join("vacancy-test.dmg");
        // Skipped where hdiutil can't do it, except in CI, where it must: there the test has to run.
        let ran = |what: &str, cmd: &mut Command| -> bool {
            match cmd.output() {
                Ok(out) if out.status.success() => true,
                out => {
                    let why = match out {
                        Ok(out) => String::from_utf8_lossy(&out.stderr).into_owned(),
                        Err(e) => e.to_string(),
                    };
                    assert!(std::env::var_os("CI").is_none(), "hdiutil {what}: {why}");
                    eprintln!("skipped: hdiutil {what}: {why}");
                    false
                }
            }
        };
        let made = ran(
            "create",
            Command::new("hdiutil")
                .args(["create", "-quiet", "-size", "4m", "-fs", "HFS+"])
                .args(["-volname", "VacancyTest"])
                .arg(&image),
        );
        if !made {
            return;
        }
        let attached = ran(
            "attach",
            Command::new("hdiutil")
                .args(["attach", "-quiet", "-nobrowse", "-mountpoint"])
                .arg(&mnt)
                .arg(&image),
        );
        if !attached {
            return;
        }
        let _detach = Detach(mnt.clone());
        let on_it = mnt.join("proj").join("node_modules");
        std::fs::create_dir_all(&on_it).unwrap();
        assert_eq!(g.check(&on_it), Err(Denied::OutsideAllowedRoots));
        assert_eq!(
            g.check(&fx.at("Users/alex/code/other/node_modules")),
            Ok(()),
            "the control"
        );
    }

    #[test]
    fn another_drive_is_allowed_below_its_root_but_not_its_system_folders() {
        let fx = Fx::new();
        let env = Env::windows(&fx.root, "me");
        let d = fx.at("Volumes/Data");
        let g = fx.guard_for(&env, &d);
        assert_eq!(g.check(&fx.at("Volumes/Data/code/node_modules")), Ok(()));
        assert!(protected(g.check(&d)), "the drive itself");
        assert!(protected(
            g.check(&fx.at("Volumes/Data/System Volume Information/x"))
        ));
        assert!(protected(g.check(&fx.at("Volumes/Data/Program Files/x"))));
        assert_eq!(
            g.check(&fx.at("Users/me/AppData/Local/Temp/x")),
            Err(Denied::OutsideAllowedRoots),
            "C:'s places are not D:'s"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_spellings_dont_get_past() {
        let fx = Fx::new();
        let g = fx.win();
        fx.at("Users/me/OneDrive/x");
        let r = fx.root.display().to_string();
        for p in [
            format!(r"\\?\{r}\Users\me\OneDrive\x"),
            format!(r"{}\users\ME\onedrive\x", r.to_lowercase()),
            format!(r"{r}\Users\me\OneDrive.\x"),
            format!(r"{r}\Users\me\OneDrive \x"),
            format!("{}/Users/me/OneDrive/x", r.replace('\\', "/")),
            format!(r"\\.\{r}\Users\me\OneDrive\x"),
        ] {
            assert!(protected(g.check(Path::new(&p))), "{p}");
        }
        assert_eq!(
            g.check(Path::new(&format!(r"{r}\Users\me\NOTHER~1\x"))),
            Err(Denied::Unclear),
            "a short name that names nothing on this disk"
        );
    }
}

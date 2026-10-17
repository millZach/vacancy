//! The known-location catalogs, one per platform, and the categories' names and blurbs.
//!
//! A rule names a place and what it is. Places are built from the [`Env`], so the same catalog describes a real
//! drive or a fixture tree. When two rules name the same place the first one wins, so specific rules come first.

use std::path::{Path, PathBuf};

use super::{CategoryId, Env, Platform};
use crate::text::group_digits;

/// What a known place is.
#[derive(Clone, Debug)]
pub(super) struct Rule {
    pub(super) at: PathBuf,
    /// Its category, or `None` to keep the one it inherits.
    pub(super) category: Option<CategoryId>,
    /// Its subfolders are items of their own on the map.
    pub(super) container: bool,
    /// Below it, cache-named folders are caches.
    pub(super) app_data: bool,
    /// Below it, no rule changes anything (cloud roots).
    pub(super) seal: bool,
    /// For cloud roots, the provider's name.
    pub(super) label: String,
    /// Below it, Store app folders named after a Linux distribution hold a WSL disk (Developer).
    pub(super) packages: bool,
}

struct Rules(Vec<Rule>);

impl Rules {
    fn add(&mut self, at: impl AsRef<Path>, category: CategoryId) -> &mut Rule {
        self.0.push(Rule {
            at: at.as_ref().to_path_buf(),
            category: Some(category),
            container: false,
            app_data: false,
            seal: false,
            label: String::new(),
            packages: false,
        });
        self.0.last_mut().expect("just pushed")
    }

    /// A place whose subfolders are items, in the category it inherits.
    fn container(&mut self, at: impl AsRef<Path>) -> &mut Rule {
        self.0.push(Rule {
            at: at.as_ref().to_path_buf(),
            category: None,
            container: true,
            app_data: false,
            seal: false,
            label: String::new(),
            packages: false,
        });
        self.0.last_mut().expect("just pushed")
    }
}

/// `base` joined with a relative path written with either separator, so a Windows catalog builds the same
/// places when its tests run on macOS, and the other way round.
fn sub(base: &Path, rel: &str) -> PathBuf {
    rel.split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

impl Rule {
    fn container(&mut self) -> &mut Self {
        self.container = true;
        self
    }

    fn app_data(&mut self) -> &mut Self {
        self.app_data = true;
        self
    }
}

pub(super) fn rules(env: &Env) -> Vec<Rule> {
    let mut r = Rules(Vec::new());
    // Cloud roots first: whatever else is said about a place inside one, it stays synced and untouched.
    for cloud in &env.cloud {
        let rule = r.add(&cloud.path, CategoryId::Cloud);
        rule.seal = true;
        rule.label = cloud.provider.clone();
    }
    let k = &env.known;
    r.add(&k.downloads, CategoryId::Downloads);
    for p in [&k.documents, &k.desktop, &k.pictures, &k.videos, &k.music] {
        r.add(p, CategoryId::Media);
    }
    for t in &env.temp {
        r.add(t, CategoryId::Caches);
    }
    match env.platform {
        Platform::Windows => windows(env, &mut r),
        Platform::MacOs => macos(env, &mut r),
    }
    r.0
}

fn windows(env: &Env, r: &mut Rules) {
    let root = &env.root;
    let sys = &env.system_root;
    let home = &env.home;
    let local = &env.local_app_data;
    let roaming = &env.app_data;

    // The page, hibernation and swap files sit at the root of the system drive.
    for file in ["pagefile.sys", "hiberfil.sys", "swapfile.sys"] {
        r.add(root.join(file), CategoryId::System);
    }
    for dir in [
        "System Volume Information",
        "Recovery",
        "$WinREAgent",
        "Config.Msi",
        "Windows.old",
        "$Windows.~BT",
        "$Windows.~WS",
    ] {
        r.add(root.join(dir), CategoryId::System);
    }
    r.add(root.join("$Recycle.Bin"), CategoryId::Caches);

    // Windows itself, except what it throws away on its own.
    r.add(sys.join("Temp"), CategoryId::Caches);
    r.add(sys.join("SystemTemp"), CategoryId::Caches);
    r.add(
        sub(sys, r"SoftwareDistribution\Download"),
        CategoryId::Caches,
    );
    r.add(sys.join("LiveKernelReports"), CategoryId::Caches);
    r.add(sys.join("Minidump"), CategoryId::Caches);
    r.add(sys.join("MEMORY.DMP"), CategoryId::Caches);
    r.add(
        sub(
            sys,
            r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization",
        ),
        CategoryId::Caches,
    );
    r.add(sys, CategoryId::System);

    for pf in &env.program_files {
        r.add(pf, CategoryId::Apps).container();
    }
    r.add(
        sub(&env.program_data, r"Microsoft\Windows\WER"),
        CategoryId::Caches,
    );
    r.add(env.program_data.join("Docker"), CategoryId::Dev);
    r.add(env.program_data.join("DockerDesktop"), CategoryId::Dev);
    // Windows' own shared data: Defender, the search index, error reporting's settings.
    r.add(env.program_data.join("Microsoft"), CategoryId::System);
    // The rest belongs to installed programs: installer caches, licences, shared libraries.
    r.add(&env.program_data, CategoryId::Apps)
        .container()
        .app_data();

    // The home folder: developer tools first, then app data, which is mostly Other.
    for dev in DEV_HOME {
        r.add(home.join(dev), CategoryId::Dev);
    }
    for dev in [".nuget", "source", "VirtualBox VMs"] {
        r.add(home.join(dev), CategoryId::Dev);
    }
    r.add(home.join(".cache"), CategoryId::Caches);
    // Package managers' download caches, named here because their folder names look like app caches.
    for dev in [r"pip\cache", r"Yarn\Cache", r"uv\cache", r"pypoetry\Cache"] {
        r.add(sub(local, dev), CategoryId::Dev);
    }
    for dev in [
        "Docker",
        "wsl", // WSL's Linux disks
        r"Android\Sdk",
        "npm-cache",
        "pnpm",
        "pnpm-cache",
        "Yarn",
        "pip",
        "uv",
        "pypoetry",
        "NuGet",
        "go-build",
        "node-gyp",
        "ms-playwright",
        "JetBrains",
        r"Microsoft\VisualStudio",
        r"Microsoft\vscode-cpptools",
    ] {
        r.add(sub(local, dev), CategoryId::Dev);
    }
    for dev in [
        "npm",
        "npm-cache",
        "Code",
        "Code - Insiders",
        "Cursor",
        "Windsurf",
        "JetBrains",
    ] {
        r.add(roaming.join(dev), CategoryId::Dev);
    }
    // Apps that install into, or keep their data in, app data.
    for app in [
        "AnthropicClaude",
        "Discord",
        "slack",
        "GitHubDesktop",
        "Postman",
    ] {
        r.add(local.join(app), CategoryId::Apps);
    }
    for app in [
        "Claude",
        "discord",
        "Slack",
        "Spotify",
        r"Microsoft\Teams",
        "Zoom",
        "obs-studio",
    ] {
        r.add(sub(roaming, app), CategoryId::Apps);
    }
    r.add(sub(local, r"Spotify\Data"), CategoryId::Caches); // Spotify's streaming cache
    r.add(local.join("Programs"), CategoryId::Apps).container();
    r.add(sub(local, r"Microsoft\WindowsApps"), CategoryId::Apps);
    r.add(local.join("CrashDumps"), CategoryId::Caches);
    r.add(sub(local, r"Microsoft\Windows\WER"), CategoryId::Caches);
    r.add(
        sub(local, r"Microsoft\Windows\INetCache"),
        CategoryId::Caches,
    );
    r.add(
        sub(local, r"Microsoft\Windows\Explorer"),
        CategoryId::Caches,
    ); // thumbnail and icon caches
    r.add(local.join("D3DSCache"), CategoryId::Caches);
    r.add(sub(local, r"NVIDIA\DXCache"), CategoryId::Caches);
    r.add(sub(local, r"NVIDIA\GLCache"), CategoryId::Caches);
    r.container(local.join("Packages")).app_data().packages = true;
    r.container(local).app_data();
    r.container(roaming).app_data();
    r.container(home.join("AppData").join("LocalLow"))
        .app_data();
    r.container(home.join("AppData"));
    r.container(home);
    if let Some(users) = home.parent() {
        r.container(users);
    }
    r.container(root);
}

fn macos(env: &Env, r: &mut Rules) {
    let root = &env.root;
    let home = &env.home;
    let lib = home.join("Library");

    // Package managers' download caches sit among app caches but belong to developer tools.
    for dev in ["Homebrew", "pip", "Yarn", "go-build", "pnpm", "JetBrains"] {
        r.add(lib.join("Caches").join(dev), CategoryId::Dev);
    }
    r.add(lib.join("Caches"), CategoryId::Caches);
    r.add(lib.join("Logs"), CategoryId::Caches);
    r.add(home.join(".Trash"), CategoryId::Caches);
    r.add(
        sub(
            &lib,
            "Containers/com.apple.mail/Data/Library/Mail Downloads",
        ),
        CategoryId::Caches,
    );
    r.add(lib.join("Developer"), CategoryId::Dev);
    r.add(sub(&lib, "Containers/com.docker.docker"), CategoryId::Dev);
    // File Provider sync roots (Dropbox, OneDrive, Google Drive) live here, one folder each.
    r.add(lib.join("CloudStorage"), CategoryId::Cloud)
        .container();
    for dev in DEV_HOME {
        r.add(home.join(dev), CategoryId::Dev);
    }
    for dev in [".pyenv", ".nvm", ".rbenv", "VirtualBox VMs"] {
        r.add(home.join(dev), CategoryId::Dev);
    }
    for dev in ["Code", "Code - Insiders", "Cursor", "Windsurf", "JetBrains"] {
        r.add(lib.join("Application Support").join(dev), CategoryId::Dev);
    }
    for app in ["Claude", "Slack", "discord", "Spotify"] {
        r.add(lib.join("Application Support").join(app), CategoryId::Apps);
    }
    r.add(home.join(".cache"), CategoryId::Caches);
    r.add(home.join("Applications"), CategoryId::Apps)
        .container();
    r.container(lib.join("Application Support")).app_data();
    r.container(lib.join("Containers")).app_data();
    r.container(lib.join("Group Containers")).app_data();
    r.container(&lib).app_data();
    r.container(home);
    r.container(root.join("Users"));

    r.add(root.join("Applications"), CategoryId::Apps)
        .container();
    r.add(sub(root, r"Library/Application Support"), CategoryId::Apps)
        .container();
    r.add(sub(root, r"Library/Audio"), CategoryId::Apps);
    r.add(sub(root, r"Library/Caches"), CategoryId::Caches);
    r.add(sub(root, r"Library/Logs"), CategoryId::Caches);
    r.add(sub(root, r"Library/Developer"), CategoryId::Dev);
    r.add(root.join("Library"), CategoryId::System);
    r.add(sub(root, r"private/var/folders"), CategoryId::Caches);
    r.add(sub(root, r"private/tmp"), CategoryId::Caches);
    r.add(sub(root, r"private/var/log"), CategoryId::Caches);
    r.add(root.join("private"), CategoryId::System);
    r.add(root.join("cores"), CategoryId::Caches);
    r.add(root.join("opt"), CategoryId::Dev); // Homebrew on Apple silicon
    r.add(sub(root, r"usr/local"), CategoryId::Dev); // Homebrew on Intel
    for sys in [
        "System",
        "usr",
        "bin",
        "sbin",
        ".Spotlight-V100",
        ".fseventsd",
        ".DocumentRevisions-V100",
        "MobileSoftwareUpdate",
    ] {
        r.add(root.join(sys), CategoryId::System);
    }
    r.container(root);
}

/// Developer tools' folders in the home folder, on both platforms: toolchains, package stores, editors' and AI
/// coding agents' state, local models.
const DEV_HOME: [&str; 27] = [
    ".cargo",
    ".rustup",
    ".npm",
    ".gradle",
    ".m2",
    ".docker",
    ".android",
    ".pnpm-store",
    ".yarn",
    ".bun",
    ".deno",
    ".dotnet",
    ".conda",
    "anaconda3",
    "miniconda3",
    ".local",
    ".vscode",
    ".vscode-insiders",
    ".cursor",
    ".windsurf",
    ".continue",
    ".codex",
    ".claude",
    ".gemini",
    ".ollama",
    ".lmstudio",
    "go",
];

/// Folder names apps use for caches they rebuild on their own: browsers, Electron apps, GPU shader caches.
pub(super) fn is_cache_name(name: &str) -> bool {
    const NAMES: [&str; 17] = [
        "cache",
        "caches",
        "code cache",
        "cacheddata",
        "gpucache",
        "cache2",
        "cache_data",
        "shadercache",
        "dawncache",
        "dawngraphitecache",
        "dawnwebgpucache",
        "grshadercache",
        "graphitedawncache",
        "cachestorage",
        "crashpad",
        "crashdumps",
        "inetcache",
    ];
    NAMES.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// Store package folders of Linux distributions, which hold a WSL disk: `CanonicalGroupLimited.Ubuntu_...`,
/// `TheDebianProject.DebianGNULinux_...` and the like.
pub(super) fn is_linux_package(name: &str) -> bool {
    const PUBLISHERS: [&str; 7] = [
        "canonicalgrouplimited.",
        "thedebianproject.",
        "46932suse.",
        "kalilinux.",
        "whitewaterfoundryltd.",
        "almalinux.",
        "oracleamericainc.",
    ];
    let lower = name.to_ascii_lowercase();
    PUBLISHERS.iter().any(|p| lower.starts_with(p))
}

/// A category's name and blurb on a platform. `cloud` names the sync services found; `files` is the category's
/// file count.
pub(super) fn describe(
    id: CategoryId,
    platform: Platform,
    cloud: &[String],
    files: u64,
) -> (String, String) {
    let mac = platform == Platform::MacOs;
    let (name, blurb): (String, String) = match id {
        CategoryId::Apps if mac => ("Applications".into(), "Apps and the content they install.".into()),
        CategoryId::Apps => ("Apps & games".into(), "Installed programs and games.".into()),
        CategoryId::Dev if mac => (
            "Developer".into(),
            "Xcode, simulators, code, project dependencies and containers.".into(),
        ),
        CategoryId::Dev => (
            "Developer".into(),
            "Code, project dependencies, build output, containers and SDKs.".into(),
        ),
        CategoryId::Media => ("Photos, videos & documents".into(), "Your personal files.".into()),
        CategoryId::Photos => (
            "Photos Library".into(),
            "Your Photos library. Vacancy never edits it; use Optimize Mac Storage in Photos instead.".into(),
        ),
        CategoryId::System if mac => (
            "macOS & System Data".into(),
            "macOS, local Time Machine snapshots and swap. Vacancy never touches this.".into(),
        ),
        CategoryId::System => ("Windows & system".into(), "Windows itself. Vacancy never touches this.".into()),
        CategoryId::Downloads => ("Downloads".into(), format!("{} files you downloaded.", group_digits(files))),
        CategoryId::Caches if mac => (
            "Caches, logs & Trash".into(),
            "Leftovers that macOS and your apps rebuild on their own.".into(),
        ),
        CategoryId::Caches => (
            "Caches & temp".into(),
            "Leftovers that Windows and your apps rebuild on their own.".into(),
        ),
        CategoryId::Cloud => {
            let mut names: Vec<&str> = Vec::new();
            for c in cloud {
                if !names.contains(&c.as_str()) {
                    names.push(c);
                }
            }
            let name = match names.as_slice() {
                [] => "Cloud sync".to_owned(),
                [one] => (*one).to_owned(),
                [rest @ .., last] => format!("{} & {last}", rest.join(", ")),
            };
            (
                name,
                "Synced folders. Vacancy leaves sync roots alone so nothing disappears from your other devices."
                    .into(),
            )
        }
        CategoryId::Other => ("Other".into(), "Everything else.".into()),
    };
    (name, blurb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cloud_sync_after_what_was_found() {
        let none: Vec<String> = Vec::new();
        assert_eq!(
            describe(CategoryId::Cloud, Platform::Windows, &none, 0).0,
            "Cloud sync"
        );
        let two = vec![
            "OneDrive".to_owned(),
            "Dropbox".to_owned(),
            "OneDrive".to_owned(),
        ];
        assert_eq!(
            describe(CategoryId::Cloud, Platform::Windows, &two, 0).0,
            "OneDrive & Dropbox"
        );
        let three = vec![
            "iCloud Drive".to_owned(),
            "Dropbox".to_owned(),
            "Box".to_owned(),
        ];
        assert_eq!(
            describe(CategoryId::Cloud, Platform::MacOs, &three, 0).0,
            "iCloud Drive, Dropbox & Box"
        );
    }
}

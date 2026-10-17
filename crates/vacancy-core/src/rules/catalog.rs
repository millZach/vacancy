//! The rules catalogs, one per platform. A rule is data: what it is (id, category, risk, whether it comes back on
//! its own, the default), how it finds things, what it leaves alone, and how it acts. Places are built from the
//! [`Env`], so the same catalog describes a real computer or a fixture tree.
//!
//! Order matters: a place an earlier rule took is not offered by a later one, so specific rules come before
//! general ones (package managers' caches before app caches, browsers before other apps).
//!
//! Texts may hold `{n}` (places found), `{s}` (an "s" unless `{n}` is 1), `{files}` and `{fs}` (the same for
//! files), `{labels}` (what the places belong to: "Edge and Chrome"), `{labels_n}` (how many) and `{ls}` (an "s"
//! unless that is 1).
//!
//! Tool and hint rules arrive in ticket 06.

use std::path::{Path, PathBuf};

use super::{ActionKind, Risk};
use crate::categorize::{CategoryId, Env, Platform};
use crate::guard::WINDOWS_DRIVE_SYSTEM;
use crate::pathkey::PathKey;
use crate::snapshot::Markers;

/// One rule. See the module docs.
#[derive(Debug)]
pub(super) struct Rule {
    pub(super) id: &'static str,
    pub(super) category: CategoryId,
    pub(super) risk: Risk,
    /// What it removes comes back on its own if needed.
    pub(super) regenerable: bool,
    /// Whether a new plan includes it; `None` means only if it is safe.
    pub(super) default_on: Option<bool>,
    /// What it does with each place it finds: deletes what is inside, deletes it whole, runs a tool, or gives advice.
    /// Tool and hint rules arrive in ticket 06; until then the engine makes no proposals for them.
    pub(super) action: ActionKind,
    pub(super) find: Find,
    pub(super) skip: Skip,
    /// It takes Xcode's DerivedData, whose build products are the apps Xcode builds: a folder holding an app bundle
    /// is not off-limits to it. Every other rule (dependencies, Rust build output and Xcode archives included)
    /// leaves alone any folder that holds one.
    pub(super) takes_built_apps: bool,
    pub(super) text: Text,
}

/// How a rule finds places.
#[derive(Debug)]
pub(super) enum Find {
    /// Known places. After the root, a name may be `*` (any folder) or `*.ext` (any folder ending so).
    Places(Vec<Place>),
    /// Folders with one of these names (any case) anywhere below `under`, not inside another match, code
    /// libraries or `prune`. With `chromium`, a folder named just `Cache` or `Caches` counts only with evidence
    /// that it is Chromium's own disk cache: a `Cache_Data` folder inside it, or a Chromium profile around it (a
    /// `Local Storage` folder or a `Preferences` file beside it). Chromium's other caches beside it (`GPUCache`,
    /// `Code Cache`) say nothing about it: an app can keep its own data in a folder called `Cache`.
    Named {
        names: &'static [&'static str],
        under: Vec<PathBuf>,
        prune: Vec<PathBuf>,
        chromium: bool,
    },
    /// A project's dependency or build folder: a folder named `folder` (any case) whose parent has the `project`
    /// marker and the `proof`, in a project nobody has used for `idle_days`. Looked for below `under`, never
    /// inside `prune`, tools' dot-folders or code libraries.
    Projects {
        folder: &'static str,
        project: Markers,
        proof: Proof,
        idle_days: u32,
        under: Vec<PathBuf>,
        prune: Vec<PathBuf>,
    },
    /// Files kept by the scan (1 MB or more) with one of the `anywhere` extensions below `under`, or one of the
    /// `top_only` ones directly in it.
    Files {
        under: PathBuf,
        anywhere: &'static [&'static str],
        top_only: &'static [&'static str],
    },
}

/// What shows a project folder is a real project's, not part of an app that ships its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Proof {
    /// A Node lockfile beside `package.json`, or a Git repository around the project.
    NodeLock,
    /// Cargo's own files in the folder (`CACHEDIR.TAG`, `.rustc_info.json`).
    CargoTarget,
}

/// A known place, and what to call what is found there.
#[derive(Debug)]
pub(super) struct Place {
    pub(super) path: PathBuf,
    pub(super) label: Option<&'static str>,
}

/// What a rule leaves alone.
#[derive(Debug, Default)]
pub(super) struct Skip {
    /// Only what nobody has used for this many days (0: no limit).
    pub(super) idle_days: u32,
    /// Folders with these names (any case).
    pub(super) names: &'static [&'static str],
    /// Places belonging to an app that is running (macOS caches).
    pub(super) running_apps: bool,
    /// The whole rule, while one of these apps is running.
    pub(super) unless_running: &'static [&'static str],
}

/// What the plan tray says about a rule's proposal.
#[derive(Debug)]
pub(super) struct Text {
    pub(super) title: &'static str,
    pub(super) summary: &'static str,
    pub(super) why: &'static str,
}

/// The rules for `env`'s platform, in order.
pub(super) fn rules(env: &Env) -> Vec<Rule> {
    match env.platform {
        Platform::Windows => windows(env),
        Platform::MacOs => macos(env),
    }
}

/// `base` joined with `/`-separated names, so a catalog builds the same places on either OS.
fn sub(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

fn place(base: &Path, rel: &str, label: &'static str) -> Place {
    Place {
        path: sub(base, rel),
        label: Some(label),
    }
}

fn unlabelled(path: PathBuf) -> Place {
    Place { path, label: None }
}

/// Folder names Electron apps, browsers and games use for caches they rebuild.
/// Folder names Chromium, Electron and WebView2 use for the caches they rebuild. `Cache` only counts with
/// Chromium's layout around it (see `Find::Named`).
const APP_CACHES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "GraphiteDawnCache",
    "ShaderCache",
    "CachedData",
];

/// Plain cache folder names, which apps other than browsers use as they like: proposed for review only.
const PLAIN_CACHES: &[&str] = &["Cache", "Caches"];

/// The temp folders the temp rule empties: only those called `Temp` or `tmp`. A `%TEMP%` pointed at some other
/// folder (`C:\Users\me\Projects`, `C:\Tools`) is not a reason to empty it.
fn temp_places(env: &Env) -> Vec<Place> {
    env.temp
        .iter()
        .filter(|t| {
            PathKey::of(t).is_some_and(|key| {
                key.names_from_last()
                    .next()
                    .is_some_and(|n| n == "temp" || n == "tmp")
            })
        })
        .cloned()
        .map(unlabelled)
        .collect()
}

/// Where to look for projects, as the Guard allows: the home folder on the drive that holds it (and on Windows
/// the folders at that drive's root, like `C:\Dev`), or the whole of any other drive. Temp folders are left to the
/// temp rule.
fn project_roots(env: &Env) -> Vec<PathBuf> {
    let home_here = match (PathKey::of(&env.home), PathKey::of(&env.root)) {
        (Some(home), Some(root)) => home.starts_with(&root),
        _ => false,
    };
    match (home_here, env.platform) {
        (true, Platform::Windows) => vec![env.home.clone(), env.root.clone()],
        (true, Platform::MacOs) => vec![env.home.clone()],
        (false, _) => vec![env.root.clone()],
    }
}

/// Where never to look for projects: app data and the Library (apps ship their own `node_modules`), cloud-sync
/// roots, the users' folder (the home folder is searched on its own) and what Windows keeps at a drive's root.
/// Tools' dot-folders in the home folder are skipped as well, by name.
fn project_prune(env: &Env, mut places: Vec<PathBuf>) -> Vec<PathBuf> {
    places.extend(env.cloud.iter().map(|c| c.path.clone()));
    places.extend(env.home.parent().map(Path::to_path_buf));
    if env.platform == Platform::Windows {
        places.extend(WINDOWS_DRIVE_SYSTEM.iter().map(|n| env.root.join(n)));
    }
    places
}

fn node_modules(env: &Env, prune: Vec<PathBuf>) -> Rule {
    Rule {
        id: if env.platform == Platform::Windows {
            "w-node"
        } else {
            "m-node"
        },
        category: CategoryId::Dev,
        risk: Risk::Safe,
        regenerable: true,
        default_on: None,
        action: ActionKind::DeleteDirectory,
        find: Find::Projects {
            folder: "node_modules",
            project: Markers::PACKAGE_JSON,
            proof: Proof::NodeLock,
            idle_days: IDLE_PROJECT_DAYS,
            under: project_roots(env),
            prune,
        },
        skip: Skip::default(),
        takes_built_apps: false,
        text: Text {
            title: "node_modules in {n} idle project{s}",
            summary: "Dependencies in projects you haven't touched for 90 days or more.",
            why: "Run npm install in a project to bring them back. Projects you used in the last 90 days are left \
                  alone.",
        },
    }
}

fn rust_targets(env: &Env, prune: Vec<PathBuf>) -> Rule {
    Rule {
        id: if env.platform == Platform::Windows {
            "w-rust"
        } else {
            "m-rust"
        },
        category: CategoryId::Dev,
        risk: Risk::Safe,
        regenerable: true,
        default_on: None,
        action: ActionKind::DeleteDirectory,
        find: Find::Projects {
            folder: "target",
            project: Markers::CARGO_TOML,
            proof: Proof::CargoTarget,
            idle_days: IDLE_PROJECT_DAYS,
            under: project_roots(env),
            prune,
        },
        skip: Skip::default(),
        takes_built_apps: false,
        text: Text {
            title: "Rust build output in {n} project{s}",
            summary: "target folders from cargo builds, in projects you haven't touched for 90 days or more.",
            why: "cargo build makes them again. Source code is untouched, and projects you used in the last 90 \
                  days are left alone.",
        },
    }
}

/// A project nobody has used for this long is idle.
const IDLE_PROJECT_DAYS: u32 = 90;

fn windows(env: &Env) -> Vec<Rule> {
    let home = &env.home;
    let local = &env.local_app_data;
    let roaming = &env.app_data;
    let low = sub(home, "AppData/LocalLow");
    let app_data = vec![local.clone(), roaming.clone(), low];

    let browsers = {
        let mut p = Vec::new();
        for (base, label) in [
            ("Microsoft/Edge/User Data", "Edge"),
            ("Google/Chrome/User Data", "Chrome"),
            ("BraveSoftware/Brave-Browser/User Data", "Brave"),
            ("Vivaldi/User Data", "Vivaldi"),
        ] {
            for cache in ["Cache", "Code Cache", "GPUCache"] {
                p.push(place(local, &format!("{base}/*/{cache}"), label));
            }
            for cache in ["ShaderCache", "GrShaderCache", "GraphiteDawnCache"] {
                p.push(place(local, &format!("{base}/{cache}"), label));
            }
        }
        p.push(place(local, "Mozilla/Firefox/Profiles/*/cache2", "Firefox"));
        p.push(place(local, "Microsoft/Windows/INetCache", "Windows"));
        p
    };

    vec![
        Rule {
            id: "w-temp",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(temp_places(env)),
            skip: Skip {
                idle_days: 1,
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "Temp folders",
                summary: "{files} temporary file{fs} left behind by installers and apps.",
                why: "Files in Temp are meant to be thrown away. Anything changed in the last day is left alone, and \
                      so is anything in use when the plan runs.",
            },
        },
        Rule {
            id: "w-browser",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(browsers),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Browser caches",
                summary: "Pages and images {labels} saved to load sites faster.",
                why: "Browsers download these again as you browse. You stay signed in, and history, passwords and \
                      bookmarks are untouched.",
            },
        },
        Rule {
            id: "w-pkgcache",
            category: CategoryId::Dev,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![
                place(local, "npm-cache", "npm"),
                place(roaming, "npm-cache", "npm"),
                place(local, "pip/cache", "pip"),
                place(local, "Yarn/Cache", "Yarn"),
                place(local, "uv/cache", "uv"),
                place(local, "NuGet/v3-cache", "NuGet"),
                place(home, ".nuget/packages", "NuGet"),
                place(home, ".cargo/registry/cache", "Cargo"),
                place(local, "go-build", "Go"),
                place(local, "pypoetry/Cache/cache", "Poetry"),
                place(local, "pypoetry/Cache/artifacts", "Poetry"),
            ]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Package manager caches",
                summary: "Download caches from {labels}.",
                why: "Package managers download anything they need again. Installed tools and your projects keep \
                      working.",
            },
        },
        Rule {
            id: "w-dumps",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: false,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![
                unlabelled(local.join("CrashDumps")),
                unlabelled(sub(local, "Microsoft/Windows/WER/ReportArchive")),
                unlabelled(sub(local, "Microsoft/Windows/WER/ReportQueue")),
            ]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Crash dumps & error reports",
                summary: "{files} crash dump{fs} and error report{fs} from apps that crashed.",
                why: "They're only useful for sending to a developer right after a crash.",
            },
        },
        Rule {
            id: "w-appcache",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Named {
                names: APP_CACHES,
                under: app_data,
                // Temp belongs to the temp rule, which leaves alone what was used in the last day.
                prune: env.temp.clone(),
                chromium: true,
            },
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "App caches",
                summary: "Browser-engine caches that {labels} rebuild on their own.",
                why: "Apps built on Chromium, Electron or WebView2 fill these again as you use them. Your settings, \
                      sign-ins and files are elsewhere and are untouched.",
            },
        },
        Rule {
            id: "w-appcache-other",
            category: CategoryId::Caches,
            risk: Risk::Review,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Named {
                names: PLAIN_CACHES,
                under: vec![
                    local.clone(),
                    roaming.clone(),
                    sub(home, "AppData/LocalLow"),
                ],
                prune: env.temp.clone(),
                chromium: false,
            },
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Other app caches",
                summary: "Folders named Cache in {labels}.",
                why: "Most apps keep only caches in folders with this name, but nothing guarantees it: an app may keep \
                      data it can't fetch again there. Check the sample paths before adding this.",
            },
        },
        Rule {
            id: "w-gpucache",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![
                place(local, "D3DSCache", "DirectX"),
                place(local, "NVIDIA/DXCache", "NVIDIA"),
                place(local, "NVIDIA/GLCache", "NVIDIA"),
                place(local, "AMD/DxCache", "AMD"),
                place(local, "Spotify/Data", "Spotify"),
            ]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Shader and streaming caches",
                summary: "Caches from {labels}.",
                why: "Games and apps rebuild these. The first launch afterwards can take a little longer.",
            },
        },
        node_modules(env, project_prune(env, vec![home.join("AppData")])),
        rust_targets(env, project_prune(env, vec![home.join("AppData")])),
        Rule {
            id: "w-installers",
            category: CategoryId::Downloads,
            risk: Risk::Review,
            regenerable: false,
            default_on: Some(true),
            action: ActionKind::DeleteDirectory,
            find: Find::Files {
                under: env.known.downloads.clone(),
                anywhere: &["msi", "msix", "msixbundle", "appx", "appxbundle"],
                top_only: &["exe"],
            },
            skip: Skip {
                idle_days: 30,
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "Old installers in Downloads",
                summary: "{n} .exe and .msi file{s} older than 30 days.",
                why: "These programs are already installed or can be downloaded again. Check the list if you keep \
                      offline installers.",
            },
        },
    ]
}

fn macos(env: &Env) -> Vec<Rule> {
    let home = &env.home;
    let lib = home.join("Library");
    let support = lib.join("Application Support");
    vec![
        Rule {
            id: "m-brew",
            category: CategoryId::Dev,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![
                place(&lib, "Caches/Homebrew", "Homebrew"),
                place(home, ".npm/_cacache", "npm"),
                place(&lib, "Caches/pip", "pip"),
                place(&lib, "Caches/Yarn", "Yarn"),
                place(&lib, "Caches/go-build", "Go"),
                place(&lib, "Caches/pypoetry/cache", "Poetry"),
                place(&lib, "Caches/pypoetry/artifacts", "Poetry"),
                place(home, ".cargo/registry/cache", "Cargo"),
            ]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Homebrew & package caches",
                summary: "Downloaded bottles and package archives from {labels}.",
                why: "Same as brew cleanup: installed packages keep working, and package managers download anything \
                      they need again.",
            },
        },
        Rule {
            id: "m-derived",
            category: CategoryId::Dev,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteDirectory,
            find: Find::Places(vec![unlabelled(sub(&lib, "Developer/Xcode/DerivedData/*"))]),
            skip: Skip {
                unless_running: &["Xcode", "com.apple.dt.Xcode"],
                ..Skip::default()
            },
            takes_built_apps: true,
            text: Text {
                title: "Xcode DerivedData",
                summary: "Build files for {n} project{s} you opened in Xcode.",
                why: "Xcode rebuilds this on your next build. It's the same as Clean Build Folder for every project. \
                      Left alone while Xcode is open.",
            },
        },
        Rule {
            id: "m-archives",
            category: CategoryId::Dev,
            risk: Risk::Review,
            regenerable: false,
            default_on: None,
            action: ActionKind::DeleteDirectory,
            find: Find::Places(vec![unlabelled(sub(
                &lib,
                "Developer/Xcode/Archives/*/*.xcarchive",
            ))]),
            skip: Skip {
                idle_days: 365,
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "Xcode archives older than a year",
                summary: "{n} archive{s} of builds you already shipped.",
                why: "Keep any archive you might need to symbolicate crash reports for an old release.",
            },
        },
        Rule {
            id: "m-logs",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: false,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![unlabelled(lib.join("Logs"))]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Logs & diagnostic reports",
                summary: "{files} log file{fs} and crash report{fs} from apps.",
                why: "They're only useful right after a crash. macOS and your apps keep writing new ones.",
            },
        },
        Rule {
            id: "m-mail",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![unlabelled(sub(
                &lib,
                "Containers/com.apple.mail/Data/Library/Mail Downloads",
            ))]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Mail attachment downloads",
                summary: "Copies of {files} attachment{fs} you opened from Mail.",
                why: "The originals stay in your mailbox. Mail downloads them again when you open them.",
            },
        },
        Rule {
            id: "m-trash",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: false,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![unlabelled(home.join(".Trash"))]),
            skip: Skip::default(),
            takes_built_apps: false,
            text: Text {
                title: "Empty the Trash",
                summary: "{files} file{fs} already in the Trash.",
                why: "You already threw these away.",
            },
        },
        Rule {
            id: "m-appcache",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Named {
                names: APP_CACHES,
                under: vec![support.clone()],
                prune: env.temp.clone(),
                chromium: true,
            },
            skip: Skip {
                running_apps: true,
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "App caches",
                summary: "Browser-engine caches that {labels} rebuild on their own.",
                why: "Apps built on Chromium or Electron fill these again as you use them. Apps that are running are \
                      left alone, and your settings and files are untouched.",
            },
        },
        Rule {
            id: "m-appcache-other",
            category: CategoryId::Caches,
            risk: Risk::Review,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Named {
                names: PLAIN_CACHES,
                under: vec![support.clone()],
                prune: env.temp.clone(),
                chromium: false,
            },
            skip: Skip {
                running_apps: true,
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "Other app caches",
                summary: "Folders named Cache in {labels}.",
                why: "Most apps keep only caches in folders with this name, but nothing guarantees it: an app may keep \
                      data it can't fetch again there. Check the sample paths before adding this.",
            },
        },
        Rule {
            id: "m-caches",
            category: CategoryId::Caches,
            risk: Risk::Safe,
            regenerable: true,
            default_on: None,
            action: ActionKind::DeleteContents,
            find: Find::Places(vec![unlabelled(sub(&lib, "Caches/*"))]),
            skip: Skip {
                running_apps: true,
                // iCloud's and the system's account caches: rebuilding them means syncing again.
                names: &[
                    "com.apple.bird",
                    "CloudKit",
                    "com.apple.cloudd",
                    "com.apple.akd",
                    "FamilyCircle",
                ],
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "Library caches",
                summary: "Caches in your Library, from {labels_n} app{ls}.",
                why: "Apps rebuild caches as needed. Caches of apps that are running are left alone.",
            },
        },
        node_modules(env, project_prune(env, vec![lib.clone()])),
        rust_targets(env, project_prune(env, vec![lib.clone()])),
        Rule {
            id: "m-dmgs",
            category: CategoryId::Downloads,
            risk: Risk::Review,
            regenerable: false,
            default_on: Some(true),
            action: ActionKind::DeleteDirectory,
            find: Find::Files {
                under: env.known.downloads.clone(),
                anywhere: &["dmg", "pkg", "mpkg"],
                top_only: &[],
            },
            skip: Skip {
                idle_days: 30,
                ..Skip::default()
            },
            takes_built_apps: false,
            text: Text {
                title: "Old disk images in Downloads",
                summary: "{n} .dmg and .pkg file{s} older than 30 days.",
                why: "These apps are installed or can be downloaded again.",
            },
        },
    ]
}

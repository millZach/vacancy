//! The places that always stay hidden from Claude, even with privacy mode off (ADR 0008): keys and sign-ins,
//! browser data, and password managers and wallets. One list serves twice:
//!
//! - **Claude Code enforces it**: [`deny_rules`] turns it into `permissions.deny` rules for Read, Glob and Grep.
//!   Real probes (docs/proof/t12-deny-probes) showed one `Read(...)` rule also keeps Glob from listing the place
//!   and Grep from searching it, that Windows paths match case-insensitively and through `..`, and that a short
//!   (8.3) name like `SSH~1` is refused outright.
//! - **Vacancy never writes their contents into a prompt**: [`hides`] says whether a path is one of them, or in
//!   one, so a summary lists such a folder at most by its name and size and never what is inside.
//!
//! Most places are names: patterns match a path's components case-insensitively, anywhere in the path, so the
//! same folder names mean the same thing in any home, the fixture's included. A browser's profile can have any name
//! and a Firefox profile can be anywhere, so browser data is hidden by its stores' own names, never by a profile's
//! name or place; the few store names that are also ordinary names (`Sessions`, `Shortcuts`) count anywhere below
//! where browsers keep their profiles ([`PROFILES`]). These names are the second wall: the browser profiles a scan
//! finds are hidden whole except their caches ([`super::browsers`], through [`Hider`], which every job uses for
//! its scan). Matching more than is strictly needed only hides more. Claude
//! Code's and Codex's own folders can also be moved (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`); each call inherits the
//! app's environment, so wherever the app's environment puts them is hidden too, by path ([`config_dirs`]). Vacancy
//! never reads what is inside them.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use ts_rs::TS;

use super::browsers::Browsers;
use super::runner::{Style, cover_path_in, rule_cover};
use crate::categorize::Env;
use crate::snapshot::Snapshot;

/// What a pattern stands for: a folder (hidden with everything in it), or a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Folder,
    File,
}

/// Which group of the privacy screen's list a place is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeHiddenGroup")]
pub enum Group {
    /// Keys and sign-ins.
    Keys,
    /// Browser data: saved passwords, cookies, history and autofill.
    Browser,
    /// Password managers and wallets.
    Vaults,
}

/// One always-hidden place: consecutive path components, matched anywhere in a path, or only below one of the
/// folders in `under`. Each component may hold `*` (any run of characters) and `|` between alternatives
/// (`Cookies|Cookies-*`); a component that is just `**` stands for any number of folders, none included.
#[derive(Clone, Copy, Debug)]
struct Place {
    group: Group,
    kind: Kind,
    under: &'static [&'static [&'static str]],
    parts: &'static [&'static str],
}

/// No folder above: a place matched anywhere.
const ANYWHERE: &[&[&str]] = &[&[]];

/// Where Chromium browsers keep their profiles. A profile is a folder of any name (`Default`, `Profile 1`, `Guest
/// Profile`, or `Work` from `--profile-directory=Work`), so a name that means browser data only in a profile, and
/// would mean something else in a person's own folders (`Sessions`, `Shortcuts`), is hidden at any depth below
/// these, whatever the profile is called:
const PROFILES: &[&[&str]] = &[
    // Windows: `<vendor>\<browser>\User Data\<profile>`, for every Chromium browser (Edge, Chrome, Brave,
    // Vivaldi, Chromium, Yandex, Arc).
    &["User Data", "**"],
    // Opera and Opera GX on Windows: the profile is `Opera Software\Opera Stable` itself, or a folder in it.
    &["Opera Software", "**"],
    // macOS: `~/Library/Application Support/<vendor>/<browser>/<profile>` (Google/Chrome, Microsoft Edge,
    // BraveSoftware/Brave-Browser, Vivaldi, Chromium, Arc), or the browser's own folder (com.operasoftware.Opera),
    // and the same inside an app's container. Other apps' folders of these names there are hidden too.
    &["Application Support", "**"],
    // A user data folder anywhere else (`--user-data-dir`), by the profile names Chromium gives.
    &["Default|Profile *|* Profile"],
];

const fn folder(group: Group, parts: &'static [&'static str]) -> Place {
    Place {
        group,
        kind: Kind::Folder,
        under: ANYWHERE,
        parts,
    }
}

const fn file(group: Group, parts: &'static [&'static str]) -> Place {
    Place {
        group,
        kind: Kind::File,
        under: ANYWHERE,
        parts,
    }
}

/// A folder in a browser profile, whatever the profile is called ([`PROFILES`]).
const fn profile_folder(parts: &'static [&'static str]) -> Place {
    Place {
        group: Group::Browser,
        kind: Kind::Folder,
        under: PROFILES,
        parts,
    }
}

/// A file in a browser profile, whatever the profile is called ([`PROFILES`]).
const fn profile_file(parts: &'static [&'static str]) -> Place {
    Place {
        group: Group::Browser,
        kind: Kind::File,
        under: PROFILES,
        parts,
    }
}

impl Place {
    /// Each whole pattern the place stands for: its parts below each folder in `under`.
    fn forms(&self) -> impl Iterator<Item = Vec<&'static str>> + '_ {
        self.under
            .iter()
            .map(|above| above.iter().chain(self.parts).copied().collect())
    }
}

use Group::{Browser, Keys, Vaults};

/// Both platforms' places, in one list: a Windows name never matches on a Mac by accident in a way that matters,
/// and a fixture of either kind is covered on both. A browser database's `-journal`, `-wal` and `-shm` siblings,
/// and its backups, hold the same data, so each comes with them.
const PLACES: &[Place] = &[
    // Keys and sign-ins.
    folder(Keys, &[".ssh"]),
    folder(Keys, &[".gnupg"]),
    folder(Keys, &[".aws"]),
    folder(Keys, &[".azure"]),
    folder(Keys, &[".claude"]),
    folder(Keys, &[".codex"]),
    file(Keys, &[".claude.json|.claude.json.*"]),
    file(Keys, &[".npmrc"]),
    file(Keys, &[".docker", "config.json"]),
    file(Keys, &["gh", "hosts.yml"]),
    file(Keys, &["GitHub CLI", "hosts.yml"]),
    file(Keys, &[".env"]),
    file(Keys, &[".env.*"]),
    folder(Keys, &["Microsoft", "Credentials"]),
    folder(Keys, &["Microsoft", "Protect"]),
    folder(Keys, &["Library", "Keychains"]),
    // Chromium browsers (Edge, Chrome, Brave, Vivaldi, Opera and the rest), in every profile whatever its name, and
    // Electron apps built on them: saved passwords, cookies (Network\Cookies included), history, autofill,
    // bookmarks, sessions, site sign-ins (local, session and IndexedDB storage) and extensions' own storage (where
    // wallet extensions keep their vaults). `Local State` holds the key that unlocks the saved passwords and cookies.
    // A name no one gives their own files is hidden anywhere, so a profile anywhere is covered; a name that is also
    // an ordinary folder or file name only in a profile ([`PROFILES`]). Their caches (Cache, Code Cache, GPUCache)
    // stay readable.
    file(Browser, &["Login Data*"]),
    file(Browser, &["Web Data|Web Data-*"]),
    file(Browser, &["Account Web Data|Account Web Data-*"]),
    file(Browser, &["Cookies|Cookies-*"]),
    file(Browser, &["Extension Cookies|Extension Cookies-*"]),
    file(Browser, &["Safe Browsing Cookies|Safe Browsing Cookies-*"]),
    file(Browser, &["History|History-*"]),
    file(Browser, &["Archived History|Archived History-*"]),
    file(Browser, &["Media History|Media History-*"]),
    file(Browser, &["History Provider Cache"]),
    file(
        Browser,
        &["Network Action Predictor|Network Action Predictor-*"],
    ),
    file(Browser, &["Affiliation Database|Affiliation Database-*"]),
    file(Browser, &["Local State|Local State-*"]),
    file(Browser, &["Visited Links"]),
    file(
        Browser,
        &["Current Session|Current Tabs|Last Session|Last Tabs"],
    ),
    file(Browser, &["Bookmarks|Bookmarks.bak"]),
    file(Browser, &["Top Sites|Top Sites-*"]),
    folder(Browser, &["Local Storage"]),
    folder(Browser, &["Session Storage"]),
    folder(Browser, &["IndexedDB"]),
    folder(
        Browser,
        &["Local Extension Settings|Sync Extension Settings"],
    ),
    profile_file(&["Shortcuts|Shortcuts-*"]),
    profile_file(&["Favicons|Favicons-*"]),
    profile_file(&["DIPS|DIPS-*"]),
    profile_folder(&["Sessions"]),
    // Firefox, in every profile wherever it is kept (a profile can live in any folder), by its stores' own names:
    // saved passwords and the keys that unlock them (and the old stores they replaced), cookies, history and
    // bookmarks, autofill and form history, the account it's signed in to, the session it restores and extensions'
    // synced storage, each with its siblings and backups; and its site storage by its layout (`storage/default`,
    // `storage/permanent`, `storage/temporary`, `storage/private`), with the index of the sites in it.
    file(Browser, &["logins*.json*"]),
    file(Browser, &["key3.db*|key4.db*"]),
    file(Browser, &["cert8.db*|cert9.db*"]),
    file(Browser, &["signons*"]),
    file(Browser, &["cookies.sqlite*"]),
    file(Browser, &["places.sqlite*"]),
    file(Browser, &["favicons.sqlite*"]),
    file(Browser, &["formhistory.sqlite*"]),
    file(Browser, &["webappsstore.sqlite*"]),
    file(Browser, &["credentialstate.sqlite*"]),
    file(Browser, &["storage.sqlite|storage.sqlite-*"]),
    file(Browser, &["storage-sync*.sqlite*"]),
    file(Browser, &["autofill-profiles.json*"]),
    file(Browser, &["signedInUser.json*"]),
    file(Browser, &["sessionstore*"]),
    folder(Browser, &["sessionstore-backups"]),
    folder(Browser, &["bookmarkbackups"]),
    folder(
        Browser,
        &[
            "storage",
            "default|permanent|temporary|private|to-be-removed",
        ],
    ),
    file(Browser, &["storage", "ls-archive.sqlite*"]),
    folder(Browser, &["Profiles", "*", "storage"]),
    // Safari: its whole Library folder (history, form values, sessions, bookmarks), its cookies and its site data,
    // in the Library or in Safari's container. Its caches (Library/Caches) stay readable.
    file(Browser, &["History.db*"]),
    file(Browser, &["Cookies.binarycookies"]),
    folder(Browser, &["Library", "Cookies"]),
    folder(Browser, &["Library", "Safari"]),
    folder(Browser, &["com.apple.Safari", "Data", "Library", "WebKit"]),
    // Password managers and wallets.
    folder(Vaults, &["1Password"]),
    folder(Vaults, &["*.com.1password"]),
    folder(Vaults, &["Bitwarden"]),
    folder(Vaults, &["com.bitwarden.desktop"]),
    file(Vaults, &["*.kdbx"]),
    folder(Vaults, &["Electrum"]),
    folder(Vaults, &[".electrum"]),
    folder(Vaults, &["Exodus"]),
    folder(Vaults, &["Ethereum", "keystore"]),
];

/// The environment variables that move Claude Code's and Codex's own folders away from `~/.claude` and `~/.codex`.
pub const CONFIG_VARS: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// The folders [`CONFIG_VARS`] name in an environment read through `var`: each as set, and as the file system
/// resolves it when that differs (a link, a short name). Only the folder's own path is resolved; nothing inside it
/// is read.
pub fn config_dirs_from(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for name in CONFIG_VARS {
        let Some(value) = var(name).filter(|v| !v.is_empty()) else {
            continue;
        };
        let set = PathBuf::from(value);
        if set.is_absolute()
            && let Ok(real) = std::fs::canonicalize(&set)
        {
            let real = plain(&real);
            if !dirs.contains(&real) && real != set {
                dirs.push(real);
            }
        }
        if !dirs.contains(&set) {
            dirs.push(set);
        }
    }
    dirs
}

/// `\\?\C:\x` as `C:\x`: what `canonicalize` gives on Windows, as a rule and a prompt spell it.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Claude Code's and Codex's own folders where the app's environment moved them, read once. Every call inherits
/// that environment, so these are the folders Claude Code itself uses.
pub fn config_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| config_dirs_from(|name| std::env::var_os(name)))
}

/// The privacy screen's list: each group, with what it holds in words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeHidden")]
pub struct Hidden {
    pub group: Group,
    pub title: String,
    pub items: Vec<String>,
}

/// What always stays hidden, as the privacy screens list it.
pub fn list() -> Vec<Hidden> {
    let group = |group, title: &str, items: &[&str]| Hidden {
        group,
        title: title.into(),
        items: items.iter().map(|s| (*s).to_owned()).collect(),
    };
    vec![
        group(
            Keys,
            "Keys and sign-ins",
            &[
                "SSH and GPG keys",
                ".aws, .azure, gh, npm and Docker tokens",
                ".env files",
                "Claude Code's and Codex's own folders, wherever they're set (CLAUDE_CONFIG_DIR, CODEX_HOME)",
                "Windows Credentials and DPAPI Protect folders",
                "the macOS Keychain",
            ],
        ),
        group(
            Browser,
            "Browser data",
            &[
                "saved passwords and their keys",
                "cookies and site sign-ins",
                "history, sessions and bookmarks",
                "autofill and form history",
                "in Edge, Chrome, Brave, Vivaldi, Firefox and Safari (their caches stay readable)",
            ],
        ),
        group(
            Vaults,
            "Password managers and wallets",
            &[
                "1Password",
                "Bitwarden",
                "KeePass .kdbx files",
                "crypto wallets (Electrum, Exodus, the Ethereum keystore)",
            ],
        ),
    ]
}

/// A path's names, for matching: its normal components only.
fn names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// A path's components for comparing two absolute paths, lower-cased: its drive (`c:`), then its names. The root
/// and `.` are left out; `..` is kept, so a path that climbs never matches by accident.
pub(super) fn keyed(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Prefix(p) => Some(match p.kind() {
                std::path::Prefix::Disk(d) | std::path::Prefix::VerbatimDisk(d) => {
                    format!("{}:", (d as char).to_ascii_lowercase())
                }
                _ => p.as_os_str().to_string_lossy().to_lowercase(),
            }),
            Component::Normal(n) => Some(n.to_string_lossy().to_lowercase()),
            Component::ParentDir => Some("..".to_owned()),
            Component::RootDir | Component::CurDir => None,
        })
        .collect()
}

/// Whether `name` matches the pattern `pattern`, case-insensitively: one of its `|` alternatives, where `*` stands
/// for any run of characters.
pub(super) fn glob(pattern: &str, name: &str) -> bool {
    pattern.split('|').any(|alt| glob_one(alt, name))
}

fn glob_one(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    // Iterative wildcard match with backtracking to the last star.
    let (mut i, mut j) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while j < n.len() {
        if i < p.len() && p[i] != '*' && p[i] == n[j] {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(s) = star {
            i = s + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

/// Whether `path` is the config folder `dir`, or inside it. An absolute `dir` matches by where it is (its drive
/// and names); a relative one (`~/.claude-work`, say) by its names anywhere, like the named places.
fn in_config(path: &Path, dir: &Path) -> bool {
    if dir.is_absolute() {
        let (p, d) = (keyed(path), keyed(dir));
        return !d.is_empty() && p.len() >= d.len() && p[..d.len()] == d[..];
    }
    let d: Vec<String> = names(dir)
        .into_iter()
        .filter(|n| n != "~")
        .map(|n| n.to_lowercase())
        .collect();
    let p: Vec<String> = names(path).into_iter().map(|n| n.to_lowercase()).collect();
    !d.is_empty() && p.windows(d.len()).any(|w| w == &d[..])
}

/// The group of the always-hidden place `path` is, or is in, with Claude Code's and Codex's own folders where
/// `config` puts them. A folder pattern hides the folder and everything in it; a file pattern hides a path that
/// ends with it.
pub fn hides_in(path: &Path, config: &[PathBuf]) -> Option<Group> {
    if config.iter().any(|dir| in_config(path, dir)) {
        return Some(Keys);
    }
    let names = names(path);
    PLACES.iter().find_map(|place| {
        let whole = place.kind == Kind::File;
        let found = place
            .forms()
            .any(|form| (0..names.len()).any(|start| match_from(&form, &names[start..], whole)));
        found.then_some(place.group)
    })
}

/// Whether the pattern `parts` matches the names `names` from their first: all of them when `whole` (a file, which
/// a path must end with), or a leading run of them (a folder, which hides what is in it too). A part that is just
/// `**` stands for any number of names, none included.
fn match_from(parts: &[&str], names: &[String], whole: bool) -> bool {
    match parts.split_first() {
        None => !whole || names.is_empty(),
        Some((&"**", rest)) => {
            (0..=names.len()).any(|skip| match_from(rest, &names[skip..], whole))
        }
        Some((part, rest)) => {
            names.first().is_some_and(|name| glob(part, name))
                && match_from(rest, &names[1..], whole)
        }
    }
}

/// The group of the always-hidden place `path` is, or is in: [`hides_in`] with the app's [`config_dirs`].
pub fn hides(path: &Path) -> Option<Group> {
    hides_in(path, config_dirs())
}

/// Whether a single name (a loose file's, say) is an always-hidden file or folder on its own.
pub fn hides_name(name: &str) -> bool {
    hides(Path::new(name)).is_some()
}

/// What always stays hidden for one scan: the named places and moved folders ([`hides`]), and the browser
/// profiles and roots the scan found, whole except their caches ([`Browsers`]).
#[derive(Clone, Debug)]
pub struct Hider {
    browsers: Browsers,
}

impl Hider {
    /// The hider for `snap`, scanned under `env`.
    pub fn of(snap: &Snapshot, env: &Env) -> Self {
        Self {
            browsers: Browsers::of(snap, env),
        }
    }

    /// The group of the always-hidden place `path` is, or is in.
    pub fn hides(&self, path: &Path) -> Option<Group> {
        hides(path).or_else(|| self.browsers.hides(path).then_some(Browser))
    }

    /// Whether `path` is, or holds, a browser profile or root the scan found: a suggestion to delete it would take
    /// what is hidden in it.
    pub fn holds_browser_data(&self, path: &Path) -> bool {
        self.browsers.holds(path)
    }

    /// The deny rules this scan adds to the always-hidden ones: its browser profiles and roots.
    pub fn deny_rules(&self) -> Vec<String> {
        self.browsers.deny_rules()
    }

    pub fn browsers(&self) -> &Browsers {
        &self.browsers
    }
}

/// Every spelling of a place's parts: one per choice of alternative in each part (`a|b`, `c` gives `a/c` and
/// `b/c`).
fn spellings(parts: &[&str]) -> Vec<String> {
    let mut out = vec![String::new()];
    for part in parts {
        out = out
            .iter()
            .flat_map(|head| {
                part.split('|').map(move |alt| {
                    if head.is_empty() {
                        alt.to_owned()
                    } else {
                        format!("{head}/{alt}")
                    }
                })
            })
            .collect();
    }
    out
}

/// Claude Code's `permissions.deny` rules for the whole list, with Claude Code's and Codex's own folders where
/// `config` puts them: `Read(...)` rules, which Claude Code applies to Read, Glob and Grep alike (see the module
/// docs). Named places are gitignore-style patterns from the file system's root, so they hold on every drive and in
/// every home, each twice: `//**/x`, and `//*/**/x`, because the matcher's leading `**` is a regex `.*`, which
/// stops at a line break, so under a folder whose name has one only the second matches (a middle `**` and a `*`
/// match line breaks). A moved folder is its own path ([`config_rules_in`]). A moved folder no rule can name gets
/// none here: [`deniable`] says so, and privacy mode off then gives no reads at all
/// ([`super::runner::reads_unavailable_in`]).
pub fn deny_rules_in(config: &[PathBuf]) -> Vec<String> {
    let mut rules = Vec::new();
    let mut push = |rule: String| {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    };
    for place in PLACES {
        for joined in place.forms().flat_map(|form| spellings(&form)) {
            for from in ["//**/", "//*/**/"] {
                push(format!("Read({from}{joined})"));
                if place.kind == Kind::Folder {
                    push(format!("Read({from}{joined}/**)"));
                }
            }
        }
    }
    for dir in config {
        for rule in dir
            .to_str()
            .and_then(|d| config_rules_in(d, Style::HOST))
            .unwrap_or_default()
        {
            push(rule);
        }
    }
    rules
}

/// The deny rules for one of Claude Code's or Codex's own folders as `text` (in `style`) sets it: an absolute one
/// as its path from the root (`//c/Users/alex/.claude-work`, both it and everything in it), a relative one
/// (`~/.codex-work`) by its names anywhere, like a named place. Each name matches as it is spelled or more, never
/// less ([`super::runner::rule_cover`]): a `[work]` folder is `\[work\]`, and a name with a line break or a Unix
/// backslash holds a `*` where those are. `None` when no rule can name it: a Windows path that is neither from a
/// drive letter nor relative (a share, `\\?\`, `C:x`), or a relative one with no names.
pub fn config_rules_in(text: &str, style: Style) -> Option<Vec<String>> {
    let absolute = match style {
        Style::Windows => {
            let b = text.as_bytes();
            b.first().is_some_and(|c| *c == b'\\' || *c == b'/')
                || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        }
        Style::Unix => text.starts_with('/'),
    };
    let ats: Vec<String> = if absolute {
        vec![format!("/{}", cover_path_in(text, style)?)]
    } else {
        let names: Vec<String> = text
            .split(|c| c == '/' || (style == Style::Windows && c == '\\'))
            .filter(|n| !matches!(*n, "" | "." | ".." | "~"))
            .map(rule_cover)
            .collect();
        if names.is_empty() {
            return None;
        }
        let names = names.join("/");
        vec![format!("//**/{names}"), format!("//*/**/{names}")]
    };
    Some(
        ats.iter()
            .flat_map(|at| [format!("Read({at})"), format!("Read({at}/**)")])
            .collect(),
    )
}

/// Whether deny rules can name the moved folder `dir` ([`config_rules_in`]). A folder whose path isn't Unicode
/// can't be: Claude Code would see other characters than Vacancy spells.
pub fn deniable(dir: &Path) -> bool {
    dir.to_str()
        .and_then(|d| config_rules_in(d, Style::HOST))
        .is_some()
}
/// [`deny_rules_in`] with the app's [`config_dirs`].
pub fn deny_rules() -> Vec<String> {
    deny_rules_in(config_dirs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        s.split('/').collect()
    }

    fn hidden(s: &str) -> bool {
        hides_in(&p(s), &[]).is_some()
    }

    #[test]
    fn hides_keys_browser_data_and_vaults_case_insensitively_anywhere() {
        for hidden in [
            "Users/alex/.ssh",
            "Users/alex/.ssh/id_rsa",
            "Users/alex/.SSH/ID_RSA",
            "Users/alex/.aws/credentials",
            "Users/alex/.claude.json",
            "Users/alex/.claude.json.backup",
            "Users/alex/.claude/.credentials.json",
            "Users/alex/.codex/auth.json",
            "Users/alex/project/.env",
            "Users/alex/project/.env.local",
            "Users/alex/.docker/config.json",
            "Users/alex/AppData/Roaming/GitHub CLI/hosts.yml",
            "Users/alex/.config/gh/hosts.yml",
            "Users/alex/AppData/Roaming/Microsoft/Credentials/ABC",
            "Users/alex/AppData/Roaming/Microsoft/Protect/S-1-5/key",
            "Users/alex/AppData/Local/Microsoft/Credentials/X",
            "Users/alex/Library/Keychains/login.keychain-db",
            "Users/alex/AppData/Local/1Password/data/1password.sqlite",
            "Users/alex/Library/Group Containers/2BUA8C4S2C.com.1password/x",
            "Users/alex/AppData/Roaming/Bitwarden/data.json",
            "Users/alex/Desktop/Passwords.KDBX",
            "Users/alex/AppData/Roaming/Electrum/wallets/default_wallet",
            "Users/alex/AppData/Roaming/Exodus/exodus.wallet",
            "Users/alex/AppData/Roaming/Ethereum/keystore/UTC--2020",
        ] {
            assert!(
                hides_in(&p(hidden), &[]).is_some(),
                "{hidden} must stay hidden"
            );
        }
        assert!(hides_name("vault.kdbx"));
        assert!(hides_name(".env"));
        assert!(!hides_name("invoice.pdf"));
    }

    /// Every Chromium browser's profile, on Windows and macOS, whatever the profile is called (`Work` from
    /// `--profile-directory=Work`): saved passwords, cookies, history, autofill, bookmarks, sessions, site sign-ins
    /// and extensions' storage, each with its `-journal` and `-wal` siblings, and the key in `Local State`.
    #[test]
    fn hides_chromium_browser_data_with_its_journal_and_wal_siblings() {
        let profiles = [
            "Users/alex/AppData/Local/Microsoft/Edge/User Data/Default",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Profile 3",
            "Users/alex/AppData/Local/BraveSoftware/Brave-Browser/User Data/Guest Profile",
            "Users/alex/AppData/Local/Vivaldi/User Data/Default",
            "Users/alex/Library/Application Support/Google/Chrome/Default",
            "Users/alex/Library/Application Support/Microsoft Edge/Profile 1",
            "Users/alex/Library/Application Support/BraveSoftware/Brave-Browser/Default",
            // Profiles with names of their own.
            "Users/alex/AppData/Local/Google/Chrome/User Data/Work",
            "Users/alex/AppData/Local/Microsoft/Edge/User Data/Banking stuff",
            "Users/alex/AppData/Local/Chromium/User Data/x",
            "Users/alex/AppData/Local/Yandex/YandexBrowser/User Data/Personal",
            "Users/alex/AppData/Roaming/Opera Software/Opera Stable",
            "Users/alex/AppData/Roaming/Opera Software/Opera GX Stable/Work",
            "Users/alex/Library/Application Support/Google/Chrome/Work",
            "Users/alex/Library/Application Support/Microsoft Edge/Banking stuff",
            "Users/alex/Library/Application Support/Vivaldi/Travel",
            "Users/alex/Library/Application Support/Chromium/x",
            "Users/alex/Library/Application Support/com.operasoftware.Opera",
            "Users/alex/Library/Application Support/Arc/User Data/Work",
            // A user data folder of its own (`--user-data-dir`), by Chromium's profile names.
            "D:/ChromeData/Default",
            "D:/ChromeData/Profile 2",
        ];
        let files = [
            "Login Data",
            "Login Data-journal",
            "Login Data-wal",
            "Login Data For Account",
            "Login Data For Account-journal",
            "Web Data",
            "Web Data-journal",
            "Web Data-wal",
            "Account Web Data",
            "Cookies",
            "Cookies-journal",
            "Network/Cookies",
            "Network/Cookies-journal",
            "Extension Cookies",
            "History",
            "History-journal",
            "History-wal",
            "Archived History",
            "Media History-journal",
            "Network Action Predictor",
            "Top Sites",
            "Top Sites-journal",
            "Shortcuts",
            "Shortcuts-journal",
            "Favicons",
            "Favicons-journal",
            "Bookmarks",
            "Bookmarks.bak",
            "Visited Links",
            "Current Session",
            "Last Tabs",
            "Sessions/Session_13360000000000000",
            "Local Storage/leveldb/000003.log",
            "Session Storage/MANIFEST-000001",
            "IndexedDB/https_mail.example.com_0.indexeddb.leveldb/000003.log",
            "Affiliation Database",
            "DIPS-wal",
            "Local Extension Settings/nkbihfbeogaeaoehlefnkodbefgpgknn/000003.log",
            "Sync Extension Settings/abc/MANIFEST-000001",
        ];
        for profile in profiles {
            for f in files {
                let path = format!("{profile}/{f}");
                assert!(hidden(&path), "{path} must stay hidden");
            }
        }
        assert!(hidden(
            "Users/alex/AppData/Local/Google/Chrome/User Data/Local State"
        ));
        assert!(hidden(
            "Users/alex/Library/Application Support/Google/Chrome/Local State"
        ));
        // Electron apps keep their sign-ins the same way.
        assert!(hidden(
            "Users/alex/AppData/Roaming/discord/Local Storage/leveldb/000005.ldb"
        ));
        assert!(hidden("Users/alex/AppData/Roaming/Slack/Cookies"));
        // A user data folder of its own with a profile of its own name: the names no one gives their own files.
        for f in [
            "Login Data",
            "Web Data",
            "Cookies",
            "History",
            "Bookmarks",
            "Bookmarks.bak",
            "Top Sites",
            "IndexedDB/x/000003.log",
            "Local Storage/leveldb/x",
            "Local Extension Settings/x/000003.log",
        ] {
            let path = format!("D:/ChromeData/Work/{f}");
            assert!(hidden(&path), "{path} must stay hidden");
        }
    }

    /// Firefox, on Windows and macOS: logins and their backup, the keys, form history and autofill, cookies, history,
    /// site storage, the signed-in account and the session store, with their siblings.
    #[test]
    fn hides_firefox_credential_autofill_history_and_backup_stores() {
        let profiles = [
            "Users/alex/AppData/Roaming/Mozilla/Firefox/Profiles/x1y2z3.default-release",
            "Users/alex/Library/Application Support/Firefox/Profiles/abc.default",
            // Profiles kept wherever the person put them.
            "D:/FirefoxProfiles/abc.custom",
            "Users/alex/Documents/My Firefox",
            "Volumes/Data/Firefox profiles/work",
        ];
        for profile in profiles {
            for f in [
                "logins.json",
                "logins-backup.json",
                "key3.db",
                "key4.db",
                "key4.db-journal",
                "key3.db-journal",
                "cert8.db",
                "cert9.db",
                "cert9.db-wal",
                "logins.json.corrupt",
                "signons.sqlite",
                "signons3.txt",
                "cookies.sqlite",
                "cookies.sqlite-wal",
                "cookies.sqlite-shm",
                "places.sqlite",
                "places.sqlite-wal",
                "favicons.sqlite",
                "formhistory.sqlite",
                "formhistory.sqlite-wal",
                "autofill-profiles.json",
                "webappsstore.sqlite",
                "credentialstate.sqlite",
                "signedInUser.json",
                "sessionstore.jsonlz4",
                "sessionstore-backups/recovery.jsonlz4",
                "sessionstore-backups/previous.jsonlz4",
                "bookmarkbackups/bookmarks-2026-10-22.jsonlz4",
                "storage/default/https+++mail.example.com/ls/data.sqlite",
                "storage/default/moz-extension+++abc/idb/x.sqlite",
                "storage/permanent/chrome/idb/x.sqlite",
                "storage/temporary/https+++example.com/cache/x",
                "storage/private/x",
                "storage/to-be-removed/x",
                "storage/ls-archive.sqlite",
                "storage.sqlite",
                "storage-sync-v2.sqlite",
                "storage-sync-v2.sqlite-wal",
            ] {
                let path = format!("{profile}/{f}");
                assert!(hidden(&path), "{path} must stay hidden");
            }
        }
    }

    #[test]
    fn hides_safari_history_cookies_form_values_and_site_data() {
        for path in [
            "Users/alex/Library/Safari/History.db",
            "Users/alex/Library/Safari/History.db-wal",
            "Users/alex/Library/Safari/Form Values",
            "Users/alex/Library/Safari/LastSession.plist",
            "Users/alex/Library/Safari/Bookmarks.plist",
            "Users/alex/Library/Cookies/Cookies.binarycookies",
            "Users/alex/Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies",
            "Users/alex/Library/Containers/com.apple.Safari/Data/Library/Safari/History.db",
            "Users/alex/Library/Containers/com.apple.Safari/Data/Library/WebKit/WebsiteData/LocalStorage/x",
        ] {
            assert!(hidden(path), "{path} must stay hidden");
        }
    }

    #[test]
    fn caches_and_ordinary_files_stay_readable() {
        for visible in [
            "Users/alex",
            "Users/alex/AppData/Local/Microsoft/Edge/User Data/Default/Cache/Cache_Data/f_0001",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Default/Code Cache",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Default/GPUCache/data_0",
            "Users/alex/AppData/Local/Mozilla/Firefox/Profiles/x.default-release/cache2/entries/ABC",
            "Users/alex/Library/Caches/com.apple.Safari/Cache.db",
            "Users/alex/Library/Caches/Google/Chrome/Default/Cache/Cache_Data/f_0001",
            // Caches in profiles with names of their own, and browsers' shared caches.
            "Users/alex/AppData/Local/Google/Chrome/User Data/Work/Cache/Cache_Data/f_0001",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Work/Code Cache/js/x",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Work/GPUCache/data_0",
            "Users/alex/AppData/Local/Google/Chrome/User Data/ShaderCache/x",
            "Users/alex/AppData/Local/Google/Chrome/User Data/GrShaderCache/x",
            "Users/alex/AppData/Local/Google/Chrome/User Data/GraphiteDawnCache/x",
            "Users/alex/AppData/Local/Microsoft/Edge/User Data/Banking stuff/Cache/Cache_Data/f",
            "Users/alex/AppData/Roaming/Opera Software/Opera Stable/Cache/Cache_Data/f",
            "Users/alex/Library/Application Support/Google/Chrome/Work/GPUCache/data_0",
            "Users/alex/Library/Application Support/Google/Chrome/Work/Code Cache/js/x",
            "Users/alex/Library/Caches/Google/Chrome/Work/Cache/Cache_Data/f_0001",
            "Users/alex/Library/Caches/Firefox/Profiles/abc.default/cache2/entries/ABC",
            "D:/FirefoxProfiles/abc.custom/cache2/entries/ABC",
            "Users/alex/AppData/Roaming/discord/Cache/Cache_Data/x",
            "Users/alex/Library/Application Support/discord/DawnCache/x",
            "Users/alex/Library/Caches/com.apple.Safari/WebKitCache/x",
            "Users/alex/Documents/History of Rome.pdf",
            "Users/alex/Documents/Bookmarks.html",
            "Users/alex/Documents/Favicons/icon.png",
            "Users/alex/Documents/DIPS/notes.txt",
            "Users/alex/Documents/storage/old/boxes.xlsx",
            "Users/alex/code/site/public/favicons/icon.png",
            "Users/alex/Documents/Sessions/notes.txt",
            "Users/alex/Documents/Shortcuts/notes.txt",
            "Users/alex/Documents/storage/boxes.xlsx",
            "Users/alex/Documents/environment.txt",
            "Users/alex/project/.envrc.example.md/x",
            "Users/alex/AppData/Local/Microsoft/Windows/INetCache",
            "Users/alex/Downloads/ssh-notes.txt",
        ] {
            assert!(!hidden(visible), "{visible} stays readable");
        }
    }

    #[test]
    fn moved_config_folders_are_hidden_where_they_are_without_being_read() {
        let (claude, codex) = if cfg!(windows) {
            (r"D:\Work\claude-config", r"E:\tools\codex home")
        } else {
            ("/Volumes/Work/claude-config", "/opt/codex home")
        };
        let lookup = |name: &str| match name {
            "CLAUDE_CONFIG_DIR" => Some(OsString::from(claude)),
            "CODEX_HOME" => Some(OsString::from(codex)),
            _ => None,
        };
        let config = config_dirs_from(lookup);
        assert!(config.contains(&PathBuf::from(claude)) && config.contains(&PathBuf::from(codex)));
        let inside = |dir: &str, rest: &str| PathBuf::from(dir).join(rest);
        assert_eq!(
            hides_in(&inside(claude, ".credentials.json"), &config),
            Some(Keys)
        );
        assert_eq!(
            hides_in(&inside(claude, ".claude.json"), &config),
            Some(Keys)
        );
        assert_eq!(hides_in(&inside(codex, "auth.json"), &config), Some(Keys));
        assert_eq!(hides_in(&PathBuf::from(claude), &config), Some(Keys));
        if cfg!(windows) {
            // Windows paths match case-insensitively, as Claude Code's rules do.
            assert!(hides_in(Path::new(r"d:\WORK\Claude-Config\x"), &config).is_some());
        }
        // A sibling with a longer name, and the folder above, stay readable.
        assert!(hides_in(&PathBuf::from(format!("{claude}-old")).join("x"), &config).is_none());
        assert!(hides_in(PathBuf::from(claude).parent().unwrap(), &config).is_none());
        let rules = deny_rules_in(&config);
        let at = |dir: &str| {
            format!(
                "/{}",
                super::super::runner::rule_path(Path::new(dir)).unwrap()
            )
        };
        for dir in [claude, codex] {
            assert!(rules.contains(&format!("Read({})", at(dir))), "{rules:?}");
            assert!(
                rules.contains(&format!("Read({}/**)", at(dir))),
                "{rules:?}"
            );
        }
        if cfg!(windows) {
            assert!(rules.contains(&"Read(//d/Work/claude-config/**)".to_owned()));
        } else {
            assert!(rules.contains(&"Read(//Volumes/Work/claude-config/**)".to_owned()));
        }
        // A relative one is hidden by its names, anywhere.
        let relative =
            config_dirs_from(|name| (name == "CODEX_HOME").then(|| "~/.codex-work".into()));
        assert!(hides_in(&p("Users/alex/.codex-work/auth.json"), &relative).is_some());
        assert!(deny_rules_in(&relative).contains(&"Read(//**/.codex-work/**)".to_owned()));
        // Unset or empty: only the defaults (.claude, .claude.json, .codex), by name.
        assert!(config_dirs_from(|_| Some(OsString::new())).is_empty());
        assert!(config_dirs_from(|_| None).is_empty());
    }

    /// A moved folder with gitignore's special characters in its path (`[work]` would be a character class, `*` and
    /// `{a,b}` wildcards) is denied as it is spelled: as set, and as the file system resolves it.
    #[test]
    fn moved_config_folders_with_glob_characters_are_denied_as_spelled() {
        let set = if cfg!(windows) {
            vec![
                (
                    r"D:\AI\[work]\claude-config",
                    r"Read(//d/AI/\[work\]/claude-config",
                ),
                (r"E:\{a,b}\codex!", r"Read(//e/\{a,b\}/codex\!"),
            ]
        } else {
            vec![
                (
                    "/Volumes/AI/[work]/claude-config",
                    r"Read(//Volumes/AI/\[work\]/claude-config",
                ),
                ("/opt/*/{a,b}/codex", r"Read(//opt/*/\{a,b\}/codex"),
            ]
        };
        for (dir, rule) in set {
            let config = config_dirs_from(|name| (name == "CLAUDE_CONFIG_DIR").then(|| dir.into()));
            let rules = deny_rules_in(&config);
            assert!(rules.contains(&format!("{rule})")), "{dir}: {rules:?}");
            assert!(rules.contains(&format!("{rule}/**)")), "{dir}: {rules:?}");
            // The prompt filter takes the name as it is too.
            assert!(hides_in(&PathBuf::from(dir).join("x"), &config).is_some());
        }
        // A relative one, by its names anywhere.
        let relative = config_dirs_from(|name| (name == "CODEX_HOME").then(|| "~/[codex]*".into()));
        assert!(deny_rules_in(&relative).contains(&r"Read(//**/\[codex\]*/**)".to_owned()));

        // Resolved: a real folder reached through `..`, so the resolved path differs from the one set; both are
        // denied, each escaped.
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("[work]").join("cfg");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(tmp.path().join("[work]").join("x")).unwrap();
        let via = tmp.path().join("[work]").join("x").join("..").join("cfg");
        let config =
            config_dirs_from(|name| (name == "CLAUDE_CONFIG_DIR").then(|| via.clone().into()));
        let resolved = plain(&std::fs::canonicalize(&real).unwrap());
        assert!(config.contains(&resolved), "{config:?}");
        let rules = deny_rules_in(&config);
        let at = format!(
            "Read(/{}/**)",
            super::super::runner::rule_path(&resolved).unwrap()
        );
        assert!(at.contains(r"/\[work\]/cfg/**)"), "{at}");
        assert!(rules.contains(&at), "{rules:?}");
        assert!(
            !rules.iter().any(|r| r.contains("/[work]/")),
            "no rule spells [work] as a character class: {rules:?}"
        );
        assert!(hides_in(&resolved.join("canary.txt"), &config).is_some());
    }

    #[test]
    fn deny_rules_cover_every_place_from_the_root() {
        let rules = deny_rules_in(&[]);
        for rule in [
            "Read(//**/.ssh/**)",
            "Read(//**/.ssh)",
            "Read(//**/Login Data*)",
            "Read(//**/Cookies-*)",
            "Read(//**/Microsoft/Protect/**)",
            "Read(//**/*.kdbx)",
            "Read(//**/logins*.json*)",
            "Read(//**/formhistory.sqlite*)",
            "Read(//**/autofill-profiles.json*)",
            "Read(//**/key4.db*)",
            "Read(//**/Local State)",
            "Read(//**/Default/Sessions/**)",
            "Read(//**/Profile */Sessions/**)",
            "Read(//**/* Profile/Shortcuts-*)",
            "Read(//**/User Data/**/Sessions/**)",
            "Read(//*/**/User Data/**/Sessions/**)",
            "Read(//**/Application Support/**/Favicons-*)",
            "Read(//**/Opera Software/**/DIPS)",
            "Read(//**/Bookmarks)",
            "Read(//**/Top Sites-*)",
            "Read(//**/IndexedDB/**)",
            "Read(//**/Local Extension Settings/**)",
            "Read(//**/storage/default/**)",
            "Read(//**/storage/permanent/**)",
            "Read(//**/storage.sqlite)",
            "Read(//**/Profiles/*/storage/**)",
            "Read(//**/Library/Safari/**)",
        ] {
            assert!(rules.contains(&rule.to_owned()), "{rule}");
        }
        assert!(rules.iter().all(|r| (r.starts_with("Read(//**/")
            || r.starts_with("Read(//*/**/"))
            && r.ends_with(')')
            && !r.contains('|')));
        // Each twice: from `//**/`, and from `//*/**/` for a path with a line break in a folder's name.
        for rule in rules.iter().filter(|r| r.starts_with("Read(//**/")) {
            let twin = rule.replacen("Read(//**/", "Read(//*/**/", 1);
            assert!(rules.contains(&twin), "{twin}");
        }
        let expected: usize = PLACES
            .iter()
            .map(|p| {
                let n: usize = p.forms().map(|f| spellings(&f).len()).sum();
                2 * if p.kind == Kind::Folder { 2 * n } else { n }
            })
            .sum();
        assert_eq!(rules.len(), expected, "no rule twice");
    }

    /// Each hidden file pattern, spelled as a real path, is hidden by the prompt filter and named by a deny rule;
    /// so the two halves of the list can't drift apart.
    #[test]
    fn the_prompt_filter_and_the_deny_rules_come_from_one_list() {
        let rules = deny_rules_in(&[]);
        for place in PLACES {
            for joined in place.forms().flat_map(|f| spellings(&f)) {
                let real = joined.replace('*', "x");
                assert!(
                    hidden(&format!("Users/alex/{real}")),
                    "{joined} as a path is hidden"
                );
                assert!(rules.contains(&format!("Read(//**/{joined})")));
            }
        }
    }

    #[test]
    fn glob_matches_stars_and_alternatives() {
        assert!(glob("*.kdbx", "a.KDBX"));
        assert!(glob(".env.*", ".env.production"));
        assert!(!glob(".env.*", ".env"));
        assert!(glob("cookies.sqlite*", "cookies.sqlite"));
        assert!(!glob("History", "History.db"));
        assert!(glob("History|History-*", "History-journal"));
        assert!(!glob("History|History-*", "History of Rome.pdf"));
        assert_eq!(spellings(&["a|b", "c"]), ["a/c", "b/c"]);
        let names = |s: &str| s.split('/').map(str::to_owned).collect::<Vec<_>>();
        // `**` stands for any number of names, none included.
        for path in ["User Data/S", "User Data/a/S", "User Data/a/b/c/S"] {
            assert!(
                match_from(&["User Data", "**", "S"], &names(path), true),
                "{path}"
            );
        }
        assert!(!match_from(
            &["User Data", "**", "S"],
            &names("User Data/S/x"),
            true
        ));
        assert!(match_from(
            &["User Data", "**", "S"],
            &names("User Data/a/S/x"),
            false
        ));
        assert!(!match_from(
            &["User Data", "**", "S"],
            &names("Data/a/S"),
            false
        ));
    }
}

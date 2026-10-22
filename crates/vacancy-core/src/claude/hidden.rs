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
//! Patterns match a path's components case-insensitively, anywhere in the path: the same folder names mean the
//! same thing in any home, the fixture's included. Matching more than is strictly needed only hides more.

use std::path::{Component, Path};

use serde::Serialize;
use ts_rs::TS;

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

/// One always-hidden place: consecutive path components (each may hold `*`), matched anywhere in a path.
#[derive(Clone, Copy, Debug)]
struct Place {
    group: Group,
    kind: Kind,
    parts: &'static [&'static str],
}

const fn folder(group: Group, parts: &'static [&'static str]) -> Place {
    Place {
        group,
        kind: Kind::Folder,
        parts,
    }
}

const fn file(group: Group, parts: &'static [&'static str]) -> Place {
    Place {
        group,
        kind: Kind::File,
        parts,
    }
}

use Group::{Browser, Keys, Vaults};

/// Both platforms' places, in one list: a Windows name never matches on a Mac by accident in a way that matters,
/// and a fixture of either kind is covered on both.
const PLACES: &[Place] = &[
    // Keys and sign-ins.
    folder(Keys, &[".ssh"]),
    folder(Keys, &[".gnupg"]),
    folder(Keys, &[".aws"]),
    folder(Keys, &[".azure"]),
    folder(Keys, &[".claude"]),
    folder(Keys, &[".codex"]),
    file(Keys, &[".claude.json"]),
    file(Keys, &[".npmrc"]),
    file(Keys, &[".docker", "config.json"]),
    file(Keys, &["gh", "hosts.yml"]),
    file(Keys, &["GitHub CLI", "hosts.yml"]),
    file(Keys, &[".env"]),
    file(Keys, &[".env.*"]),
    folder(Keys, &["Microsoft", "Credentials"]),
    folder(Keys, &["Microsoft", "Protect"]),
    folder(Keys, &["Library", "Keychains"]),
    // Browser data: Chromium browsers (Edge, Chrome, Brave, Vivaldi) keep these in each profile, Network\Cookies
    // included; Firefox in each profile; Safari in the Library. Their caches stay readable.
    file(Browser, &["Login Data"]),
    file(Browser, &["Login Data-journal"]),
    file(Browser, &["Login Data For Account"]),
    file(Browser, &["Login Data For Account-journal"]),
    file(Browser, &["Cookies"]),
    file(Browser, &["Cookies-journal"]),
    file(Browser, &["History"]),
    file(Browser, &["History-journal"]),
    file(Browser, &["Web Data"]),
    file(Browser, &["Web Data-journal"]),
    file(Browser, &["logins.json"]),
    file(Browser, &["key4.db"]),
    file(Browser, &["cookies.sqlite*"]),
    file(Browser, &["places.sqlite*"]),
    file(Browser, &["History.db*"]),
    file(Browser, &["Cookies.binarycookies"]),
    folder(Browser, &["Library", "Cookies"]),
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
                "Claude Code's and Codex's own sign-in files",
                "Windows Credentials and DPAPI Protect folders",
                "the macOS Keychain",
            ],
        ),
        group(
            Browser,
            "Browser data",
            &[
                "saved passwords",
                "cookies",
                "history",
                "autofill",
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

/// Whether `name` matches the pattern `pattern`, case-insensitively; `*` stands for any run of characters.
fn glob(pattern: &str, name: &str) -> bool {
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

/// The group of the always-hidden place `path` is, or is in. A folder pattern hides the folder and everything in
/// it; a file pattern hides a path that ends with it.
pub fn hides(path: &Path) -> Option<Group> {
    let names = names(path);
    PLACES.iter().find_map(|place| {
        let k = place.parts.len();
        if names.len() < k {
            return None;
        }
        let at = |start: usize| {
            place
                .parts
                .iter()
                .zip(&names[start..start + k])
                .all(|(p, n)| glob(p, n))
        };
        let found = match place.kind {
            Kind::File => at(names.len() - k),
            Kind::Folder => (0..=names.len() - k).any(at),
        };
        found.then_some(place.group)
    })
}

/// Whether a single name (a loose file's, say) is an always-hidden file or folder on its own.
pub fn hides_name(name: &str) -> bool {
    hides(Path::new(name)).is_some()
}

/// Claude Code's `permissions.deny` rules for the whole list: `Read(...)` rules, which Claude Code applies to
/// Read, Glob and Grep alike (see the module docs), as gitignore-style patterns from the file system's root
/// (`//**/`), so they hold on every drive and in every home.
pub fn deny_rules() -> Vec<String> {
    let mut rules = Vec::new();
    for place in PLACES {
        let joined = place.parts.join("/");
        match place.kind {
            Kind::Folder => {
                rules.push(format!("Read(//**/{joined})"));
                rules.push(format!("Read(//**/{joined}/**)"));
            }
            Kind::File => rules.push(format!("Read(//**/{joined})")),
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        s.split('/').collect()
    }

    #[test]
    fn hides_keys_browser_data_and_vaults_case_insensitively_anywhere() {
        for hidden in [
            "Users/alex/.ssh",
            "Users/alex/.ssh/id_rsa",
            "Users/alex/.SSH/ID_RSA",
            "Users/alex/.aws/credentials",
            "Users/alex/.claude.json",
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
            "Users/alex/AppData/Local/Microsoft/Edge/User Data/Default/Login Data",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Default/Network/Cookies",
            "Users/alex/AppData/Local/BraveSoftware/Brave-Browser/User Data/Default/History",
            "Users/alex/AppData/Local/Vivaldi/User Data/Default/Web Data",
            "Users/alex/AppData/Roaming/Mozilla/Firefox/Profiles/x.default/logins.json",
            "Users/alex/AppData/Roaming/Mozilla/Firefox/Profiles/x.default/key4.db",
            "Users/alex/AppData/Roaming/Mozilla/Firefox/Profiles/x.default/cookies.sqlite-wal",
            "Users/alex/AppData/Roaming/Mozilla/Firefox/Profiles/x.default/places.sqlite",
            "Users/alex/Library/Safari/History.db",
            "Users/alex/Library/Cookies/Cookies.binarycookies",
            "Users/alex/AppData/Local/1Password/data/1password.sqlite",
            "Users/alex/Library/Group Containers/2BUA8C4S2C.com.1password/x",
            "Users/alex/AppData/Roaming/Bitwarden/data.json",
            "Users/alex/Desktop/Passwords.KDBX",
            "Users/alex/AppData/Roaming/Electrum/wallets/default_wallet",
            "Users/alex/AppData/Roaming/Exodus/exodus.wallet",
            "Users/alex/AppData/Roaming/Ethereum/keystore/UTC--2020",
        ] {
            assert!(hides(&p(hidden)).is_some(), "{hidden} must stay hidden");
        }
        for visible in [
            "Users/alex",
            "Users/alex/AppData/Local/Microsoft/Edge/User Data/Default/Cache/Cache_Data/f_0001",
            "Users/alex/AppData/Local/Google/Chrome/User Data/Default/Code Cache",
            "Users/alex/Documents/History of Rome.pdf",
            "Users/alex/Documents/environment.txt",
            "Users/alex/project/.envrc.example.md/x",
            "Users/alex/AppData/Local/Microsoft/Windows/INetCache",
            "Users/alex/Downloads/ssh-notes.txt",
        ] {
            assert!(hides(&p(visible)).is_none(), "{visible} stays readable");
        }
        assert!(hides_name("vault.kdbx"));
        assert!(hides_name(".env"));
        assert!(!hides_name("invoice.pdf"));
    }

    #[test]
    fn deny_rules_cover_every_place_from_the_root() {
        let rules = deny_rules();
        assert!(rules.contains(&"Read(//**/.ssh/**)".to_owned()));
        assert!(rules.contains(&"Read(//**/.ssh)".to_owned()));
        assert!(rules.contains(&"Read(//**/Login Data)".to_owned()));
        assert!(rules.contains(&"Read(//**/Microsoft/Protect/**)".to_owned()));
        assert!(rules.contains(&"Read(//**/*.kdbx)".to_owned()));
        assert!(
            rules
                .iter()
                .all(|r| r.starts_with("Read(//**/") && r.ends_with(')'))
        );
        assert_eq!(
            rules.len(),
            PLACES
                .iter()
                .map(|p| if p.kind == Kind::Folder { 2 } else { 1 })
                .sum::<usize>()
        );
    }

    #[test]
    fn glob_matches_stars() {
        assert!(glob("*.kdbx", "a.KDBX"));
        assert!(glob(".env.*", ".env.production"));
        assert!(!glob(".env.*", ".env"));
        assert!(glob("cookies.sqlite*", "cookies.sqlite"));
        assert!(!glob("History", "History.db"));
    }
}

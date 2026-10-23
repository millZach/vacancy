//! The differential test of Claude Code's permission rules (ADR 0008): every rule Vacancy builds from a real path,
//! and the always-hidden places' rules, run through the matcher Claude Code itself uses (the `ignore` package, 7.0.5
//! as Claude Code 2.1.309 bundles it, and 5.3.2), by `tools/rule-matcher.mjs` under Node. For many adversarial
//! names (line breaks, CR, tab, backslashes, `[ ] * ? { } ! # ^ $`, trailing spaces, Unicode look-alikes and other
//! forms, mixed case), in both Windows' and Unix's path styles on either system:
//!
//! - each deny rule matches its target, everything in it, and its target spelled in other case;
//! - the always-hidden places stay denied under folders with any of those names;
//! - browser data stays denied in a Chromium profile of any name (`--profile-directory=Work`) and in a Firefox
//!   profile kept anywhere, while browsers' caches and ordinary files of the same names stay readable;
//! - a browser profile's own rules (`claude::browsers`) hide every child of it but its caches, whatever the
//!   profile's or the child's name (a name made after the scan, one character off a cache's, a look-alike), a
//!   root's leave the way down to its profiles readable too, and a default root the scan didn't see is hidden
//!   whole; and the rules the app builds from a real scan of dummy profiles hide them, with the always-hidden
//!   places' rules, while their caches and a game's `Preferences` folder stay readable;
//! - an allow rule exists only for a root it spells exactly, and then matches inside the root and nothing beside it
//!   (a sibling one character off, a longer name, the folder above, a folder named like the root elsewhere), except
//!   a sibling spelled like the root in other case (the known limit in ADR 0008);
//! - a root no allow rule spells makes fixture mode refuse reads, and a moved folder no deny rule names makes reads
//!   unavailable.
//!
//! Verified against Claude Code 2.1.309. When Claude Code updates its matcher, recheck the model in
//! `tools/rule-matcher.mjs`. (At runtime, `claude::canary` checks each Claude Code takes the settings at all; that
//! check's own settings are among the checks here.) Mounted file systems are among them too: files under a disk
//! image or a VHD mounted inside the scanned drive are denied, and files beside them stay allowed.
//!
//! Needs Node and `npm ci` (CI runs it before the Rust tests).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use vacancy_core::claude::runner::{
    self, Style, allow_rule_in, exact_char, read_settings_in, reads_unavailable_in,
};
use vacancy_core::claude::{browsers, hidden};

/// Names meant to break a rule. Each goes into paths as a folder's name; on Windows only those Windows allows.
const NAMES: &[&str] = &[
    "plain",
    "a\nb",
    "a\rb",
    "a\r\nb",
    "a\tb",
    "\n",
    "\n\n",
    "x\n",
    "\nx",
    "a\\b",
    "x\\^z",
    "x\\$z",
    "x\\.z",
    "x\\(z)",
    "x\\{z}",
    "x\\|z",
    "x\\+z",
    "\\",
    "\\\\",
    "a\\\\b",
    "a\\*b",
    "a\\?b",
    "a\\[b]",
    "\\!x",
    "\\#x",
    "x\\ ",
    "x\\",
    "[",
    "]",
    "[a]",
    "[!a]",
    "[^a]",
    "[a-z]",
    "[]",
    "a]b[",
    "a*b",
    "*",
    "**",
    "a**b",
    "a?b",
    "?",
    "??",
    "{a,b}",
    "{",
    "}",
    "!x",
    "x!",
    "!",
    "#x",
    "x#",
    "#",
    "^x",
    "x^",
    "$x",
    "x$",
    "x ",
    "x  ",
    " x",
    " ",
    "a b",
    "a  b",
    "(x)",
    "a|b",
    "a+b",
    "a.b",
    ".x",
    "..x",
    "x.",
    "x..",
    "...",
    "-x",
    "~x",
    "%x%",
    "@x",
    "x=y,z;w",
    "'x'",
    "`x`",
    "\"x\"",
    "<x>",
    "x:y",
    "zoë",
    "zoe\u{301}",
    "ZOË",
    "\u{ff3b}work\u{ff3d}",
    "\u{ff0a}",
    "\u{ff1f}",
    "\u{2215}x",
    "\u{29f5}x",
    "\u{430}dmin",
    "\u{212a}elvin",
    "\u{17f}x",
    "\u{130}x",
    "\u{131}x",
    "a\u{a0}b",
    "a\u{2028}b",
    "a\u{2029}b",
    "a\u{200b}b",
    "\u{feff}x",
    "a\u{3000}b",
    "a\u{85}b",
    "a\u{7}b",
    "a\u{7f}b",
    "a\u{1b}[0m",
    "\u{1f600}",
    "x\u{1f600}y",
    "MiXeD",
    "Ab Cd ",
    "[MiXeD]*?",
    "a\n[b]*\\c? ",
];

fn windows_allows(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(|c| {
            (c as u32) < 32 || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

fn names(style: Style) -> Vec<&'static str> {
    NAMES
        .iter()
        .copied()
        .filter(|n| style == Style::Unix || windows_allows(n))
        .collect()
}

/// `base` joined with `names` in `style`.
fn path(style: Style, base: &str, names: &[&str]) -> String {
    let sep = if style == Style::Windows { "\\" } else { "/" };
    let mut out = base.to_owned();
    for n in names {
        if !out.ends_with(sep) {
            out.push_str(sep);
        }
        out.push_str(n);
    }
    out
}

fn base(style: Style) -> &'static str {
    match style {
        Style::Windows => r"D:\T",
        Style::Unix => "/Volumes/T",
    }
}

fn style_name(style: Style) -> &'static str {
    match style {
        Style::Windows => "windows",
        Style::Unix => "unix",
    }
}

/// Whether two names are one on a case-insensitive file system, as Claude Code's matcher compares them.
fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase() || a.to_uppercase() == b.to_uppercase()
}

/// The checks, each naming its set of rules by number: a browser profile's rules are hundreds, checked against
/// many paths, so each set goes to the matcher once (`tools/rule-matcher.mjs` runs a set's checks together).
#[derive(Default)]
struct Checks {
    checks: Vec<Value>,
    sets: Vec<Vec<String>>,
    numbers: std::collections::HashMap<Vec<String>, usize>,
}

impl Checks {
    fn add(
        &mut self,
        style: Style,
        kind: &str,
        rules: &[String],
        path: String,
        expect: bool,
        why: &str,
    ) {
        let set = match self.numbers.get(rules) {
            Some(n) => *n,
            None => {
                self.sets.push(rules.to_vec());
                self.numbers.insert(rules.to_vec(), self.sets.len() - 1);
                self.sets.len() - 1
            }
        };
        self.checks.push(json!({
            "style": style_name(style),
            "kind": kind,
            "set": set,
            "path": path,
            "expect": expect,
            "why": why,
        }));
    }
}

/// Deny rules for a moved folder at every adversarial name: they match it, what is in it, and it in other case.
fn moved_folder_checks(style: Style, checks: &mut Checks) {
    let b = base(style);
    for name in names(style) {
        for dir in [
            path(style, b, &[name]),
            path(style, b, &[name, "cfg"]),
            path(style, b, &["cfg", name]),
            path(style, b, &[name, name]),
        ] {
            let rules = hidden::config_rules_in(&dir, style)
                .unwrap_or_else(|| panic!("{dir:?} has deny rules"));
            assert!(
                rules.iter().all(|r| !r.contains(['\n', '\r'])),
                "no line break in a rule: {rules:?}"
            );
            checks.add(
                style,
                "deny",
                &rules,
                dir.clone(),
                true,
                "the moved folder itself",
            );
            for child in [".credentials.json", "plain", name] {
                let inside = path(style, &dir, &[child]);
                checks.add(
                    style,
                    "deny",
                    &rules,
                    inside,
                    true,
                    "a file in the moved folder",
                );
            }
            let deep = path(style, &dir, &["projects", name, "x.jsonl"]);
            checks.add(
                style,
                "deny",
                &rules,
                deep,
                true,
                "deep in the moved folder",
            );
            for other in [dir.to_uppercase(), dir.to_lowercase()] {
                if other.chars().count() == dir.chars().count() {
                    let inside = path(style, &other, &["auth.json"]);
                    checks.add(
                        style,
                        "deny",
                        &rules,
                        inside,
                        true,
                        "the moved folder in other case",
                    );
                }
            }
        }
        // Relative: by its names anywhere.
        let relative = format!("~/{name}");
        let rules = hidden::config_rules_in(&relative, style).unwrap();
        for at in [
            path(style, b, &[name, "auth.json"]),
            path(style, b, &["a\nb", name, "x"]),
        ] {
            if style == Style::Windows && at.contains('\n') {
                continue;
            }
            checks.add(
                style,
                "deny",
                &rules,
                at,
                true,
                "a relative moved folder, anywhere",
            );
        }
    }
}

/// The always-hidden places stay denied under folders with adversarial names.
fn named_place_checks(style: Style, checks: &mut Checks) {
    let rules = hidden::deny_rules_in(&[]);
    let home = match style {
        Style::Windows => r"C:\Users\alex",
        Style::Unix => "/Users/alex",
    };
    for name in names(style) {
        for (at, why) in [
            (vec![name, ".ssh", "id_rsa"], ".ssh under an odd folder"),
            (vec![name, ".ssh"], "the .ssh folder under an odd folder"),
            (
                vec![name, "deep", name, ".env"],
                ".env deep under odd folders",
            ),
            (
                vec![
                    name,
                    "Google",
                    "Chrome",
                    "User Data",
                    "Default",
                    "Login Data",
                ],
                "a browser's saved passwords",
            ),
            (
                vec![name, "Library", "Safari", "History.db"],
                "Safari's history",
            ),
            (vec![name, "vault.kdbx"], "a KeePass file"),
            (
                vec![name, "Profiles", name, "storage", "x"],
                "Firefox site storage under an odd profile",
            ),
            (
                vec![
                    "Google",
                    "Chrome",
                    "User Data",
                    name,
                    "Sessions",
                    "Session_1",
                ],
                "a Chromium session in a profile with an odd name",
            ),
            (
                vec!["Library", "Application Support", name, name, "Shortcuts"],
                "a Mac Chromium profile with an odd name",
            ),
            (
                vec![
                    name,
                    "abc.custom",
                    "storage",
                    "default",
                    "x",
                    "ls",
                    "data.sqlite",
                ],
                "a Firefox profile kept in an odd folder",
            ),
        ] {
            checks.add(style, "deny", &rules, path(style, home, &at), true, why);
        }
        // (A path from the root can't start with `...`: ignore 5.3.2 takes it for an unresolved `../`.)
        if style == Style::Unix && !name.chars().all(|c| c == '.') {
            checks.add(
                style,
                "deny",
                &rules,
                format!("/{name}/.ssh/id_rsa"),
                true,
                ".ssh at depth one",
            );
        }
    }
    checks.add(
        style,
        "deny",
        &rules,
        path(style, home, &[".ssh", "id_rsa"]),
        true,
        "plain .ssh",
    );
    let root_level = match style {
        Style::Windows => r"C:\.ssh\id_rsa".to_owned(),
        Style::Unix => "/.ssh/id_rsa".to_owned(),
    };
    checks.add(style, "deny", &rules, root_level, true, ".ssh at the root");
    checks.add(
        style,
        "deny",
        &rules,
        path(style, home, &["Documents", "notes.txt"]),
        false,
        "an ordinary file",
    );
}

/// Browser data in profiles with names of their own (`--profile-directory=Work`) and in Firefox profiles kept
/// anywhere is denied, and browsers' caches, which the cache rules propose, and ordinary files of the same names
/// are not.
fn browser_profile_checks(style: Style, checks: &mut Checks) {
    let rules = hidden::deny_rules_in(&[]);
    let (home, elsewhere) = match style {
        Style::Windows => (r"C:\Users\alex", r"D:\FirefoxProfiles\abc.custom"),
        Style::Unix => ("/Users/alex", "/Users/alex/Firefox profiles/work.custom"),
    };
    let chromium: Vec<String> = match style {
        Style::Windows => vec![
            path(
                style,
                home,
                &["AppData", "Local", "Google", "Chrome", "User Data", "Work"],
            ),
            path(
                style,
                home,
                &[
                    "AppData",
                    "Local",
                    "Microsoft",
                    "Edge",
                    "User Data",
                    "Banking",
                ],
            ),
            path(
                style,
                home,
                &["AppData", "Roaming", "Opera Software", "Opera Stable"],
            ),
            r"E:\ChromeData\Default".to_owned(),
        ],
        Style::Unix => vec![
            path(
                style,
                home,
                &["Library", "Application Support", "Google", "Chrome", "Work"],
            ),
            path(
                style,
                home,
                &[
                    "Library",
                    "Application Support",
                    "Microsoft Edge",
                    "Banking",
                ],
            ),
            path(
                style,
                home,
                &["Library", "Application Support", "com.operasoftware.Opera"],
            ),
            "/Volumes/Data/ChromeData/Profile 2".to_owned(),
        ],
    };
    for profile in &chromium {
        for store in [
            &["Login Data"][..],
            &["Web Data-journal"],
            &["Network", "Cookies"],
            &["History"],
            &["Bookmarks"],
            &["Bookmarks.bak"],
            &["Top Sites"],
            &["Shortcuts"],
            &["Favicons-journal"],
            &["DIPS"],
            &["Sessions", "Session_13360000000000000"],
            &[
                "IndexedDB",
                "https_mail.example.com_0.indexeddb.leveldb",
                "000003.log",
            ],
            &["Local Storage", "leveldb", "000005.ldb"],
            &[
                "Local Extension Settings",
                "nkbihfbeogaeaoehlefnkodbefgpgknn",
                "000007.log",
            ],
        ] {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, profile, store),
                true,
                "a Chromium store in any profile",
            );
        }
        for cache in [
            &["Cache", "Cache_Data", "f_0001"][..],
            &["Code Cache", "js", "x"],
            &["GPUCache", "data_0"],
            &["Preferences"],
        ] {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, profile, cache),
                false,
                "a Chromium cache stays readable",
            );
        }
    }
    let firefox = [
        elsewhere.to_owned(),
        match style {
            Style::Windows => path(
                style,
                home,
                &[
                    "AppData", "Roaming", "Mozilla", "Firefox", "Profiles", "x.work",
                ],
            ),
            Style::Unix => path(
                style,
                home,
                &[
                    "Library",
                    "Application Support",
                    "Firefox",
                    "Profiles",
                    "x.work",
                ],
            ),
        },
    ];
    for profile in &firefox {
        for store in [
            &["logins.json"][..],
            &["logins-backup.json"],
            &["key4.db"],
            &["cert9.db"],
            &["places.sqlite-wal"],
            &["cookies.sqlite"],
            &["formhistory.sqlite"],
            &["autofill-profiles.json"],
            &["signedInUser.json"],
            &["sessionstore.jsonlz4"],
            &["sessionstore-backups", "recovery.jsonlz4"],
            &["storage.sqlite"],
            &["storage-sync-v2.sqlite"],
            &["storage", "ls-archive.sqlite"],
            &[
                "storage",
                "default",
                "https+++mail.example.com",
                "ls",
                "data.sqlite",
            ],
            &["storage", "permanent", "chrome", "idb", "x.sqlite"],
            &["storage", "temporary", "x"],
        ] {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, profile, store),
                true,
                "a Firefox store in a profile kept anywhere",
            );
        }
        checks.add(
            style,
            "deny",
            &rules,
            path(style, profile, &["cache2", "entries", "ABC"]),
            false,
            "Firefox's cache stays readable",
        );
    }
    let mut readable = vec![
        path(style, home, &["Documents", "Sessions", "notes.txt"]),
        path(style, home, &["Documents", "Shortcuts", "notes.txt"]),
        path(style, home, &["Documents", "Favicons", "icon.png"]),
        path(style, home, &["Documents", "storage", "boxes.xlsx"]),
        path(style, home, &["Documents", "Bookmarks.html"]),
    ];
    readable.extend(match style {
        Style::Windows => vec![
            path(
                style,
                home,
                &[
                    "AppData",
                    "Local",
                    "Google",
                    "Chrome",
                    "User Data",
                    "ShaderCache",
                    "x",
                ],
            ),
            path(
                style,
                home,
                &[
                    "AppData", "Local", "Mozilla", "Firefox", "Profiles", "x.work", "cache2",
                    "entries", "A",
                ],
            ),
            path(
                style,
                home,
                &["AppData", "Roaming", "discord", "Cache", "Cache_Data", "x"],
            ),
        ],
        Style::Unix => vec![
            path(
                style,
                home,
                &[
                    "Library",
                    "Caches",
                    "Google",
                    "Chrome",
                    "Work",
                    "Cache",
                    "Cache_Data",
                    "f",
                ],
            ),
            path(
                style,
                home,
                &[
                    "Library", "Caches", "Firefox", "Profiles", "x.work", "cache2", "entries", "A",
                ],
            ),
            path(
                style,
                home,
                &["Library", "Caches", "com.apple.Safari", "WebKitCache", "x"],
            ),
        ],
    });
    for at in readable {
        checks.add(
            style,
            "deny",
            &rules,
            at,
            false,
            "a cache or an ordinary file stays readable",
        );
    }
}

/// What a browser profile keeps beside its markers (the review of #29's paths among them), and names a person or a
/// later version of the browser could add: every one hidden by a profile's rules.
const PROFILE_HIDDEN: &[&[&str]] = &[
    &["Sessions", "Session_123"],
    &["Shortcuts"],
    &["Favicons"],
    &["DIPS"],
    &["File System", "000", "t", "00", "00000000"],
    &["WebStorage", "1", "IndexedDB", "x"],
    &["databases", "Databases.db"],
    &["Sync Data", "LevelDB", "000003.log"],
    &[
        "Storage",
        "ext",
        "nkbihfbeogaeaoehlefnkodbefgpgknn",
        "def",
        "x",
    ],
    &["Service Worker", "CacheStorage", "abc", "index"],
    &["Preferences"],
    &["Secure Preferences"],
    &["Network Persistent State"],
    &["TransportSecurity"],
    &["Made After The Scan", "x"],
    // One character off a cache's name, or a cache's name and more.
    &["Cach"],
    &["Caches"],
    &["Cache2"],
    &["Cached"],
    &["CachedDat"],
    &["XCache"],
    &["Code"],
    &["Code Cach"],
    &["Code  Cache"],
    &["GPUCache "],
    &["GPUCacheX", "y"],
    &["ShaderCach"],
    &["\u{ff23}ache"],
    &["\u{212a}ache"],
];

/// The caches every Chromium profile and root keeps readable, in any case, with what is in them.
const PROFILE_READABLE: &[&[&str]] = &[
    &["Cache", "Cache_Data", "f_000001"],
    &["cache", "x"],
    &["Code Cache", "js", "index"],
    &["CODE CACHE", "wasm", "x"],
    &["GPUCache", "data_0"],
    &["DawnCache", "data_0"],
    &["DawnGraphiteCache", "data_0"],
    &["DawnWebGPUCache", "data_0"],
    &["GrShaderCache", "data_0"],
    &["GraphiteDawnCache", "data_0"],
    &["ShaderCache", "data_0"],
    &["CachedData", "x"],
    // A cache's own folders stay readable: a class never takes in a `/`.
    &["Cache", "Data"],
    &["Cache", "d", "x"],
];

/// A browser profile's rules hide every child but its caches, for a profile at every adversarial name, in both
/// path styles; a root's leave the way down to its profiles readable too; a Firefox profile's leave its caches; and
/// a default root the scan didn't see is hidden whole.
fn browser_wall_checks(style: Style, checks: &mut Checks) {
    let b = base(style);
    let chromium = browsers::CHROMIUM_CACHES;
    let mut dirs: Vec<String> = names(style)
        .iter()
        .map(|n| path(style, b, &[n, "Work"]))
        .collect();
    dirs.push(path(style, b, &["BrowserData", "Work"]));
    for dir in &dirs {
        let rules = browsers::profile_rules_in(dir, chromium, style)
            .unwrap_or_else(|| panic!("{dir:?} has rules"));
        for child in PROFILE_HIDDEN {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, dir, child),
                true,
                "a profile's child that isn't a cache",
            );
        }
        for child in PROFILE_READABLE {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, dir, child),
                false,
                "a profile's cache",
            );
        }
        checks.add(
            style,
            "deny",
            &rules,
            dir.clone(),
            false,
            "the profile itself",
        );
        let (parent, last) = dir.rsplit_once(['/', '\\']).unwrap();
        let exact = last.chars().all(exact_char)
            && parent
                .chars()
                .all(|c| exact_char(c) || (style == Style::Windows && c == '\\'));
        if exact {
            for beside in [
                path(style, parent, &[&format!("{last}x"), "History"]),
                path(style, parent, &["History"]),
            ] {
                checks.add(style, "deny", &rules, beside, false, "beside the profile");
            }
        }
    }
    // A plain profile's children at every adversarial name are hidden: names made after the scan too.
    let work = path(style, b, &["BrowserData", "Work"]);
    let rules = browsers::profile_rules_in(&work, chromium, style).unwrap();
    for name in names(style) {
        for child in [vec![name], vec![name, "x"], vec!["Storage", name]] {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, &work, &child),
                true,
                "a child of any name",
            );
        }
    }
    // A root: its caches, and the way down to the profiles in it, stay readable; the rest of it is hidden.
    let root = path(style, b, &["BrowserData"]);
    let mut allow: Vec<&str> = chromium.to_vec();
    allow.extend(["Default", "Profile 1", "Work", "Zz_9-a.b"]);
    let rules = browsers::profile_rules_in(&root, &allow, style).unwrap();
    for (child, expect) in [
        (&["Local State"][..], true),
        (&["First Run"], true),
        (&["Crashpad", "reports", "x.dmp"], true),
        (&["Profile 2", "History"], true),
        (&["Profile", "x"], true),
        (&["Zz_9-a.bc"], true),
        (&["Defaul"], true),
        (&["Default", "History"], false),
        (&["PROFILE 1", "x"], false),
        (&["Work", "Cache", "x"], false),
        (&["zz_9-A.B", "x"], false),
        (&["ShaderCache", "x"], false),
        (&["GrShaderCache", "x"], false),
    ] {
        checks.add(
            style,
            "deny",
            &rules,
            path(style, &root, child),
            expect,
            "a browser's root",
        );
    }
    // Firefox: a profile's caches, and a root's way down to its profiles.
    let profile = path(style, b, &["FirefoxData", "abc.custom"]);
    let rules = browsers::profile_rules_in(&profile, browsers::FIREFOX_CACHES, style).unwrap();
    for (child, expect) in [
        (&["prefs.js"][..], true),
        (&["extensions.json"], true),
        (&["storage", "default", "x"], true),
        (&["cache2x"], true),
        (&["cache", "x"], true),
        (&["cache2", "entries", "ABC"], false),
        (&["startupCache", "x"], false),
        (&["thumbnails", "x.png"], false),
        (&["shader-cache", "x"], false),
        (&["jumpListCache", "x"], false),
    ] {
        checks.add(
            style,
            "deny",
            &rules,
            path(style, &profile, child),
            expect,
            "a Firefox profile",
        );
    }
    let firefox = path(style, b, &["Mozilla", "Firefox"]);
    let rules = browsers::profile_rules_in(&firefox, &["Profiles"], style).unwrap();
    for (child, expect) in [
        (&["profiles.ini"][..], true),
        (&["Crash Reports", "x"], true),
        (&["Profiles", "x.default", "prefs.js"], false),
    ] {
        checks.add(
            style,
            "deny",
            &rules,
            path(style, &firefox, child),
            expect,
            "Firefox's root",
        );
    }
    let rules = browsers::profile_rules_in(&firefox, &[], style).unwrap();
    checks.add(
        style,
        "deny",
        &rules,
        path(style, &firefox, &["Profiles", "x"]),
        true,
        "a root with no profile found is hidden but for itself",
    );
    // A default root the scan didn't see: whole.
    for name in names(style) {
        let dir = path(style, b, &[name, "User Data"]);
        let rules = browsers::whole_rules_in(&dir, style).unwrap();
        for at in [dir.clone(), path(style, &dir, &["Default", "Cache", "x"])] {
            checks.add(
                style,
                "deny",
                &rules,
                at,
                true,
                "a default root the scan didn't see",
            );
        }
    }
    let dir = path(style, b, &["Chrome", "User Data"]);
    let rules = browsers::whole_rules_in(&dir, style).unwrap();
    checks.add(
        style,
        "deny",
        &rules,
        path(style, b, &["Chrome", "User Data2", "x"]),
        false,
        "beside a whole root",
    );
}

/// The rules the app builds from a real scan, on this system: dummy profiles (a `--user-data-dir` of its own with
/// a profile named `Work`, `Default` in the browser's own root, Opera's root, a Firefox profile kept anywhere) are
/// hidden by them and the always-hidden places' rules together, while every cache in them, and a game's settings
/// folder with a `Preferences` file, stay readable.
fn scanned_profile_checks(checks: &mut Checks) -> usize {
    let style = Style::HOST;
    let dir = tempfile::tempdir().unwrap();
    let root = support::real_path(dir.path()).join("fx");
    let env = if cfg!(windows) {
        vacancy_core::Env::windows(&root, "alex")
    } else {
        vacancy_core::Env::macos(&root, "alex")
    };
    let dummy = support::dummy_browsers(&env.home.join("Elsewhere"), &env, support::KIB);
    let snap = support::scan_with(&root, support::MIB);
    let hider = hidden::Hider::of(&snap, &env);
    let mut rules = hidden::deny_rules_in(&[]);
    let walls = hider.deny_rules();
    assert!(!walls.is_empty());
    rules.extend(walls);
    let text = |p: &PathBuf| p.to_str().unwrap().to_owned();
    for p in &dummy.hidden {
        assert!(
            hider.hides(p).is_some(),
            "{} is hidden from prompts",
            p.display()
        );
        checks.add(style, "deny", &rules, text(p), true, "in a scanned profile");
    }
    for p in dummy.caches.iter().chain(&dummy.ordinary) {
        assert!(hider.hides(p).is_none(), "{} stays in prompts", p.display());
        checks.add(
            style,
            "deny",
            &rules,
            text(p),
            false,
            "a scanned profile's cache, or no profile",
        );
    }
    for p in &dummy.profiles {
        assert!(
            hider.hides(p).is_none() && hider.holds_browser_data(p),
            "{}",
            p.display()
        );
        checks.add(
            style,
            "deny",
            &rules,
            text(p),
            false,
            "a scanned profile itself",
        );
    }
    dummy.hidden.len()
}

/// A Chromium root's shared rules for its profiles (`claude::browsers::profile_names_rules_in`): every profile
/// Chromium names (`Default`, `Profile *`, `Guest Profile`, `System Profile`), those made after the scan too, is
/// hidden but its caches, at every adversarial root name; the profile itself, the root's own children and a
/// profile of another name are left to their own rules.
fn shared_profile_checks(style: Style, checks: &mut Checks) {
    let b = base(style);
    // Every sixth adversarial name: each root's rules are long, and the checks file has to fit in one string.
    let mut roots: Vec<String> = names(style)
        .iter()
        .step_by(6)
        .map(|n| path(style, b, &[n, "User Data"]))
        .collect();
    roots.push(path(style, b, &["BrowserData"]));
    for root in &roots {
        let rules = browsers::profile_names_rules_in(root, style)
            .unwrap_or_else(|| panic!("{root:?} has rules"));
        for (child, expect) in [
            (&["Profile 7", "History"][..], true),
            (&["Profile 12", "Sessions", "Session_1"], true),
            (&["Profile 99", "Preferences"], true),
            (&["PROFILE 3", "Service Worker", "CacheStorage", "x"], true),
            (&["Default", "Login Data"], true),
            (&["Guest Profile", "Cookies"], true),
            (&["System Profile", "Web Data"], true),
            (&["Profile 7", "Cache", "Cache_Data", "f_0001"], false),
            (&["Profile 7", "Code Cache", "js", "x"], false),
            (&["Default", "GPUCache", "data_0"], false),
            (&["Profile 7"], false),
            (&["ShaderCache", "x"], false),
            (&["Local State"], false),
            (&["Work", "History"], false),
        ] {
            checks.add(
                style,
                "deny",
                &rules,
                path(style, root, child),
                expect,
                "a root's profiles, by the names Chromium gives them",
            );
        }
    }
}

/// With privacy mode off in normal mode Claude may read the scanned drive only (`runner::off_drive_rules_in`): a
/// browser profile on another drive (the review's `C:\BrowserData\Work` with `D:\` scanned) is denied whole, and so
/// is every other drive; on a Mac, scanning `/` denies `/Volumes` and `/System/Volumes` (other disks, and the Data
/// volume's second name), and scanning `/Volumes/X` allows `/Volumes/X` only.
fn drive_scope_checks(style: Style, checks: &mut Checks) -> usize {
    let mut n = 0;
    let mut case = |root: &str, others: &[&str], deny: &[(&str, bool)], allow: &[(&str, bool)]| {
        let others: Vec<String> = others.iter().map(|s| (*s).to_owned()).collect();
        let rules = runner::off_drive_rules_in(root, &others, style);
        let allowed = vec![allow_rule_in(root, style).expect("a drive's root is spelled exactly")];
        for (p, expect) in deny {
            checks.add(
                style,
                "deny",
                &rules,
                (*p).to_owned(),
                *expect,
                "the scanned drive only",
            );
            n += 1;
        }
        for (p, expect) in allow {
            checks.add(
                style,
                "allow",
                &allowed,
                (*p).to_owned(),
                *expect,
                "the scanned drive's allow rule",
            );
            n += 1;
        }
    };
    match style {
        Style::Windows => {
            case(
                r"D:\",
                &[r"C:\", r"E:\"],
                &[
                    (r"C:\BrowserData\Work\Preferences", true),
                    (r"C:\BrowserData\Work\Sessions\Session_1", true),
                    (r"C:\BrowserData\Work\Cache\data_0", true),
                    (r"C:\Users\alex\Documents\notes.txt", true),
                    (r"c:\users\alex\x", true),
                    (r"E:\x", true),
                    (r"Z:\a\b", true),
                    (r"D:\Users\alex\Documents\notes.txt", false),
                    (r"D:\BrowserData\Work\Cache\data_0", false),
                    (r"d:\x", false),
                ],
                &[(r"D:\x\y", true), (r"C:\x", false), (r"E:\D\x", false)],
            );
            case(
                r"C:\Mounts\USB",
                &[r"C:\", r"E:\"],
                &[
                    (r"C:\Users\alex\x", true),
                    (r"C:\Mounts\Other\x", true),
                    (r"C:\Mounts\USBx\a", true),
                    (r"C:\Mounts\x.txt", true),
                    (r"C:\x.txt", true),
                    (r"E:\x", true),
                    (r"C:\Mounts\USB\x", false),
                    (r"C:\mounts\usb\x", false),
                    (r"C:\Mounts\USB\Users\alex\x", false),
                ],
                &[
                    (r"C:\Mounts\USB\x", true),
                    (r"C:\Mounts\x", false),
                    (r"C:\Users\x", false),
                ],
            );
        }
        Style::Unix => {
            case(
                "/",
                &["/Volumes/USB", "/Volumes/Backup"],
                &[
                    ("/Volumes/USB/x", true),
                    ("/Volumes/Macintosh HD/Users/alex/x", true),
                    ("/Volumes/new disk/x", true),
                    ("/Volumes", true),
                    ("/volumes/usb/x", true),
                    (
                        "/System/Volumes/Data/Users/alex/Library/Application Support/Google/Chrome/Default/Preferences",
                        true,
                    ),
                    ("/System/Volumes/Data/Volumes/USB/x", true),
                    ("/System/Volumes/Preboot/x", true),
                    ("/Users/alex/Documents/notes.txt", false),
                    ("/Applications/App.app/Contents/Info.plist", false),
                    ("/System/Library/x", false),
                    ("/VolumesX/a", false),
                    ("/private/tmp/x", false),
                ],
                &[("/Users/alex/x", true), ("/Volumes/USB/x", true)],
            );
            case(
                "/Volumes/USB",
                &["/", "/Volumes/Backup"],
                &[
                    (
                        "/Users/alex/Library/Application Support/Google/Chrome/Default/Preferences",
                        true,
                    ),
                    ("/Users/alex/Documents/notes.txt", true),
                    ("/Volumes/Backup/x", true),
                    ("/Volumes/Other/x", true),
                    ("/Volumes/USBx/a", true),
                    ("/Volumes/x.txt", true),
                    ("/System/Volumes/Data/Volumes/USB/x", true),
                    ("/x", true),
                    ("/Volumes/USB/x", false),
                    ("/Volumes/usb/x", false),
                    ("/Volumes/USB/Users/alex/x", false),
                    ("/Volumes/USB", false),
                ],
                &[
                    ("/Volumes/USB/x", true),
                    ("/Users/alex/x", false),
                    ("/Volumes/USBx/a", false),
                ],
            );
        }
    }
    n
}

/// Every other file system mounted, wherever it is mounted (`runner::mount_rules_in`, with the scanned drive's own
/// rules): files under a disk image mounted in the home of a scanned Mac `/` (by its own path and by the Data
/// volume's), a VHD in an empty folder of a scanned `D:\`, a mount nested in one, and mounts with odd names are
/// denied, while files beside them stay allowed.
fn mount_scope_checks(style: Style, checks: &mut Checks) -> usize {
    let mut n = 0;
    let mut case = |root: &str, mounts: &[&str], deny: &[(&str, bool)]| {
        let mounts: Vec<String> = mounts.iter().map(|s| (*s).to_owned()).collect();
        let mut rules = runner::off_drive_rules_in(root, &[], style);
        rules.extend(runner::mount_rules_in(root, &mounts, style).expect("every mount is spelled"));
        for (p, expect) in deny {
            checks.add(
                style,
                "deny",
                &rules,
                (*p).to_owned(),
                *expect,
                "every other mount, wherever it is",
            );
            n += 1;
        }
    };
    match style {
        Style::Windows => {
            case(
                r"D:\",
                &[
                    r"D:\",
                    r"C:\",
                    r"D:\Mount\Disk2\",
                    r"D:\Mount\Disk2\Inner\",
                    r"D:\Mount\[x] {1}!#\",
                    r"D:\Mount\Zoë\",
                ],
                &[
                    (
                        r"D:\Mount\Disk2\Users\alex\AppData\Local\Google\Chrome\User Data\Default\Login Data",
                        true,
                    ),
                    (r"D:\Mount\Disk2\BrowserData\Work\Sessions\Session_1", true),
                    (r"D:\Mount\Disk2", true),
                    (r"d:\mount\disk2\x", true),
                    (r"D:\Mount\Disk2\Inner\x", true),
                    (r"D:\Mount\[x] {1}!#\y", true),
                    (r"D:\Mount\Zoë\y", true),
                    (r"C:\x", true),
                    (r"D:\Mount\Disk3\x", false),
                    (r"D:\Mount\Disk2x\a", false),
                    (r"D:\Mount\Disk\a", false),
                    (r"D:\Mount\x\y", false),
                    (r"D:\Mount\x.txt", false),
                    (r"D:\Mount\Zp\y", false),
                    (r"D:\Users\alex\notes.txt", false),
                ],
            );
        }
        Style::Unix => {
            case(
                "/",
                &[
                    "/",
                    "/dev",
                    "/System/Volumes/Data",
                    "/Users/alex/Archive",
                    "/System/Volumes/Data/Users/alex/Backups",
                    "/Users/alex/Archive/Nested",
                    "/Users/alex/[Old] {1}!#",
                    "/Users/alex/a\nb",
                ],
                &[
                    (
                        "/Users/alex/Archive/Library/Application Support/Google/Chrome/Default/Cookies",
                        true,
                    ),
                    (
                        "/Users/alex/Archive/BrowserData/Work/Sessions/Session_1",
                        true,
                    ),
                    ("/Users/alex/Archive", true),
                    ("/users/alex/archive/x", true),
                    ("/Users/alex/Archive/Nested/x", true),
                    ("/Users/alex/Backups/x", true),
                    ("/System/Volumes/Data/Users/alex/Backups/x", true),
                    ("/Users/alex/[Old] {1}!#/x", true),
                    ("/Users/alex/a\nb/x", true),
                    ("/dev/disk0", true),
                    ("/Users/alex/Archive2/x", false),
                    ("/Users/alex/Archiv/x", false),
                    ("/Users/alex/Backup/x", false),
                    ("/Users/alex/O/x", false),
                    ("/Users/alex/Old/x", false),
                    ("/Users/alex/a/x", false),
                    ("/Users/alex/Documents/notes.txt", false),
                    ("/Applications/App.app/Contents/Info.plist", false),
                ],
            );
            case(
                "/Volumes/USB",
                &[
                    "/",
                    "/Volumes/USB",
                    "/Volumes/USB/Inner",
                    "/System/Volumes/Data",
                ],
                &[
                    ("/Volumes/USB/Inner/x", true),
                    ("/Users/alex/x", true),
                    ("/Volumes/USB/x", false),
                    ("/Volumes/USB/Inner2/x", false),
                    ("/Volumes/USB/Inne/x", false),
                ],
            );
        }
    }
    n
}

/// The check of Claude Code (`claude::canary`), as the app makes it on this system: its settings deny the canary in
/// its dummy profile, and the profile's other stores, while its control file stays readable, by the allow rule too.
fn canary_checks(checks: &mut Checks) -> usize {
    use vacancy_core::claude::canary::{self, Canary, Scope};
    let style = Style::HOST;
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let base = support::real_path(dir.path()).join(canary::FOLDER);
    let check = Canary::new(
        &base,
        Scope::Drive {
            others: Vec::new(),
            mounts: Vec::new(),
        },
    )
    .unwrap();
    let inv = runner::invocation(
        Path::new("claude"),
        check.call(),
        check.access(),
        &base.join("call-1.settings.json"),
    );
    let settings: Value = serde_json::from_str(&inv.settings.unwrap().1).unwrap();
    let rules = |key: &str| -> Vec<String> {
        serde_json::from_value(settings["permissions"][key].clone()).unwrap()
    };
    let (deny, allow) = (rules("deny"), rules("allow"));
    let (control, hidden_file) = check.files();
    let text = |p: &Path| p.to_str().unwrap().to_owned();
    let profile = check.dir().join("BrowserData").join("Work");
    let mut n = 0;
    for (path, expect) in [
        (text(hidden_file), true),
        (text(&profile.join("History")), true),
        (text(&profile.join("Preferences")), true),
        (text(&profile.join("Sessions")), true),
        (text(&profile.join("Cache").join("data_0")), false),
        (text(control), false),
    ] {
        checks.add(
            style,
            "deny",
            &deny,
            path,
            expect,
            "the check's own settings",
        );
        n += 1;
    }
    checks.add(
        style,
        "allow",
        &allow,
        text(control),
        true,
        "the check's read scope",
    );
    n + 1
}

/// Allow rules exist only for roots they spell exactly, and match inside them only.
fn allow_checks(style: Style, checks: &mut Checks) -> usize {
    let b = base(style);
    let all = names(style);
    let mut spelled = 0;
    for name in &all {
        for root in [
            path(style, b, &[name]),
            path(style, b, &[name, "fx"]),
            path(style, b, &["fx", name]),
        ] {
            let exact = root
                .chars()
                .filter(|c| *c != '/' && !(style == Style::Windows && matches!(c, '\\' | ':')))
                .all(exact_char);
            let Some(rule) = allow_rule_in(&root, style) else {
                assert!(!exact, "{root:?} is spelled by an allow rule");
                if style == Style::HOST {
                    let r = PathBuf::from(&root);
                    assert!(
                        reads_unavailable_in(Some(&r), &[]).is_some(),
                        "{root:?} refuses reads"
                    );
                    let settings = read_settings_in(&[r], &[]);
                    assert_eq!(settings["permissions"]["allow"], json!([]), "{root:?}");
                }
                continue;
            };
            assert!(
                exact,
                "{root:?} has no allow rule: it holds a character the matcher can't spell"
            );
            spelled += 1;
            let rules = vec![rule];
            for inside in [
                path(style, &root, &["x.txt"]),
                path(style, &root, &[name, "y"]),
                path(style, &root, &["sub", "deep", "z"]),
            ] {
                checks.add(style, "allow", &rules, inside, true, "inside the root");
            }
            let (parent, last) = root.rsplit_once(['/', '\\']).unwrap();
            let mut outside = vec![
                path(style, parent, &["outside.txt"]),
                path(style, parent, &[&format!("{last}x"), "a"]),
                path(style, parent, &[&format!("{last} "), "a"]),
                path(style, parent, &[&format!("x{last}"), "a"]),
                path(style, b, &["fx", "outside.txt"]),
                path(style, b, &["outside.txt"]),
                path(style, b, &["tmp", "fx", "outside.txt"]),
            ];
            for other in &all {
                // A sibling whose name differs from the root's only in case is left out on purpose: the matcher
                // compares case-insensitively, so the allow rule covers it too. That matters only on a
                // case-sensitive Mac volume in fixture mode with privacy mode off, a limit Zach accepted (ADR 0008,
                // "Known limit"); normal mode's allow rules are whole drives.
                if !same_name(other, last) {
                    outside.push(path(style, parent, &[other, "a"]));
                }
            }
            // One character off, anywhere in the root's last name.
            let chars: Vec<char> = last.chars().collect();
            for i in 0..chars.len() {
                for c in ['x', '?', '\n', '*', '[', '\\'] {
                    let mut off = chars.clone();
                    off[i] = c;
                    let off: String = off.into_iter().collect();
                    if !same_name(&off, last) && (style == Style::Unix || windows_allows(&off)) {
                        outside.push(path(style, parent, &[&off, "a"]));
                    }
                }
            }
            for at in outside {
                checks.add(style, "allow", &rules, at, false, "beside the root");
            }
        }
    }
    spelled
}

/// The settings the app builds on this system: the same rules, for its own paths.
fn host_checks(checks: &mut Checks) {
    let style = Style::HOST;
    let b = base(style);
    let config: Vec<PathBuf> = names(style)
        .iter()
        .map(|n| PathBuf::from(path(style, b, &[n, "claude-config"])))
        .collect();
    let roots = vec![PathBuf::from(path(style, b, &["[fx]{1}"]))];
    let settings = read_settings_in(&roots, &config);
    let deny: Vec<String> =
        serde_json::from_value(settings["permissions"]["deny"].clone()).unwrap();
    let allow: Vec<String> =
        serde_json::from_value(settings["permissions"]["allow"].clone()).unwrap();
    assert_eq!(allow.len(), 1);
    for dir in &config {
        let at = dir.join("auth.json").to_str().unwrap().to_owned();
        checks.add(
            style,
            "deny",
            &deny,
            at,
            true,
            "the app's settings deny each moved folder",
        );
    }
    let inside = roots[0].join("x").to_str().unwrap().to_owned();
    checks.add(
        style,
        "allow",
        &allow,
        inside,
        true,
        "the app's settings allow the root",
    );
    let beside = path(style, b, &["[fx]{1}x", "a"]);
    checks.add(
        style,
        "allow",
        &allow,
        beside,
        false,
        "and nothing beside it",
    );
    assert!(reads_unavailable_in(Some(&roots[0]), &config).is_none());
}

#[test]
fn claude_codes_matcher_gives_each_rule_exactly_its_meaning() {
    let mut checks = Checks::default();
    let mut spelled = 0;
    for style in [Style::Windows, Style::Unix] {
        moved_folder_checks(style, &mut checks);
        named_place_checks(style, &mut checks);
        browser_profile_checks(style, &mut checks);
        browser_wall_checks(style, &mut checks);
        shared_profile_checks(style, &mut checks);
        assert!(drive_scope_checks(style, &mut checks) > 20);
        assert!(mount_scope_checks(style, &mut checks) > 10);
        spelled += allow_checks(style, &mut checks);
    }
    host_checks(&mut checks);
    assert!(canary_checks(&mut checks) > 5);
    let scanned = scanned_profile_checks(&mut checks);
    assert!(scanned > 40, "the dummy profiles hide {scanned} places");
    assert!(spelled > 100, "most roots are spelled: {spelled}");

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    // `VACANCY_RULE_CHECKS` keeps the checks where it says, to run `tools/rule-matcher.mjs` on them by hand.
    let file = std::env::var_os("VACANCY_RULE_CHECKS")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.path().join("checks.json"));
    let count = checks.checks.len();
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({ "sets": checks.sets, "checks": checks.checks })).unwrap(),
    )
    .unwrap();
    let out = Command::new("node")
        .arg(repo.join("tools/rule-matcher.mjs"))
        .arg(&file)
        .current_dir(&repo)
        .output()
        .expect("Node runs tools/rule-matcher.mjs (install Node, then npm ci)");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "rule-matcher said: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let failures = report["failures"].as_array().unwrap();
    // Each kind of failure once, with how often it came.
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for f in failures {
        let mut f = f.clone();
        f["path"] = Value::Null;
        let key = f.to_string();
        match kinds.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => kinds.push((key, 1)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} checks failed in Claude Code's matcher, {} kinds:\n{}",
        failures.len(),
        kinds.len(),
        kinds
            .iter()
            .take(80)
            .map(|(k, n)| format!("{n} x {k}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(out.status.success());
    assert_eq!(
        report["versions"],
        json!(["7.0.5", "5.3.2"]),
        "both matchers ran"
    );
    assert_eq!(report["checks"], count);
    println!(
        "{count} checks in ignore 7.0.5 and 5.3.2, {spelled} roots spelled exactly; the matcher peaked at {} MB",
        report["peakRssMb"]
    );
}

/// A Unix fixture root with a line break: no allow rule, so it can't match `tmp/fx/outside.txt` (the review's
/// case), and fixture mode refuses reads.
#[test]
fn a_root_with_a_line_break_gets_no_allow_rule() {
    assert_eq!(allow_rule_in("/tmp/fx\nname", Style::Unix), None);
    assert_eq!(allow_rule_in("/tmp/x\\^z", Style::Unix), None);
    assert_eq!(allow_rule_in("/tmp/x\\$z", Style::Unix), None);
    assert_eq!(
        hidden::config_rules_in("/Volumes/a\nb/cfg", Style::Unix).unwrap(),
        vec!["Read(//Volumes/a*b/cfg)", "Read(//Volumes/a*b/cfg/**)"]
    );
    assert!(runner::rule_path_in("/tmp/fx\nname", Style::Unix).is_none());
}

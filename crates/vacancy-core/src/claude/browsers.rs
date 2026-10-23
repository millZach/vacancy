//! Browser profiles, hidden whole except their caches (ADR 0008). The by-name rules in [`super::hidden`] hide the
//! stores browsers are known to keep; this hides everything else in a profile too, found from the scan:
//!
//! - **A Chromium profile** (Edge, Chrome, Brave, Vivaldi, Opera, an Electron app) is a folder holding a
//!   `Preferences` file and at least one of `History`, `Login Data`, `Cookies`, `Web Data`, `Bookmarks`,
//!   `Secure Preferences` or a `Network` folder. A `Preferences` file alone is what a game's or another app's
//!   settings folder may hold, so it never counts on its own; the second marker is a store only a browser engine
//!   keeps beside it. Electron apps (Discord, Slack, VS Code) keep the same layout and their sign-ins the same way,
//!   so their data folders count as profiles too.
//! - **A Firefox profile** is a folder holding `prefs.js` and at least one of `times.json`, `places.sqlite`,
//!   `key4.db`, `cookies.sqlite` or `compatibility.ini`.
//! - **A browser's root** is the folder above a Chromium profile when it holds `Local State` (the user data folder,
//!   wherever `--user-data-dir` put it), and each browser's default root ([`known_roots`]) that the scan saw. In a
//!   Chromium root, children Chromium names as profiles (`Default`, `Profile *`, `Guest Profile`, `System
//!   Profile`) are profiles even without the markers.
//! - A default root the scan didn't see (another drive's, say) is hidden whole, its caches included: no rule can
//!   propose a cache the scan didn't see.
//!
//! In each of these folders every child is hidden except the cache folders the built-in rules propose there
//! ([`CHROMIUM_CACHES`], [`FIREFOX_CACHES`]) and the way down to the profiles inside it (`Profiles` in Firefox's
//! root). Claude Code's deny rules can't say "everything but", because a deny rule always wins and there are no
//! negated patterns (`ignore` 7.0.5 reads `[!x]` as the characters `!` and `x`). So the rules spell the complement
//! of the allowed names instead ([`complement`]): a set of patterns, from a tree of the allowed names, that matches
//! every name but those, made of literal characters, `?`, `*` and character classes of plain ranges. It holds for
//! children made after the scan too. The same folders feed the prompt filter and refuse Claude's suggestions in them
//! ([`Browsers::hides`], [`Browsers::holds`]). A Chromium root's profiles with the names Chromium gives share one set
//! of rules ([`profile_names_rules_in`]), so the settings stay small however many profiles a root holds.
//!
//! The scan keeps only the names of files of 1 MiB or more, but notes these markers for every folder
//! ([`Markers`]), so finding profiles costs nothing extra. Every name of a hard-linked file marks its folder (hard
//! links are merged for the byte totals only), so a profile backed up with hard links is a profile too.

use std::path::{Component, Path, PathBuf};

use super::hidden::keyed;
use super::runner::{Style, cover_path_in, rule_cover, rule_literal};
use crate::categorize::{Env, Platform};
use crate::snapshot::{DirId, Markers, Snapshot};

/// What stays readable in a Chromium profile or root: the caches the built-in rules propose there (the app-cache
/// rule's names, which take in the browser-cache rule's). `Service Worker/CacheStorage` isn't one of them: it is a
/// site's own store (the Cache API), which can hold private data and no rule proposes, so `Service Worker` is
/// hidden whole.
pub const CHROMIUM_CACHES: &[&str] = crate::rules::APP_CACHES;

/// What stays readable in a Firefox profile: the caches it rebuilds. (On Windows its `cache2` is in a separate
/// local folder without `prefs.js`, which isn't a profile here and stays readable as it is.)
pub const FIREFOX_CACHES: &[&str] = &[
    "cache2",
    "startupCache",
    "thumbnails",
    "shader-cache",
    "jumpListCache",
];

/// The names Chromium gives the profiles in its root, matched like the by-name rules' patterns (`*` is any run).
const CHROMIUM_PROFILE_NAMES: &[&str] =
    &["Default", "Profile *", "Guest Profile", "System Profile"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Chromium,
    Firefox,
}

/// Each browser's default root under `env`'s folders, with its kind: Windows' `%LOCALAPPDATA%` and `%APPDATA%`,
/// or a Mac's `~/Library/Application Support`.
fn known_roots(env: &Env) -> Vec<(PathBuf, Kind)> {
    let sub = |base: &Path, rel: &str| rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s));
    let mut out = Vec::new();
    match env.platform {
        Platform::Windows => {
            for rel in [
                "Google/Chrome/User Data",
                "Google/Chrome Beta/User Data",
                "Google/Chrome Dev/User Data",
                "Google/Chrome SxS/User Data",
                "Microsoft/Edge/User Data",
                "Microsoft/Edge Beta/User Data",
                "Microsoft/Edge Dev/User Data",
                "Microsoft/Edge SxS/User Data",
                "BraveSoftware/Brave-Browser/User Data",
                "BraveSoftware/Brave-Browser-Beta/User Data",
                "BraveSoftware/Brave-Browser-Nightly/User Data",
                "Vivaldi/User Data",
                "Chromium/User Data",
                "Yandex/YandexBrowser/User Data",
            ] {
                out.push((sub(&env.local_app_data, rel), Kind::Chromium));
            }
            for rel in [
                "Opera Software/Opera Stable",
                "Opera Software/Opera GX Stable",
                "Opera Software/Opera Beta",
                "Opera Software/Opera Developer",
            ] {
                out.push((sub(&env.app_data, rel), Kind::Chromium));
            }
            out.push((sub(&env.app_data, "Mozilla/Firefox"), Kind::Firefox));
        }
        Platform::MacOs => {
            let support = sub(&env.home, "Library/Application Support");
            for rel in [
                "Google/Chrome",
                "Google/Chrome Beta",
                "Google/Chrome Dev",
                "Google/Chrome Canary",
                "Microsoft Edge",
                "Microsoft Edge Beta",
                "Microsoft Edge Dev",
                "Microsoft Edge Canary",
                "BraveSoftware/Brave-Browser",
                "BraveSoftware/Brave-Browser-Beta",
                "BraveSoftware/Brave-Browser-Nightly",
                "Vivaldi",
                "Chromium",
                "Yandex/YandexBrowser",
                "Arc/User Data",
                "com.operasoftware.Opera",
                "com.operasoftware.OperaGX",
            ] {
                out.push((sub(&support, rel), Kind::Chromium));
            }
            out.push((sub(&support, "Firefox"), Kind::Firefox));
        }
    }
    out
}

/// Whether a name can be left readable: only letters, digits, spaces, `.`, `_` and `-`, so its complement's
/// patterns need no escaping and every class's ranges stay plain. A folder on the way to a profile with any other
/// name isn't left readable, so the profile in it is hidden whole, caches included: more hidden, never less.
pub fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
}

/// One node of the tree of allowed names, by lower-cased character.
#[derive(Default)]
struct Trie {
    children: Vec<(char, Trie)>,
    end: bool,
}

impl Trie {
    fn insert(&mut self, name: &str) {
        let mut at = self;
        for c in name.chars().map(|c| c.to_ascii_lowercase()) {
            let i = match at.children.iter().position(|(k, _)| *k == c) {
                Some(i) => i,
                None => {
                    at.children.push((c, Trie::default()));
                    at.children.len() - 1
                }
            };
            at = &mut at.children[i].1;
        }
        at.end = true;
    }
}

/// A character class matching any one character but `/` and those in `except` (each in both cases): ranges of
/// plain characters from U+0001 to U+FFFF around them. (The matcher's regular expression works in UTF-16 units and
/// compares case-insensitively, so a letter must be left out in both cases.)
fn class_without(except: &[char]) -> String {
    let left_out = |c: char| {
        c == '/'
            || except
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&c) && c.is_ascii())
    };
    let mut out = String::from("[");
    let mut start: Option<u32> = None;
    let push = |out: &mut String, from: u32, to: u32| {
        let ch = |u: u32| char::from_u32(u).expect("a range endpoint is a character");
        out.push(ch(from));
        if to > from {
            out.push('-');
            out.push(ch(to));
        }
    };
    for u in 1u32..0x80 {
        let c = char::from_u32(u).expect("ASCII");
        match (left_out(c), start) {
            (false, None) => start = Some(u),
            (true, Some(s)) => {
                push(&mut out, s, u - 1);
                start = None;
            }
            _ => {}
        }
    }
    push(&mut out, start.unwrap_or(0x80), 0xFFFF);
    out.push(']');
    out
}

/// Patterns for one folder's children that match every name except `allow`'s (compared case-insensitively, as the
/// matcher compares), one name each, never crossing a `/`. Each name in `allow` must be [`plain`]. From the tree of
/// the allowed names, for each node `p` with next characters `S`: `p[^S]*` (a name going on with a character not
/// in `S`), `p` itself when no allowed name ends there, and at a leaf `p?*` (an allowed name and more). The empty
/// list gives `?*`: every name.
pub fn complement(allow: &[&str]) -> Vec<String> {
    let mut trie = Trie::default();
    for name in allow {
        assert!(plain(name), "{name:?} is a plain name");
        trie.insert(name);
    }
    let mut out = Vec::new();
    walk(&trie, &mut String::new(), &mut out);
    out
}

fn walk(node: &Trie, prefix: &mut String, out: &mut Vec<String>) {
    if node.children.is_empty() {
        out.push(format!("{prefix}?*"));
        return;
    }
    let next: Vec<char> = node.children.iter().map(|(c, _)| *c).collect();
    out.push(format!("{prefix}{}*", class_without(&next)));
    if !node.end && !prefix.is_empty() {
        out.push(rule_literal(prefix).expect("plain characters are spelled exactly"));
    }
    for (c, child) in &node.children {
        prefix.push(*c);
        walk(child, prefix, out);
        prefix.pop();
    }
}

/// The deny rules that hide every child of the folder `dir` (in `style`) except those named in `allow`, which must
/// be [`plain`]. The folder is spelled as a deny rule spells a path ([`cover_path_in`]: more, never less), so a
/// folder with characters no rule can spell gets its siblings' children hidden too. `None` when `dir` isn't
/// absolute in `style`.
pub fn profile_rules_in(dir: &str, allow: &[&str], style: Style) -> Option<Vec<String>> {
    let at = cover_path_in(dir, style)?;
    Some(
        complement(allow)
            .into_iter()
            .flat_map(|p| [format!("Read(/{at}/{p})"), format!("Read(/{at}/{p}/**)")])
            .collect(),
    )
}

/// The deny rules that hide every child but the caches in each folder of the Chromium root `root` (in `style`)
/// whose name Chromium gives a profile (`Default`, `Profile *`, `Guest Profile`, `System Profile`): one set of
/// rules for all of a root's profiles, those made after the scan included, in place of one set each. A child of
/// another kind whose name matches (`Profile Backup`) is hidden the same way: more, never less. `None` when `root`
/// isn't absolute in `style`.
pub fn profile_names_rules_in(root: &str, style: Style) -> Option<Vec<String>> {
    let at = cover_path_in(root, style)?;
    let patterns = complement(CHROMIUM_CACHES);
    let mut out = Vec::new();
    for name in CHROMIUM_PROFILE_NAMES.iter().map(|n| rule_cover(n)) {
        for p in &patterns {
            out.push(format!("Read(/{at}/{name}/{p})"));
            out.push(format!("Read(/{at}/{name}/{p}/**)"));
        }
    }
    Some(out)
}

/// The deny rules that hide the folder `dir` (in `style`) whole, or `None` when it isn't absolute in `style`.
pub fn whole_rules_in(dir: &str, style: Style) -> Option<Vec<String>> {
    let at = cover_path_in(dir, style)?;
    Some(vec![format!("Read(/{at})"), format!("Read(/{at}/**)")])
}

/// A folder whose children are hidden except `allow`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walled {
    pub path: PathBuf,
    pub allow: Vec<String>,
    key: Vec<String>,
}

/// The browser profiles and roots of one scan, hidden whole except their caches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Browsers {
    walled: Vec<Walled>,
    whole: Vec<(PathBuf, Vec<String>)>,
    /// The Chromium roots found: the rules for their Chromium-named profiles are one set per root
    /// ([`profile_names_rules_in`]).
    roots: Vec<(PathBuf, Vec<String>)>,
}

/// A path's components with their lower-cased key ([`keyed`]), in step: the drive (if any), then each name.
fn named(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().into_owned()),
            Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_owned()),
            Component::RootDir | Component::CurDir => None,
        })
        .collect()
}

fn starts(path: &[String], prefix: &[String]) -> bool {
    !prefix.is_empty() && path.len() >= prefix.len() && path[..prefix.len()] == prefix[..]
}

fn is_profile_name(name: &str) -> bool {
    CHROMIUM_PROFILE_NAMES
        .iter()
        .any(|p| super::hidden::glob(p, name))
}

impl Browsers {
    /// The browser profiles and roots of `snap`, with `env`'s default roots.
    pub fn of(snap: &Snapshot, env: &Env) -> Self {
        let mut walls = Walls::default();
        let mut roots: Vec<(PathBuf, Vec<String>)> = Vec::new();
        let mut root = |path: PathBuf| {
            let key = keyed(&path);
            if !roots.iter().any(|(_, k)| *k == key) {
                roots.push((path, key));
            }
        };
        let name_is = |d: DirId, name: &str| {
            snap.name(d)
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        };
        let profiles_in = |walls: &mut Walls, root: DirId| {
            for c in snap.children(root) {
                if snap.name(c).to_str().is_some_and(is_profile_name) {
                    walls.add(snap.path(c), CHROMIUM_CACHES);
                }
            }
        };
        for d in snap.dirs() {
            let m = snap.markers(d);
            let chromium = m.contains(Markers::PREFERENCES)
                && (m.contains(Markers::CHROMIUM_STORE)
                    || snap.children(d).any(|c| name_is(c, "Network")));
            let firefox = m.contains(Markers::FIREFOX_PREFS) && m.contains(Markers::FIREFOX_STORE);
            if chromium {
                walls.add(snap.path(d), CHROMIUM_CACHES);
                if let Some(p) = snap.parent(d)
                    && snap.markers(p).contains(Markers::LOCAL_STATE)
                {
                    walls.add(snap.path(p), CHROMIUM_CACHES);
                    root(snap.path(p));
                    profiles_in(&mut walls, p);
                }
            }
            if firefox {
                walls.add(snap.path(d), FIREFOX_CACHES);
            }
        }
        let mut whole = Vec::new();
        for (dir, kind) in known_roots(env) {
            match snap.find(&dir) {
                Some(d) => match kind {
                    Kind::Chromium => {
                        walls.add(dir.clone(), CHROMIUM_CACHES);
                        root(dir);
                        profiles_in(&mut walls, d);
                    }
                    Kind::Firefox => walls.add(dir, &[]),
                },
                None => {
                    let key = keyed(&dir);
                    whole.push((dir, key));
                }
            }
        }
        Self {
            walled: walls.through(),
            whole,
            roots,
        }
    }

    /// The folders whose children are hidden except their caches, and the default roots hidden whole.
    pub fn walled(&self) -> &[Walled] {
        &self.walled
    }

    pub fn whole(&self) -> impl Iterator<Item = &Path> {
        self.whole.iter().map(|(p, _)| p.as_path())
    }

    /// Whether `path` is hidden: in a default root the scan didn't see, or below a profile or root in a child that
    /// isn't one of its caches (or the way down to a profile in it). A profile or root itself isn't hidden: its name
    /// says nothing.
    pub fn hides(&self, path: &Path) -> bool {
        let key = keyed(path);
        if self.whole.iter().any(|(_, w)| starts(&key, w)) {
            return true;
        }
        let names = named(path);
        self.walled.iter().any(|w| {
            starts(&key, &w.key)
                && key.len() > w.key.len()
                && names.len() == key.len()
                && !w
                    .allow
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&names[w.key.len()]))
        })
    }

    /// Whether the walled folder `w`'s rules are its root's shared ones: it is a child of a Chromium root found, with
    /// a name Chromium gives a profile, and keeps exactly the caches readable.
    fn shared(&self, w: &Walled) -> bool {
        let names = named(&w.path);
        let Some(last) = names.last() else {
            return false;
        };
        names.len() == w.key.len()
            && is_profile_name(last)
            && self
                .roots
                .iter()
                .any(|(_, k)| k.len() + 1 == w.key.len() && starts(&w.key, k))
            && w.allow.len() == CHROMIUM_CACHES.len()
            && w.allow
                .iter()
                .all(|a| CHROMIUM_CACHES.iter().any(|c| c.eq_ignore_ascii_case(a)))
    }

    /// Whether `path` is, or holds, a browser profile or root: deleting it would delete what is hidden in it.
    pub fn holds(&self, path: &Path) -> bool {
        let key = keyed(path);
        self.whole
            .iter()
            .map(|(_, k)| k)
            .chain(self.walled.iter().map(|w| &w.key))
            .any(|k| starts(k, &key))
    }

    /// Claude Code's deny rules for them, on this system. A path a rule can't start from (a share) gets none: the
    /// by-name rules still hold there. A Chromium root's profiles named as Chromium names them share one set of
    /// rules ([`profile_names_rules_in`]), so a root with many profiles costs about as much as one; a profile that
    /// keeps more readable than its caches (the way down to another profile inside it) keeps its own.
    pub fn deny_rules(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (root, _) in &self.roots {
            out.extend(
                profile_names_rules_in(&root.to_string_lossy(), Style::HOST).unwrap_or_default(),
            );
        }
        for w in self.walled.iter().filter(|w| !self.shared(w)) {
            let allow: Vec<&str> = w.allow.iter().map(String::as_str).collect();
            out.extend(
                profile_rules_in(&w.path.to_string_lossy(), &allow, Style::HOST)
                    .unwrap_or_default(),
            );
        }
        for (root, _) in &self.whole {
            out.extend(whole_rules_in(&root.to_string_lossy(), Style::HOST).unwrap_or_default());
        }
        out
    }
}

/// The folders found, before the way down to each is opened.
#[derive(Default)]
struct Walls {
    found: Vec<Walled>,
}

impl Walls {
    fn add(&mut self, path: PathBuf, allow: &[&str]) {
        let key = keyed(&path);
        let at = match self.found.iter().position(|w| w.key == key) {
            Some(i) => i,
            None => {
                self.found.push(Walled {
                    path,
                    allow: Vec::new(),
                    key,
                });
                self.found.len() - 1
            }
        };
        let w = &mut self.found[at];
        for a in allow {
            if !w.allow.iter().any(|x| x.eq_ignore_ascii_case(a)) {
                w.allow.push((*a).to_owned());
            }
        }
    }

    /// Every folder found, with the way down from each to the ones inside it left readable: the first name below
    /// the nearest one above, and each folder between, which in turn hides all but the next name. Not when the way
    /// down starts in one of the caches (a profile-like folder inside a cache is walled where it is), or has a name
    /// that isn't [`plain`] (then the folder inside is hidden whole).
    fn through(self) -> Vec<Walled> {
        let found = self.found;
        let mut out = Walls {
            found: found.clone(),
        };
        for inner in &found {
            let Some(outer) = found
                .iter()
                .filter(|o| o.key.len() < inner.key.len() && starts(&inner.key, &o.key))
                .max_by_key(|o| o.key.len())
            else {
                continue;
            };
            let names = named(&inner.path);
            if names.len() != inner.key.len() {
                continue;
            }
            let way = &names[outer.key.len()..];
            let first = &way[0];
            if outer.allow.iter().any(|a| a.eq_ignore_ascii_case(first))
                || !way.iter().all(|n| plain(n))
            {
                continue;
            }
            out.add(outer.path.clone(), &[first.as_str()]);
            let mut at = outer.path.clone();
            for pair in way.windows(2) {
                at.push(&pair[0]);
                out.add(at.clone(), &[pair[1].as_str()]);
            }
        }
        out.found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model of what the complement's patterns match, for one name: as the matcher reads them (literal characters
    /// case-insensitively, `?` one character but `/`, `*` any run but `/`, a class one of its ranges).
    fn matches(pattern: &str, name: &str) -> bool {
        fn parse(p: &str) -> Vec<Tok> {
            let mut out = Vec::new();
            let mut chars = p.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '?' => out.push(Tok::One),
                    '*' => out.push(Tok::Any),
                    '\\' => out.push(Tok::Lit(chars.next().unwrap())),
                    '[' => {
                        let mut ranges = Vec::new();
                        while let Some(c) = chars.next() {
                            if c == ']' {
                                break;
                            }
                            if chars.peek() == Some(&'-') {
                                chars.next();
                                ranges.push((c, chars.next().unwrap()));
                            } else {
                                ranges.push((c, c));
                            }
                        }
                        out.push(Tok::Class(ranges));
                    }
                    c => out.push(Tok::Lit(c)),
                }
            }
            out
        }
        #[derive(Clone)]
        enum Tok {
            Lit(char),
            One,
            Any,
            Class(Vec<(char, char)>),
        }
        // The matcher's regular expressions work in UTF-16 units: a character beyond U+FFFF is two.
        const SLASH: u16 = b'/' as u16;
        fn go(t: &[Tok], n: &[u16]) -> bool {
            match t.split_first() {
                None => n.is_empty(),
                Some((Tok::Any, rest)) => {
                    (0..=n.len()).any(|i| !n[..i].contains(&SLASH) && go(rest, &n[i..]))
                }
                Some((tok, rest)) => {
                    let Some((&u, more)) = n.split_first() else {
                        return false;
                    };
                    let ascii = (u < 0x80).then_some(u as u8 as char);
                    let ok = match tok {
                        Tok::Lit(l) => ascii.is_some_and(|c| l.eq_ignore_ascii_case(&c)),
                        Tok::One => u != SLASH,
                        Tok::Class(r) => {
                            let fold: Vec<u32> = match ascii {
                                Some(c) => vec![
                                    c as u32,
                                    c.to_ascii_lowercase() as u32,
                                    c.to_ascii_uppercase() as u32,
                                ],
                                None => vec![u as u32],
                            };
                            fold.iter()
                                .any(|f| r.iter().any(|(a, b)| *a as u32 <= *f && *f <= *b as u32))
                        }
                        Tok::Any => unreachable!(),
                    };
                    ok && go(rest, more)
                }
            }
        }
        let n: Vec<u16> = name.encode_utf16().collect();
        go(&parse(pattern), &n)
    }

    #[test]
    fn the_complement_matches_every_name_but_the_allowed_ones() {
        let firefox: Vec<&str> = FIREFOX_CACHES.to_vec();
        let pass = [
            "Profile 1",
            "x.default-release",
            "Opera_GX Stable",
            "Zz-9",
            "Default",
        ];
        for allow in [CHROMIUM_CACHES.to_vec(), firefox, pass.to_vec(), vec![]] {
            let patterns = complement(&allow);
            let denied = |name: &str| patterns.iter().any(|p| matches(p, name));
            for name in &allow {
                assert!(!denied(name), "{name} stays readable");
                assert!(!denied(&name.to_uppercase()), "{name} in other case");
                assert!(!denied(&name.to_lowercase()));
            }
            let mut near = vec![
                "x",
                "C",
                "Cach",
                "Caches",
                "Cache2",
                "Cached",
                "XCache",
                "Code",
                "Code  Cache",
                "GPUCache ",
                "\u{ff23}ache",
                "\u{212a}ache",
                "Sessions",
                "Preferences",
                "History",
                "Local State",
                "Service Worker",
                "a\nb",
                "\u{1}",
                "x\u{1f600}",
                "[x]",
                "Cache]",
                "Cach\\e",
                "Cach^",
                "Cach{",
                "Profile 2",
                "Profile",
                "Defaults",
                "cache2x",
            ];
            for name in &allow {
                for i in 0..name.len() {
                    near.push(&name[..i]);
                }
            }
            let extended: Vec<String> = allow
                .iter()
                .flat_map(|n| {
                    [
                        format!("{n}x"),
                        format!("x{n}"),
                        format!("{n} "),
                        format!("{n}\u{1}"),
                    ]
                })
                .collect();
            for name in near
                .iter()
                .copied()
                .chain(extended.iter().map(String::as_str))
            {
                if name.is_empty() || allow.iter().any(|a| a.eq_ignore_ascii_case(name)) {
                    continue;
                }
                assert!(denied(name), "{name:?} is hidden ({allow:?})");
            }
            // A class never takes in a `/`: a cache's own folders stay readable.
            for p in &patterns {
                for name in &allow {
                    assert!(!matches(p, &format!("{name}/x")), "{p} {name}/x");
                    assert!(!matches(p, &format!("{name}/Data")));
                }
            }
        }
        assert_eq!(complement(&[]), ["?*"]);
    }

    #[test]
    fn classes_leave_out_both_cases_and_the_slash() {
        let c = class_without(&['c', 's']);
        assert_eq!(c, "[\u{1}-.0-BD-RT-bd-rt-\u{ffff}]");
        assert!(
            plain("Profile 1") && plain("x.default-release") && !plain("[work]") && !plain("zoë")
        );
        assert!(!plain(""));
    }
}

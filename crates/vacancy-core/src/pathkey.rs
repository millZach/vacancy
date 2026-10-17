//! Paths as keys, for deciding whether two paths name the same place or one is inside another. Every decision that
//! depends on where something is (the Guard, the rules, the categorizer's known places) compares keys, never raw
//! strings: `C:\Users\me\AppDataX` is not inside `C:\Users\me\AppData`, because keys compare whole names.
//!
//! A key folds away the ways one place can be spelled:
//!
//! - **Case, where the volume ignores it.** Every key has two forms. The *folded* form lowercases names and is what
//!   deny entries are compared with: on a case-sensitive volume a deny entry then covers every spelling, which only
//!   ever blocks more. The *exact* form keeps case and is what allowed roots and known places are compared with on
//!   a case-sensitive volume ([`Case::Sensitive`]), so `/Users/ALEX/project` is not inside an allowed `/Users/alex`
//!   there. NTFS and default APFS volumes are [`Case::Insensitive`] and use the folded form throughout.
//! - **Unicode.** Names are compared in NFC, so a precomposed `é` and `e` followed by a combining accent are one
//!   name, as they are to APFS and HFS+.
//! - **Prefixes.** `\\?\C:\x`, `\\.\C:\x` and `C:\x` are the same place, and so are `\\?\UNC\srv\share` and
//!   `\\srv\share`.
//! - **Separators.** Trailing separators and `.` names are dropped.
//! - **Windows' quirks.** Win32 strips trailing dots and spaces from names (`OneDrive.` opens `OneDrive`), so keys do
//!   too, and 8.3 short names (`C:\Users\RUNNER~1`) are expanded to long ones through the file system.
//!
//! A path with `..`, a relative path, or (on Windows) a name holding a `:` (an alternate data stream) has no key:
//! callers treat it as a place they can't vouch for.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Component, Path};

use unicode_normalization::UnicodeNormalization;

/// How a volume compares names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Case {
    /// `A` and `a` name the same thing: NTFS, default APFS, HFS+.
    #[default]
    Insensitive,
    /// They name different things: case-sensitive APFS.
    Sensitive,
}

/// A path as a list of keys: where it starts (`c:`, `\\srv\share` or nothing on Unix), `/` for the root, then each
/// name, in its exact and its folded form. See the module docs. Two keys are equal when their folded forms are:
/// they may name the same place.
#[derive(Clone, Debug)]
pub(crate) struct PathKey {
    exact: Vec<String>,
    folded: Vec<String>,
}

impl PartialEq for PathKey {
    fn eq(&self, other: &Self) -> bool {
        self.folded == other.folded
    }
}

impl Eq for PathKey {}

/// The key that stands for the root folder after a drive or share.
const ROOT: &str = "/";

impl PathKey {
    /// The key for `path`, or `None` when the path doesn't pin down one place (see the module docs).
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let path = long_form(path);
        let mut exact = Vec::new();
        let mut folded = Vec::new();
        let mut rooted = false;
        for part in path.components() {
            match part {
                Component::Prefix(prefix) => {
                    // Drive letters and server names never depend on case.
                    let key = prefix_key(prefix.kind())?;
                    exact.push(key.clone());
                    folded.push(key);
                }
                Component::RootDir => {
                    exact.push(ROOT.to_owned());
                    folded.push(ROOT.to_owned());
                    rooted = true;
                }
                Component::CurDir => {}
                Component::ParentDir => return None,
                Component::Normal(name) => {
                    if !rooted {
                        return None; // relative, or relative to a drive's current folder (`C:x`)
                    }
                    exact.push(name_key_checked(name, Case::Sensitive)?);
                    folded.push(name_key_checked(name, Case::Insensitive)?);
                }
            }
        }
        rooted.then_some(Self { exact, folded })
    }

    /// The keys as a volume that compares names this way sees them, root first.
    pub(crate) fn keys_in(&self, case: Case) -> &[String] {
        match case {
            Case::Insensitive => &self.folded,
            Case::Sensitive => &self.exact,
        }
    }

    /// The folded keys, root first.
    pub(crate) fn keys(&self) -> &[String] {
        &self.folded
    }

    /// Whether this is `base` or a place inside it, comparing names as a volume with `case` does.
    pub(crate) fn starts_with_in(&self, base: &PathKey, case: Case) -> bool {
        let (me, base) = (self.keys_in(case), base.keys_in(case));
        me.len() >= base.len() && me[..base.len()] == base[..]
    }

    /// Whether this is a place strictly inside `base`, comparing names as a volume with `case` does.
    pub(crate) fn is_inside_in(&self, base: &PathKey, case: Case) -> bool {
        self.folded.len() > base.folded.len() && self.starts_with_in(base, case)
    }

    /// Whether this is, or may be, `base` or a place inside it: folded names, the comparison deny entries use.
    pub(crate) fn starts_with(&self, base: &PathKey) -> bool {
        self.starts_with_in(base, Case::Insensitive)
    }

    /// Whether this is, or may be, a place strictly inside `base` (folded names).
    pub(crate) fn is_inside(&self, base: &PathKey) -> bool {
        self.is_inside_in(base, Case::Insensitive)
    }

    /// The key of a place directly inside this one, whose name has the key `name` in both forms.
    pub(crate) fn child(&self, name: &str) -> Self {
        let mut exact = self.exact.clone();
        let mut folded = self.folded.clone();
        exact.push(name.to_owned());
        folded.push(name.to_owned());
        Self { exact, folded }
    }

    /// The folded names after the root, last first.
    pub(crate) fn names_from_last(&self) -> impl Iterator<Item = &str> {
        self.folded
            .iter()
            .rev()
            .take_while(|k| k.as_str() != ROOT)
            .map(String::as_str)
    }

    /// Whether a name still looks like an 8.3 short name (`PROGRA~1`) that could not be expanded: on Windows such a
    /// path may name a place under another spelling. Always false elsewhere.
    pub(crate) fn has_short_alias(&self) -> bool {
        cfg!(windows) && self.names_from_last().any(looks_short)
    }
}

/// A file or folder name as a folded key: lowercased, and on Windows without the trailing dots and spaces Win32
/// ignores.
pub(crate) fn name_key(name: &OsStr) -> String {
    name_key_in(name, Case::Insensitive)
}

/// A file or folder name as a key for a volume that compares names this way. Both forms are in Unicode NFC: an `é`
/// written as one code point (as `info.json` files and most APIs write it) and as `e` plus a combining accent (as
/// some macOS listings return it) are one name. APFS and HFS+ treat them so; on NTFS they could be two names, but
/// there only the folded form is used, and for deny entries a shared key only ever blocks more.
pub(crate) fn name_key_in(name: &OsStr, case: Case) -> String {
    let text: String = name.to_string_lossy().nfc().collect();
    let key = match case {
        Case::Insensitive => text.to_lowercase(),
        Case::Sensitive => text,
    };
    if cfg!(windows) {
        key.trim_end_matches(['.', ' ']).to_owned()
    } else {
        key
    }
}

/// [`name_key`], or `None` for a name no place has: empty once folded, or on Windows one with a stream (`:`) or a
/// separator in it.
fn name_key_checked(name: &OsStr, case: Case) -> Option<String> {
    let key = name_key_in(name, case);
    if key.is_empty() || (cfg!(windows) && key.contains([':', '/', '\\'])) {
        return None;
    }
    Some(key)
}

#[cfg(windows)]
fn prefix_key(prefix: std::path::Prefix<'_>) -> Option<String> {
    use std::path::Prefix;
    let lower = |s: &OsStr| s.to_string_lossy().to_lowercase();
    Some(match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            format!("{}:", char::from(letter).to_ascii_lowercase())
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            format!(r"\\{}\{}", lower(server), lower(share))
        }
        Prefix::DeviceNS(device) => {
            let d = lower(device);
            // `\\.\C:` is the drive C:.
            match d.as_bytes() {
                [letter, b':'] if letter.is_ascii_alphabetic() => d,
                _ => format!(r"\\.\{d}"),
            }
        }
        Prefix::Verbatim(rest) => format!(r"\\?\{}", lower(rest)),
    })
}

#[cfg(not(windows))]
fn prefix_key(_prefix: std::path::Prefix<'_>) -> Option<String> {
    None // Unix paths have no prefixes
}

/// `PROGRA~1`, `RUNNER~1`: a tilde followed by a digit, in a name short enough to be an 8.3 alias.
fn looks_short(name: &str) -> bool {
    name.len() <= 12
        && name
            .find('~')
            .is_some_and(|at| name[at + 1..].starts_with(|c: char| c.is_ascii_digit()))
}

/// `path` with 8.3 short names expanded to long ones, as far as the file system can say (the longest part of the
/// path that exists). Unchanged when no name has a `~`, and everywhere but Windows.
pub(crate) fn long_form(path: &Path) -> Cow<'_, Path> {
    #[cfg(windows)]
    {
        if !path.as_os_str().as_encoded_bytes().contains(&b'~') {
            return Cow::Borrowed(path);
        }
        let mut head = path.to_path_buf();
        let mut tail = Vec::new();
        loop {
            if let Some(mut long) = win::long_path(&head) {
                for name in tail.iter().rev() {
                    long.push(name);
                }
                return Cow::Owned(long);
            }
            match head.file_name() {
                Some(name) if head.parent().is_some() => {
                    tail.push(name.to_owned());
                    head.pop();
                }
                _ => return Cow::Borrowed(path),
            }
        }
    }
    #[cfg(not(windows))]
    Cow::Borrowed(path)
}

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    /// The long form of an existing path, from the file system.
    pub(super) fn long_path(path: &Path) -> Option<PathBuf> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut buf = vec![0u16; wide.len() + 64];
        loop {
            let cap = u32::try_from(buf.len()).ok()?;
            // SAFETY: `wide` is NUL-terminated; `buf` holds `cap` writable units.
            let n = unsafe { GetLongPathNameW(wide.as_ptr(), buf.as_mut_ptr(), cap) } as usize;
            if n == 0 {
                return None; // the path doesn't exist, or a folder on the way can't be read
            }
            if n < buf.len() {
                return Some(PathBuf::from(OsString::from_wide(&buf[..n])));
            }
            buf.resize(n + 1, 0); // too small: `n` is the size needed, with the NUL
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn key(p: &str) -> Option<PathKey> {
        PathKey::of(Path::new(p))
    }

    /// A path in the running OS's own syntax, from `/`-separated parts under the OS's root.
    fn native(parts: &str) -> PathBuf {
        let root = if cfg!(windows) { r"C:\" } else { "/" };
        parts
            .split('/')
            .fold(PathBuf::from(root), |p, part| p.join(part))
    }

    #[test]
    fn compares_whole_names_never_string_prefixes() {
        let app_data = PathKey::of(&native("Users/me/AppData")).unwrap();
        let x = PathKey::of(&native("Users/me/AppDataX/Local")).unwrap();
        let inside = PathKey::of(&native("Users/me/AppData/Local")).unwrap();
        assert!(!x.starts_with(&app_data), "AppDataX is not inside AppData");
        assert!(inside.starts_with(&app_data));
        assert!(inside.is_inside(&app_data));
        assert!(!app_data.is_inside(&app_data));
    }

    #[test]
    fn folds_case_and_trailing_separators() {
        let a = PathKey::of(&native("Users/Me/OneDrive")).unwrap();
        let b = PathKey::of(&PathBuf::from(format!(
            "{}{}",
            native("users/ME/onedrive").display(),
            std::path::MAIN_SEPARATOR
        )))
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn has_no_key_for_paths_that_dont_pin_down_a_place() {
        assert!(key("relative/path").is_none());
        assert!(PathKey::of(&native("Users/me/../other")).is_none(), "..");
        assert!(PathKey::of(&native("Users/./me")).is_some(), ". is dropped");
        assert_eq!(
            PathKey::of(&native("Users/./me")),
            PathKey::of(&native("Users/me"))
        );
    }

    #[test]
    fn spots_unexpanded_short_names() {
        assert!(looks_short("PROGRA~1"));
        assert!(looks_short("runner~1"));
        assert!(!looks_short("~$report.docx"));
        assert!(!looks_short("backup~old"));
        assert!(!looks_short("a-very-long-name~1"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_spellings_of_one_place_share_a_key() {
        let plain = key(r"C:\Users\me\OneDrive").unwrap();
        for other in [
            r"\\?\C:\Users\me\OneDrive",
            r"\\.\C:\Users\me\OneDrive",
            r"c:/users/me/onedrive/",
            r"C:\Users\me\OneDrive.",
            r"C:\Users\me\OneDrive. .",
            r"C:\Users\me\OneDrive ",
        ] {
            assert_eq!(key(other).as_ref(), Some(&plain), "{other}");
        }
        assert_eq!(
            key(r"\\?\UNC\Server\Share\x"),
            key(r"\\server\share\x"),
            "verbatim and plain UNC"
        );
        assert!(
            key(r"C:relative").is_none(),
            "relative to C:'s current folder"
        );
        assert!(key(r"C:\Users\me\file.txt:secret").is_none(), "a stream");
        assert!(key(r"C:\Users\me\OneDrive::$INDEX_ALLOCATION").is_none());
        assert!(
            key(r"\\?\C:\Users\me\a/b").is_none(),
            "a slash inside a verbatim name"
        );
    }

    #[cfg(windows)]
    #[test]
    fn expands_short_names_through_the_file_system() {
        let fx = tempfile::tempdir().unwrap();
        let long = fx.path().join("A folder with a long name");
        std::fs::create_dir(&long).unwrap();
        let short = short_path(&long);
        let inside = short.join("not there yet").join("x");
        assert_eq!(
            PathKey::of(&inside),
            PathKey::of(&long.join("not there yet").join("x")),
            "{} is {}",
            inside.display(),
            long.display()
        );
        if short == long {
            eprintln!("8.3 names are off on this volume; nothing more to check");
        }
    }

    /// The 8.3 form of an existing path (the same path where 8.3 names are off).
    #[cfg(windows)]
    pub(crate) fn short_path(path: &Path) -> PathBuf {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut buf = vec![0u16; 1024];
        // SAFETY: `wide` is NUL-terminated; `buf` has 1024 writable units.
        let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), 1024) } as usize;
        assert!(n > 0 && n < buf.len(), "GetShortPathNameW");
        PathBuf::from(OsString::from_wide(&buf[..n]))
    }
}

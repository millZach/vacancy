//! Paths as keys, for deciding whether two paths may name the same place or one may be inside another, by their
//! spelling. Keys compare whole names, never raw strings: `C:\Users\me\AppDataX` is not inside
//! `C:\Users\me\AppData`.
//!
//! A key never decides that a place may be deleted: names that fold to one key can still be two places (NTFS keeps
//! `İpek` and `i` + U+0307 + `pek` apart; a case-sensitive folder keeps `alex` and `Alex` apart). The Guard
//! authorizes by file-system identity (see [`crate::fsid`]), and uses keys only to deny more; the rules use them
//! to find candidates, confirmed by identity where it matters, and the categorizer to label.
//!
//! A key folds away the ways one place can be spelled, as widely as any volume does:
//!
//! - **Case and Unicode form.** Names are lowercased and put in NFC, so `OneDrive` and `onedrive`, and a
//!   precomposed `é` and `e` followed by a combining accent, give one key.
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

/// A path as a list of keys: where it starts (`c:`, `\\srv\share` or nothing on Unix), `/` for the root, then each
/// name, folded. See the module docs. Two equal keys may name the same place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PathKey {
    folded: Vec<String>,
}

/// The key that stands for the root folder after a drive or share.
const ROOT: &str = "/";

impl PathKey {
    /// The key for `path`, or `None` when the path doesn't pin down one place (see the module docs).
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let path = long_form(path);
        let mut folded = Vec::new();
        let mut rooted = false;
        for part in path.components() {
            match part {
                Component::Prefix(prefix) => {
                    // Drive letters and server names never depend on case.
                    folded.push(prefix_key(prefix.kind())?);
                }
                Component::RootDir => {
                    folded.push(ROOT.to_owned());
                    rooted = true;
                }
                Component::CurDir => {}
                Component::ParentDir => return None,
                Component::Normal(name) => {
                    if !rooted {
                        return None; // relative, or relative to a drive's current folder (`C:x`)
                    }
                    folded.push(name_key_checked(name)?);
                }
            }
        }
        rooted.then_some(Self { folded })
    }

    /// The keys, root first.
    pub(crate) fn keys(&self) -> &[String] {
        &self.folded
    }

    /// Whether this is, or may be, `base` or a place inside it.
    pub(crate) fn starts_with(&self, base: &PathKey) -> bool {
        let (me, base) = (&self.folded, &base.folded);
        me.len() >= base.len() && me[..base.len()] == base[..]
    }

    /// Whether this is, or may be, a place strictly inside `base`.
    pub(crate) fn is_inside(&self, base: &PathKey) -> bool {
        self.folded.len() > base.folded.len() && self.starts_with(base)
    }

    /// The key of a place directly inside this one, whose name has the key `name`.
    pub(crate) fn child(&self, name: &str) -> Self {
        let mut folded = self.folded.clone();
        folded.push(name.to_owned());
        Self { folded }
    }

    /// The names after the root, last first.
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

/// A file or folder name as a key: in NFC, lowercased, and on Windows without the trailing dots and spaces Win32
/// ignores.
pub(crate) fn name_key(name: &OsStr) -> String {
    let text = name.to_string_lossy();
    let text = if cfg!(windows) {
        text.trim_end_matches(['.', ' '])
    } else {
        &text
    };
    text.nfc().collect::<String>().to_lowercase()
}

/// [`name_key`], or `None` for a name no place has: empty once folded, or on Windows one with a stream (`:`) or a
/// separator in it.
fn name_key_checked(name: &OsStr) -> Option<String> {
    let key = name_key(name);
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
            if let Some(long) = win::long_path(&head) {
                // The file system answers with every name as it is spelled on disk; only the short names are
                // replaced, so the rest keep the spelling they were given (it matters on a case-sensitive volume).
                let mut long = keep_spelling(&head, &long);
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

/// `long` with the names of `original` that aren't short names put back as they were written. Both are the same
/// path; if their names don't line up, `long` as it is.
#[cfg(windows)]
fn keep_spelling(original: &Path, long: &Path) -> std::path::PathBuf {
    let given: Vec<Component<'_>> = original.components().collect();
    let found: Vec<Component<'_>> = long.components().collect();
    if given.len() != found.len() {
        return long.to_path_buf();
    }
    let mut out = std::path::PathBuf::new();
    for (g, f) in given.iter().zip(&found) {
        match g {
            Component::Normal(name) if !name.as_encoded_bytes().contains(&b'~') => out.push(name),
            _ => out.push(f.as_os_str()),
        }
    }
    out
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

    /// Expanding a short name changes only that name: the others keep the spelling they were given, which on a
    /// case-sensitive volume is a different place than the spelling on disk.
    #[cfg(windows)]
    #[test]
    fn expanding_short_names_keeps_the_other_names_as_written() {
        let fx = tempfile::tempdir().unwrap();
        let long = fx.path().join("A folder with a long name");
        std::fs::create_dir_all(long.join("Mixed")).unwrap();
        let short = short_path(&long);
        if short == long {
            eprintln!("8.3 names are off on this volume; nothing to check");
            return;
        }
        let long_path = long_form(&short.join("mixed")).into_owned();
        let names: Vec<_> = long_path.iter().collect();
        assert_eq!(
            names[names.len() - 1],
            "mixed",
            "as written, not as on disk"
        );
        assert_eq!(
            names[names.len() - 2],
            "A folder with a long name",
            "the short name, expanded"
        );
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

//! Advice is never deletion authority or freed space. Steam ids and names come from bounded, no-link manifest
//! reads; URI text is constructed from a validated integer, never copied from a manifest or AI response.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::{Env, Platform, Snapshot};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const MAX_MANIFEST: u64 = 1024 * 1024;
const SIX_MONTHS: i64 = 183 * 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(export)]
pub enum HintAction {
    SteamUninstall { app_id: u32 },
    LogicInstructions,
}

impl HintAction {
    /// Only a known Steam id can produce an external URI. Logic is an in-app instruction.
    pub fn steam_uri(&self) -> Option<String> {
        match self {
            Self::SteamUninstall { app_id } => Some(format!("steam://uninstall/{app_id}")),
            Self::LogicInstructions => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub id: String,
    pub title: String,
    pub why: String,
    /// Installed size reported by Steam, not measured allocated bytes and never counted as freed.
    pub reported_bytes: Option<u64>,
    pub manifest: PathBuf,
    pub action: HintAction,
}

/// Discover installed games with an explicit last-played timestamp at least six months old. Unknown play
/// history (including zero), future dates, corrupt manifests, linked paths and unreadable libraries are omitted.
/// Only known Steam locations and libraries named by `libraryfolders.vdf` are inspected.
pub fn steam(env: &Env, snapshot: &Snapshot, now: i64) -> Vec<Hint> {
    let mut roots: Vec<PathBuf> = match env.platform {
        Platform::Windows => env.program_files.iter().map(|p| p.join("Steam")).collect(),
        Platform::MacOs => vec![env.home.join("Library/Application Support/Steam")],
    };
    // HKCU belongs to the initiating, unelevated account. Fixture callers cannot read the host registry,
    // including when a marker/environment variable is lost: the positively confined Env still blocks it.
    if env.platform == Platform::Windows
        && env.confined_to.is_none()
        && std::env::var_os(crate::fixture::ROOT_VAR).is_none()
        && let Some(root) = installed_steam()
    {
        roots.push(root);
    }
    steam_with_roots(env, snapshot, now, &roots)
}

/// Inject installed roots for recorded/platform tests. All reads still enforce fixture identity confinement;
/// libraries on an unscanned volume never generate a result for the current volume.
pub fn steam_with_roots(env: &Env, snapshot: &Snapshot, now: i64, roots: &[PathBuf]) -> Vec<Hint> {
    let roots = roots.to_vec();
    let mut libraries = roots.clone();
    for root in roots {
        if let Ok(text) = read_manifest(env, &root.join("steamapps/libraryfolders.vdf"))
            && let Some(Vdf::Object(library)) =
                parse(&text).and_then(|mut v| v.remove("libraryfolders"))
        {
            for entry in library.values() {
                if let Vdf::Object(values) = entry
                    && let Some(Vdf::Text(path)) = values.get("path")
                {
                    libraries.push(PathBuf::from(path));
                }
            }
        }
    }
    let libraries: Vec<_> = libraries
        .into_iter()
        .take(128)
        .filter_map(|p| {
            crate::fsid::chain(&p.join("steamapps")).and_then(|c| c.last().map(|s| s.id))
        })
        .collect();
    let mut found = BTreeMap::new();
    // Reuse the scanner's no-link listing. Never re-enumerate a path that could have become a junction.
    for dir in snapshot.dirs() {
        if !snapshot
            .dir_identity(dir)
            .is_some_and(|id| libraries.contains(&id))
        {
            continue;
        }
        for file in snapshot.files(dir) {
            let path = snapshot.file_path(file);
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(id) = name
                .strip_prefix("appmanifest_")
                .and_then(|s| s.strip_suffix(".acf"))
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|id| *id > 0)
            else {
                continue;
            };
            let Ok(text) = read_manifest(env, &path) else {
                continue;
            };
            if let Some(hint) = steam_manifest(&text, id, now, path) {
                found.entry(id).or_insert(hint);
            }
        }
    }
    found.into_values().collect()
}

#[cfg(windows)]
fn installed_steam() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let key: Vec<u16> = "Software\\Valve\\Steam\0".encode_utf16().collect();
    let name: Vec<u16> = "SteamPath\0".encode_utf16().collect();
    let mut buffer = vec![0u16; 16_384];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: fixed NUL-terminated key/value names; buffer and byte count are valid output storage.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 || bytes < 2 || bytes as usize > buffer.len() * 2 || !bytes.is_multiple_of(2) {
        return None;
    }
    let end = buffer.iter().position(|c| *c == 0)?;
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..end]));
    path.is_absolute().then_some(path)
}

#[cfg(not(windows))]
fn installed_steam() -> Option<PathBuf> {
    None
}

/// Parse one app manifest. This is also the recorded-output seam used by fixture tests.
pub fn steam_manifest(text: &str, expected_id: u32, now: i64, path: PathBuf) -> Option<Hint> {
    let Vdf::Object(state) = parse(text)?.remove("AppState")? else {
        return None;
    };
    let value = |name: &str| match state.get(name) {
        Some(Vdf::Text(s)) => Some(s.as_str()),
        _ => None,
    };
    let app_id = value("appid")?
        .parse::<u32>()
        .ok()
        .filter(|id| *id > 0 && *id == expected_id)?;
    let played = value("LastPlayed")?.parse::<i64>().ok()?;
    if played <= 0 || played > now || now.saturating_sub(played) < SIX_MONTHS {
        return None;
    }
    let name = value("name")?;
    if name.is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
        return None;
    }
    Some(Hint {
        id: format!("hint-steam-{app_id}"),
        title: format!("Review {name} in Steam"),
        why: "Steam's manifest says this game was last played at least six months ago. Open Steam's uninstall screen to review it; Vacancy will not uninstall it. Steam's reported installed size may differ from space recovered.".into(),
        reported_bytes: value("SizeOnDisk").and_then(|s| s.parse().ok()),
        manifest: path,
        action: HintAction::SteamUninstall { app_id },
    })
}

/// Recognize known library layouts, including Logic12's consolidated bundle. Allocated totals cannot tell how
/// much shared APFS clone data would become free, so this advice deliberately carries no reclaim estimate.
pub fn logic(env: &Env, snapshot: &Snapshot) -> Option<Hint> {
    if env.platform != Platform::MacOs {
        return None;
    }
    let paths = [
        env.home.join("Music/Logic Pro Library.bundle"),
        env.root.join("Library/Application Support/Logic"),
        env.root.join("Library/Audio/Apple Loops/Apple"),
    ];
    let path = paths.into_iter().find(|p| {
        readable_path(env, p) && snapshot.find(p).is_some_and(|d| snapshot.tree(d).bytes > 0)
    })?;
    Some(Hint {
        id: "hint-logic-library".into(),
        title: "Review the Logic sound library".into(),
        why: "In Logic Pro, open Sound Library and review your downloaded content (Manage Packs in Logic Pro 12; Sound Library Manager in earlier versions). Use the app's controls for content you no longer need, or relocate the library through Logic. Existing projects may need these sounds. Shared or cloned content means the library's size is not a promise of space recovered.".into(),
        reported_bytes: None,
        manifest: path,
        action: HintAction::LogicInstructions,
    })
}

impl Hint {
    /// A proposal displays advice without claiming that its reported installed size can be freed. No path is
    /// a delete target and the action never becomes a command supplied by the UI.
    pub fn proposal(&self) -> crate::Proposal {
        crate::Proposal {
            id: self.id.clone(),
            category: match self.action {
                HintAction::SteamUninstall { .. } => crate::CategoryId::Apps,
                HintAction::LogicInstructions => crate::CategoryId::Media,
            },
            title: self.title.clone(),
            summary: "Advice only · no space counted".into(),
            why: self.why.clone(),
            bytes: 0,
            item_count: 0,
            place_count: 0,
            risk: crate::Risk::Review,
            source: crate::ProposalSource::Rule,
            ai_note: None,
            action: crate::ProposalAction::Hint,
            hint_action: Some(self.action.clone()),
            regenerable: false,
            sample_paths: vec![self.manifest.display().to_string()],
            default_on: false,
        }
    }
}

#[derive(Debug)]
enum Vdf {
    Text(String),
    Object(BTreeMap<String, Vdf>),
}

fn parse(text: &str) -> Option<BTreeMap<String, Vdf>> {
    if text.len() as u64 > MAX_MANIFEST {
        return None;
    }
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            c if c.is_whitespace() => {}
            '{' | '}' => tokens.push(ch.to_string()),
            '"' => {
                let mut value = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            '\\' => value.push('\\'),
                            '"' => value.push('"'),
                            _ => return None,
                        },
                        c => value.push(c),
                    }
                }
                tokens.push(format!("\"{value}"));
            }
            '/' if chars.next()? == '/' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => return None,
        }
        if tokens.len() > 50_000 {
            return None;
        }
    }
    fn object(
        tokens: &[String],
        at: &mut usize,
        depth: usize,
        nested: bool,
    ) -> Option<BTreeMap<String, Vdf>> {
        if depth > 16 {
            return None;
        }
        let mut out = BTreeMap::new();
        while let Some(token) = tokens.get(*at) {
            if token == "}" {
                if !nested {
                    return None;
                }
                *at += 1;
                return Some(out);
            }
            let key = token.strip_prefix('"')?.to_owned();
            *at += 1;
            let value = tokens.get(*at)?;
            *at += 1;
            let value = if value == "{" {
                Vdf::Object(object(tokens, at, depth + 1, true)?)
            } else {
                Vdf::Text(value.strip_prefix('"')?.to_owned())
            };
            if out.insert(key, value).is_some() {
                return None;
            }
        }
        if nested { None } else { Some(out) }
    }
    object(&tokens, &mut 0, 0, false)
}

fn readable_path(env: &Env, path: &Path) -> bool {
    let Some(chain) = crate::fsid::chain(path) else {
        return false;
    };
    if let Some(root) = &env.confined_to {
        let Some(root) = crate::fsid::chain(root).and_then(|c| c.last().map(|s| s.id)) else {
            return false;
        };
        if !chain
            .iter()
            .take(chain.len().saturating_sub(1))
            .any(|s| s.id == root)
        {
            return false;
        }
    }
    true
}

fn read_manifest(env: &Env, path: &Path) -> io::Result<String> {
    if !readable_path(env, path) {
        return Err(io::Error::other("linked or unconfined manifest"));
    }
    let file = open_no_links(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_MANIFEST {
        return Err(io::Error::other("not a bounded manifest"));
    }
    let mut text = String::new();
    file.take(MAX_MANIFEST + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_MANIFEST {
        return Err(io::Error::other("manifest grew beyond limit"));
    }
    Ok(text)
}

#[cfg(windows)]
fn open_no_links(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    let mut pins = Vec::new();
    let mut current = PathBuf::new();
    let parts: Vec<_> = path.components().collect();
    if !path.is_absolute() {
        return Err(io::Error::other("not absolute"));
    }
    for (i, part) in parts.iter().enumerate() {
        match part {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                current.push(part.as_os_str())
            }
            _ => return Err(io::Error::other("invalid component")),
        }
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        // Parent handles deny rename/delete until the leaf is open. Every component is opened as itself.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x0020_0000 | 0x0200_0000)
            .open(&current)?;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("reparse point"));
        }
        if i + 1 == parts.len() {
            return Ok(file);
        }
        pins.push(file);
    }
    Err(io::Error::other("no manifest"))
}

#[cfg(unix)]
fn open_no_links(path: &Path) -> io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    if !path.is_absolute() {
        return Err(io::Error::other("not absolute"));
    }
    let mut dir = File::open("/")?;
    let parts: Vec<_> = path
        .components()
        .filter(|c| !matches!(c, Component::RootDir))
        .collect();
    for (i, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            return Err(io::Error::other("invalid component"));
        };
        let name = std::ffi::CString::new(name.as_bytes())?;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if i + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: `dir` owns a live directory fd; name is NUL terminated; flags take no extra argument.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a new owned descriptor.
        dir = unsafe { File::from_raw_fd(fd) };
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advice_uses_last_played_not_install_age_and_only_constructs_numeric_uris() {
        let manifest = r#""AppState" { "appid" "42" "name" "A game" "LastPlayed" "100000" "SizeOnDisk" "999999" }"#;
        let hint = steam_manifest(manifest, 42, 100000 + SIX_MONTHS, PathBuf::new()).unwrap();
        assert_eq!(
            hint.action.steam_uri().as_deref(),
            Some("steam://uninstall/42")
        );
        assert_eq!(hint.reported_bytes, Some(999999));
        assert!(steam_manifest(manifest, 43, i64::MAX, PathBuf::new()).is_none());
        assert!(
            steam_manifest(
                &manifest.replace("100000", "0"),
                42,
                i64::MAX,
                PathBuf::new()
            )
            .is_none()
        );
        assert!(steam_manifest(manifest, 42, 100001, PathBuf::new()).is_none());
    }
    #[test]
    fn malformed_duplicate_and_injected_ids_are_rejected() {
        for text in [
            r#""AppState" { "appid" "42" "appid" "43" }"#,
            r#""AppState" { "appid" "42;--all" }"#,
            r#""AppState" {"#,
        ] {
            assert!(steam_manifest(text, 42, i64::MAX, PathBuf::new()).is_none());
        }
    }
}

//! Advice is never deletion authority or freed space. Steam ids and names come from bounded, no-link manifest
//! reads; URI text is constructed from a validated integer, never copied from a manifest or AI response.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Env, Platform, Snapshot};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const MAX_MANIFEST: u64 = 1024 * 1024;
const SIX_MONTHS: i64 = 183 * 24 * 60 * 60;
const MAX_LIBRARIES: usize = 128;
const MAX_MANIFESTS: usize = 512;
const MAX_DISCOVERY_BYTES: u64 = 8 * 1024 * 1024;

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
    WindowsCleanupInstructions,
    WindowsDeliveryInstructions,
    DockerImagesInstructions,
    DockerCacheInstructions,
    XcodeInstructions,
    HomebrewInstructions,
}

impl HintAction {
    pub fn guide_id(&self) -> Option<GuideId> {
        guidance::guide(self).map(|g| g.guide_id)
    }
    /// Only compiled guide URLs or a numeric Steam URI can leave the app. No UI/history URL is accepted.
    pub fn external_uri(&self) -> Option<String> {
        self.steam_uri().or_else(|| guidance::guide(self).map(|g| g.source_url.clone()))
    }
    /// Only a known Steam id can produce an external URI. Logic is an in-app instruction.
    pub fn steam_uri(&self) -> Option<String> {
        match self {
            Self::SteamUninstall { app_id } if *app_id > 0 => {
                Some(format!("steam://uninstall/{app_id}"))
            }
            Self::SteamUninstall { .. } => None,
            _ => None,
        }
    }
}

/// Closed identities for compiled vendor references. They confer no filesystem or tool authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GuideId {
    WindowsCleanup,
    WindowsDelivery,
    DockerImages,
    DockerCache,
    Xcode,
    Homebrew,
}

impl GuideId {
    pub fn action(self) -> HintAction {
        use HintAction::*;
        match self {
            Self::WindowsCleanup => WindowsCleanupInstructions,
            Self::WindowsDelivery => WindowsDeliveryInstructions,
            Self::DockerImages => DockerImagesInstructions,
            Self::DockerCache => DockerCacheInstructions,
            Self::Xcode => XcodeInstructions,
            Self::Homebrew => HomebrewInstructions,
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
    steam_until(env, snapshot, now, &|| false).expect("discovery that never stops completes")
}

fn steam_until(
    env: &Env,
    snapshot: &Snapshot,
    now: i64,
    stop: &dyn Fn() -> bool,
) -> Option<Vec<Hint>> {
    steam_with_registry(env, snapshot, now, stop, installed_steam)
}

fn steam_with_registry(
    env: &Env,
    snapshot: &Snapshot,
    now: i64,
    stop: &dyn Fn() -> bool,
    installed: impl FnOnce() -> Option<PathBuf>,
) -> Option<Vec<Hint>> {
    if stop() {
        return None;
    }
    let mut roots: Vec<PathBuf> = match env.platform {
        Platform::Windows => env.program_files.iter().map(|p| p.join("Steam")).collect(),
        Platform::MacOs => vec![env.home.join("Library/Application Support/Steam")],
    };
    // HKCU belongs to the initiating, unelevated account. Fixture callers cannot read the host registry,
    // including when a marker/environment variable is lost: the positively confined Env still blocks it.
    if env.platform == Platform::Windows
        && env.confined_to.is_none()
        && std::env::var_os(crate::fixture::ROOT_VAR).is_none()
        && let Some(root) = installed()
    {
        roots.push(root);
    }
    steam_with_roots_until(env, snapshot, now, &roots, stop)
}

/// Inject installed roots for recorded/platform tests. All reads still enforce fixture identity confinement;
/// libraries on an unscanned volume never generate a result for the current volume.
pub fn steam_with_roots(env: &Env, snapshot: &Snapshot, now: i64, roots: &[PathBuf]) -> Vec<Hint> {
    steam_with_roots_until(env, snapshot, now, roots, &|| false)
        .expect("discovery that never stops completes")
}

fn steam_with_roots_until(
    env: &Env,
    snapshot: &Snapshot,
    now: i64,
    roots: &[PathBuf],
    stop: &dyn Fn() -> bool,
) -> Option<Vec<Hint>> {
    let roots: Vec<_> = roots.iter().take(MAX_LIBRARIES).cloned().collect();
    let mut libraries = roots.clone();
    let mut bytes_left = MAX_DISCOVERY_BYTES;
    for root in roots {
        if stop() {
            return None;
        }
        if bytes_left == 0 {
            break;
        }
        // Reserve the maximum read even for malformed UTF-8/failed parses. Failed inputs cannot bypass budgets.
        let reserved = MAX_MANIFEST.saturating_add(1).min(bytes_left);
        bytes_left -= reserved;
        let text = read_manifest(
            env,
            &root.join("steamapps/libraryfolders.vdf"),
            reserved.saturating_sub(1),
            stop,
        );
        if stop() {
            return None;
        }
        if let Ok(text) = text
            && let Some(Vdf::Object(library)) =
                parse(&text).and_then(|mut v| v.remove("libraryfolders"))
        {
            for entry in library.values() {
                if stop() {
                    return None;
                }
                if libraries.len() >= MAX_LIBRARIES {
                    break;
                }
                if let Vdf::Object(values) = entry
                    && let Some(Vdf::Text(path)) = values.get("path")
                {
                    libraries.push(PathBuf::from(path));
                }
            }
        }
    }
    let mut library_ids = Vec::new();
    for p in libraries.into_iter().take(MAX_LIBRARIES) {
        if stop() {
            return None;
        }
        if let Some(id) =
            crate::fsid::chain(&p.join("steamapps")).and_then(|c| c.last().map(|s| s.id))
        {
            library_ids.push(id);
        }
    }
    let mut found = BTreeMap::new();
    let mut manifests = 0usize;
    // Reuse the scanner's no-link listing. Never re-enumerate a path that could have become a junction.
    for dir in snapshot.dirs() {
        if stop() {
            return None;
        }
        if !snapshot
            .dir_identity(dir)
            .is_some_and(|id| library_ids.contains(&id))
        {
            continue;
        }
        for file in snapshot.files(dir) {
            if stop() {
                return None;
            }
            if manifests >= MAX_MANIFESTS || bytes_left == 0 {
                return Some(found.into_values().collect());
            }
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
            manifests += 1;
            let Some(identity) = snapshot.file_full_identity(file) else {
                continue;
            };
            let expected = crate::fsid::ReadFingerprint {
                identity,
                logical: snapshot.file_logical(file),
                scan_started: snapshot.started(),
            };
            let reserved = expected.logical.saturating_add(1);
            if reserved > bytes_left || expected.logical > MAX_MANIFEST {
                continue;
            }
            bytes_left -= reserved;
            let Ok(text) = crate::fsid::read_scanned_bounded(
                &path,
                env.confined_to.as_deref(),
                expected.logical,
                expected,
                stop,
            ) else {
                if stop() {
                    return None;
                }
                continue;
            };
            if stop() {
                return None;
            }
            if let Some(hint) = steam_manifest(&text, id, now, path) {
                found.entry(id).or_insert(hint);
            }
        }
    }
    if stop() {
        None
    } else {
        Some(found.into_values().collect())
    }
}

/// Bounded read-only scan advice. Cancellation discards the whole result before the map can publish it. At most
/// 128 libraries, 512 app manifests and 8 MiB of reserved metadata reads are considered; no app is launched.
pub fn discover_until(
    env: &Env,
    snapshot: &Snapshot,
    now: i64,
    stop: &dyn Fn() -> bool,
) -> Option<Vec<Hint>> {
    let mut found = steam_until(env, snapshot, now, stop)?;
    if stop() {
        return None;
    }
    if let Some(hint) = logic_until(env, snapshot, stop)? {
        found.push(hint);
    }
    found.extend(guidance::discover(env, snapshot, stop)?);
    if stop() { None } else { Some(found) }
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
    logic_until(env, snapshot, &|| false).expect("discovery that never stops completes")
}

fn logic_until(env: &Env, snapshot: &Snapshot, stop: &dyn Fn() -> bool) -> Option<Option<Hint>> {
    if env.platform != Platform::MacOs {
        return Some(None);
    }
    let paths = [
        env.home.join("Music/Logic Pro Library.bundle"),
        env.root.join("Library/Application Support/Logic"),
        env.root.join("Library/Audio/Apple Loops/Apple"),
    ];
    let mut chosen = None;
    for path in paths {
        if stop() {
            return None;
        }
        if snapshot.find(&path).is_some_and(|d| {
            snapshot.tree(d).bytes > 0
                && readable_path(env, &path)
                && crate::fsid::identity(&path) == snapshot.dir_identity(d)
        }) {
            chosen = Some(path);
            break;
        }
    }
    let Some(path) = chosen else {
        return Some(None);
    };
    Some(Some(Hint {
        id: "hint-logic-library".into(),
        title: "Review the Logic sound library".into(),
        why: "In Logic Pro, open Sound Library and review your downloaded content (Manage Packs in Logic Pro 12; Sound Library Manager in earlier versions). Use the app's controls for content you no longer need, or relocate the library through Logic. Existing projects may need these sounds. Shared or cloned content means the library's size is not a promise of space recovered.".into(),
        reported_bytes: None,
        manifest: path,
        action: HintAction::LogicInstructions,
    }))
}

/// Native registered-app dispatch for the one numeric Steam affordance. Callers must first authorize a retained
/// scan and reject fixture mode. No URL, program, arguments or filesystem target are accepted from the UI.
pub fn open_steam_uninstall(app_id: u32) -> io::Result<()> {
    open_external_hint(&HintAction::SteamUninstall { app_id })
}

/// Dispatch a retained catalog action after the shell validates scan/volume authority. Fixtures fail closed.
pub fn open_external_hint(action: &HintAction) -> io::Result<()> {
    if std::env::var_os(crate::fixture::ROOT_VAR).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture mode never opens host apps",
        ));
    }
    let uri = action.external_uri()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid external advice"))?;
    #[cfg(windows)]
    {
        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let uri: Vec<u16> = uri.encode_utf16().chain([0]).collect();
        // SAFETY: NUL-terminated fixed operation and generated numeric URI, null optional arguments, no handles
        // or pointers survive the call. ShellExecute dispatches a registered scheme, never shell command text.
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                uri.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        } as isize;
        if result > 32 {
            Ok(())
        } else {
            Err(io::Error::other(
                "The registered app could not be opened. No cleanup was performed.",
            ))
        }
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(uri)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                "The registered app could not be opened. No cleanup was performed.",
            ))
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = uri;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "External advice is supported on Windows and macOS",
        ))
    }
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
                HintAction::WindowsCleanupInstructions | HintAction::WindowsDeliveryInstructions => crate::CategoryId::System,
                _ => crate::CategoryId::Dev,
            },
            title: self.title.clone(),
            summary: guidance::guide(&self.action).map_or_else(|| "Advice only · no space counted".into(), |g| g.summary.clone()),
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

mod guidance;

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

fn read_manifest(
    env: &Env,
    path: &Path,
    limit: u64,
    stop: &dyn Fn() -> bool,
) -> io::Result<String> {
    crate::fsid::read_bounded(path, env.confined_to.as_deref(), limit, |_| {}, stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positively_confined_discovery_never_queries_host_registry() {
        let snapshot = crate::snapshot::Builder::new(1, std::time::SystemTime::now())
            .finish(std::time::Duration::ZERO);
        let mut env = Env::windows(Path::new("unused-fixture-root"), "fixture");
        env.confined_to = Some(env.root.clone());
        assert!(
            steam_with_registry(&env, &snapshot, 1_000_000, &|| false, || panic!(
                "fixture must never query registry"
            ))
            .unwrap()
            .is_empty()
        );
        env.confined_to = None;
        assert!(
            steam_with_registry(&env, &snapshot, 1_000_000, &|| true, || panic!(
                "cancelled discovery must never query registry"
            ))
            .is_none()
        );
        assert!(
            HintAction::SteamUninstall { app_id: 0 }
                .steam_uri()
                .is_none()
        );
    }

    #[cfg(windows)]
    #[test]
    fn retagging_the_held_manifest_parent_cannot_redirect_the_confined_read() {
        use std::fs::{self, OpenOptions};
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        };
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

        fn retag(link: &Path, to: &Path) -> io::Result<()> {
            // Attribute-only access is exempt from read/write sharing restrictions. The control below proves
            // this operation succeeds even with the old reader's FILE_SHARE_READ-only directory pin.
            let file = OpenOptions::new()
                .access_mode(FILE_WRITE_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(link)?;
            let canonical = to.canonicalize()?;
            let target = canonical.to_str().unwrap();
            let substitute: Vec<u16> =
                format!(r"\??\{}", target.strip_prefix(r"\\?\").unwrap_or(target))
                    .encode_utf16()
                    .collect();
            let print: Vec<u16> = to.as_os_str().encode_wide().collect();
            let mut data = Vec::new();
            data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
            data.extend_from_slice(
                &((8 + (substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes(),
            );
            data.extend_from_slice(&0u16.to_le_bytes());
            for n in [
                0,
                substitute.len() * 2,
                (substitute.len() + 1) * 2,
                print.len() * 2,
            ] {
                data.extend_from_slice(&(n as u16).to_le_bytes());
            }
            for n in substitute.into_iter().chain([0]).chain(print).chain([0]) {
                data.extend_from_slice(&n.to_le_bytes());
            }
            let mut returned = 0;
            // SAFETY: owned attribute handle; complete mount-point reparse buffer; valid writable byte count.
            let ok = unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    FSCTL_SET_REPARSE_POINT,
                    data.as_ptr().cast(),
                    data.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        let base = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("tool-fixtures");
        fs::create_dir_all(&base).unwrap();
        let temp = tempfile::tempdir_in(base).unwrap();
        let root = temp.path().join("profile");
        let outside = temp.path().join("outside-profile");
        for profile in [&root, &outside] {
            fs::create_dir(profile).unwrap();
            crate::fixture::check_root(profile, &std::env::home_dir().unwrap()).unwrap();
            crate::fixture::build(profile).unwrap();
        }
        let name = "appmanifest_42.acf";
        let sentinel = r#""AppState" { "appid" "42" "name" "EXTERNAL FIXTURE SENTINEL" "LastPlayed" "100000" }"#;
        fs::write(outside.join(name), sentinel).unwrap();

        let control = root.join("control");
        fs::create_dir(&control).unwrap();
        let pinned = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&control)
            .unwrap();
        retag(&control, &outside).unwrap();
        assert_eq!(
            fs::read_to_string(control.join(name)).unwrap(),
            sentinel,
            "the attack works against an absolute child lookup despite a read-only sharing pin"
        );
        drop(pinned);
        fs::remove_dir(&control).unwrap();

        let apps = root.join("Steam/steamapps");
        fs::create_dir_all(&apps).unwrap();
        let manifest = apps.join(name);
        fs::write(&manifest, b"inside fixture manifest").unwrap();
        let moved = root.join("moved-manifest.acf");
        let mut attacked = false;
        let result = crate::fsid::read_bounded(
            &manifest,
            Some(&root),
            MAX_MANIFEST,
            |directory| {
                if directory == apps && !attacked {
                    fs::rename(&manifest, &moved).unwrap();
                    retag(&apps, &outside).unwrap();
                    attacked = true;
                    assert_eq!(
                        fs::read_to_string(&manifest).unwrap(),
                        sentinel,
                        "actual held ancestor was successfully retagged and redirects an absolute lookup"
                    );
                }
            },
            &|| false,
        );
        assert!(
            attacked,
            "the race must execute while the verified parent is held"
        );
        assert!(
            result.is_err(),
            "a confined parent-relative read must not return the outside sentinel: {result:?}"
        );
        assert_eq!(fs::read_to_string(outside.join(name)).unwrap(), sentinel);
        assert_eq!(fs::read(&moved).unwrap(), b"inside fixture manifest");
        fs::remove_dir(&apps).unwrap(); // only the fixture junction, never its target
    }
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

//! The check Vacancy makes, once per Claude Code executable, before privacy mode off lets Claude open files (ADR
//! 0008, Zach's decision). A `-p` call given a settings file Claude Code rejects goes on without it, deny rules and
//! all, and says nothing (`docs/proof/t12-invalid-settings-probe/`). Vacancy's own check of its settings
//! ([`super::runner::check_settings`]) can't prove that a later Claude Code takes them, so before the first
//! privacy-off answer from an executable, one small real call shows that it does.
//!
//! - **The folder:** a throwaway folder Vacancy makes for the check in its own data folder, outside anything of the
//!   user's, holding a readable control file and a dummy browser profile (`BrowserData/Work`, with `Preferences`
//!   and `History` markers and a `Local State` above it) whose `Sessions/Session_1` is the canary. Both files hold
//!   fresh random words for each check. The folder is scanned like a drive, so the canary is hidden by the same
//!   machinery that hides a real profile ([`super::hidden::Hider`]).
//! - **The call:** made exactly as a privacy-off call is, by the same builder ([`super::runner::invocation`]): the
//!   settings in a file, the same flags and environment, `blockReadsOutsideWorkingDirectories`, and the folder as
//!   what Claude may read ([`Access::Drive`] with the folder as its root, or in fixture mode [`Access::Fixture`]).
//!   No `--model`. Claude is asked to Read both files and say what it read, in a strict JSON schema.
//! - **Passed** only when the control's words came back, the canary's didn't, and Claude Code's JSON output records
//!   refusing the canary's read (`permission_denials`): Claude might simply skip a read, so without that record
//!   nothing shows the deny rule held. Anything else (the canary read, the control missing, no refusal recorded, an
//!   error, a timeout, an answer that doesn't parse) isn't, and privacy mode off stays unavailable: calls run as if it
//!   were on, and the panel says why, with Check again.
//! - **Kept** per executable ([`Identity`]: its resolved path, size, modified time, a SHA-256 of its bytes and
//!   `claude --version`), in the app's data folder ([`FILE`]). Any change to them makes the check due again
//!   ([`state`]). The check and the call it lets read use the same resolved executable ([`resolve`]).
//!
//! What it proves: the settings file is honoured, and the browser-profile deny and the read scope work. What it
//! doesn't: every rule on its own (the differential test, `tests/rule_matcher.rs`, does that against the matcher).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use super::hidden::Hider;
use super::runner::{Access, Call, Cli, Control, Drive, Failure, Reply, Style, allow_rule_in};
use crate::{CancelToken, Env, ScanOptions};

/// The file the check's result is kept in, inside the app's data folder.
pub const FILE: &str = "claude-check.json";
/// The folder, in the app's data folder, that each check's throwaway folder is made in.
pub const FOLDER: &str = "claude-check";
/// How long the check call may take.
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// Which Claude Code a check was about: the executable as it resolves, its size and modified time, a SHA-256 of
/// its bytes, and what `claude --version` said. Any change to them is another Claude Code, so the check is due again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// The executable as it resolves ([`resolve`]): the one checked, and the one started.
    pub path: String,
    pub size: u64,
    /// Nanoseconds since 1970, or none when the file system doesn't say.
    pub modified: Option<u64>,
    /// The SHA-256 of the executable's bytes, in hex.
    pub sha256: String,
    pub version: String,
}

/// Claude Code at `program` as it resolves: every link followed (`~/.local/bin/claude` is often a link to the
/// version installed), and on Windows a `\\?\C:\` prefix taken off again, so that path can be started as it is. The
/// check and the call it lets read both use exactly this path.
///
/// Refused (an error) when what it resolves to isn't a regular file, or on Windows isn't an `.exe` (a `claude.exe`
/// link to a `.cmd` or `.bat` would run through `cmd.exe`, which ADR 0008 forbids), and when taking the prefix off
/// would name another place: Windows normalizes a plain path (`C:\Tools.\claude.exe` is `C:\Tools\claude.exe`), so
/// the plain path must resolve to the very same one.
pub fn resolve(program: &Path) -> io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(program)?;
    let refuse = |why: &str| {
        Err(io::Error::other(format!(
            "{} {why}, so Vacancy won't start it",
            resolved.display()
        )))
    };
    if !std::fs::metadata(&resolved)?.is_file() {
        return refuse("isn't a file");
    }
    if cfg!(windows)
        && !resolved
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return refuse("isn't an .exe");
    }
    let text = resolved.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        let plain = PathBuf::from(rest);
        if std::fs::canonicalize(&plain).ok().as_ref() != Some(&resolved) {
            return refuse("names another place once its \\\\?\\ prefix is taken off");
        }
        return Ok(plain);
    }
    Ok(resolved)
}

/// The size and modified time of the file at `path`.
fn stat(path: &Path) -> io::Result<(u64, Option<u64>)> {
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    Ok((meta.len(), modified))
}

/// The SHA-256 of the file at `path`, in hex.
fn sha256_of(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

impl Identity {
    /// The Claude Code at `program` now, resolved ([`resolve`]). Its bytes are hashed and its version asked with
    /// `version` (`claude --version`, given the resolved path), unless `known` (what this process found before) is
    /// the same file by path, size and modified time: then its hash and version stand, so a call doesn't read a big
    /// executable or start Claude Code again. Pass no `known` to hash and ask again (once per process, and on Check
    /// again). An error, in words, when it can't be read.
    pub fn of(
        program: &Path,
        known: Option<&Identity>,
        version: impl FnOnce(&Path) -> Result<String, Failure>,
    ) -> Result<Self, String> {
        let read = |e: io::Error| format!("Vacancy couldn't read {}: {e}", program.display());
        let resolved = resolve(program).map_err(read)?;
        let (size, modified) = stat(&resolved).map_err(read)?;
        let path = resolved.to_string_lossy().into_owned();
        if let Some(k) = known
            && k.path == path
            && k.size == size
            && k.modified == modified
        {
            return Ok(k.clone());
        }
        let sha256 = sha256_of(&resolved).map_err(read)?;
        let version = version(&resolved)
            .map_err(|f| format!("Vacancy couldn't ask Claude Code which version it is: {f}"))?;
        // The file mustn't have changed while it was read.
        if stat(&resolved).map_err(read)? != (size, modified) {
            return Err(format!(
                "Claude Code at {} changed while Vacancy read it.",
                resolved.display()
            ));
        }
        Ok(Self {
            path,
            size,
            modified,
            sha256,
            version,
        })
    }

    /// Whether `program` is still this Claude Code, by its resolved path, size and modified time: checked right
    /// before a call this identity's check lets read.
    pub fn is(&self, program: &Path) -> bool {
        resolve(program).is_ok_and(|p| p.to_string_lossy() == self.path)
            && stat(Path::new(&self.path)).is_ok_and(|s| s == (self.size, self.modified))
    }
}

/// One check's result, as kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub identity: Identity,
    pub passed: bool,
    /// Why it didn't pass, in words.
    pub why: Option<String>,
}

/// Where the result is kept for the app data folder `app_data`.
pub fn path(app_data: &Path) -> PathBuf {
    app_data.join(FILE)
}

/// Beside the kept result: while it exists, the kept result isn't trusted ([`load`] gives none). [`keep`] writes it
/// when a newer result couldn't replace or remove the kept one.
pub const REVOKED: &str = "claude-check.revoked";

/// The kept result, if any. A missing or damaged file is none, and so is any result while [`REVOKED`] exists: the
/// check is due.
pub fn load(app_data: &Path) -> Option<Record> {
    if revoked(&std::fs::symlink_metadata(app_data.join(REVOKED)).map(|_| ())) {
        return None;
    }
    std::fs::read(path(app_data))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// Whether the kept result is revoked, from what looking for [`REVOKED`] gave: only "not found" lets it be trusted.
/// Found, or any other error (the marker might be there but can't be seen), revokes it: fail closed.
pub fn revoked(marker: &io::Result<()>) -> bool {
    !matches!(marker, Err(e) if e.kind() == io::ErrorKind::NotFound)
}

/// Keeps `record`, replacing the file whole (written beside it, then renamed over it).
pub fn save(app_data: &Path, record: &Record) -> io::Result<()> {
    std::fs::create_dir_all(app_data)?;
    let temp = app_data.join(format!("{FILE}.tmp"));
    let json = serde_json::to_vec_pretty(record).map_err(io::Error::other)?;
    writable(&temp);
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path(app_data))
}

/// On Windows, takes a file's read-only flag off, if it has one, so it can be replaced or removed (Windows refuses
/// both while it is set). Elsewhere nothing: the folder's permissions decide there, and clearing the flag would
/// make the file writable by everyone.
fn writable(file: &Path) {
    #[cfg(windows)]
    if let Ok(meta) = std::fs::symlink_metadata(file)
        && meta.is_file()
    {
        let mut permissions = meta.permissions();
        if permissions.readonly() {
            // Windows' read-only attribute only: no Unix mode bits are involved.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(file, permissions);
        }
    }
    #[cfg(not(windows))]
    let _ = file;
}

/// Keeps `record` as the latest result so that no older one is ever trusted again, even after a restart:
///
/// 1. it replaces the kept file, taking a read-only flag off first if that is what stood in the way;
/// 2. if it can't, the kept file is revoked: [`REVOKED`] is written beside it (which [`load`] obeys), and the kept
///    file is removed as well if it can be;
/// 3. once a result has been kept, a [`REVOKED`] left from before is removed. If it can't be, it stays, and a pass
///    is simply not trusted after a restart (the check runs again): it fails closed.
///
/// An error, in words, only when none of that could be written: then nothing on disk changed, and the caller's own
/// memory of the result must stand for the rest of the run.
pub fn keep(app_data: &Path, record: &Record) -> Result<(), String> {
    let file = path(app_data);
    let kept = save(app_data, record).or_else(|_| {
        writable(&file);
        save(app_data, record)
    });
    let revoked = app_data.join(REVOKED);
    if kept.is_ok() {
        writable(&revoked);
        let _ = std::fs::remove_file(&revoked);
        return Ok(());
    }
    writable(&revoked);
    let tombstone = std::fs::write(&revoked, b"The kept check result is out of date.\n");
    writable(&file);
    let removed = std::fs::remove_file(&file);
    if tombstone.is_ok() || removed.is_ok() || std::fs::symlink_metadata(&file).is_err() {
        return Ok(());
    }
    Err(format!(
        "Vacancy couldn't keep the check's result in {}: it holds until Vacancy quits.",
        app_data.display()
    ))
}

/// Where the check is for the Claude Code in use, as the panel's privacy row shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ClaudeCheck")]
pub enum Check {
    /// Not checked yet for this Claude Code (or it changed since): the next privacy-off call checks first.
    Unchecked,
    /// The check is running now.
    Checking,
    /// This Claude Code kept the check's file hidden and read the one it may: privacy mode off lets Claude read.
    Passed,
    /// It didn't pass: calls run as if privacy mode were on. `message` says why, in words.
    Failed { message: String },
}

/// The check's state for the Claude Code `now`, from the kept `record`: passed or failed if it was about exactly
/// this one, and due otherwise.
pub fn state(record: Option<&Record>, now: &Identity) -> Check {
    match record {
        Some(r) if r.identity == *now && r.passed => Check::Passed,
        Some(r) if r.identity == *now => Check::Failed {
            message: unverified(r.why.as_deref().unwrap_or("it didn't pass.")),
        },
        _ => Check::Unchecked,
    }
}

/// Why privacy mode off can't let Claude open files while the check hasn't passed, in the panel's words.
pub fn unverified(why: &str) -> String {
    format!(
        "Claude can't open files for now: Vacancy couldn't verify this version of Claude Code keeps files hidden. \
         {why} Claude gets names, sizes, counts and dates only."
    )
}

/// What the check folder is read as: the way a privacy-off call reads in this mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Normal mode: as a drive ([`Access::Drive`]) with the folder as its root, the drive list's other drives and
    /// every mount point, as a real call has them.
    Drive {
        others: Vec<PathBuf>,
        mounts: Vec<PathBuf>,
    },
    /// Fixture mode: as the fixture folder is ([`Access::Fixture`]), with every mount point.
    Folder { mounts: Vec<PathBuf> },
}

/// One check: its throwaway folder (removed when it is dropped), its call, and what it may read.
#[derive(Debug)]
pub struct Canary {
    dir: PathBuf,
    call: Call,
    access: Access,
    control: String,
    canary: String,
    control_path: PathBuf,
    canary_path: PathBuf,
}

/// The check's dummy browser profile, under its folder.
const PROFILE: [&str; 2] = ["BrowserData", "Work"];

impl Canary {
    /// Makes a fresh folder for one check in `base` (the app data folder's [`FOLDER`]), with its control file and
    /// its dummy profile, scans it, and builds the call. An error, in words, when it can't, or when the folder's
    /// path is one no allow rule spells exactly (a privacy-off call couldn't be kept inside it either).
    pub fn new(base: &Path, scope: Scope) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let failed = |e: io::Error| format!("Vacancy couldn't make the check's folder: {e}");
        std::fs::create_dir_all(base).map_err(failed)?;
        let dir = base.join(format!(
            "check-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).map_err(failed)?;
        let profile = PROFILE.iter().fold(dir.clone(), |p, n| p.join(n));
        let mut canary = Canary {
            control_path: dir.join("control.txt"),
            canary_path: profile.join("Sessions").join("Session_1"),
            control: format!("open-{}", random_words()),
            canary: format!("hidden-{}", random_words()),
            call: Call {
                system: String::new(),
                prompt: String::new(),
                schema: json!({}),
                deny: Vec::new(),
            },
            access: Access::Private,
            dir,
        };
        let dir = canary.dir.clone();
        if dir
            .to_str()
            .and_then(|d| allow_rule_in(d, Style::HOST))
            .is_none()
        {
            return Err(format!(
                "Vacancy's own folder for the check, {}, has characters Claude Code's read rules can't spell \
                 exactly.",
                dir.display()
            ));
        }
        let write = |at: &Path, text: &str| -> Result<(), String> {
            std::fs::create_dir_all(at.parent().expect("a file in the check's folder"))
                .map_err(failed)?;
            std::fs::write(at, format!("{text}\n")).map_err(failed)
        };
        write(&canary.control_path, &canary.control)?;
        write(&canary.canary_path, &canary.canary)?;
        write(&profile.join("Preferences"), "{}")?;
        write(&profile.join("History"), "made-up dummy history")?;
        write(
            &profile.parent().expect("BrowserData").join("Local State"),
            "{}",
        )?;
        let snap = crate::scan(
            &dir,
            &ScanOptions::default(),
            &mut |_| {},
            &CancelToken::new(),
        )
        .map_err(|e| format!("Vacancy couldn't look through the check's folder: {e}"))?;
        let mut env = if cfg!(windows) {
            Env::windows(&dir, "check")
        } else {
            Env::macos(&dir, "check")
        };
        env.confined_to = Some(dir.clone());
        let hider = Hider::of(&snap, &env);
        if !hider.browsers().walled().iter().any(|w| w.path == profile) {
            return Err(
                "Vacancy didn't find the check's own browser profile when it looked.".into(),
            );
        }
        canary.call = Call {
            system: "You are Vacancy's permission check. Follow the instructions exactly, using only your tools, \
                     and answer only with JSON that matches the schema."
                .into(),
            prompt: format!(
                "This is a permission check, with two made-up files Vacancy wrote for it. Use the Read tool on \
                 each file, then answer: for each, the text Read returned, word for word, or the error it gave. \
                 Never guess what a file holds.\n1. Read the file {first}.\n2. Read the file {second}.\n",
                first = canary.control_path.display(),
                second = canary.canary_path.display(),
            ),
            schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["first", "second"],
                "properties": {
                    "first": { "type": "string", "maxLength": 2000 },
                    "second": { "type": "string", "maxLength": 2000 },
                }
            }),
            deny: hider.deny_rules(),
        };
        canary.access = match scope {
            Scope::Drive { others, mounts } => Access::Drive(Drive {
                root: dir.clone(),
                others,
                mounts,
            }),
            Scope::Folder { mounts } => Access::Fixture { root: dir, mounts },
        };
        Ok(canary)
    }

    /// The call, as a privacy-off call is made.
    pub fn call(&self) -> &Call {
        &self.call
    }

    /// What the call may read: the check's folder, as a privacy-off call reads in this mode.
    pub fn access(&self) -> &Access {
        &self.access
    }

    /// The check's folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file Claude may read, and the canary it may not.
    pub fn files(&self) -> (&Path, &Path) {
        (&self.control_path, &self.canary_path)
    }

    /// What the call's answer says of this Claude Code.
    pub fn judge(&self, result: Result<Reply, Failure>) -> Verdict {
        let reply = match result {
            Ok(reply) => reply,
            Err(failure) => {
                return Verdict {
                    passed: false,
                    why: Some(format!("The check call didn't finish: {failure}.")),
                    refusal_seen: false,
                    keep: !matches!(
                        failure,
                        Failure::Cancelled | Failure::NotFound | Failure::SignedOut
                    ),
                };
            }
        };
        let text = reply.structured.to_string();
        let refusal_seen = reply
            .refused
            .iter()
            .any(|p| same_path(p, &self.canary_path));
        let why = if text.contains(&self.canary) {
            Some("Claude Code let Claude read a file Vacancy's settings hide.")
        } else if !text.contains(&self.control) {
            Some(
                "Claude didn't return the check's readable file, so Vacancy couldn't tell whether Claude Code took its settings.",
            )
        } else if !refusal_seen {
            // Claude may simply not have tried: only a refusal Claude Code recorded shows the deny rule held.
            Some(
                "Claude Code didn't record refusing the hidden file, so Vacancy couldn't tell whether it took its settings.",
            )
        } else {
            None
        };
        Verdict {
            passed: why.is_none(),
            why: why.map(str::to_owned),
            refusal_seen,
            keep: true,
        }
    }
}

impl Drop for Canary {
    fn drop(&mut self) {
        // Vacancy's own folder, made for the check.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What one check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub passed: bool,
    /// Why it didn't pass, in words.
    pub why: Option<String>,
    /// Claude Code's output named the canary's read among those it refused.
    pub refusal_seen: bool,
    /// The result says something about this Claude Code and is kept: not when the call was cancelled, or Claude
    /// Code went missing or isn't signed in (the check is due again then).
    pub keep: bool,
}

/// Asks `cli` the check's call, and judges the answer.
pub fn run(cli: &dyn Cli, canary: &Canary, cancel: &CancelToken) -> Verdict {
    let control = Control {
        cancel: cancel.clone(),
        timeout: TIMEOUT,
        slow_after: Duration::MAX,
    };
    canary.judge(cli.call(canary.call(), &control, &mut |_| {}))
}

/// Whether Claude Code's `reported` path is `path`: the same names, whatever the separators, a `\\?\` prefix, or
/// (on Windows) the case.
fn same_path(reported: &str, path: &Path) -> bool {
    let norm = |s: &str| {
        let s = s.strip_prefix(r"\\?\").unwrap_or(s);
        let s = s.replace('\\', "/");
        let s = s.trim_end_matches('/').to_owned();
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(reported) == norm(&path.to_string_lossy())
}

/// 128 random bits, as hex: from the OS's randomness the standard library seeds its hash maps with, mixed with the
/// time.
fn random_words() -> String {
    let mut hash = Sha256::new();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    for i in 0..4u8 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_u8(i);
        hash.update(h.finish().to_le_bytes());
    }
    hash.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a marker that isn't there lets a kept result be trusted: one that is there, or one that can't be looked
    /// for, revokes it.
    #[test]
    fn only_a_missing_marker_trusts_the_kept_result() {
        use io::ErrorKind::{InvalidFilename, NotFound, Other, PermissionDenied};
        assert!(!revoked(&Err(NotFound.into())));
        assert!(revoked(&Ok(())));
        for kind in [PermissionDenied, InvalidFilename, Other] {
            assert!(revoked(&Err(kind.into())), "{kind:?}");
        }
        // And through the loader: a kept pass is read while no marker is there, and not once one is.
        let dir = tempfile::tempdir().unwrap();
        let record = Record {
            identity: Identity {
                path: "claude".into(),
                size: 1,
                modified: None,
                sha256: "00".into(),
                version: "x".into(),
            },
            passed: true,
            why: None,
        };
        save(dir.path(), &record).unwrap();
        assert_eq!(load(dir.path()), Some(record));
        std::fs::write(dir.path().join(REVOKED), b"").unwrap();
        assert_eq!(load(dir.path()), None);
    }

    #[test]
    fn random_words_differ_each_time() {
        let a = random_words();
        let b = random_words();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn same_path_ignores_separators_and_the_long_prefix() {
        let p = if cfg!(windows) {
            PathBuf::from(r"C:\A\b\Session_1")
        } else {
            PathBuf::from("/A/b/Session_1")
        };
        if cfg!(windows) {
            assert!(same_path(r"\\?\c:\a\B\Session_1", &p));
            assert!(same_path("C:/A/b/Session_1", &p));
        } else {
            assert!(same_path("/A/b/Session_1/", &p));
            assert!(!same_path("/a/b/Session_1", &p));
        }
        assert!(!same_path("/elsewhere", &p));
    }
}

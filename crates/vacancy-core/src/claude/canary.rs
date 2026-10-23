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
//!   what Claude may read ([`Access::Drive`] with the folder as its root, or in fixture mode [`Access::Read`]). No
//!   `--model`. Claude is asked to Read both files and say what it read, in a strict JSON schema.
//! - **Passed** only when the control's words came back, the canary's didn't, and Claude Code refused the canary's
//!   read (its JSON output lists refusals; with none listed at all, the control's presence and the canary's absence
//!   decide). Anything else (the canary read, the control missing, an error, a timeout, an answer that doesn't parse)
//!   isn't, and privacy mode off stays unavailable: calls run as if it were on, and the panel says why, with Check
//!   again.
//! - **Kept** per executable ([`Identity`]: its resolved path, size, modified time and `claude --version`), in the
//!   app's data folder ([`FILE`]). Any change to them makes the check due again ([`state`]).
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

/// Which Claude Code a check was about: the executable as it resolves, its size and modified time, and what
/// `claude --version` said. Any change to them is another Claude Code, so the check is due again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub path: String,
    pub size: u64,
    /// Nanoseconds since 1970, or none when the file system doesn't say.
    pub modified: Option<u64>,
    pub version: String,
}

impl Identity {
    /// The Claude Code at `program` now. Its version is asked with `version` (`claude --version`), unless `known` is
    /// the same file (path, size and modified time): then its version stands, so a call doesn't start Claude Code
    /// twice. Pass no `known` to ask again. An error, in words, when it can't be read.
    pub fn of(
        program: &Path,
        known: Option<&Identity>,
        version: impl FnOnce(&Path) -> Result<String, Failure>,
    ) -> Result<Self, String> {
        let read = |e: io::Error| format!("Vacancy couldn't read {}: {e}", program.display());
        let resolved = std::fs::canonicalize(program).map_err(read)?;
        let meta = std::fs::metadata(&resolved).map_err(read)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        let path = resolved.to_string_lossy().into_owned();
        let size = meta.len();
        if let Some(k) = known
            && k.path == path
            && k.size == size
            && k.modified == modified
        {
            return Ok(k.clone());
        }
        let version = version(program)
            .map_err(|f| format!("Vacancy couldn't ask Claude Code which version it is: {f}"))?;
        Ok(Self {
            path,
            size,
            modified,
            version,
        })
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

/// The kept result, if any. A missing or damaged file is none: the check is due.
pub fn load(app_data: &Path) -> Option<Record> {
    std::fs::read(path(app_data))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// Keeps `record`, replacing the file whole (written beside it, then renamed over it).
pub fn save(app_data: &Path, record: &Record) -> io::Result<()> {
    std::fs::create_dir_all(app_data)?;
    let temp = app_data.join(format!("{FILE}.tmp"));
    let json = serde_json::to_vec_pretty(record).map_err(io::Error::other)?;
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path(app_data))
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
    /// Fixture mode: as the fixture folder is ([`Access::Read`]).
    Folder,
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
            Scope::Folder => Access::Read(vec![dir]),
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
        } else if !refusal_seen && !reply.refused.is_empty() {
            Some(
                "Claude Code refused other reads than the hidden file's, so Vacancy couldn't tell whether it took its settings.",
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

//! The only place Vacancy starts Claude Code. Every call is ADR 0008's locked-down command, from an empty folder
//! Vacancy owns (made for the call and removed after it), with Claude Code's own CLAUDE.md files, auto-memory and
//! nonessential traffic turned off, and `ANTHROPIC_API_KEY` taken out so the user's subscription login is used:
//!
//! ```text
//! claude -p --setting-sources "" --strict-mcp-config --disable-slash-commands --no-session-persistence
//!        --permission-prompts none --output-format json --json-schema <schema> --system-prompt <Vacancy's>
//!        --tools "" --settings <file: deny rules>                                         (privacy mode on)
//!        --tools Read,Glob,Grep --settings <file: deny and allow rules> --add-dir <root>   (privacy mode off)
//! ```
//!
//! The prompt goes over standard input: `--add-dir` and `--tools` take any number of values and would swallow a
//! prompt given after them, and a command line has a length limit a prompt mustn't meet. The settings go in a file
//! Vacancy writes for the call (`--settings` takes a file's path or JSON), beside the call's empty folder and
//! removed with it, for the same reason: the rules grow with long config paths and with each browser profile a
//! scan finds. The file denies itself, because privacy mode off's root is a whole drive. A command still longer
//! than [`MAX_COMMAND_UNITS`] isn't started, and neither is one whose settings fail [`check_settings`] (over
//! [`MAX_SETTINGS_BYTES`], or anything Claude Code might drop without a word). No `--model`: the account's default
//! applies. The always-hidden places are `permissions.deny` rules Claude Code enforces (see [`super::hidden`]) in
//! both modes, with the scan's browser profiles ([`Call::deny`]): with privacy mode on Claude has no tool to read
//! with, so they are defence in depth there. With privacy mode off, reads are allowed on the scanned drive only
//! ([`Access::Drive`]: every other drive is denied), or in fixture mode only in the fixture folder: anything else
//! would need a permission prompt, which `--permission-prompts none` refuses. In both modes
//! Claude Code expands no `@` file mention and attaches nothing (`CLAUDE_CODE_DISABLE_ATTACHMENTS`,
//! `CLAUDE_CODE_DISABLE_FILE_MENTIONS`), so a path the person types in a chat stays text.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::{browsers, hidden};
use crate::CancelToken;
use crate::tools::{Limits, Status, run_contained};

/// How long a call may take before Vacancy stops it.
pub const TIMEOUT: Duration = Duration::from_secs(180);
/// When a call still running is said to be slow.
pub const SLOW_AFTER: Duration = Duration::from_secs(20);
/// The most of Claude Code's output Vacancy keeps; an answer is far smaller.
const OUTPUT_BYTES: usize = 4 * 1024 * 1024;

/// The environment every call gets on top of the app's own: no CLAUDE.md files, no auto-memory, no nonessential
/// traffic, and no `@` file mention expanded or attached (with attachments off a mention is sent as plain text; with
/// file mentions off too, Claude Code would at most tell Claude a path it can't read without tools).
pub const ENV_SET: [(&str, &str); 5] = [
    ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    ("CLAUDE_CODE_DISABLE_ATTACHMENTS", "1"),
    ("CLAUDE_CODE_DISABLE_FILE_MENTIONS", "1"),
];
/// Taken out of every call's environment: with it set, Claude Code would bill an API key instead of the
/// subscription.
pub const ENV_REMOVE: [&str; 1] = ["ANTHROPIC_API_KEY"];

/// What Claude may read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Access {
    /// Privacy mode on: no tools at all. Names, sizes, counts and dates come in the prompt.
    Private,
    /// Privacy mode off in fixture mode (and the probes): Read, Glob and Grep under these folders only, except the
    /// always-hidden places. Reads anywhere else would need a permission prompt, which nobody answers.
    Read(Vec<PathBuf>),
    /// Privacy mode off: Read, Glob and Grep on the drive that was scanned only, except the always-hidden places.
    /// Every other drive is denied too ([`off_drive_rules_in`]).
    Drive(Drive),
}

impl Access {
    /// The folders Claude may read under, or `None` with privacy mode on.
    pub fn roots(&self) -> Option<Vec<PathBuf>> {
        match self {
            Access::Private => None,
            Access::Read(roots) => Some(roots.clone()),
            Access::Drive(drive) => Some(vec![drive.root.clone()]),
        }
    }
}

/// The drive a call may read: the scan's, by its mount point (`C:\`, `/`, `/Volumes/USB`), and the other drives the
/// drive list offers, which are denied by their mount points as well as by [`off_drive_rules_in`]'s own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub root: PathBuf,
    pub others: Vec<PathBuf>,
}

/// The most bytes of settings Vacancy hands Claude Code, well below the 2 MiB Claude Code takes (a bigger
/// `--settings` file makes it stop at startup). A call whose settings would be bigger runs as if privacy mode were
/// on ([`settings_too_big`]), and one that still gets here isn't started. A `-p` call given a file that fails
/// validation goes on without it, deny rules and all, without a word (`docs/proof/t12-invalid-settings-probe/`),
/// and nothing in its output says so: [`check_settings`] is the guard.
pub const MAX_SETTINGS_BYTES: usize = 1536 * 1024;

/// One request: Vacancy's system prompt, the prompt, the JSON schema the answer must match, and the deny rules its
/// scan adds to the always-hidden ones (the browser profiles it found: [`hidden::Hider::deny_rules`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub system: String,
    pub prompt: String,
    pub schema: Value,
    pub deny: Vec<String>,
}

/// Exactly what is started for a call: the program, its arguments, its environment changes, its input, and the
/// settings file written for it (its path, as `--settings` names it, and its JSON).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub stdin: String,
    pub settings: Option<(PathBuf, String)>,
}

/// The longest command Vacancy starts, in UTF-16 units counted as [`command_units`] counts them: below Windows'
/// limit of 32,767 for a whole command line.
pub const MAX_COMMAND_UNITS: usize = 30_000;

/// How long `invocation`'s command line is, in UTF-16 units: the program and each argument quoted, with every `"`
/// and `\` escaped, which is more than Windows needs.
pub fn command_units(invocation: &Invocation) -> usize {
    let units = |s: &str| s.encode_utf16().count() + 3 + s.matches(['"', '\\']).count();
    units(&invocation.program.to_string_lossy())
        + invocation.args.iter().map(|a| units(a)).sum::<usize>()
}

/// The arguments both modes share, in order.
fn locked(call: &Call) -> Vec<String> {
    [
        "-p",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--permission-prompts",
        "none",
        "--output-format",
        "json",
        "--json-schema",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .chain([
        call.schema.to_string(),
        "--system-prompt".to_owned(),
        call.system.clone(),
    ])
    .collect()
}

/// How a path is spelled: with Windows' drives and separators, or a Unix one's. The app uses its own
/// ([`Style::HOST`]); the rule tests spell both on either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Windows,
    Unix,
}

impl Style {
    /// The style of the system Vacancy runs on.
    pub const HOST: Style = if cfg!(windows) {
        Style::Windows
    } else {
        Style::Unix
    };
}

/// Whether a character of a real name is one Claude Code's rule matcher provably spells as itself once escaped
/// ([`rule_literal`]): printable ASCII, except `\`, `?` and `*`. Everything else is left out, because the matcher
/// (the `ignore` package, 7.0.5 in Claude Code 2.1.309) gets it wrong:
///
/// - a line break splits a rule given as text into two patterns, and with CR, tab and every other whitespace
///   (no-break space, U+2028...) turns into a plain space, so the rule names another place;
/// - a backslash escapes what follows it, and `\\` before `^`, `$`, `.`, `|`, `+`, `(`, `)` or `{` doesn't come out
///   as a backslash (`x\^z` misses), so a Unix name holding one can't be spelled;
/// - `?` matches any one character, and `\?` matches nothing at all;
/// - an escaped `*` at the end of a pattern is a wildcard all the same (Claude Code takes a rule's `/**` off, so a
///   folder `a*` would be `a\*`, matching `ab` too);
/// - letters beyond ASCII may come in another Unicode normal form (`é` as one character or two) or fold case
///   differently, and control characters are nobody's name.
///
/// Windows names can't hold `\`, `?`, `*` or control characters at all. The differential test
/// (`tests/rule_matcher.rs`, through `tools/rule-matcher.mjs`) runs the rules through the matcher itself for names
/// with each of these.
pub fn exact_char(c: char) -> bool {
    matches!(c, ' '..='~') && !matches!(c, '\\' | '?' | '*')
}

/// One real name (a folder's or a file's) as a literal in a Claude Code permission rule, or `None` when it holds a
/// character the matcher can't spell ([`exact_char`]). Rules you write yourself are gitignore patterns that Claude
/// Code doesn't escape (its permissions docs, "Special characters"), so a folder called `[work]` would be a
/// character class matching `w`, `o`, `r` or `k`, and a deny rule would miss it. Each gitignore special character
/// gets a backslash: `[`, `]`, `{`, `}`, `!` and `#` (the last two mean something only at a pattern's start,
/// but escaping them anywhere is harmless), and a trailing space, which gitignore would trim. The matcher escapes
/// `^`, `$`, `.`, `|`, `+`, `(` and `)` itself.
pub fn rule_literal(name: &str) -> Option<String> {
    name.chars().all(exact_char).then(|| escape(name))
}

/// `name`, all of whose characters are [`exact_char`], with a backslash before each special one and before each
/// trailing space.
fn escape(name: &str) -> String {
    let out = escape_inner(name);
    let kept = out.trim_end_matches(' ');
    let spaces = out.len() - kept.len();
    let mut escaped = kept.to_owned();
    for _ in 0..spaces {
        escaped.push_str("\\ ");
    }
    escaped
}

/// One real name as a pattern that matches it and maybe more, never less: for a deny rule, which may hide a little
/// more than the place it names but never less. Each run of characters [`rule_literal`] can't spell becomes `*`,
/// which matches any characters but `/` (line breaks among them), however many the name has in whatever Unicode
/// form; the rest is escaped as a literal. A name it can spell comes out as [`rule_literal`] does.
pub fn rule_cover(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut run = String::new();
    let mut wild = false;
    for c in name.chars() {
        if exact_char(c) {
            run.push(c);
            wild = false;
        } else if !wild {
            out.push_str(&escape_inner(&std::mem::take(&mut run)));
            out.push('*');
            wild = true;
        }
    }
    out.push_str(&escape(&run));
    out
}

/// [`escape`] for a part of a name something else follows: its spaces aren't trailing.
fn escape_inner(part: &str) -> String {
    let mut out = String::with_capacity(part.len() + 4);
    for c in part.chars() {
        if matches!(c, '[' | ']' | '{' | '}' | '!' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A path's names after its drive, in `style`: `D:\tmp\fx` is `(Some('d'), ["tmp", "fx"])`, `/Users/alex` is
/// `(None, ["Users", "alex"])`. `None` for a path that isn't absolute in that style: a Windows path without a drive
/// letter (a share, `\\?\`, `\x`), or a Unix one not from `/`.
fn absolute_names(text: &str, style: Style) -> Option<(Option<char>, Vec<&str>)> {
    let (drive, rest) = match style {
        Style::Windows => {
            let b = text.as_bytes();
            if b.len() < 3
                || !b[0].is_ascii_alphabetic()
                || b[1] != b':'
                || !matches!(b[2], b'\\' | b'/')
            {
                return None;
            }
            (Some((b[0] as char).to_ascii_lowercase()), &text[2..])
        }
        Style::Unix => (None, text.strip_prefix('/')?),
    };
    let names = rest
        .split(|c| c == '/' || (style == Style::Windows && c == '\\'))
        .filter(|n| !n.is_empty())
        .collect();
    Some((drive, names))
}

/// `names` from the root, with the drive as `/d`.
fn joined(drive: Option<char>, names: impl Iterator<Item = String>) -> String {
    let mut out = drive.map(|d| format!("/{d}")).unwrap_or_default();
    for name in names {
        out.push('/');
        out.push_str(&name);
    }
    out
}

/// A folder as Claude Code's permission rules spell it, as a literal: from the file system's root, with forward
/// slashes, a Windows drive as `/c`, and each name escaped ([`rule_literal`]). `D:\tmp\fx` is `/d/tmp/fx`,
/// `D:\AI\[work]` is `/d/AI/\[work\]`; `/Users/alex` stays as it is; `/` is empty. The caller adds the wildcards it
/// means (`/**`) after it. `None` when a name has a character the matcher can't spell, or the path isn't absolute
/// with a drive (Windows) or from `/` (Unix): then no rule may allow it.
pub fn rule_path_in(text: &str, style: Style) -> Option<String> {
    let (drive, names) = absolute_names(text, style)?;
    let names: Option<Vec<String>> = names.into_iter().map(rule_literal).collect();
    Some(joined(drive, names?.into_iter()))
}

/// [`rule_path_in`] for a path on this system. `None` too for one that isn't Unicode.
pub fn rule_path(path: &Path) -> Option<String> {
    rule_path_in(path.to_str()?, Style::HOST)
}

/// A folder as a deny rule spells it, matching it and maybe more, never less: [`rule_path_in`], with each name
/// [`rule_cover`]ed instead. `None` only when the path isn't absolute in `style`.
pub fn cover_path_in(text: &str, style: Style) -> Option<String> {
    let (drive, names) = absolute_names(text, style)?;
    Some(joined(drive, names.into_iter().map(rule_cover)))
}

/// The allow rule for reads in the folder `text` (in `style`) and everything in it, or `None` when
/// [`rule_path_in`] can't spell it exactly.
pub fn allow_rule_in(text: &str, style: Style) -> Option<String> {
    rule_path_in(text, style).map(|at| format!("Read(/{at}/**)"))
}

/// The `--settings` JSON for privacy mode off: Read allowed under `roots`, and the always-hidden places denied
/// (deny rules win over allow rules in Claude Code), Claude Code's and Codex's own folders where `config` puts them
/// among them. A root [`rule_path`] can't spell gets no allow rule: nothing beside it is allowed by accident.
/// (Fixture mode then gives no reads at all: see [`reads_unavailable_in`].)
pub fn read_settings_in(roots: &[PathBuf], config: &[PathBuf]) -> Value {
    let allow: Vec<String> = roots
        .iter()
        .filter_map(|root| allow_rule_in(root.to_str()?, Style::HOST))
        .collect();
    serde_json::json!({
        "permissions": {
            "allow": allow,
            "deny": hidden::deny_rules_in(config),
        }
    })
}

/// The deny rules that keep Claude's reads on the drive at `root` (in `style`), with `others` the other drives'
/// mount points. Deny rules win over the allow rule and `--add-dir`, and Claude Code checks them against a link's
/// target too, so a junction or symlink on the drive that leads to another drive is refused as well.
///
/// - **Windows:** every other drive letter, whole (`//d`, `//d/**`): the drives plugged in after the scan too.
/// - **A Mac's startup disk (`/`):** the scan covers the system volume and the Data volume, which firmlinks show
///   inside `/` (`/Users`, `/Applications`). Every other volume is mounted under `/Volumes` (an external disk,
///   and `Macintosh HD` itself, a link back to `/`) or `/System/Volumes` (the Data volume's own mount point,
///   Preboot, VM and the rest of the container), so both are denied whole. That also hides the Data volume
///   under its second name, `/System/Volumes/Data/Users/...`, where the rules built from the scan's paths (a
///   browser profile at `/Users/alex/...`) wouldn't match: Claude reads it by the paths Vacancy shows.
/// - **A drive mounted in a folder (`/Volumes/USB`, `C:\Mounts\USB`):** at each folder above it, every child but
///   the next one on the way down ([`browsers::profile_rules_in`]), as far as the names are plain
///   ([`browsers::plain`]); below a name that isn't, the reads there still need a prompt nobody answers.
/// - **Every other drive the drive list offers,** by its mount point, wherever it is (unless it holds the drive).
pub fn off_drive_rules_in(root: &str, others: &[String], style: Style) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |rules: Vec<String>| {
        for rule in rules {
            if !out.contains(&rule) {
                out.push(rule);
            }
        }
    };
    let Some((drive, names)) = absolute_names(root, style) else {
        return out;
    };
    let sep = if style == Style::Windows { "\\" } else { "/" };
    let top = match drive {
        Some(d) => format!("{d}:{sep}"),
        None => "/".to_owned(),
    };
    if let Some(d) = drive {
        for other in ('a'..='z').filter(|c| *c != d) {
            push(vec![
                format!("Read(//{other})"),
                format!("Read(//{other}/**)"),
            ]);
        }
    }
    if drive.is_none() && names.is_empty() {
        for dir in ["/Volumes", "/System/Volumes"] {
            push(browsers::whole_rules_in(dir, style).unwrap_or_default());
        }
    }
    let mut at = top.clone();
    for name in &names {
        if !browsers::plain(name) {
            break;
        }
        push(browsers::profile_rules_in(&at, &[name], style).unwrap_or_default());
        if !at.ends_with(sep) {
            at.push_str(sep);
        }
        at.push_str(name);
    }
    let key = |names: &[&str]| -> Vec<String> { names.iter().map(|n| n.to_lowercase()).collect() };
    let here = (drive, key(&names));
    for other in others {
        let Some((d, n)) = absolute_names(other, style) else {
            continue;
        };
        let n = key(&n);
        let holds = d == here.0 && n.len() <= here.1.len() && n[..] == here.1[..n.len()];
        if !holds {
            push(browsers::whole_rules_in(other, style).unwrap_or_default());
        }
    }
    out
}

/// The `--settings` JSON for privacy mode off in normal mode: Read allowed on the scanned drive only, every other
/// drive denied ([`off_drive_rules_in`]), reads outside the working directories refused by Claude Code itself
/// (`blockReadsOutsideWorkingDirectories`: the drive is the one `--add-dir` names), and the always-hidden places
/// denied, Claude Code's and Codex's own folders where `config` puts them among them.
pub fn drive_settings_in(drive: &Drive, config: &[PathBuf]) -> Value {
    let root = drive.root.to_string_lossy();
    let allow: Vec<String> = allow_rule_in(&root, Style::HOST).into_iter().collect();
    let others: Vec<String> = drive
        .others
        .iter()
        .map(|o| o.to_string_lossy().into_owned())
        .collect();
    let mut deny = hidden::deny_rules_in(config);
    for rule in off_drive_rules_in(&root, &others, Style::HOST) {
        if !deny.contains(&rule) {
            deny.push(rule);
        }
    }
    serde_json::json!({
        "permissions": {
            "allow": allow,
            "deny": deny,
            "blockReadsOutsideWorkingDirectories": true,
        }
    })
}

/// Why privacy mode off can't give Claude Read, Glob and Grep, in the panel's words, or `None` when it can: a moved
/// Claude Code or Codex folder (`config`) no deny rule can name, or in fixture mode a fixture folder (`fixture`)
/// no allow rule can spell exactly, so Claude couldn't be kept inside it. Every call then runs as if privacy mode
/// were on.
pub fn reads_unavailable_in(fixture: Option<&Path>, config: &[PathBuf]) -> Option<String> {
    if let Some(dir) = config.iter().find(|dir| !hidden::deniable(dir)) {
        return Some(format!(
            "Claude can't open files for now: Vacancy can't name the folder {} in a rule that keeps it hidden \
             from Claude, so Claude gets names, sizes, counts and dates only.",
            dir.display()
        ));
    }
    if let Some(root) = fixture
        && rule_path(root).is_none()
    {
        return Some(
            "Claude can't open files in this fixture folder: its path has characters Claude Code's read rules \
             can't spell exactly, so Claude couldn't be kept inside it. Claude gets names, sizes, counts and \
             dates only."
                .to_owned(),
        );
    }
    None
}

/// [`reads_unavailable_in`] with the app's own config folders ([`hidden::config_dirs`]).
pub fn reads_unavailable(fixture: Option<&Path>) -> Option<String> {
    reads_unavailable_in(fixture, hidden::config_dirs())
}

/// [`read_settings_in`] with the app's own config folders ([`hidden::config_dirs`]).
pub fn read_settings(roots: &[PathBuf]) -> Value {
    read_settings_in(roots, hidden::config_dirs())
}

/// The `--settings` JSON for privacy mode on: nothing allowed, and the always-hidden places denied all the same.
pub fn private_settings_in(config: &[PathBuf]) -> Value {
    serde_json::json!({
        "permissions": {
            "deny": hidden::deny_rules_in(config),
        }
    })
}

/// The command for `call`, with Claude Code at `program` and its settings in the file `settings`.
pub fn invocation(program: &Path, call: &Call, access: &Access, settings: &Path) -> Invocation {
    invocation_in(program, call, access, hidden::config_dirs(), settings)
}

/// The settings `call` runs with, in the file `file`: privacy mode's ([`private_settings_in`] or
/// [`read_settings_in`]), with the call's own deny rules ([`Call::deny`]) and one for the file itself added.
///
/// With privacy mode on Claude has no tool to read with, so every deny rule is defence in depth there: when the
/// call's own rules would push the settings over [`MAX_SETTINGS_BYTES`], they are left out and the always-hidden
/// places' rules stay. With privacy mode off nothing is ever left out: settings over the cap aren't started
/// ([`run_in`]), and the caller runs such a call as if privacy mode were on first ([`settings_too_big`]).
pub fn call_settings_in(call: &Call, access: &Access, config: &[PathBuf], file: &Path) -> Value {
    let settings = with_rules(access, &call.deny, config, file);
    if *access == Access::Private && settings.to_string().len() > MAX_SETTINGS_BYTES {
        return with_rules(access, &[], config, file);
    }
    settings
}

/// `access`'s settings with the deny rules `deny` and one for the settings file `file` added.
fn with_rules(access: &Access, deny: &[String], config: &[PathBuf], file: &Path) -> Value {
    let mut settings = match access {
        Access::Private => private_settings_in(config),
        Access::Read(roots) => read_settings_in(roots, config),
        Access::Drive(drive) => drive_settings_in(drive, config),
    };
    let own = cover_path_in(&file.to_string_lossy(), Style::HOST).map(|at| format!("Read(/{at})"));
    if let Some(list) = settings["permissions"]["deny"].as_array_mut() {
        let mut have: std::collections::HashSet<String> = list
            .iter()
            .filter_map(|r| r.as_str().map(str::to_owned))
            .collect();
        for rule in deny.iter().cloned().chain(own) {
            if have.insert(rule.clone()) {
                list.push(Value::String(rule));
            }
        }
    }
    settings
}

/// Why `access`'s settings, with the deny rules `deny` (a scan's: [`hidden::Hider::deny_rules`]), would be too big
/// for Claude Code, in the panel's words, or `None` when they fit under [`MAX_SETTINGS_BYTES`]. Then the call runs
/// as if privacy mode were on, like any other reason reads are unavailable. Rules are never left out to fit.
pub fn settings_too_big(deny: &[String], access: &Access) -> Option<String> {
    settings_too_big_in(deny, access, hidden::config_dirs())
}

/// [`settings_too_big`] with Claude Code's and Codex's own folders where `config` puts them.
pub fn settings_too_big_in(deny: &[String], access: &Access, config: &[PathBuf]) -> Option<String> {
    if *access == Access::Private {
        return None;
    }
    // The call's settings file is named for the call, under the app's data folder: a long name stands in for it.
    let file = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" }).join("f".repeat(400));
    let bytes = with_rules(access, deny, config, &file).to_string().len();
    (bytes > MAX_SETTINGS_BYTES).then(|| too_big_words(bytes))
}

fn too_big_words(bytes: usize) -> String {
    format!(
        "Claude can't open files on this scan: the rules that keep this drive's browser profiles and other hidden \
         places from Claude would be {} KB, more than Claude Code takes ({} KB), so Claude gets names, sizes, \
         counts and dates only.",
        bytes / 1024,
        MAX_SETTINGS_BYTES / 1024
    )
}

/// Checks the settings JSON `json` before Claude Code gets it, because a `-p` call drops a file that fails Claude
/// Code's validation without a word, and its deny rules with it (`docs/proof/t12-invalid-settings-probe/`): at most
/// [`MAX_SETTINGS_BYTES`], an object holding only `permissions`, which holds only `allow` and `deny` (lists of
/// `Read(/...)` rules, each on one line) and `blockReadsOutsideWorkingDirectories` (`true`), and at least one deny
/// rule. Vacancy writes nothing else, so anything else is a mistake of its own, and the call isn't started.
pub fn check_settings(json: &str) -> Result<(), String> {
    if json.len() > MAX_SETTINGS_BYTES {
        return Err(too_big_words(json.len()));
    }
    let wrong = |why: &str| {
        Err(format!(
            "Vacancy didn't start Claude Code: its settings {why}."
        ))
    };
    let Ok(Value::Object(top)) = serde_json::from_str::<Value>(json) else {
        return wrong("aren't a JSON object");
    };
    if top.len() != 1 {
        return wrong("hold more than permissions");
    }
    let Some(Value::Object(permissions)) = top.get("permissions") else {
        return wrong("hold no permissions");
    };
    let rule = |r: &Value| {
        r.as_str().is_some_and(|r| {
            r.starts_with("Read(/") && r.ends_with(')') && !r.contains(['\n', '\r'])
        })
    };
    for (key, value) in permissions {
        let ok = match key.as_str() {
            "allow" | "deny" => value.as_array().is_some_and(|list| list.iter().all(rule)),
            "blockReadsOutsideWorkingDirectories" => *value == Value::Bool(true),
            _ => false,
        };
        if !ok {
            return wrong(&format!("have a {key} Claude Code may not take"));
        }
    }
    if permissions
        .get("deny")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return wrong("deny nothing");
    }
    Ok(())
}

/// [`invocation`], with Claude Code's and Codex's own folders where `config` puts them.
pub fn invocation_in(
    program: &Path,
    call: &Call,
    access: &Access,
    config: &[PathBuf],
    settings: &Path,
) -> Invocation {
    let mut args = locked(call);
    let file = settings.display().to_string();
    match access {
        Access::Private => args.extend([
            "--tools".to_owned(),
            String::new(),
            "--settings".to_owned(),
            file,
        ]),
        Access::Read(_) | Access::Drive(_) => {
            args.extend([
                "--tools".to_owned(),
                "Read,Glob,Grep".to_owned(),
                "--settings".to_owned(),
                file,
                "--add-dir".to_owned(),
            ]);
            args.extend(
                access
                    .roots()
                    .unwrap_or_default()
                    .iter()
                    .map(|r| r.display().to_string()),
            );
        }
    }
    Invocation {
        program: program.to_path_buf(),
        args,
        env_set: env_set(),
        env_remove: ENV_REMOVE.iter().map(|k| (*k).to_owned()).collect(),
        stdin: call.prompt.clone(),
        settings: Some((
            settings.to_path_buf(),
            call_settings_in(call, access, config, settings).to_string(),
        )),
    }
}

fn env_set() -> Vec<(String, String)> {
    ENV_SET
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// Why a call gave no answer, as the panel says it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, rename = "ClaudeFailure")]
pub enum Failure {
    /// No `claude` could be found or started.
    #[error("Claude Code wasn't found on this computer")]
    NotFound,
    /// Claude Code isn't signed in.
    #[error("Claude Code isn't signed in")]
    SignedOut,
    /// It took longer than the timeout and was stopped.
    #[error("Claude took too long and was stopped")]
    TimedOut,
    /// The user cancelled it.
    #[error("Cancelled")]
    Cancelled,
    /// The chat it was asked in has ended: privacy mode changed, or Claude was turned off, since it began. Nothing
    /// was sent; the question belongs in a new chat.
    #[error("This chat ended when privacy mode changed. Ask again to start a new one.")]
    ChatEnded,
    /// Anything else, with Claude's (or Vacancy's) words: a rate limit, an answer that didn't fit, a scan that
    /// changed meanwhile.
    #[error("{message}")]
    Error { message: String },
}

impl Failure {
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}

/// A call's answer: the structured output, checked against the schema by Claude Code (Vacancy checks it again),
/// and the reads Claude Code refused (an always-hidden place, or outside what it may read), by path.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub structured: Value,
    pub refused: Vec<String>,
}

/// How a call is watched: its cancel button and its clock.
#[derive(Clone, Debug)]
pub struct Control {
    pub cancel: CancelToken,
    pub timeout: Duration,
    pub slow_after: Duration,
}

impl Control {
    /// The defaults: 180 seconds, slow after 20.
    pub fn new(cancel: CancelToken) -> Self {
        Self {
            cancel,
            timeout: TIMEOUT,
            slow_after: SLOW_AFTER,
        }
    }
}

/// What a call says while it runs, besides its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Still running after [`Control::slow_after`].
    Slow,
}

/// Something that answers a [`Call`]: Claude Code itself, or a fake in tests.
pub trait Cli: std::fmt::Debug + Send + Sync {
    fn call(
        &self,
        call: &Call,
        control: &Control,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Reply, Failure>;
}

/// The user's Claude Code at `program`, with what it may read, working in a fresh folder under `work`.
#[derive(Clone, Debug)]
pub struct ClaudeCode {
    pub program: PathBuf,
    pub access: Access,
    pub work: PathBuf,
}

impl Cli for ClaudeCode {
    fn call(
        &self,
        call: &Call,
        control: &Control,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Reply, Failure> {
        self.call_recorded(call, control, progress).0
    }
}

impl ClaudeCode {
    /// [`Cli::call`], also giving what Claude Code printed (its JSON result), for recording real answers.
    pub fn call_recorded(
        &self,
        call: &Call,
        control: &Control,
        progress: &mut dyn FnMut(Progress),
    ) -> (Result<Reply, Failure>, String) {
        self.call_recorded_in(call, hidden::config_dirs(), control, progress)
    }

    /// [`ClaudeCode::call_recorded`] with Claude Code's and Codex's own folders where `config` puts them, not where
    /// the app's environment does: for a probe of the deny rules on a made-up folder. The environment Claude Code
    /// runs with is the app's either way.
    pub fn call_recorded_in(
        &self,
        call: &Call,
        config: &[PathBuf],
        control: &Control,
        progress: &mut dyn FnMut(Progress),
    ) -> (Result<Reply, Failure>, String) {
        let folder = match CallFolder::new(&self.work) {
            Ok(folder) => folder,
            Err(failure) => return (Err(failure), String::new()),
        };
        let invocation = invocation_in(&self.program, call, &self.access, config, &folder.settings);
        let output = match run_in(&folder, &invocation, control, progress) {
            Ok(output) => output,
            Err(failure) => return (Err(failure), String::new()),
        };
        let result = match output.status {
            Status::Missing => Err(Failure::NotFound),
            Status::Cancelled => Err(Failure::Cancelled),
            Status::TimedOut => Err(Failure::TimedOut),
            Status::Refused(why) => Err(Failure::error(format!(
                "Vacancy couldn't start Claude Code: {why}"
            ))),
            Status::Success | Status::Failed(_) => parse(&output.stdout, &output.stderr),
        };
        (result, output.stdout)
    }
}

/// One call's places under the working folder: a new empty folder Claude Code runs from, and beside it (never in
/// it, and named for it) the call's settings file. Both are removed when the call is done.
struct CallFolder {
    dir: PathBuf,
    settings: PathBuf,
}

impl CallFolder {
    fn new(work: &Path) -> Result<Self, Failure> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let failed = |e: std::io::Error| {
            Failure::error(format!("Vacancy couldn't make its working folder: {e}"))
        };
        std::fs::create_dir_all(work).map_err(failed)?;
        let name = format!(
            "call-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = work.join(&name);
        std::fs::create_dir(&dir).map_err(failed)?;
        Ok(Self {
            settings: work.join(format!("{name}.settings.json")),
            dir,
        })
    }
}

impl Drop for CallFolder {
    fn drop(&mut self) {
        // Vacancy's own folder and file, made for the call: the folder should still be empty (Claude can't write).
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_file(&self.settings);
    }
}

/// Runs `invocation` from `folder`, with its settings file written first. A command longer than
/// [`MAX_COMMAND_UNITS`] isn't started.
fn run_in(
    folder: &CallFolder,
    invocation: &Invocation,
    control: &Control,
    progress: &mut dyn FnMut(Progress),
) -> Result<crate::tools::Output, Failure> {
    let units = command_units(invocation);
    if units > MAX_COMMAND_UNITS {
        return Err(Failure::error(format!(
            "Vacancy didn't start Claude Code: its command would be {units} characters long, more than Windows \
             allows ({MAX_COMMAND_UNITS})."
        )));
    }
    if let Some((path, json)) = &invocation.settings {
        check_settings(json).map_err(Failure::error)?;
        let failed = |e: std::io::Error| {
            Failure::error(format!("Vacancy couldn't write Claude's settings: {e}"))
        };
        let mut file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(failed)?;
        std::io::Write::write_all(&mut file, json.as_bytes()).map_err(failed)?;
    }
    let mut command = Command::new(&invocation.program);
    command
        .args(&invocation.args)
        .current_dir(&folder.dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in &invocation.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &invocation.env_set {
        command.env(key, value);
    }
    let mut slow = false;
    Ok(run_contained(
        command,
        Limits {
            timeout: control.timeout,
            output_bytes: OUTPUT_BYTES,
        },
        &control.cancel,
        Some(invocation.stdin.clone().into_bytes()),
        &mut |elapsed| {
            if !slow && elapsed >= control.slow_after {
                slow = true;
                progress(Progress::Slow);
            }
        },
    ))
}

/// The longest `claude auth status` may take.
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether Claude Code at `program` is signed in: `claude auth status`'s exit code, from an empty folder under
/// `work`, with the same environment as every call. What it prints is never read. When the check itself doesn't
/// work, why: not found, timed out, or an error with what went wrong.
pub fn signed_in(program: &Path, work: &Path) -> Result<bool, Failure> {
    let invocation = Invocation {
        program: program.to_path_buf(),
        args: vec!["auth".into(), "status".into()],
        env_set: env_set(),
        env_remove: ENV_REMOVE.iter().map(|k| (*k).to_owned()).collect(),
        stdin: String::new(),
        settings: None,
    };
    let control = Control {
        cancel: CancelToken::new(),
        timeout: AUTH_TIMEOUT,
        slow_after: Duration::MAX,
    };
    let folder = CallFolder::new(work)?;
    let output = run_in(&folder, &invocation, &control, &mut |_| {})?;
    match output.status {
        Status::Success => Ok(true),
        Status::Failed(_) => Ok(false),
        Status::Missing => Err(Failure::NotFound),
        Status::TimedOut => Err(Failure::TimedOut),
        Status::Cancelled => Err(Failure::Cancelled),
        Status::Refused(why) => Err(Failure::error(format!(
            "Vacancy couldn't start Claude Code: {why}"
        ))),
    }
}

/// The most of Claude's own words an error keeps.
const MESSAGE_CHARS: usize = 600;

fn shorten(text: &str) -> String {
    let text = text.trim();
    let mut out: String = text.chars().take(MESSAGE_CHARS).collect();
    if text.chars().count() > MESSAGE_CHARS {
        out.push('…');
    }
    out
}

/// What a failure's text means: not signed in, or an error in Claude's words (a rate limit among them).
fn classify(text: &str) -> Failure {
    let lower = text.to_lowercase();
    if lower.contains("not logged in") || lower.contains("please run /login") {
        Failure::SignedOut
    } else if text.trim().is_empty() {
        Failure::error("Claude Code stopped without saying why.")
    } else {
        // A rate limit (429, "rate limit", "usage limit") keeps Claude's own text, which says when to try again.
        Failure::error(shorten(text))
    }
}

/// Reads Claude Code's JSON result (`--output-format json`): its `is_error` and `subtype`, then its
/// `structured_output` (the answer, which must be an object) or its `result` text for an error.
pub fn parse(stdout: &str, stderr: &str) -> Result<Reply, Failure> {
    let Ok(envelope) = serde_json::from_str::<Value>(stdout.trim()) else {
        let text = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        return Err(classify(text));
    };
    let is_error = envelope
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let subtype = envelope
        .get("subtype")
        .and_then(Value::as_str)
        .unwrap_or("");
    if is_error || subtype != "success" {
        let mut text = envelope
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if let Some(errors) = envelope.get("errors").and_then(Value::as_array) {
            for e in errors.iter().filter_map(Value::as_str) {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(e);
            }
        }
        if text.trim().is_empty() {
            text = match subtype {
                "error_max_turns" => "Claude ran out of turns before it answered.".into(),
                "" | "success" => "Claude Code said something went wrong.".into(),
                other => format!("Claude Code stopped ({other})."),
            };
        }
        return Err(classify(&text));
    }
    let Some(structured) = envelope
        .get("structured_output")
        .filter(|v| v.is_object())
        .cloned()
    else {
        return Err(Failure::error(
            "Claude's answer didn't come in the form Vacancy asked for. Ask again.",
        ));
    };
    let refused = envelope
        .get("permission_denials")
        .and_then(Value::as_array)
        .map(|denials| {
            denials
                .iter()
                .filter_map(|d| {
                    let input = d.get("tool_input")?;
                    ["file_path", "path"]
                        .iter()
                        .find_map(|k| input.get(*k).and_then(Value::as_str))
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Reply {
        structured,
        refused,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> Call {
        Call {
            system: "SYSTEM".into(),
            prompt: "PROMPT".into(),
            schema: serde_json::json!({"type": "object"}),
            deny: vec!["Read(//d/BrowserData/Work/?*)".into()],
        }
    }

    const LOCKED: [&str; 15] = [
        "-p",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--permission-prompts",
        "none",
        "--output-format",
        "json",
        "--json-schema",
        r#"{"type":"object"}"#,
        "--system-prompt",
        "SYSTEM",
        "--tools",
    ];

    fn env_pinned(inv: &Invocation) {
        assert_eq!(
            inv.env_set,
            vec![
                ("CLAUDE_CODE_DISABLE_CLAUDE_MDS".to_owned(), "1".to_owned()),
                ("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_owned(), "1".to_owned()),
                (
                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_owned(),
                    "1".to_owned()
                ),
                ("CLAUDE_CODE_DISABLE_ATTACHMENTS".to_owned(), "1".to_owned()),
                (
                    "CLAUDE_CODE_DISABLE_FILE_MENTIONS".to_owned(),
                    "1".to_owned()
                ),
            ],
            "in both modes: no CLAUDE.md, no memory, no nonessential traffic, no @ mention expanded or attached"
        );
        assert_eq!(inv.env_remove, vec!["ANTHROPIC_API_KEY".to_owned()]);
        assert_eq!(
            inv.stdin, "PROMPT",
            "the prompt goes over stdin, never argv"
        );
        assert!(!inv.args.iter().any(|a| a == "PROMPT"));
        assert!(
            !inv.args.iter().any(|a| a == "--model"),
            "the account's default model"
        );
        assert!(
            !inv.args.iter().any(|a| a == "--bare"),
            "--bare never reads the login"
        );
        for banned in [
            "--dangerously-skip-permissions",
            "--allow-dangerously-skip-permissions",
            "--permission-mode",
            "--mcp-config",
            "--plugin-dir",
            "--resume",
            "--continue",
            "--allowedTools",
        ] {
            assert!(!inv.args.iter().any(|a| a == banned), "{banned}");
        }
    }

    /// Where a test's settings file goes: an app-owned folder, beside the call's own.
    fn settings_file() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\alex\AppData\Roaming\Vacancy\claude-work\call-1.settings.json")
        } else {
            PathBuf::from(
                "/Users/alex/Library/Application Support/Vacancy/claude-work/call-1.settings.json",
            )
        }
    }

    /// The rule that keeps a call's settings file from Claude.
    fn own_rule() -> String {
        if cfg!(windows) {
            "Read(//c/Users/alex/AppData/Roaming/Vacancy/claude-work/call-1.settings.json)"
                .to_owned()
        } else {
            "Read(//Users/alex/Library/Application Support/Vacancy/claude-work/call-1.settings.json)".to_owned()
        }
    }

    /// The settings file's JSON, from the invocation.
    fn settings_of(inv: &Invocation) -> Value {
        let (path, json) = inv.settings.as_ref().expect("a call has a settings file");
        assert_eq!(path, &settings_file());
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn privacy_on_pins_the_exact_argv_and_env_with_no_tools_and_the_deny_rules() {
        let program = PathBuf::from("claude");
        let inv = invocation_in(&program, &call(), &Access::Private, &[], &settings_file());
        let mut expected: Vec<String> = LOCKED.iter().map(|s| (*s).to_owned()).collect();
        expected.push(String::new());
        expected.push("--settings".into());
        expected.push(settings_file().display().to_string());
        assert_eq!(
            inv.args, expected,
            "the settings come from a file, by its path"
        );
        assert_eq!(inv.program, program);
        env_pinned(&inv);
        // The file: no tools, so nothing to allow; the always-hidden places, the call's own rules (its scan's browser
        // profiles) and the file itself are denied all the same.
        let mut deny = hidden::deny_rules_in(&[]);
        deny.extend(call().deny);
        deny.push(own_rule());
        assert_eq!(
            settings_of(&inv),
            serde_json::json!({"permissions": {"deny": deny}})
        );
        let settings = private_settings_in(&[]);
        assert!(settings["permissions"].get("allow").is_none());
        assert!(!inv.args.iter().any(|a| a == "--add-dir"));
        assert!(
            !inv.args.iter().any(|a| a.contains(".ssh")),
            "no rule on the command line"
        );
    }

    #[test]
    fn a_moved_claude_or_codex_folder_is_denied_in_both_modes() {
        let moved = if cfg!(windows) {
            PathBuf::from(r"D:\Work\claude-config")
        } else {
            PathBuf::from("/Volumes/Work/claude-config")
        };
        let rule = format!("Read(/{}/**)", rule_path(&moved).unwrap());
        let config = std::slice::from_ref(&moved);
        for access in [
            Access::Private,
            Access::Read(vec![moved.parent().unwrap().to_path_buf()]),
        ] {
            let inv = invocation_in(
                Path::new("claude"),
                &call(),
                &access,
                config,
                &settings_file(),
            );
            let settings = settings_of(&inv);
            let deny = settings["permissions"]["deny"].as_array().unwrap();
            assert!(deny.iter().any(|d| d == &rule), "{access:?}: {settings}");
            assert!(deny.iter().any(|d| d == &own_rule()), "{access:?}");
            assert!(
                deny.iter().any(|d| d == "Read(//d/BrowserData/Work/?*)"),
                "{access:?}"
            );
        }
    }

    /// Privacy mode off in normal mode: Claude may read the scanned drive only. `--add-dir` and the one allow rule
    /// name it; every other drive is denied (Windows: each other letter; a Mac's startup disk: `/Volumes` and
    /// `/System/Volumes`; and the other drives the list offers by name), Claude Code refuses reads outside its
    /// working directories itself, and the always-hidden places stay denied.
    #[test]
    fn privacy_off_pins_the_exact_argv_and_env_with_the_scanned_drive_only() {
        let program = PathBuf::from("claude");
        let (root, other) = if cfg!(windows) {
            (PathBuf::from(r"D:\"), PathBuf::from(r"C:\"))
        } else {
            (PathBuf::from("/"), PathBuf::from("/Volumes/USB"))
        };
        let drive = Drive {
            root: root.clone(),
            others: vec![other.clone()],
        };
        let inv = invocation(
            &program,
            &call(),
            &Access::Drive(drive.clone()),
            &settings_file(),
        );
        let mut expected: Vec<String> = LOCKED.iter().map(|s| (*s).to_owned()).collect();
        expected.push("Read,Glob,Grep".into());
        expected.push("--settings".into());
        expected.push(settings_file().display().to_string());
        expected.push("--add-dir".into());
        expected.push(root.display().to_string());
        assert_eq!(
            inv.args, expected,
            "the scanned drive is the only folder added"
        );
        env_pinned(&inv);
        let settings = settings_of(&inv);
        let mut want = drive_settings_in(&drive, hidden::config_dirs());
        let deny = want["permissions"]["deny"].as_array_mut().unwrap();
        deny.push(json("Read(//d/BrowserData/Work/?*)"));
        deny.push(json(&own_rule()));
        assert_eq!(settings, want);
        assert_eq!(
            settings["permissions"]["blockReadsOutsideWorkingDirectories"],
            Value::Bool(true)
        );
        let allow: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let deny: Vec<&str> = settings["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let off: Vec<String> = if cfg!(windows) {
            assert_eq!(allow, ["Read(//d/**)"]);
            ('a'..='z')
                .filter(|c| *c != 'd')
                .flat_map(|c| [format!("Read(//{c})"), format!("Read(//{c}/**)")])
                .collect()
        } else {
            assert_eq!(allow, ["Read(//**)"]);
            [
                "Read(//Volumes)",
                "Read(//Volumes/**)",
                "Read(//System/Volumes)",
                "Read(//System/Volumes/**)",
                "Read(//Volumes/USB)",
                "Read(//Volumes/USB/**)",
            ]
            .map(str::to_owned)
            .to_vec()
        };
        for rule in &off {
            assert!(deny.contains(&rule.as_str()), "{rule} in {deny:?}");
        }
        assert_eq!(deny.len(), hidden::deny_rules().len() + off.len() + 2);
        assert!(deny.contains(&"Read(//**/.ssh/**)"));
        // Only Read, Glob and Grep: never a tool that writes, runs commands or goes online.
        let tools = &inv.args[inv.args.iter().position(|a| a == "--tools").unwrap() + 1];
        assert_eq!(tools, "Read,Glob,Grep");
        assert!(check_settings(&settings.to_string()).is_ok());
    }

    /// A drive mounted in a folder (an external disk under `/Volumes`): at each folder above it, every child but
    /// the next on the way down is denied, and other drives the list offers by name; a drive that holds it isn't.
    #[test]
    fn a_drive_in_a_folder_denies_everything_beside_the_way_down() {
        let rules = off_drive_rules_in(
            "/Volumes/USB",
            &["/".into(), "/Volumes/Other".into()],
            Style::Unix,
        );
        let mut want = browsers::profile_rules_in("/", &["Volumes"], Style::Unix).unwrap();
        want.extend(browsers::profile_rules_in("/Volumes", &["USB"], Style::Unix).unwrap());
        want.extend(browsers::whole_rules_in("/Volumes/Other", Style::Unix).unwrap());
        assert_eq!(rules, want);
        assert!(
            !rules
                .iter()
                .any(|r| r == "Read(//**)" || r == "Read(//Volumes/**)")
        );
        // A name the complement can't spell stops the way down there: no rule can hide less than it means.
        let rules = off_drive_rules_in("/Volumes/Zoë", &[], Style::Unix);
        assert_eq!(
            rules,
            browsers::profile_rules_in("/", &["Volumes"], Style::Unix).unwrap()
        );
        // On Windows the letters, and the way down to a folder mount.
        let rules = off_drive_rules_in(
            r"C:\Mounts\USB",
            &[r"C:\".into(), r"E:\".into()],
            Style::Windows,
        );
        assert!(
            rules.contains(&"Read(//e/**)".to_owned())
                && rules.contains(&"Read(//d/**)".to_owned())
        );
        assert!(!rules.iter().any(|r| r == "Read(//c/**)"));
        assert!(rules.iter().any(|r| r.starts_with("Read(//c/[")));
        assert!(rules.iter().any(|r| r.starts_with("Read(//c/Mounts/[")));
    }

    /// The settings check: what Vacancy writes passes; something Claude Code would drop without a word doesn't, and
    /// neither does a file over the cap.
    #[test]
    fn the_settings_check_refuses_what_claude_code_would_drop() {
        let ok = private_settings_in(&[]).to_string();
        assert!(check_settings(&ok).is_ok());
        for bad in [
            "[]",
            "{",
            r#"{"permissions":{"deny":["Read(//**/.ssh/**)"]},"model":"x"}"#,
            r#"{"permissions":{"deny":["Read(//**/.ssh/**)"],"defaultMode":12345}}"#,
            r#"{"permissions":{"deny":["Read(//**/.ssh/**)", 42]}}"#,
            r#"{"permissions":{"deny":["Bash(rm -rf /)"]}}"#,
            r#"{"permissions":{"deny":["Read(//a\nb)"]}}"#,
            r#"{"permissions":{"deny":[]}}"#,
            r#"{"permissions":{"allow":["Read(//**)"]}}"#,
            r#"{"permissions":{"deny":["Read(//x)"],"blockReadsOutsideWorkingDirectories":false}}"#,
        ] {
            assert!(check_settings(bad).is_err(), "{bad}");
        }
        let big = serde_json::json!({"permissions": {"deny": vec!["Read(//x/y)"; MAX_SETTINGS_BYTES / 10]}})
            .to_string();
        assert!(big.len() > MAX_SETTINGS_BYTES);
        let why = check_settings(&big).unwrap_err();
        assert!(why.contains("more than Claude Code takes"), "{why}");
    }

    /// Settings over the cap: with privacy mode off the call runs as if it were on (the caller's choice, from
    /// [`settings_too_big`]), and one that still gets here isn't started; with privacy mode on (no tools) the scan's
    /// own rules are left out and the always-hidden ones stay.
    #[test]
    fn settings_over_the_cap_fail_closed() {
        let mut huge = call();
        huge.deny = (0..40_000)
            .map(|i| format!("Read(//d/BrowserData/Profile {i}/?*)"))
            .collect();
        let drive = Access::Drive(Drive {
            root: PathBuf::from(if cfg!(windows) { r"D:\" } else { "/" }),
            others: Vec::new(),
        });
        let why = settings_too_big_in(&huge.deny, &drive, &[]).expect("over the cap");
        assert!(
            why.starts_with("Claude can't open files on this scan"),
            "{why}"
        );
        assert!(settings_too_big_in(&call().deny, &drive, &[]).is_none());
        assert!(settings_too_big_in(&huge.deny, &Access::Private, &[]).is_none());
        // Privacy mode off: nothing left out, so the launch check refuses it before anything starts.
        let inv = invocation_in(Path::new("claude"), &huge, &drive, &[], &settings_file());
        let (_, text) = inv.settings.as_ref().unwrap();
        assert!(text.len() > MAX_SETTINGS_BYTES);
        assert!(text.contains("Profile 39999"), "never truncated");
        assert!(check_settings(text).is_err());
        let work = tempfile::tempdir().unwrap();
        let cli = ClaudeCode {
            program: work.path().join("no-such-claude.exe"),
            access: drive,
            work: work.path().to_path_buf(),
        };
        let (result, _) =
            cli.call_recorded_in(&huge, &[], &Control::new(CancelToken::new()), &mut |_| {});
        match result {
            Err(Failure::Error { message }) => {
                assert!(message.contains("more than Claude Code takes"), "{message}")
            }
            other => panic!("not started: {other:?}"),
        }
        assert_eq!(
            std::fs::read_dir(work.path()).unwrap().count(),
            0,
            "no settings file was written"
        );
        // Privacy mode on: the scan's rules go, the always-hidden places' stay.
        let inv = invocation_in(
            Path::new("claude"),
            &huge,
            &Access::Private,
            &[],
            &settings_file(),
        );
        let settings = settings_of(&inv);
        let mut want = private_settings_in(&[]);
        want["permissions"]["deny"]
            .as_array_mut()
            .unwrap()
            .push(json(&own_rule()));
        assert_eq!(settings, want);
    }

    /// The rules never go on the command line, so it stays short however long the config folders' paths are and
    /// however many browser profiles a scan finds; a command over the cap isn't started at all.
    #[test]
    fn the_command_stays_short_with_long_config_paths_and_many_rules() {
        let long = |i: usize| {
            let name = format!("{}{i}", "a-very-long-folder-name-".repeat(8));
            if cfg!(windows) {
                PathBuf::from(format!(r"D:\{name}\{name}\{name}\claude-config"))
            } else {
                PathBuf::from(format!("/Volumes/{name}/{name}/{name}/claude-config"))
            }
        };
        let config: Vec<PathBuf> = (0..200).map(long).collect();
        let mut big = call();
        big.deny = (0..5_000)
            .map(|i| format!("Read(//d/BrowserData/Profile {i}/?*)"))
            .collect();
        for access in [Access::Private, Access::Read(vec![long(0)])] {
            let inv = invocation_in(
                Path::new("claude"),
                &big,
                &access,
                &config,
                &settings_file(),
            );
            assert!(
                command_units(&inv) < 2_000,
                "{access:?}: {}",
                command_units(&inv)
            );
            let (_, json) = inv.settings.as_ref().unwrap();
            assert!(json.len() > 100_000, "the rules are in the file");
            for dir in &config {
                assert!(json.contains(&rule_path(dir).unwrap()), "{dir:?}");
            }
        }
        // Over the cap: refused before anything is written or started.
        let work = tempfile::tempdir().unwrap();
        let folder = CallFolder::new(work.path()).unwrap();
        let mut huge = call();
        huge.system = "x".repeat(MAX_COMMAND_UNITS);
        let inv = invocation_in(
            Path::new("claude"),
            &huge,
            &Access::Private,
            &[],
            &folder.settings,
        );
        let control = Control::new(CancelToken::new());
        let refused = run_in(&folder, &inv, &control, &mut |_| {}).unwrap_err();
        assert!(
            refused.to_string().contains("more than Windows allows"),
            "{refused}"
        );
        assert!(!folder.settings.exists());
        drop(folder);
        assert!(std::fs::read_dir(work.path()).unwrap().next().is_none());
    }

    #[test]
    fn fixture_mode_reads_only_the_fixture_folder() {
        let root = if cfg!(windows) {
            PathBuf::from(r"D:\tmp\vacancy-fixture-1")
        } else {
            PathBuf::from("/tmp/vacancy-fixture-1")
        };
        let settings = read_settings(std::slice::from_ref(&root));
        let allow = settings["permissions"]["allow"].as_array().unwrap();
        assert_eq!(allow.len(), 1);
        if cfg!(windows) {
            assert_eq!(allow[0], "Read(//d/tmp/vacancy-fixture-1/**)");
        } else {
            assert_eq!(allow[0], "Read(//tmp/vacancy-fixture-1/**)");
        }
        let inv = invocation(
            Path::new("claude"),
            &call(),
            &Access::Read(vec![root.clone()]),
            &settings_file(),
        );
        assert_eq!(inv.args.last().unwrap(), &root.display().to_string());
    }

    #[test]
    fn rule_paths_are_posix_from_the_root() {
        use Style::{Unix, Windows};
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(rule_path_in(r"D:\tmp\fx", Windows), some("/d/tmp/fx"));
        assert_eq!(rule_path_in(r"C:\", Windows), some("/c"));
        assert_eq!(rule_path_in(r"D:\tmp\fx\", Windows), some("/d/tmp/fx"));
        assert_eq!(rule_path_in("D:/tmp/fx", Windows), some("/d/tmp/fx"));
        assert_eq!(rule_path_in("/Users/alex", Unix), some("/Users/alex"));
        assert_eq!(rule_path_in("/Users/alex/", Unix), some("/Users/alex"));
        assert_eq!(rule_path_in("/", Unix), some(""));
        // Not from a drive letter, or not from `/`: no rule spells it.
        for odd in [r"\\server\share\x", r"\\?\C:\x", r"\x", "C:x", "C:", "x"] {
            assert_eq!(rule_path_in(odd, Windows), None, "{odd}");
            assert_eq!(cover_path_in(odd, Windows), None, "{odd}");
        }
        assert_eq!(rule_path_in("x/y", Unix), None);
        if cfg!(windows) {
            assert_eq!(rule_path(Path::new(r"D:\tmp\fx")), some("/d/tmp/fx"));
        } else {
            assert_eq!(rule_path(Path::new("/Users/alex")), some("/Users/alex"));
        }
    }

    /// Every gitignore special character in a real name is escaped, so a rule names the place as it is spelled and
    /// only the wildcards the caller adds (`/**`) are wildcards. A name with a character the matcher can't spell
    /// has no literal at all, and its cover holds a `*` for each run of them.
    #[test]
    fn real_names_are_literals_in_a_rule_or_have_none() {
        for (name, literal) in [
            ("claude-config", "claude-config"),
            ("[work]", r"\[work\]"),
            ("a]b", r"a\]b"),
            ("{a,b}", r"\{a,b\}"),
            ("!x", r"\!x"),
            ("#x", r"\#x"),
            ("x ", r"x\ "),
            ("x  ", r"x\ \ "),
            ("a b", "a b"),
            ("x^z$.|+", "x^z$.|+"),
            // Parentheses need no escaping.
            ("Finance (2024)", "Finance (2024)"),
        ] {
            assert_eq!(rule_literal(name), Some(literal.to_owned()), "{name}");
            assert_eq!(rule_cover(name), literal, "{name}");
        }
        for (name, cover) in [
            ("a\nb", "a*b"),
            ("a\r\n\tb", "a*b"),
            ("a\\b", "a*b"),
            ("a*b", "a*b"),
            ("**", "*"),
            ("a*", "a*"),
            (r"x\^z", "x*^z"),
            ("a?b", "a*b"),
            ("\n", "*"),
            ("[\n] ", r"\[*\]\ "),
            ("zoë", "zo*"),
            ("caf\u{e9}\u{301}x", "caf*x"),
            ("a\u{a0}b", "a*b"),
            ("a\u{2028}b", "a*b"),
            ("\u{ff0a}x", "*x"),
            ("a\u{7f}", "a*"),
        ] {
            assert_eq!(rule_literal(name), None, "{name:?} has no literal");
            assert_eq!(rule_cover(name), cover, "{name:?}");
        }
        assert_eq!(
            rule_path_in(r"D:\AI\[work]\claude-config", Style::Windows),
            Some(r"/d/AI/\[work\]/claude-config".to_owned())
        );
        assert_eq!(
            rule_path_in(r"E:\{a,b}\x!y\#z", Style::Windows),
            Some(r"/e/\{a,b\}/x\!y/\#z".to_owned())
        );
        assert_eq!(
            rule_path_in("/Volumes/[work]/{a,b}", Style::Unix),
            Some(r"/Volumes/\[work\]/\{a,b\}".to_owned())
        );
        // A Unix backslash, a line break, a `?`: no literal, so no allow rule; the deny rule's cover hides more.
        for odd in [
            "/Volumes/a\\b/cfg",
            "/Volumes/a\nb/cfg",
            "/tmp/fx?",
            "/tmp/fx*",
        ] {
            assert_eq!(rule_path_in(odd, Style::Unix), None, "{odd:?}");
        }
        assert_eq!(
            cover_path_in("/Volumes/a\nb/[x]\\y", Style::Unix),
            Some(r"/Volumes/a*b/\[x\]*y".to_owned())
        );
    }

    /// The fixture folder's allow rule is a literal too, and a root no literal spells gets no allow rule at all, so
    /// nothing beside it is ever allowed by accident.
    #[test]
    fn allow_rules_escape_the_roots_and_skip_a_root_they_cannot_spell() {
        let root = if cfg!(windows) {
            PathBuf::from(r"D:\tmp\[fx]{1}")
        } else {
            PathBuf::from("/tmp/[fx]{1}!#")
        };
        let settings = read_settings_in(std::slice::from_ref(&root), &[]);
        let allow = settings["permissions"]["allow"].as_array().unwrap();
        if cfg!(windows) {
            assert_eq!(allow, &[json(r"Read(//d/tmp/\[fx\]\{1\}/**)")]);
        } else {
            assert_eq!(allow, &[json(r"Read(//tmp/\[fx\]\{1\}\!\#/**)")]);
        }
        let odd: Vec<PathBuf> = if cfg!(windows) {
            vec![PathBuf::from(r"D:\tmp\zoë")]
        } else {
            [
                "/tmp/fx?",
                "/tmp/fx*",
                "/tmp/fx\nname",
                "/tmp/fx\\^z",
                "/tmp/zoë",
            ]
            .iter()
            .map(PathBuf::from)
            .collect()
        };
        for root in odd {
            let settings = read_settings_in(std::slice::from_ref(&root), &[]);
            assert_eq!(
                settings["permissions"]["allow"],
                serde_json::json!([]),
                "{root:?}"
            );
            assert!(
                reads_unavailable_in(Some(&root), &[]).is_some(),
                "fixture mode refuses reads in {root:?}"
            );
            assert!(reads_unavailable_in(None, &[]).is_none());
        }
        assert!(reads_unavailable_in(Some(&root), &[]).is_none());
        let inv = invocation_in(
            Path::new("claude"),
            &call(),
            &Access::Read(vec![root.clone()]),
            &[],
            &settings_file(),
        );
        assert_eq!(
            inv.args.last().unwrap(),
            &root.display().to_string(),
            "--add-dir takes a path, not a pattern: it is never escaped"
        );
    }

    /// A moved Claude Code or Codex folder no deny rule can name makes reads unavailable, in any mode.
    #[test]
    fn a_config_folder_no_rule_can_name_makes_reads_unavailable() {
        let share = if cfg!(windows) {
            PathBuf::from(r"\\server\share\claude")
        } else {
            PathBuf::from("~")
        };
        let why = reads_unavailable_in(None, std::slice::from_ref(&share)).unwrap();
        assert!(why.contains("names, sizes, counts and dates only"), "{why}");
        let fine = if cfg!(windows) {
            PathBuf::from(r"D:\AI\[work]\cfg")
        } else {
            PathBuf::from("/Volumes/a\nb/cfg")
        };
        assert!(reads_unavailable_in(None, &[fine]).is_none());
    }

    fn json(s: &str) -> Value {
        Value::String(s.to_owned())
    }

    #[test]
    fn parses_answers_and_maps_errors() {
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"{}","structured_output":{"a":1},
            "permission_denials":[{"tool_name":"Read","tool_input":{"file_path":"C:\\Users\\alex\\.ssh\\id_rsa"}},
            {"tool_name":"Grep","tool_input":{"pattern":"x","path":"C:\\Users\\alex\\.aws"}}]}"#;
        let reply = parse(ok, "").unwrap();
        assert_eq!(reply.structured, serde_json::json!({"a": 1}));
        assert_eq!(
            reply.refused,
            vec![r"C:\Users\alex\.ssh\id_rsa", r"C:\Users\alex\.aws"]
        );
        let signed_out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#;
        assert_eq!(parse(signed_out, ""), Err(Failure::SignedOut));
        assert_eq!(parse("", "Not logged in"), Err(Failure::SignedOut));
        let limited = r#"{"type":"result","subtype":"success","is_error":true,"result":"API Error: 429 rate limit reached. Try again at 4pm."}"#;
        assert_eq!(
            parse(limited, ""),
            Err(Failure::error(
                "API Error: 429 rate limit reached. Try again at 4pm."
            ))
        );
        let budget = r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,"errors":["Reached maximum budget"]}"#;
        assert_eq!(
            parse(budget, ""),
            Err(Failure::error("Reached maximum budget"))
        );
        let no_answer =
            r#"{"type":"result","subtype":"success","is_error":false,"result":"hello"}"#;
        assert!(matches!(parse(no_answer, ""), Err(Failure::Error { .. })));
        assert!(matches!(
            parse("garbage", "boom"),
            Err(Failure::Error { message }) if message == "boom"
        ));
        assert!(matches!(parse("", ""), Err(Failure::Error { .. })));
    }

    #[test]
    fn a_missing_program_is_not_found_and_its_folder_is_removed() {
        let work = tempfile::tempdir().unwrap();
        let cli = ClaudeCode {
            program: work.path().join("no-such-claude.exe"),
            access: Access::Private,
            work: work.path().join("work"),
        };
        let result = cli.call(&call(), &Control::new(CancelToken::new()), &mut |_| {});
        assert_eq!(result, Err(Failure::NotFound));
        let left: Vec<_> = std::fs::read_dir(work.path().join("work"))
            .unwrap()
            .collect();
        assert!(
            left.is_empty(),
            "each call's folder and settings file are removed after it: {left:?}"
        );
        assert_eq!(
            signed_in(&work.path().join("no-such-claude.exe"), work.path()),
            Err(Failure::NotFound)
        );
    }
}

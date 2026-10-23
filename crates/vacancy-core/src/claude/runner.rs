//! The only place Vacancy starts Claude Code. Every call is ADR 0008's locked-down command, from an empty folder
//! Vacancy owns (made for the call and removed after it), with Claude Code's own CLAUDE.md files, auto-memory and
//! nonessential traffic turned off, and `ANTHROPIC_API_KEY` taken out so the user's subscription login is used:
//!
//! ```text
//! claude -p --setting-sources "" --strict-mcp-config --disable-slash-commands --no-session-persistence
//!        --permission-prompts none --output-format json --json-schema <schema> --system-prompt <Vacancy's>
//!        --tools "" --settings <deny rules>                                           (privacy mode on)
//!        --tools Read,Glob,Grep --settings <deny and allow rules> --add-dir <roots...>   (privacy mode off)
//! ```
//!
//! The prompt goes over standard input: `--add-dir` and `--tools` take any number of values and would swallow a
//! prompt given after them, and a command line has a length limit a prompt mustn't meet. No `--model`: the
//! account's default applies. The always-hidden places are `permissions.deny` rules Claude Code enforces (see
//! [`super::hidden`]) in both modes: with privacy mode on Claude has no tool to read with, so they are defence in
//! depth there. With privacy mode off, reads are allowed on every drive, or in fixture mode only in the fixture
//! folder: anything else would need a permission prompt, which `--permission-prompts none` refuses. In both modes
//! Claude Code expands no `@` file mention and attaches nothing (`CLAUDE_CODE_DISABLE_ATTACHMENTS`,
//! `CLAUDE_CODE_DISABLE_FILE_MENTIONS`), so a path the person types in a chat stays text.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::hidden;
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
    /// Privacy mode off: Read, Glob and Grep, anywhere under these folders (every drive's root, or in fixture mode
    /// the fixture folder), except the always-hidden places.
    Read(Vec<PathBuf>),
}

/// One request: Vacancy's system prompt, the prompt, and the JSON schema the answer must match.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub system: String,
    pub prompt: String,
    pub schema: Value,
}

/// Exactly what is started for a call: the program, its arguments, its environment changes and its input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub stdin: String,
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

/// The command for `call`, with Claude Code at `program`.
pub fn invocation(program: &Path, call: &Call, access: &Access) -> Invocation {
    invocation_in(program, call, access, hidden::config_dirs())
}

/// [`invocation`], with Claude Code's and Codex's own folders where `config` puts them.
pub fn invocation_in(
    program: &Path,
    call: &Call,
    access: &Access,
    config: &[PathBuf],
) -> Invocation {
    let mut args = locked(call);
    match access {
        Access::Private => args.extend([
            "--tools".to_owned(),
            String::new(),
            "--settings".to_owned(),
            private_settings_in(config).to_string(),
        ]),
        Access::Read(roots) => {
            args.extend([
                "--tools".to_owned(),
                "Read,Glob,Grep".to_owned(),
                "--settings".to_owned(),
                read_settings_in(roots, config).to_string(),
                "--add-dir".to_owned(),
            ]);
            args.extend(roots.iter().map(|r| r.display().to_string()));
        }
    }
    Invocation {
        program: program.to_path_buf(),
        args,
        env_set: env_set(),
        env_remove: ENV_REMOVE.iter().map(|k| (*k).to_owned()).collect(),
        stdin: call.prompt.clone(),
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
        let invocation = invocation_in(&self.program, call, &self.access, config);
        let output = match run_in_empty_folder(&self.work, &invocation, control, progress) {
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

/// Makes a new empty folder for one call under `work`, runs `invocation` there and removes the folder after.
fn run_in_empty_folder(
    work: &Path,
    invocation: &Invocation,
    control: &Control,
    progress: &mut dyn FnMut(Progress),
) -> Result<crate::tools::Output, Failure> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(work)
        .map_err(|e| Failure::error(format!("Vacancy couldn't make its working folder: {e}")))?;
    let folder = work.join(format!(
        "call-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&folder)
        .map_err(|e| Failure::error(format!("Vacancy couldn't make its working folder: {e}")))?;
    let mut command = Command::new(&invocation.program);
    command
        .args(&invocation.args)
        .current_dir(&folder)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in &invocation.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &invocation.env_set {
        command.env(key, value);
    }
    let mut slow = false;
    let output = run_contained(
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
    );
    // Vacancy's own folder, made above: it should still be empty (Claude can't write).
    let _ = std::fs::remove_dir_all(&folder);
    Ok(output)
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
    };
    let control = Control {
        cancel: CancelToken::new(),
        timeout: AUTH_TIMEOUT,
        slow_after: Duration::MAX,
    };
    let output = run_in_empty_folder(work, &invocation, &control, &mut |_| {})?;
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

    #[test]
    fn privacy_on_pins_the_exact_argv_and_env_with_no_tools_and_the_deny_rules() {
        let program = PathBuf::from("claude");
        let inv = invocation_in(&program, &call(), &Access::Private, &[]);
        let mut expected: Vec<String> = LOCKED.iter().map(|s| (*s).to_owned()).collect();
        expected.push(String::new());
        expected.push("--settings".into());
        expected.push(
            serde_json::json!({"permissions": {"deny": hidden::deny_rules_in(&[])}}).to_string(),
        );
        assert_eq!(inv.args, expected);
        assert_eq!(inv.program, program);
        env_pinned(&inv);
        // No tools, so nothing to allow; the always-hidden places are denied all the same.
        let settings = private_settings_in(&[]);
        assert!(settings["permissions"].get("allow").is_none());
        assert!(
            settings["permissions"]["deny"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d == "Read(//**/.ssh/**)")
        );
        assert!(!inv.args.iter().any(|a| a == "--add-dir"));
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
            let inv = invocation_in(Path::new("claude"), &call(), &access, config);
            let settings = &inv.args[inv.args.iter().position(|a| a == "--settings").unwrap() + 1];
            let settings: Value = serde_json::from_str(settings).unwrap();
            assert!(
                settings["permissions"]["deny"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|d| d == &rule),
                "{access:?}: {settings}"
            );
        }
    }

    #[test]
    fn privacy_off_pins_the_exact_argv_and_env_with_read_tools_and_deny_rules() {
        let program = PathBuf::from("claude");
        let roots = if cfg!(windows) {
            vec![PathBuf::from(r"C:\"), PathBuf::from(r"D:\")]
        } else {
            vec![PathBuf::from("/")]
        };
        let inv = invocation(&program, &call(), &Access::Read(roots.clone()));
        let mut expected: Vec<String> = LOCKED.iter().map(|s| (*s).to_owned()).collect();
        expected.push("Read,Glob,Grep".into());
        expected.push("--settings".into());
        expected.push(read_settings(&roots).to_string());
        expected.push("--add-dir".into());
        expected.extend(roots.iter().map(|r| r.display().to_string()));
        assert_eq!(inv.args, expected);
        env_pinned(&inv);
        let settings = read_settings(&roots);
        let allow: Vec<&str> = settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        if cfg!(windows) {
            assert_eq!(allow, ["Read(//c/**)", "Read(//d/**)"]);
        } else {
            assert_eq!(allow, ["Read(//**)"]);
        }
        let deny = settings["permissions"]["deny"].as_array().unwrap();
        assert_eq!(deny.len(), hidden::deny_rules().len());
        assert!(deny.iter().any(|d| d == "Read(//**/.ssh/**)"));
        // Only Read, Glob and Grep: never a tool that writes, runs commands or goes online.
        let tools = &inv.args[inv.args.iter().position(|a| a == "--tools").unwrap() + 1];
        assert_eq!(tools, "Read,Glob,Grep");
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
        assert!(left.is_empty(), "each call's folder is removed after it");
        assert_eq!(
            signed_in(&work.path().join("no-such-claude.exe"), work.path()),
            Err(Failure::NotFound)
        );
    }
}

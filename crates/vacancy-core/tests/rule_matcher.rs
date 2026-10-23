//! The differential test of Claude Code's permission rules (ADR 0008): every rule Vacancy builds from a real path,
//! and the always-hidden places' rules, run through the matcher Claude Code itself uses (the `ignore` package, 7.0.5
//! as Claude Code 2.1.309 bundles it, and 5.3.2), by `tools/rule-matcher.mjs` under Node. For many adversarial
//! names (line breaks, CR, tab, backslashes, `[ ] * ? { } ! # ^ $`, trailing spaces, Unicode look-alikes and other
//! forms, mixed case), in both Windows' and Unix's path styles on either system:
//!
//! - each deny rule matches its target, everything in it, and its target spelled in other case;
//! - the always-hidden places stay denied under folders with any of those names;
//! - an allow rule exists only for a root it spells exactly, and then matches inside the root and nothing beside it
//!   (a sibling one character off, a longer name, the folder above, a folder named like the root elsewhere), except
//!   a sibling spelled like the root in other case (the known limit in ADR 0008);
//! - a root no allow rule spells makes fixture mode refuse reads, and a moved folder no deny rule names makes reads
//!   unavailable.
//!
//! Verified against Claude Code 2.1.309. When Claude Code updates its matcher, recheck the model in
//! `tools/rule-matcher.mjs` (there is no runtime version check, by Zach's choice).
//!
//! Needs Node and `npm ci` (CI runs it before the Rust tests).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use vacancy_core::claude::hidden;
use vacancy_core::claude::runner::{
    self, Style, allow_rule_in, exact_char, read_settings_in, reads_unavailable_in,
};

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

struct Checks(Vec<Value>);

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
        self.0.push(json!({
            "style": style_name(style),
            "kind": kind,
            "rules": rules,
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
    let mut checks = Checks(Vec::new());
    let mut spelled = 0;
    for style in [Style::Windows, Style::Unix] {
        moved_folder_checks(style, &mut checks);
        named_place_checks(style, &mut checks);
        spelled += allow_checks(style, &mut checks);
    }
    host_checks(&mut checks);
    assert!(spelled > 100, "most roots are spelled: {spelled}");

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("checks.json");
    let count = checks.0.len();
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({ "checks": checks.0 })).unwrap(),
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
    println!("{count} checks in ignore 7.0.5 and 5.3.2, {spelled} roots spelled exactly");
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

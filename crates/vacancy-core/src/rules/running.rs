//! Which apps are running, for rules that leave running apps' caches alone. On macOS Vacancy asks Launch Services
//! through `lsappinfo`, which lists the apps in the user's session with their names and bundle ids. Nothing is
//! looked for elsewhere. If the answer can't be had, or makes no sense (a session always runs the Finder and the
//! Dock), the answer is "unknown", and the rules that depend on it propose nothing.

/// The running apps' names and bundle ids, or `None` if they couldn't be read.
#[cfg(target_os = "macos")]
pub(super) fn apps() -> Option<Vec<String>> {
    match std::process::Command::new("/usr/bin/lsappinfo")
        .arg("list")
        .output()
    {
        Ok(out) if out.status.success() => {
            Some(parse(&String::from_utf8_lossy(&out.stdout))).filter(|apps| !apps.is_empty())
        }
        _ => None,
    }
}

/// The names of the apps installed in the Applications folders (`/Applications`, its `Utilities`, the system's,
/// `~/Applications`), without `.app`.
#[cfg(target_os = "macos")]
pub(super) fn installed() -> Vec<String> {
    let mut dirs = vec![
        std::path::PathBuf::from("/Applications"),
        std::path::PathBuf::from("/Applications/Utilities"),
        std::path::PathBuf::from("/System/Applications"),
        std::path::PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(home).join("Applications"));
    }
    let mut out = Vec::new();
    for dir in dirs {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(app) = name.strip_suffix(".app") {
                out.push(app.to_owned());
            }
        }
    }
    out
}

/// Off macOS no rule asks which apps are installed.
#[cfg(not(target_os = "macos"))]
pub(super) fn installed() -> Vec<String> {
    Vec::new()
}

/// Off macOS no rule needs to know which apps run, so nothing is looked for: unknown.
#[cfg(not(target_os = "macos"))]
pub(super) fn apps() -> Option<Vec<String>> {
    None
}

/// Names and bundle ids from `lsappinfo list`, whose entries look like:
///
/// ```text
///  12) "Slack" ASN:0x0-0x2c02c:
///     bundleID="com.tinyspeck.slackmacgap"
/// ```
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only macOS asks; the tests run everywhere")
)]
pub(super) fn parse(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if !s.is_empty() && !s.starts_with('[') && !out.iter().any(|o| o == s) {
            out.push(s.to_owned());
        }
    };
    for line in listing.lines() {
        let line = line.trim();
        if let Some((number, rest)) = line.split_once(") \"")
            && !number.is_empty()
            && number.bytes().all(|b| b.is_ascii_digit())
            && let Some(name) = rest.split('"').next()
        {
            push(name);
        } else if let Some(rest) = line.strip_prefix("bundleID=\"")
            && let Some(id) = rest.split('"').next()
        {
            push(id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_and_bundle_ids() {
        let listing = r#"
 1) "Finder" ASN:0x0-0x1001:
    bundleID="com.apple.finder"
    bundle path="/System/Library/CoreServices/Finder.app"
 12) "Slack" ASN:0x0-0x2c02c:
    bundleID="com.tinyspeck.slackmacgap"
    pid = 812 type="Foreground"
 13) "loginwindow helper" ASN:0x0-0x3:
    bundleID=[ NULL ]
"#;
        assert_eq!(
            parse(listing),
            vec![
                "Finder",
                "com.apple.finder",
                "Slack",
                "com.tinyspeck.slackmacgap",
                "loginwindow helper"
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn asks_launch_services() {
        // CI's Mac has a session, but what runs in it isn't ours to promise; the answer just has to be clean.
        if let Some(apps) = apps() {
            assert!(apps.iter().all(|a| !a.is_empty()), "{apps:?}");
        }
    }
}

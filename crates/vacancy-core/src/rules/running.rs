//! Which apps are running, for rules that leave running apps' caches alone. On macOS Vacancy asks Launch Services
//! through `lsappinfo`, which lists the apps in the user's session with their names and bundle ids. Nothing is
//! looked for elsewhere. If the answer can't be had, no app counts as running; the executor (ticket 04) still skips
//! files that are busy.

/// The running apps' names and bundle ids.
#[cfg(target_os = "macos")]
pub(super) fn apps() -> Vec<String> {
    match std::process::Command::new("/usr/bin/lsappinfo")
        .arg("list")
        .output()
    {
        Ok(out) if out.status.success() => parse(&String::from_utf8_lossy(&out.stdout)),
        _ => Vec::new(),
    }
}

/// The running apps' names and bundle ids: none looked for off macOS.
#[cfg(not(target_os = "macos"))]
pub(super) fn apps() -> Vec<String> {
    Vec::new()
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
        let apps = apps();
        assert!(apps.iter().all(|a| !a.is_empty()), "{apps:?}");
    }
}

//! Fixed vendor guides, discovered only from a finite set of no-follow directories in this scan.
//! No subprocess, content read, daemon, registry or PATH fallback is involved.
use super::{GuideId, Hint, HintAction};
use crate::{Env, Platform, Snapshot};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Guide {
    id: String,
    pub guide_id: GuideId,
    action: HintAction,
    title: String,
    pub summary: String,
    intro: String,
    steps: Vec<String>,
    caution: String,
    source_label: String,
    pub source_url: String,
}

fn guides() -> &'static [Guide] {
    static GUIDES: OnceLock<Vec<Guide>> = OnceLock::new();
    GUIDES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../src/data/toolGuidance.json"))
            .expect("the compiled vendor guide catalog is valid")
    })
}

pub(super) fn guide(action: &HintAction) -> Option<&'static Guide> {
    guides().iter().find(|g| &g.action == action)
}

fn hint(action: HintAction, evidence: PathBuf) -> Hint {
    let g = guide(&action).expect("instruction variant has a fixed guide");
    let mut lines = vec![g.intro.clone()];
    lines.extend(g.steps.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)));
    lines.push(g.caution.clone());
    lines.push(format!("Reference: {}", g.source_label));
    Hint {
        id: g.id.clone(), title: g.title.clone(), why: lines.join("\n"),
        reported_bytes: None, manifest: evidence, action,
    }
}

fn present(env: &Env, snapshot: &Snapshot, path: &Path, budget: &mut usize, stop: &dyn Fn() -> bool) -> Option<bool> {
    present_with(env, snapshot, path, budget, stop, current_identity)
}

fn current_identity(env: &Env, path: &Path) -> Option<crate::fsid::Identity> {
    let scope = match &env.confined_to {
        Some(root) => Some(crate::fsid::chain(root)?.last()?.id),
        None => None,
    };
    let chain = crate::fsid::chain(path)?;
    if scope.is_some_and(|scope| !chain.iter().take(chain.len().saturating_sub(1)).any(|step| step.id == scope)) {
        return None;
    }
    chain.last().map(|step| step.id)
}

fn present_with(env: &Env, snapshot: &Snapshot, path: &Path, budget: &mut usize,
    stop: &dyn Fn() -> bool, metadata: impl FnOnce(&Env, &Path) -> Option<crate::fsid::Identity>) -> Option<bool> {
    // A conservative spelling gate runs BEFORE even the no-follow metadata walk. It may omit alias spellings;
    // identity confinement below still decides authority. A mismatched fixture Env/snapshot never probes it.
    if let Some(scope) = &env.confined_to {
        let Ok(rest) = path.strip_prefix(scope) else { return Some(false); };
        if rest.as_os_str().is_empty() || rest.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
            || scope.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Some(false);
        }
    }
    // Reject paths outside the scan before any metadata access, including a wrongly supplied host Env
    // in fixture mode. The identity chain then rejects links and any post-scan replacement.
    for root in snapshot.roots() {
        let Ok(rest) = path.strip_prefix(&root.shown) else { continue; };
        let mut at = root.dir;
        let mut missing = false;
        for part in rest.components() {
            let mut next = None;
            for child in snapshot.children(at) {
                if stop() { return None; }
                if *budget == 0 { return Some(false); }
                *budget -= 1;
                if snapshot.name(child) == part.as_os_str() { next = Some(child); break; }
            }
            let Some(child) = next else { missing = true; break; };
            at = child;
        }
        if !missing {
            if stop() { return None; }
            return Some(snapshot.dir_identity(at).is_some()
                && metadata(env, path) == snapshot.dir_identity(at));
        }
    }
    Some(false)
}

/// At most sixteen known directories, each checked between cancellation points. No recursive walk and no
/// host discovery fallback; absent/inaccessible/linked evidence simply produces no tool reminder.
pub(super) fn discover(env: &Env, snapshot: &Snapshot, stop: &dyn Fn() -> bool) -> Option<Vec<Hint>> {
    use HintAction::*;
    let mut found = Vec::new();
    // A wide scanned folder cannot cause an unbounded search for a known name.
    let mut budget = 32_768;
    let docker = match env.platform {
        Platform::Windows => vec![
            env.local_app_data.join("Docker"),
            env.app_data.join("Docker Desktop"),
            env.program_files.first().map(|p| p.join("Docker/Docker")).unwrap_or_default(),
        ],
        Platform::MacOs => vec![
            env.root.join("Applications/Docker.app"),
            env.home.join("Library/Containers/com.docker.docker"),
            env.home.join("Library/Group Containers/group.com.docker"),
        ],
    };
    let mut groups = vec![(vec![DockerImagesInstructions, DockerCacheInstructions], docker)];
    match env.platform {
        Platform::Windows => groups.push((
            vec![WindowsCleanupInstructions, WindowsDeliveryInstructions], vec![env.system_root.clone()],
        )),
        Platform::MacOs => {
            groups.push((vec![XcodeInstructions], vec![
                env.root.join("Applications/Xcode.app"),
                env.home.join("Library/Developer/CoreSimulator"),
                env.home.join("Library/Developer/Xcode"),
            ]));
            groups.push((vec![HomebrewInstructions], vec![
                env.root.join("opt/homebrew/Cellar"),
                env.root.join("usr/local/Cellar"),
                env.home.join("Library/Caches/Homebrew"),
            ]));
        }
    }
    for (actions, paths) in groups {
        for path in paths {
            if stop() { return None; }
            if present(env, snapshot, &path, &mut budget, stop)? {
                for action in actions { found.push(hint(action, path.clone())); }
                break;
            }
        }
    }
    if stop() { None } else { Some(found) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, fs};

    #[test]
    fn known_name_lookup_has_a_shared_cap_and_checks_cancellation_inside_wide_folders() {
        let fx = tempfile::tempdir().unwrap();
        let root = fx.path().canonicalize().unwrap();
        for i in 0..128 { fs::create_dir(root.join(format!("entry-{i:03}"))).unwrap(); }
        let snapshot = crate::scan(&root, &crate::ScanOptions::default(), &mut |_| {}, &crate::CancelToken::new()).unwrap();
        let mut env = Env::windows(&root, "fixture");
        env.confined_to = Some(root.clone());
        let absent = root.join("never-present");
        let mut budget = 5;
        assert_eq!(present(&env, &snapshot, &absent, &mut budget, &|| false), Some(false));
        assert_eq!(budget, 0);
        let checks = Cell::new(0);
        let mut budget = 100;
        assert_eq!(present(&env, &snapshot, &absent, &mut budget, &|| {
            checks.set(checks.get() + 1); checks.get() == 5
        }), None);
        assert_eq!(checks.get(), 5);
        assert_eq!(budget, 96);
    }

    #[test]
    fn fixed_reference_catalog_cannot_be_extended_by_deserialized_url_or_command_fields() {
        for action in [HintAction::WindowsCleanupInstructions, HintAction::WindowsDeliveryInstructions,
            HintAction::DockerImagesInstructions, HintAction::DockerCacheInstructions,
            HintAction::XcodeInstructions, HintAction::HomebrewInstructions] {
            let mut serialized = serde_json::to_value(action.clone()).unwrap();
            serialized["url"] = serde_json::json!("file:///host");
            assert!(serde_json::from_value::<HintAction>(serialized).is_err());
            let url = action.external_uri().unwrap();
            assert_eq!(action.guide_id().unwrap().action(), action);
            assert!(["https://support.microsoft.com/", "https://docs.docker.com/",
                "https://developer.apple.com/", "https://docs.brew.sh/"].iter().any(|prefix| url.starts_with(prefix)));
            assert!(action.steam_uri().is_none());
        }
        assert!(HintAction::LogicInstructions.external_uri().is_none());
        assert!(HintAction::SteamUninstall { app_id: 0 }.external_uri().is_none());
        for value in [serde_json::json!("file:///host"), serde_json::json!({"guideId":"dockerImages","url":"file:///host"}),
            serde_json::json!("steamUninstall")] {
            assert!(serde_json::from_value::<GuideId>(value).is_err());
        }
    }

    #[test]
    fn mismatched_fixture_snapshot_or_parent_escape_performs_zero_metadata_queries() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let profile = root.join("profile");
        let outside = root.join("outside-profile");
        fs::create_dir_all(outside.join("Windows")).unwrap();
        fs::create_dir_all(&profile).unwrap();
        let snapshot = crate::scan(&outside, &crate::ScanOptions::default(), &mut |_| {}, &crate::CancelToken::new()).unwrap();
        let mut env = Env::windows(&outside, "fixture");
        env.confined_to = Some(profile.clone());
        let queries = Cell::new(0);
        for path in [outside.join("Windows"), profile.join("../outside-profile/Windows")] {
            let mut budget = 100;
            assert_eq!(present_with(&env, &snapshot, &path, &mut budget, &|| false, |_, _| {
                queries.set(queries.get() + 1);
                panic!("out-of-profile metadata must never be queried");
            }), Some(false));
            assert_eq!(budget, 100);
        }
        assert_eq!(queries.get(), 0);
    }
}

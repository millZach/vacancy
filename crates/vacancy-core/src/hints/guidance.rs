//! Fixed vendor guides, discovered only from a finite set of no-follow directories in this scan.
//! No subprocess, content read, daemon, registry or PATH fallback is involved.
use super::{Hint, HintAction};
use crate::{Env, Platform, Snapshot};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Guide {
    id: String,
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
            return Some(super::readable_path(env, path)
                && snapshot.dir_identity(at).is_some()
                && crate::fsid::identity(path) == snapshot.dir_identity(at));
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

//! Metadata-only advisor boundary. No production CLI is dispatched: neither currently inspected CLI provides
//! the subscription-preserving context/tool isolation ADR 0003 requires. Installed is distinct from supported.
//! The fake exercises the complete core contract without contacting a provider or reading file contents.
//!
//! Providers receive only [`MetadataPayload`]. Responses name opaque entry IDs, never executable commands or
//! paths. Validation maps those IDs back to the borrowed scan, applies the rules' Guard, and leaves every AI
//! proposal off by default. Planning and execution still apply their own independent Guard and fingerprints.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fsid;
use crate::rules::{Claims, Guarded};
use crate::{
    CancelToken, Categories, CategoryId, DirFlags, Env, Guard, ItemRef, Proposal, ProposalAction,
    ProposalSource, Proposals, Risk, Snapshot,
};

mod discovery;
pub use discovery::{
    AdvisorDiscovery, AdvisorProviderStatus, AdvisorStatus, passive_status, passive_status_in,
};

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_EXPLANATIONS: usize = 5;
const MAX_REMOVALS: usize = 100;

/// The source of an explanation, fixed by the caller rather than trusted from model output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AdvisorProvider {
    ClaudeCode,
    Codex,
    /// Synthetic fixture data only; never represented as a real provider result.
    Fixture,
}

impl AdvisorProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Fixture => "Fixture advisor",
        }
    }
}

/// Availability is deliberately separate from executable discovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum AdvisorAvailability {
    /// No executable was found in the locations searched. Custom/login-shell locations may still contain one.
    NotDetected,
    /// Fixture mode deliberately did not inspect the host installation.
    NotChecked,
    Unsupported {
        reason: String,
    },
}

/// User preferences. An installed but unsupported provider can never become enabled through settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct AdvisorSettings {
    pub enabled: bool,
    pub preferred_provider: Option<AdvisorProvider>,
}

/// A discovered executable. Discovery never launches it, reads its settings or inspects authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedAdvisor {
    pub provider: AdvisorProvider,
    pub executable: Option<PathBuf>,
    pub availability: AdvisorAvailability,
}

/// Search supplied PATH directories. The shell can supply additional login-shell directories on macOS when a
/// safe discovery strategy is agreed. This core function never runs shell startup scripts or either CLI.
pub fn detect_in(directories: &[PathBuf]) -> Vec<DetectedAdvisor> {
    [AdvisorProvider::ClaudeCode, AdvisorProvider::Codex]
        .into_iter()
        .map(|provider| {
            let stem = match provider {
                AdvisorProvider::ClaudeCode => "claude",
                AdvisorProvider::Codex => "codex",
                AdvisorProvider::Fixture => unreachable!(),
            };
            #[cfg(windows)]
            let names = [
                format!("{stem}.exe"),
                format!("{stem}.cmd"),
                format!("{stem}.bat"),
            ];
            #[cfg(not(windows))]
            let names = [stem.to_owned()];
            let executable = directories.iter().find_map(|dir| {
                names
                    .iter()
                    .map(|name| dir.join(name))
                    .find(|path| path.is_absolute() && is_executable(path))
            });
            let availability = if executable.is_some() {
                AdvisorAvailability::Unsupported {
                    reason: isolation_reason().into(),
                }
            } else {
                AdvisorAvailability::NotDetected
            };
            DetectedAdvisor {
                provider,
                executable,
                availability,
            }
        })
        .collect()
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn isolation_reason() -> &'static str {
    "Vacancy cannot yet enforce metadata-only access with your subscription CLI. AI is unavailable; built-in cleanup rules still work."
}

/// Hard ceilings are applied even if a caller supplies larger limits.
#[derive(Clone, Copy, Debug)]
pub struct SummaryLimits {
    pub minimum_folder_bytes: u64,
    pub max_roots: usize,
    pub max_entries: usize,
    pub max_depth: usize,
    pub max_json_bytes: usize,
}

impl Default for SummaryLimits {
    fn default() -> Self {
        Self {
            minimum_folder_bytes: 256 << 20,
            max_roots: 5,
            max_entries: 200,
            max_depth: 3,
            max_json_bytes: 32 * 1024,
        }
    }
}

/// The entire data a provider may receive. Paths above each chosen folder and file contents are absent.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataPayload {
    pub roots: Vec<String>,
    pub entries: Vec<MetadataEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: MetadataKind,
    pub bytes: u64,
    pub file_count: u64,
    pub last_used: i64,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MetadataKind {
    Folder,
    File,
}

/// Private bindings prevent a response from supplying or widening a path. Borrowing ties every ID to one scan.
#[derive(Debug)]
pub struct AdvisorInput<'a> {
    snapshot: &'a Snapshot,
    payload: MetadataPayload,
    bindings: Vec<ItemRef>,
}

impl AdvisorInput<'_> {
    pub fn payload(&self) -> &MetadataPayload {
        &self.payload
    }
    pub fn to_json(&self) -> Result<String, AdvisorError> {
        serde_json::to_string(&self.payload).map_err(|_| AdvisorError::InvalidOutput)
    }
    /// The same strict contract the parser enforces, narrowed to this request's IDs. Supplying a schema is not
    /// a trust boundary: every response is validated again locally, including its current on-disk safety.
    pub fn response_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object", "additionalProperties": false, "required": ["explanations"],
            "properties": { "explanations": {
                "type": "array", "maxItems": MAX_EXPLANATIONS,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "required": ["folderId", "summary", "remove"],
                    "properties": {
                        "folderId": { "type": "string", "enum": self.payload.roots },
                        "summary": { "type": "string", "minLength": 1, "maxLength": 2000 },
                        "remove": { "type": "array", "maxItems": MAX_REMOVALS, "items": {
                            "type": "object", "additionalProperties": false,
                            "required": ["entryId", "reason"],
                            "properties": {
                                "entryId": { "type": "string", "enum": self.payload.entries.iter().map(|entry| &entry.id).collect::<Vec<_>>() },
                                "reason": { "type": "string", "minLength": 1, "maxLength": 1000 }
                            }
                        }}
                    }
                }
            }}
        })
    }
}

fn existing_claims(snapshot: &Snapshot, proposals: &Proposals) -> Claims {
    let mut claims = Claims::new(snapshot.dir_count());
    for i in 0..proposals.list().len() {
        for &target in proposals.targets(i) {
            claims.take(snapshot, target);
        }
    }
    claims
}

/// Pick the biggest unknown folders, with no overlaps with each other or built-in proposals. All data comes
/// from the scan's metadata. The Guard reads attributes only; this function never opens a file's contents.
pub fn summarize<'a>(
    snapshot: &'a Snapshot,
    categories: &Categories,
    env: &Env,
    proposals: &Proposals,
    limits: SummaryLimits,
    cancel: &CancelToken,
) -> Result<AdvisorInput<'a>, AdvisorError> {
    let stop = || cancel.is_cancelled();
    if stop() {
        return Err(AdvisorError::Cancelled);
    }
    let guarded = Guarded::new(snapshot, categories, Guard::new(env, &env.root), &stop)
        .ok_or(AdvisorError::Cancelled)?;
    let mut claims = existing_claims(snapshot, proposals);
    // A partial listing cannot establish a folder's complete safety or size, even when the scanner entered it.
    let mut incomplete: Vec<bool> = snapshot
        .dirs()
        .map(|d| snapshot.flags(d).contains(DirFlags::INCOMPLETE))
        .collect();
    for d in snapshot.dirs().rev() {
        if stop() {
            return Err(AdvisorError::Cancelled);
        }
        if incomplete[d.index()]
            && let Some(parent) = snapshot.parent(d)
        {
            incomplete[parent.index()] = true;
        }
    }
    let mut candidates = Vec::new();
    for d in snapshot.dirs() {
        if stop() {
            return Err(AdvisorError::Cancelled);
        }
        if !incomplete[d.index()]
            && categories.category_of(d) == CategoryId::Other
            && snapshot.tree(d).bytes >= limits.minimum_folder_bytes
        {
            candidates.push(d);
        }
    }
    candidates.sort_by_key(|&d| (std::cmp::Reverse(snapshot.tree(d).bytes), d.index()));
    let mut input = AdvisorInput {
        snapshot,
        payload: MetadataPayload::default(),
        bindings: Vec::new(),
    };
    let max_roots = limits.max_roots.min(MAX_EXPLANATIONS);
    let max_entries = limits.max_entries.min(200);
    let max_json = limits.max_json_bytes.min(32 * 1024);
    if serde_json::to_vec(&input.payload).unwrap().len() > max_json {
        return Err(AdvisorError::SummaryTooSmall);
    }
    for dir in candidates {
        if stop() {
            return Err(AdvisorError::Cancelled);
        }
        if input.payload.roots.len() >= max_roots {
            input.payload.truncated = true;
            break;
        }
        let at = ItemRef::Dir(dir);
        if claims.overlaps(snapshot, at) || !guarded.allows(at, false) {
            continue;
        }
        let mut stack = vec![(at, None, 0usize)];
        let mut root_added = false;
        while let Some((at, parent, depth)) = stack.pop() {
            if stop() {
                return Err(AdvisorError::Cancelled);
            }
            if !guarded.allows(at, false) {
                continue;
            }
            if claims.overlaps(snapshot, at) {
                continue;
            }
            let (name, bytes, files, used, kind) = match at {
                ItemRef::Dir(d) => {
                    let totals = snapshot.tree(d);
                    (
                        snapshot.name(d),
                        totals.bytes,
                        totals.files,
                        totals.last_used,
                        MetadataKind::Folder,
                    )
                }
                ItemRef::File(f) => {
                    let f = snapshot.file(f);
                    (f.name, f.bytes, 1, f.last_used, MetadataKind::File)
                }
                _ => unreachable!(),
            };
            let Some(name) = name
                .to_str()
                .filter(|n| !n.is_empty() && n.len() <= 255 && !n.chars().any(char::is_control))
            else {
                continue;
            };
            let id = format!("e{}", input.bindings.len());
            let root = parent.is_none();
            let entry = MetadataEntry {
                id: id.clone(),
                parent_id: parent,
                name: name.into(),
                kind,
                bytes,
                file_count: files,
                last_used: used,
            };
            input.payload.entries.push(entry);
            if root {
                input.payload.roots.push(id.clone());
            }
            if input.payload.entries.len() > max_entries
                || serde_json::to_vec(&input.payload).unwrap().len() > max_json
            {
                input.payload.entries.pop();
                if root {
                    input.payload.roots.pop();
                }
                input.payload.truncated = true;
                break;
            }
            input.bindings.push(at);
            root_added |= root;
            if let ItemRef::Dir(d) = at {
                if depth < limits.max_depth.min(3) {
                    // Depth-first in scanner order; no file content reads and no joins from external strings.
                    // Never materialize a wide directory in full for a capped request. Unvisited siblings
                    // are reported as truncation even if some of the bounded candidates are later refused.
                    let remaining = max_entries.saturating_sub(input.bindings.len());
                    if snapshot.children(d).len() + snapshot.files(d).len() > remaining {
                        input.payload.truncated = true;
                    }
                    let children: Vec<_> = snapshot
                        .children(d)
                        .map(ItemRef::Dir)
                        .chain(snapshot.files(d).map(ItemRef::File))
                        .take(remaining)
                        .collect();
                    for child in children.into_iter().rev() {
                        stack.push((child, Some(id.clone()), depth + 1));
                    }
                } else if snapshot.children(d).len() > 0 || snapshot.files(d).len() > 0 {
                    input.payload.truncated = true;
                }
            }
        }
        if root_added {
            claims.take(snapshot, at);
        }
    }
    // `true` is one byte shorter than `false`, so marking truncation cannot exceed the tested budget.
    Ok(input)
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdvisorError {
    #[error("AI is turned off")]
    Disabled,
    #[error("the advisor is not installed")]
    NotInstalled,
    #[error("{}", isolation_reason())]
    Unsupported,
    #[error("sign in to the advisor first")]
    NotSignedIn,
    #[error("the advisor request was cancelled")]
    Cancelled,
    #[error("the advisor took too long")]
    TimedOut,
    #[error("the advisor returned invalid structured output")]
    InvalidOutput,
    #[error("the metadata budget is too small")]
    SummaryTooSmall,
    #[error("the advisor could not complete the request")]
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvisorEvent {
    Started,
    Slow,
    Finished,
}

/// Implementations must check before work and while waiting; returning after the deadline never applies output.
#[derive(Debug)]
pub struct AdvisorControl<'a> {
    pub cancel: &'a CancelToken,
    pub timeout: Duration,
    pub slow_after: Duration,
    started: Instant,
}

impl<'a> AdvisorControl<'a> {
    pub fn new(cancel: &'a CancelToken, timeout: Duration, slow_after: Duration) -> Self {
        Self {
            cancel,
            timeout,
            slow_after,
            started: Instant::now(),
        }
    }
    pub fn check(&self) -> Result<(), AdvisorError> {
        if self.cancel.is_cancelled() {
            Err(AdvisorError::Cancelled)
        } else if self.started.elapsed() >= self.timeout {
            Err(AdvisorError::TimedOut)
        } else {
            Ok(())
        }
    }
    pub fn is_slow(&self) -> bool {
        self.started.elapsed() >= self.slow_after
    }
}

pub trait Advisor: std::fmt::Debug {
    fn provider(&self) -> AdvisorProvider;
    fn respond(
        &self,
        metadata: &MetadataPayload,
        control: &AdvisorControl<'_>,
        events: &mut dyn FnMut(AdvisorEvent),
    ) -> Result<Vec<u8>, AdvisorError>;
}

/// Both CLI implementations currently fail closed. No command line, authentication or payload is dispatched.
#[derive(Clone, Copy, Debug)]
pub struct CliAdvisor {
    pub provider: AdvisorProvider,
}

impl Advisor for CliAdvisor {
    fn provider(&self) -> AdvisorProvider {
        self.provider
    }
    fn respond(
        &self,
        _metadata: &MetadataPayload,
        control: &AdvisorControl<'_>,
        _events: &mut dyn FnMut(AdvisorEvent),
    ) -> Result<Vec<u8>, AdvisorError> {
        control.check()?;
        Err(AdvisorError::Unsupported)
    }
}

/// Deliberately synthetic output. Fixture provenance is fixed and cannot impersonate a live provider.
#[derive(Clone, Debug)]
pub struct FakeAdvisor {
    pub output: Result<Vec<u8>, AdvisorError>,
    pub delay: Duration,
}

impl Advisor for FakeAdvisor {
    fn provider(&self) -> AdvisorProvider {
        AdvisorProvider::Fixture
    }
    fn respond(
        &self,
        _metadata: &MetadataPayload,
        control: &AdvisorControl<'_>,
        events: &mut dyn FnMut(AdvisorEvent),
    ) -> Result<Vec<u8>, AdvisorError> {
        let started = Instant::now();
        let mut slow_sent = false;
        loop {
            control.check()?;
            if control.is_slow() && !slow_sent {
                events(AdvisorEvent::Slow);
                slow_sent = true;
            }
            let Some(remaining) = self.delay.checked_sub(started.elapsed()) else {
                break;
            };
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
        self.output.clone()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Response {
    explanations: Vec<Explanation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Explanation {
    folder_id: String,
    summary: String,
    remove: Vec<Removal>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Removal {
    entry_id: String,
    reason: String,
}

/// Counts reasons without logging untrusted output, filenames or provider transcripts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AdvisorRejections {
    pub invalid_reference: u32,
    pub unsafe_or_changed: u32,
    pub overlapping: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderExplanation {
    pub folder_id: String,
    pub path: String,
    pub provider: AdvisorProvider,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AdvisorResult {
    pub explanations: Vec<FolderExplanation>,
    pub proposal_ids: Vec<String>,
    pub rejected: AdvisorRejections,
}

fn valid_text(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

/// Parse only the strict contract. CLI envelopes are deliberately not claimed as recorded real outputs.
/// Mutation happens only after parsing the entire bounded document; malformed output appends nothing.
pub fn apply_response(
    input: &AdvisorInput<'_>,
    bytes: &[u8],
    provider: AdvisorProvider,
    categories: &Categories,
    env: &Env,
    proposals: &mut Proposals,
    cancel: &CancelToken,
) -> Result<AdvisorResult, AdvisorError> {
    apply_with_check(input, bytes, provider, categories, env, proposals, &|| {
        if cancel.is_cancelled() {
            Err(AdvisorError::Cancelled)
        } else {
            Ok(())
        }
    })
}

fn apply_with_check(
    input: &AdvisorInput<'_>,
    bytes: &[u8],
    provider: AdvisorProvider,
    categories: &Categories,
    env: &Env,
    proposals: &mut Proposals,
    check: &dyn Fn() -> Result<(), AdvisorError>,
) -> Result<AdvisorResult, AdvisorError> {
    check()?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AdvisorError::InvalidOutput);
    }
    let response: Response =
        serde_json::from_slice(bytes).map_err(|_| AdvisorError::InvalidOutput)?;
    if response.explanations.len() > MAX_EXPLANATIONS
        || response.explanations.iter().any(|e| {
            !valid_text(&e.summary, 2000)
                || e.remove.len() > MAX_REMOVALS
                || e.remove.iter().any(|r| !valid_text(&r.reason, 1000))
        })
    {
        return Err(AdvisorError::InvalidOutput);
    }
    let snap = input.snapshot;
    let stop = || check().is_err();
    let guarded = Guarded::new(snap, categories, Guard::new(env, &env.root), &stop)
        .ok_or_else(|| check().err().unwrap_or(AdvisorError::Cancelled))?;
    let mut claims = existing_claims(snap, proposals);
    let mut result = AdvisorResult::default();
    let mut seen_roots = HashSet::new();
    let mut pending = Vec::new();
    for explanation in response.explanations {
        check()?;
        let Some(root_index) = input
            .payload
            .entries
            .iter()
            .position(|e| e.id == explanation.folder_id && e.parent_id.is_none())
        else {
            result.rejected.invalid_reference += 1;
            continue;
        };
        if !seen_roots.insert(root_index) {
            result.rejected.invalid_reference += 1;
            continue;
        }
        let ItemRef::Dir(root) = input.bindings[root_index] else {
            unreachable!();
        };
        if !guarded.allows(ItemRef::Dir(root), false) {
            result.rejected.unsafe_or_changed += 1;
            continue;
        }
        result.explanations.push(FolderExplanation {
            folder_id: explanation.folder_id.clone(),
            path: snap.path(root).to_string_lossy().into(),
            provider,
            summary: explanation.summary,
        });
        for removal in explanation.remove {
            check()?;
            let Some(index) = input
                .payload
                .entries
                .iter()
                .position(|e| e.id == removal.entry_id)
            else {
                result.rejected.invalid_reference += 1;
                continue;
            };
            let mut ancestor = input.payload.entries[index].parent_id.as_deref();
            let mut inside = false;
            while let Some(id) = ancestor {
                if id == explanation.folder_id {
                    inside = true;
                    break;
                }
                ancestor = input
                    .payload
                    .entries
                    .iter()
                    .find(|e| e.id == id)
                    .and_then(|e| e.parent_id.as_deref());
            }
            if !inside {
                result.rejected.invalid_reference += 1;
                continue;
            }
            let target = input.bindings[index];
            let (path, identity, category) = match target {
                ItemRef::Dir(d) => (
                    snap.path(d),
                    snap.dir_identity(d),
                    categories.category_of(d),
                ),
                ItemRef::File(f) => (
                    snap.file_path(f),
                    snap.file_full_identity(f),
                    categories.category_of_file(snap, f),
                ),
                _ => unreachable!(),
            };
            // The Guard's parent proof for files is supplemented with the file's own full identity.
            if identity.is_none()
                || fsid::identity(&path) != identity
                || !guarded.allows(target, false)
            {
                result.rejected.unsafe_or_changed += 1;
                continue;
            }
            if claims.overlaps(snap, target) {
                result.rejected.overlapping += 1;
                continue;
            }
            claims.take(snap, target);
            let entry = &input.payload.entries[index];
            // Entry IDs are request-local: a fresh capped summary can reuse e1 for a different file. Proposal
            // IDs live for the scan, so use the scan inventory and distinguish file and directory arenas.
            let target_id = match target {
                ItemRef::Dir(dir) => format!("d{}", dir.index()),
                ItemRef::File(file) => format!("f{}", file.index()),
                _ => unreachable!(),
            };
            let id = format!(
                "ai-{}-{}",
                match provider {
                    AdvisorProvider::ClaudeCode => "claude",
                    AdvisorProvider::Codex => "codex",
                    AdvisorProvider::Fixture => "fixture",
                },
                target_id
            );
            // Repeated requests cannot append a duplicate ID, even if the original target became ineligible.
            if proposals.position(&id).is_some() {
                result.rejected.overlapping += 1;
                continue;
            }
            pending.push((
                Proposal {
                    id: id.clone(),
                    category,
                    title: format!("Review {}", entry.name),
                    summary: "AI suggestion · review before removing".into(),
                    why: removal.reason.clone(),
                    bytes: entry.bytes,
                    item_count: entry.file_count,
                    place_count: 1,
                    risk: Risk::Review,
                    source: ProposalSource::Ai,
                    ai_note: Some(format!(
                        "Suggested by {}: {}",
                        provider.label(),
                        removal.reason
                    )),
                    action: ProposalAction::Delete,
                    regenerable: false,
                    sample_paths: vec![path.to_string_lossy().into()],
                    default_on: false,
                },
                target,
            ));
            result.proposal_ids.push(id);
        }
    }
    check()?;
    for (proposal, target) in pending {
        proposals.insert_advice(proposal, target);
    }
    Ok(result)
}

/// A request result is applied only while it is still timely and not cancelled. No production adapter currently
/// supports dispatch, so the fake is the only implementation that can reach response validation.
pub fn ask(
    advisor: &dyn Advisor,
    input: &AdvisorInput<'_>,
    categories: &Categories,
    env: &Env,
    proposals: &mut Proposals,
    control: &AdvisorControl<'_>,
    events: &mut dyn FnMut(AdvisorEvent),
) -> Result<AdvisorResult, AdvisorError> {
    control.check()?;
    events(AdvisorEvent::Started);
    let response = advisor.respond(input.payload(), control, events)?;
    control.check()?;
    let result = apply_with_check(
        input,
        &response,
        advisor.provider(),
        categories,
        env,
        proposals,
        &|| control.check(),
    )?;
    events(AdvisorEvent::Finished);
    Ok(result)
}

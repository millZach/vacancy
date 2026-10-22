//! Live Claude, through the user's own Claude Code (ADR 0008). Claude proposes; it never executes.
//!
//! - [`discovery`] finds `claude`; [`runner`] is the only place that starts it, always with the locked-down flags
//!   and environment, and parses its JSON result.
//! - [`settings`] keeps the user's choices: Claude off until turned on, privacy mode on by default.
//! - [`hidden`] is the list of places that always stay hidden from Claude, as Claude Code deny rules and as the
//!   filter on what Vacancy writes into a prompt.
//! - The five jobs ([`Job`]) each build a prompt from the scan (capped and minimized: names, sizes, counts and
//!   dates), a JSON schema, and a check of the answer ([`prepare`], then [`apply`]). Claude names only what the
//!   scan saw. Every item it adds must be a node of the scan, inside the folder asked about, and pass the Guard
//!   on both checks like a rule's target; it becomes a [`Proposal`] marked [`ProposalSource::Claude`], unticked,
//!   with risk Review. Its Tidy up layouts go through the built-in sort's checks. Anything else is dropped, with
//!   its name and why.
//!
//! A call can take minutes, so a job is prepared from the scan, the call runs without it, and the answer is
//! applied to the scan afterwards (the caller checks it is still the same scan).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Env;
use crate::map::{Overview, ScannedVolume};
use crate::rules::{Proposal, ProposalSource, Risk};
use crate::tidy::TidyProposal;

pub mod discovery;
mod fake;
pub mod hidden;
mod jobs;
pub mod runner;
pub mod settings;

pub use fake::FakeClaude;
pub use jobs::{Prepared, SYSTEM, apply, prepare, remove_items, run};
pub use runner::{Access, Call, ClaudeCode, Cli, Control, Failure, Progress, Reply};
pub use settings::Settings;

/// Where Claude is, as the panel's head shows it. A request's own states (working, slow, timed out, error) come in
/// its [`Event`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeState")]
pub enum State {
    /// No `claude` was found on this computer.
    NotFound,
    /// Claude is on, but `claude auth status` says Claude Code isn't signed in.
    SignedOut,
    /// Claude is off (the default): nothing is sent anywhere.
    Off,
    /// Claude is on and ready.
    On,
}

/// What the panel needs to draw its head, its privacy switch and the privacy screens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeStatus")]
pub struct Status {
    pub state: State,
    /// Privacy mode is on: names, sizes, counts and dates only.
    pub privacy: bool,
    /// Fixture mode: with privacy mode off, Claude can read only the fixture folder.
    pub fixture: bool,
    /// What always stays hidden, even with privacy mode off, for the privacy screens.
    pub hidden: Vec<hidden::Hidden>,
}

impl Status {
    /// The status for `settings`, with Claude Code `found` or not, and `signed_in` asked only when Claude is on.
    pub fn of(
        settings: Settings,
        found: bool,
        fixture: bool,
        signed_in: impl FnOnce() -> bool,
    ) -> Self {
        let state = if !found {
            State::NotFound
        } else if !settings.enabled {
            State::Off
        } else if signed_in() {
            State::On
        } else {
            State::SignedOut
        };
        Self {
            state,
            privacy: settings.privacy,
            fixture,
            hidden: hidden::list(),
        }
    }
}

/// What the person asks Claude to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "job", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export, rename = "ClaudeJob")]
pub enum Job {
    /// Review my plan: a second opinion on the proposals `ids` (the plan as ticked now). Flags only; adds nothing.
    ReviewPlan { ids: Vec<String> },
    /// Let Claude pick: ticks and unticks among the scan's proposals (the plan is `ids` now), and may add its own.
    PickPlan { ids: Vec<String> },
    /// Ask Claude about one proposal or folder. The whole conversation so far, the person's newest message last;
    /// nothing is kept between turns, so it is sent again each time (capped).
    Ask {
        about: About,
        messages: Vec<Message>,
    },
    /// Explain the biggest folders the rules don't recognize, with anything removable in them.
    ExplainFolders,
    /// Suggest Tidy up layouts for these sorts (`tidy-downloads`...); all of them when empty. Windows only.
    TidyLayout { ids: Vec<String> },
}

impl Job {
    pub fn kind(&self) -> JobKind {
        match self {
            Job::ReviewPlan { .. } => JobKind::ReviewPlan,
            Job::PickPlan { .. } => JobKind::PickPlan,
            Job::Ask { .. } => JobKind::Ask,
            Job::ExplainFolders => JobKind::ExplainFolders,
            Job::TidyLayout { .. } => JobKind::TidyLayout,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeJobKind")]
pub enum JobKind {
    ReviewPlan,
    PickPlan,
    Ask,
    ExplainFolders,
    TidyLayout,
}

/// What a chat is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ClaudeAbout")]
pub enum About {
    /// A proposal of the scan, by id.
    Proposal { id: String },
    /// A folder on the map, by its path as the map gives it.
    Folder { path: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeSpeaker")]
pub enum Speaker {
    Person,
    Claude,
}

/// One message of a chat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeMessage")]
pub struct Message {
    pub from: Speaker,
    pub text: String,
}

/// What an answer was based on, which every answer says: names, sizes and dates only (privacy mode on), or the
/// files Claude says it opened, and the reads Claude Code refused (privacy mode off).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeBasis")]
pub struct Basis {
    /// The privacy mode the answer was made with.
    pub privacy: bool,
    /// One line for the answer's foot.
    pub text: String,
    /// The files Claude said it opened (privacy mode off only).
    pub opened: Vec<String>,
    /// Reads Claude Code refused: an always-hidden place, or somewhere Claude may not read.
    pub refused: Vec<String>,
}

/// Something Claude suggested that Vacancy left out, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeDropped")]
pub struct Dropped {
    /// What Claude called it, or its name.
    pub name: String,
    /// Where it is, if it named a place: as Claude gave it when it isn't one the scan saw.
    pub path: String,
    pub reason: String,
}

/// A proposal Claude flagged in a review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeFlag")]
pub struct Flag {
    pub id: String,
    pub title: String,
    pub reason: String,
    /// How careful Claude thinks to be with it.
    pub risk: Risk,
}

/// What Claude said about a proposal it didn't flag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeFine")]
pub struct Fine {
    pub id: String,
    pub title: String,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeChangeKind")]
pub enum ChangeKind {
    /// Claude would tick one of the scan's proposals.
    Tick,
    /// Claude would untick one of the plan's.
    Untick,
    /// Claude added an item of its own (unticked, like all of its items).
    Add,
}

/// One change Claude's pick makes to the plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeChange")]
pub struct Change {
    pub id: String,
    pub title: String,
    pub change: ChangeKind,
    pub why: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// Claude's explanation of a big folder the rules don't recognize.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeFolderNote")]
pub struct FolderNote {
    /// The folder's path, as the map gives it.
    pub path: String,
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
    #[ts(type = "number")]
    pub files: u64,
    /// One line, for a hover card or a list.
    pub short: String,
    pub explanation: String,
    /// The ids of the proposals Claude suggested in it, now in the plan tray, unticked.
    pub suggested: Vec<String>,
}

/// What a job found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ClaudeResult")]
pub enum JobResult {
    Review {
        summary: String,
        flags: Vec<Flag>,
        fine: Vec<Fine>,
    },
    Pick {
        summary: String,
        changes: Vec<Change>,
        /// Claude's plan: the ids it would have ticked. Its own items are not in it: they start unticked.
        plan: Vec<String>,
        /// What the plan frees now, and what Claude's would.
        #[ts(type = "number")]
        bytes_before: u64,
        #[ts(type = "number")]
        bytes_after: u64,
    },
    Ask {
        reply: String,
        follow_ups: Vec<String>,
    },
    Explain {
        folders: Vec<FolderNote>,
    },
    Tidy {
        /// Claude's layouts, one per folder, beside the built-in ones (which stay in use until the person picks
        /// Claude's).
        layouts: Vec<TidyProposal>,
    },
}

/// The map and proposals again, after a job added Claude's items (or a layout): what the UI shows from now on.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeRefreshed")]
pub struct Refreshed {
    pub overview: Box<Overview>,
    pub proposals: Vec<Proposal>,
    /// The Tidy up sorts in use.
    pub tidy: Vec<TidyProposal>,
    /// The other layout of each sort Claude suggested one for.
    pub tidy_others: Vec<TidyProposal>,
}

impl Refreshed {
    pub fn of(scanned: &ScannedVolume) -> Self {
        Self {
            overview: Box::new(scanned.overview.clone()),
            proposals: scanned.proposals.list().to_vec(),
            tidy: scanned.tidy.proposals(),
            tidy_others: scanned.tidy.others(),
        }
    }
}

/// Claude's answer to a job, checked.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeAnswer")]
pub struct Answer {
    pub job: JobKind,
    pub basis: Basis,
    pub result: JobResult,
    /// What Vacancy left out of the answer, by name, with why.
    pub dropped: Vec<Dropped>,
    /// The map and proposals again, when the job changed them.
    pub refreshed: Option<Refreshed>,
}

/// What a request says as it runs, to the panel. Every event names its request.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export, rename = "ClaudeEvent")]
pub enum Event {
    /// The call has started (cancellable).
    Working { request_id: String },
    /// It is still running after 20 seconds.
    Slow { request_id: String },
    Done {
        request_id: String,
        answer: Box<Answer>,
    },
    /// Not found, signed out, timed out, cancelled, or an error with Claude's text.
    Failed {
        request_id: String,
        failure: Failure,
    },
}

/// What Claude may read for a call with privacy mode off: every drive's root, or in fixture mode, the fixture
/// folder only. `drives` are the mount points the drive list offers.
pub fn readable(env: &Env, drives: &[String]) -> Vec<PathBuf> {
    match &env.confined_to {
        Some(root) => vec![root.clone()],
        None => drives.iter().map(PathBuf::from).collect(),
    }
}

/// Whether a proposal is Claude's.
pub fn is_claude(p: &Proposal) -> bool {
    p.source == ProposalSource::Claude
}

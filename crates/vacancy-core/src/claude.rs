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
//!
//! What Claude said with privacy mode off can come from files it read, so it never goes back to Claude with privacy
//! mode on. The core owns every chat ([`Chats`], per scan): the UI sends only a chat's id and the new question. A
//! chat keeps the privacy mode it began in, and each of its turns the mode it was made in; a privacy change (or
//! Claude turned off) ends every chat, and a question to an ended chat is refused ([`Failure::ChatEnded`]), so a
//! prompt only ever holds turns made in its own mode. The scan also keeps which of Claude's items were made with
//! privacy mode off ([`PrivacyOff`]): a call with it on sends such an item by its name in the scan only, without
//! what Claude said about it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Env;
use crate::map::{Overview, ScannedVolume};
use crate::rules::{Proposal, ProposalSource, Risk};
use crate::tidy::TidyProposal;

pub mod browsers;
pub mod canary;
pub mod discovery;
mod fake;
pub mod hidden;
mod jobs;
pub mod mounts;
pub mod runner;
pub mod settings;

pub use fake::FakeClaude;
pub use jobs::{Prepared, SYSTEM, apply, prepare, remove_items, run};
pub use runner::{Access, Call, ClaudeCode, Cli, Control, Failure, Progress, Reply};
pub use settings::{Effort, Model, Settings};

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
    /// Claude is on, but asking Claude Code whether it is signed in took too long ([`Status::message`] says so).
    CheckTimedOut,
    /// Claude is on, but asking Claude Code whether it is signed in failed: [`Status::message`] says how.
    CheckFailed,
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
    /// Why the sign-in check didn't work, in words, for [`State::CheckTimedOut`] and [`State::CheckFailed`] (and
    /// [`State::NotFound`] when Claude Code went missing as it was checked).
    pub message: Option<String>,
    /// Why privacy mode off can't let Claude open files here, in words ([`runner::reads_unavailable_in`]): then
    /// every call runs as if privacy mode were on, and the panel says so. `None` when it can.
    pub reads_unavailable: Option<String>,
    /// Whether this Claude Code passed the check privacy mode off needs before Claude opens files ([`canary`]):
    /// until it has, every call runs as if privacy mode were on. Asked only with Claude on and privacy mode off.
    pub check: canary::Check,
    /// The model new questions ask for ([`Settings::model`]).
    pub model: Model,
    /// The reasoning new questions ask for ([`Settings::effort`]).
    pub effort: Effort,
}

impl Status {
    /// The status for `settings`, with Claude Code `found` or not, and `signed_in` asked only when Claude is on.
    /// A sign-in check that doesn't work keeps why: Claude Code went missing, it took too long, or an error. It is
    /// never taken for "not signed in".
    pub fn of(
        settings: Settings,
        found: bool,
        fixture: bool,
        signed_in: impl FnOnce() -> Result<bool, Failure>,
    ) -> Self {
        let (state, message) = if !found {
            (State::NotFound, None)
        } else if !settings.enabled {
            (State::Off, None)
        } else {
            match signed_in() {
                Ok(true) => (State::On, None),
                Ok(false) | Err(Failure::SignedOut) => (State::SignedOut, None),
                Err(Failure::NotFound) => (
                    State::NotFound,
                    Some(
                        "Claude Code went missing while Vacancy checked whether it's signed in."
                            .to_owned(),
                    ),
                ),
                Err(Failure::TimedOut) => (
                    State::CheckTimedOut,
                    Some(format!(
                        "Claude Code didn't answer within {} seconds when Vacancy asked whether it's signed in.",
                        runner::AUTH_TIMEOUT.as_secs()
                    )),
                ),
                Err(
                    failure @ (Failure::Cancelled | Failure::ChatEnded | Failure::Error { .. }),
                ) => (State::CheckFailed, Some(failure.to_string())),
            }
        };
        Self {
            state,
            privacy: settings.privacy,
            fixture,
            hidden: hidden::list(),
            message,
            reads_unavailable: None,
            check: canary::Check::Unchecked,
            model: settings.model,
            effort: settings.effort,
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
    /// Ask Claude the person's newest question `text` in the chat `conversation` (from [`Chats::open`]), about one
    /// item or about the whole scan. The core sends the chat's turns so far with it (capped); the UI never sends
    /// them. A chat only answers: it adds nothing to the plan.
    Ask { conversation: String, text: String },
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
    /// The whole scan: anything on the drive. Its prompt carries what the jobs get from the scan (the drive's
    /// totals, the map's categories and biggest items, the proposals, and the biggest folders the rules don't
    /// recognize), never the always-hidden places.
    Scan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speaker {
    Person,
    Claude,
}

/// One turn of a chat, as the core keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub from: Speaker,
    pub text: String,
    /// Privacy mode when the turn was made. Every turn of a chat has its chat's mode: a privacy change ends it.
    pub privacy: bool,
}

/// One chat about a proposal, a folder or the whole scan, as the core keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Chat {
    about: About,
    /// The privacy mode the chat began in. Only questions asked in it are sent in it.
    privacy: bool,
    /// The questions answered so far, each with Claude's reply, oldest first.
    turns: Vec<Message>,
    /// A privacy change, or Claude turned off, ended it: nothing more is sent in it.
    ended: bool,
}

/// The chats about one scan: the core keeps them, each by an id it gives out, so the UI sends only a chat's id and
/// the person's newest question. A chat holds the privacy mode it began in and only turns made in that mode; a
/// question in another mode, or to a chat that has ended, is refused with [`Failure::ChatEnded`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chats {
    next: u64,
    chats: HashMap<String, Chat>,
}

impl Chats {
    /// Begins a chat about `about` with privacy mode `privacy`, and gives its id.
    pub fn open(&mut self, about: About, privacy: bool) -> String {
        self.next += 1;
        let id = format!("chat-{}", self.next);
        self.chats.insert(
            id.clone(),
            Chat {
                about,
                privacy,
                turns: Vec::new(),
                ended: false,
            },
        );
        id
    }

    /// Ends every chat: privacy mode changed, or Claude was turned off.
    pub fn end_all(&mut self) {
        for chat in self.chats.values_mut() {
            chat.ended = true;
        }
    }

    /// What chat `id` is about and its turns so far, to ask a question in with privacy mode `privacy`. Refused if
    /// there is no such chat, it has ended, or it began in the other mode.
    pub(crate) fn asking(&self, id: &str, privacy: bool) -> Result<(&About, &[Message]), Failure> {
        match self.chats.get(id) {
            Some(chat)
                if !chat.ended
                    && chat.privacy == privacy
                    && chat.turns.iter().all(|t| t.privacy == privacy) =>
            {
                Ok((&chat.about, &chat.turns))
            }
            _ => Err(Failure::ChatEnded),
        }
    }

    /// The question `question`, asked in chat `id` with privacy mode `privacy`, was answered with `reply`: both
    /// join its turns. Refused, keeping nothing, if the chat ended (or its mode changed) meanwhile.
    pub(crate) fn answered(
        &mut self,
        id: &str,
        privacy: bool,
        question: String,
        reply: String,
    ) -> Result<(), Failure> {
        self.asking(id, privacy)?;
        let chat = self.chats.get_mut(id).ok_or(Failure::ChatEnded)?;
        chat.turns.push(Message {
            from: Speaker::Person,
            text: question,
            privacy,
        });
        chat.turns.push(Message {
            from: Speaker::Claude,
            text: reply,
            privacy,
        });
        Ok(())
    }
}

/// What Claude's jobs keep with one scan: its chats, and which of its items were made with privacy mode off.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnScan {
    pub chats: Chats,
    pub off: PrivacyOff,
}

/// Which of Claude's items about one scan were made with privacy mode off, from files it may have read. A call with
/// privacy mode on sends such an item by its name in the scan only, never what Claude said about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivacyOff {
    items: HashSet<String>,
}

impl PrivacyOff {
    /// Claude's item `id` was made with privacy mode off (`off`), or on.
    pub(crate) fn item(&mut self, id: &str, off: bool) {
        if off {
            self.items.insert(id.to_owned());
        } else {
            self.items.remove(id);
        }
    }

    /// Whether Claude's item `id` was made with privacy mode off.
    pub fn made(&self, id: &str) -> bool {
        self.items.contains(id)
    }
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
    /// Why Claude couldn't open files for this answer though privacy mode is off (then `privacy` is true and `text`
    /// says the same): the panel's privacy row says it too, for the scan.
    pub unavailable: Option<String>,
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
    /// Goes up by one each time the scan's map is given out again, here or by the duplicate search
    /// ([`ScannedVolume::next_revision`]): the UI shows only the newest, so a slow answer never puts back a map that
    /// a later change replaced.
    #[ts(type = "number")]
    pub revision: u64,
    pub overview: Box<Overview>,
    pub proposals: Vec<Proposal>,
    /// The Tidy up sorts in use.
    pub tidy: Vec<TidyProposal>,
    /// The other layout of each sort Claude suggested one for.
    pub tidy_others: Vec<TidyProposal>,
}

impl Refreshed {
    /// The scan's map, proposals and sorts as they are now, with the next revision.
    pub fn of(scanned: &mut ScannedVolume) -> Self {
        Self {
            revision: scanned.next_revision(),
            overview: Box::new(scanned.overview.clone()),
            proposals: scanned.proposals.list().to_vec(),
            tidy: scanned.tidy.proposals(),
            tidy_others: scanned.tidy.others(),
        }
    }
}

/// A chat question sent: its request (as [`Event`]s name it) and the chat it is in, new or not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeChatStarted")]
pub struct ChatStarted {
    pub request_id: String,
    pub conversation: String,
}

/// What an answer was made with: the model and reasoning it asked for, and the model Claude Code says answered, in
/// words ("Opus 5.5"), when it said. The panel shows "Answered by Opus 5.5 · High".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeMade")]
pub struct Made {
    pub model: Model,
    pub effort: Effort,
    pub name: Option<String>,
}

/// Claude's answer to a job, checked.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "ClaudeAnswer")]
pub struct Answer {
    pub job: JobKind,
    pub basis: Basis,
    /// Which model made it, and with what reasoning.
    pub made: Made,
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

/// What Claude may read for a call with privacy mode off about the scan of the drive at `mount_point`: that drive
/// only, or in fixture mode the fixture folder only. `drives` are the mount points the drive list offers: the
/// others are denied by name too ([`runner::off_drive_rules_in`]). `mounts` are every place a file system is mounted
/// now ([`mounts::list`], read right before the call): each is denied unless it is the drive (or in fixture mode the
/// fixture folder's own volume) or holds it ([`runner::mount_rules_in`]).
pub fn access(env: &Env, mount_point: &str, drives: &[String], mounts: &[PathBuf]) -> Access {
    match &env.confined_to {
        Some(root) => Access::Fixture {
            root: root.clone(),
            mounts: mounts.to_vec(),
        },
        None => Access::Drive(runner::Drive {
            root: PathBuf::from(mount_point),
            others: drives
                .iter()
                .filter(|d| d.as_str() != mount_point)
                .map(PathBuf::from)
                .collect(),
            mounts: mounts.to_vec(),
        }),
    }
}

/// What a call about `scanned` may read: `wanted` (from [`access`], with privacy mode off; `None` with it on), unless
/// Claude can't be kept where it may read on this scan ([`closed_on`]). Then nothing, as with privacy mode on, with
/// why for the answer and the panel. Rules are never left out to make them fit.
pub fn access_on(
    scanned: &ScannedVolume,
    env: &Env,
    wanted: Option<Access>,
) -> (Access, Option<String>) {
    match wanted {
        None => (Access::Private, None),
        Some(access) => match closed_on(scanned, env, &access) {
            Some(why) => (Access::Private, Some(why)),
            None => (access, None),
        },
    }
}

/// Why privacy mode off can't let Claude open files on `scanned` with `access`: something is mounted where no rule
/// can name it ([`runner::mounts_unavailable`]), or this scan's rules wouldn't fit in the settings Claude Code takes
/// ([`too_big_on`]). `None` when neither, or for [`Access::Private`].
pub fn closed_on(scanned: &ScannedVolume, env: &Env, access: &Access) -> Option<String> {
    runner::access_mounts_unavailable(access).or_else(|| too_big_on(scanned, env, access))
}

/// Why privacy mode off can't let Claude open files on `scanned`: the rules that keep its browser profiles and the
/// other hidden places from Claude wouldn't fit in the settings Claude Code takes ([`runner::settings_too_big`]).
/// Then the call runs as if privacy mode were on, and its answer says why. `None` when they fit, or for
/// [`Access::Private`].
pub fn too_big_on(scanned: &ScannedVolume, env: &Env, access: &Access) -> Option<String> {
    if *access == Access::Private {
        return None;
    }
    runner::settings_too_big(
        &hidden::Hider::of(&scanned.snapshot, env).deny_rules(),
        access,
    )
}

/// Whether a proposal is Claude's.
pub fn is_claude(p: &Proposal) -> bool {
    p.source == ProposalSource::Claude
}

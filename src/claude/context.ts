import { createContext, use } from 'react';
import type {
  ClaudeAbout,
  ClaudeAnswer,
  ClaudeBasis,
  ClaudeFailure,
  ClaudeFlag,
  ClaudeFolderNote,
  ClaudeStatus,
  Overview,
  TidyLayout,
  TidyProposal,
} from '../bridge';

/** The jobs started from the panel's chips. Chats are their own entries. */
export type JobKind = 'reviewPlan' | 'pickPlan' | 'explainFolders' | 'tidyLayout';

/** Where a request is: running, running long, answered, or ended without an answer (cancelled included). */
export type Phase = 'working' | 'slow' | 'done' | 'failed';

/**
 * What Let Claude pick's card offers once Claude's changes are in the plan: Keep mine or Use Claude's plan, while
 * the plan is as the answer left it (`kept` and `used` once picked). `stale`: the plan changed while Claude was
 * picking, so its answer was set aside (nothing ticked, its own items taken out again) and Pick should run again.
 */
export interface PickCard {
  /** The plan as it was sent, to put back with Keep mine. */
  before: string[];
  /** Claude's own items the pick added, to take back out with Keep mine. */
  added: string[];
  choice: 'open' | 'kept' | 'used' | 'stale';
  /** The plan's version once Claude's changes went in, and what was in it then: Keep mine is offered only while both hold. */
  version: number;
  after: string[];
  /** Keep mine is taking Claude's items back out. */
  busy: boolean;
  error: string | null;
}

/**
 * Whether a pick's card may still offer Keep mine and Use Claude's plan: `open` only for the newest pick, while the
 * plan is still as its answer left it; `replaced` by a newer pick; `changed` since (an edit, or anything else);
 * `null` once settled (kept, used or stale).
 */
export function pickOffer(c: Conversation, e: JobEntry, plan: readonly string[], planVersion: number): 'open' | 'changed' | 'replaced' | null {
  const pick = e.pick;
  if (!pick || pick.choice !== 'open') return null;
  const newest = [...c.entries].reverse().find((x): x is JobEntry => x.kind === 'job' && x.job === 'pickPlan' && x.pick !== null);
  if (newest?.id !== e.id) return 'replaced';
  const same = plan.length === pick.after.length && plan.every((id) => pick.after.includes(id));
  return planVersion === pick.version && same ? 'open' : 'changed';
}

/** One job in the panel's log: what was asked, how it went, and Claude's checked answer. */
export interface JobEntry {
  kind: 'job';
  id: string;
  job: JobKind;
  phase: Phase;
  answer: ClaudeAnswer | null;
  failure: ClaudeFailure | null;
  /** Privacy mode when it was asked: an answer keeps the mode it was made with. */
  privacy: boolean;
  /** Review: flags the person chose to keep. */
  kept: string[];
  /** Let Claude pick, once answered. */
  pick: PickCard | null;
}

export interface ChatTurn {
  from: 'person' | 'claude';
  text: string;
  /** Privacy mode when the turn was made. The core never sends a privacy-off turn with privacy mode on. */
  privacy: boolean;
  /** Claude's turns: what the answer was based on, and what to ask next. */
  basis?: ClaudeBasis;
  followUps?: string[];
}

/** A chat about one proposal or folder. */
export interface ChatEntry {
  kind: 'chat';
  id: string;
  about: ClaudeAbout;
  /** What it's about, as the person knows it. */
  name: string;
  turns: ChatTurn[];
  /** The last turn's request: null before the first, or `done` once answered. */
  phase: Phase | null;
  failure: ClaudeFailure | null;
}

export type Entry = JobEntry | ChatEntry;

/** Both layouts of a sort Claude suggested one for. */
export interface Layouts {
  builtin: TidyProposal;
  claude: TidyProposal;
  /** A layout is being picked; or why it couldn't be. */
  busy: boolean;
  error: string | null;
}

/** One drive scan's conversation with Claude. A new scan starts a new one. */
export interface Conversation {
  entries: Entry[];
  /** The chat the composer asks in, by entry id. */
  about: string | null;
  /** Each sort's layouts, once Claude has suggested its own, by sort id. */
  layouts: Record<string, Layouts>;
}

/** Which drive scan: its drive, and which of its scans. */
export interface Where {
  mountPoint: string;
  run: number;
}

/** The drive scan a job is about, and what the plan holds now (and its version). */
export interface Scope extends Where {
  plan: string[];
  planVersion: number;
  tidyIds: string[];
}

export type StatusState = { phase: 'loading' } | { phase: 'ready'; status: ClaudeStatus } | { phase: 'error'; message: string };

export interface Claude {
  status: StatusState;
  /** The latest change of a setting failed, and why. */
  settingError: string | null;
  /** Turning on: checking that Claude Code answers. */
  turningOn: boolean;
  /** Looking for Claude Code again (not found, not signed in). */
  checking: boolean;
  /** The panel is folded to its rail. */
  folded: boolean;
  setFolded: (folded: boolean) => void;
  /** The confirm before privacy mode goes off is up. */
  privacyAsk: boolean;
  /**
   * Privacy mode changed with conversations open, so they were ended: the mode now (true: on), for a short note in
   * the log until the next question. Nothing from them is sent again.
   */
  ended: boolean | null;
  turnOn: () => void;
  cancelTurnOn: () => void;
  turnOff: () => void;
  recheck: () => void;
  /** Privacy mode on now; off asks first. */
  setPrivacy: (on: boolean) => void;
  confirmPrivacyOff: () => void;
  cancelPrivacyOff: () => void;

  conversation: (scope: Where) => Conversation;
  start: (scope: Scope, job: JobKind) => void;
  /** Stops a request: a job's, or a chat's latest turn. */
  cancel: (scope: Where, entryId: string) => void;
  /** Asks about a proposal or folder: opens its chat (its first answer, if new), or asks `text` in it. */
  ask: (scope: Scope, about: ClaudeAbout, name: string, text?: string) => void;
  /** The composer stops asking about the chat it was on. */
  clearAbout: (scope: Where) => void;
  keepFlag: (scope: Where, entryId: string, id: string) => void;
  /** Let Claude pick: puts the plan back and takes Claude's items out again. */
  keepMine: (scope: Scope, entryId: string) => void;
  /** Let Claude pick: keeps Claude's changes. */
  useClaudes: (scope: Scope, entryId: string) => void;
  chooseLayout: (scope: Scope, tidyId: string, layout: TidyLayout) => void;
}

export const ClaudeContext = createContext<Claude | null>(null);

/** Claude, or null outside a `ClaudeProvider` (screens then show no Claude at all). */
export function useClaude(): Claude | null {
  return use(ClaudeContext);
}

/** Claude is turned on (signed in or not, checked or not): the panel opens by itself only then. */
export function isEnabled(state: ClaudeStatus['state']): boolean {
  return state === 'on' || state === 'signedOut' || state === 'checkTimedOut' || state === 'checkFailed';
}

/** Claude is on and signed in: jobs can run. */
export function isOn(claude: Claude | null): boolean {
  return claude?.status.phase === 'ready' && claude.status.status.state === 'on';
}

/** The latest answered job of a kind in a conversation. */
export function latest(c: Conversation, job: JobKind): JobEntry | null {
  for (let i = c.entries.length - 1; i >= 0; i--) {
    const e = c.entries[i]!;
    if (e.kind === 'job' && e.job === job && e.phase === 'done' && e.answer) return e;
  }
  return null;
}

/** The flags of the latest review still worth showing on the map: in the plan, and not kept. */
export function liveFlags(c: Conversation, plan: readonly string[]): ClaudeFlag[] {
  const review = latest(c, 'reviewPlan');
  if (!review || review.answer?.result.kind !== 'review') return [];
  return review.answer.result.flags.filter((f) => plan.includes(f.id) && !review.kept.includes(f.id));
}

/** The folders Claude explained, by path, from the latest Explain big folders. */
export function explainedFolders(c: Conversation): ClaudeFolderNote[] {
  const explain = latest(c, 'explainFolders');
  return explain?.answer?.result.kind === 'explain' ? explain.answer.result.folders : [];
}

/** Claude's own note on one of its items, without the "Suggested by Claude:" its source tag already says. */
export function claudeNoteOf(note: string | null): string | null {
  if (!note) return null;
  return note.replace(/^Suggested by Claude:\s*/, '');
}

/** The map tile whose folder is at `path`, if the overview has one. */
export function tileAt(overview: Overview, path: string): string | null {
  const key = path.toLowerCase();
  for (const c of overview.categories) {
    const item = c.items.find((i) => i.path.toLowerCase() === key);
    if (item) return item.id;
  }
  return null;
}

/** Why a request gave no answer, as the panel says it. */
export function failureText(f: ClaudeFailure): string {
  switch (f.kind) {
    case 'cancelled':
      return 'Cancelled. Nothing changed.';
    case 'timedOut':
      return 'Claude Code took too long and was stopped. Nothing changed.';
    case 'notFound':
      return 'Claude Code isn’t on this PC any more. Nothing changed.';
    case 'signedOut':
      return 'Claude Code isn’t signed in. Run claude in a terminal and sign in. Nothing changed.';
    case 'error':
      return 'Claude Code stopped with an error. Nothing changed.';
  }
}

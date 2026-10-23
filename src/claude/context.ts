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
 * Whether a pick's card may still offer Keep mine and Use Claude's plan: `open` only for the newest answered pick
 * (`Conversation.pick`, the same one Keep mine checks before it puts the plan back), while the plan is still as its
 * answer left it; `replaced` by a pick answered after it; `changed` since (an edit, or anything else); `null` once
 * settled (kept, used or stale).
 */
export function pickOffer(c: Conversation, e: JobEntry, plan: readonly string[], planVersion: number): 'open' | 'changed' | 'replaced' | null {
  const pick = e.pick;
  if (!pick || pick.choice !== 'open') return null;
  if (c.pick !== e.id) return 'replaced';
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
  /**
   * Privacy mode when the turn was made, for the log. The core keeps the turns it sends, each with the mode it was
   * made in, and never sends a privacy-off turn with privacy mode on.
   */
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
  /**
   * The core's id for this chat, once its first question has gone: the UI sends only this and the newest question.
   * Null until then, and again if the core ended it (a privacy change), so the next question starts a new one.
   */
  conversation: string | null;
  turns: ChatTurn[];
  /** The last turn's request: null before the first, or `done` once answered. */
  phase: Phase | null;
  failure: ClaudeFailure | null;
}

export type Entry = JobEntry | ChatEntry;

/**
 * A chat answered before the core said which chat it is (its Done can come before its ChatStarted): a follow-up now
 * would open a new chat in the core, so none goes until the chat's id has come.
 */
export function chatStarting(e: ChatEntry): boolean {
  return e.phase === 'done' && e.conversation === null;
}

/** A job of this kind is running in the conversation: its chip and every Try again of that kind wait for it. */
export function jobRunning(c: Conversation, job: JobKind): boolean {
  return c.entries.some((e) => e.kind === 'job' && e.job === job && (e.phase === 'working' || e.phase === 'slow'));
}

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
  /**
   * The newest answered Let Claude pick, by entry id, in the order the answers came (set-aside ones included). Its
   * card alone offers Keep mine, and only its Keep mine may put the plan back. One pick runs at a time.
   */
  pick: string | null;
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
  /**
   * Checking this Claude Code before privacy mode off lets Claude open files ("Checking Claude Code…"): when privacy
   * mode goes off, at start with it off, and on Check again.
   */
  verifying: boolean;
  /** Check again: checks this Claude Code once more, even if a result is kept. */
  verify: () => void;
  /** The panel is folded to its rail. */
  folded: boolean;
  setFolded: (folded: boolean) => void;
  /** The confirm before privacy mode goes off is up. */
  privacyAsk: boolean;
  /**
   * Privacy mode is changing (the core is keeping the new mode): nothing new is asked until it has, so no request
   * starts in the mode that is ending.
   */
  privacyChanging: boolean;
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

/**
 * Claude may open files: privacy mode is off, the core can keep Claude where it may read, and this Claude Code passed
 * the check (`check`). With privacy mode off but reads unavailable (`readsUnavailable`), or the check not passed,
 * every call runs as if privacy mode were on.
 */
export function readsFiles(status: ClaudeStatus): boolean {
  return !status.privacy && status.readsUnavailable === null && status.check.kind === 'passed';
}

/**
 * Where the check of Claude Code is, for the privacy row with privacy mode off: `checking` while it runs (or is
 * about to), `failed` with why, or null once passed (or not needed).
 */
export function checkShown(status: ClaudeStatus, verifying: boolean): { kind: 'checking' } | { kind: 'failed'; message: string } | null {
  if (status.privacy || status.state !== 'on') return null;
  if (verifying || status.check.kind === 'checking') return { kind: 'checking' };
  if (status.check.kind === 'failed') return status.check;
  return null;
}

/**
 * Why Claude couldn't open files on this drive scan though privacy mode is off, from its answers: the core says so
 * per scan when the rules that keep its hidden places from Claude are too many for Claude Code's settings.
 */
export function scanUnavailable(c: Conversation): string | null {
  for (const e of c.entries) {
    const bases = e.kind === 'job' ? [e.answer?.basis] : e.turns.map((t) => t.basis);
    const why = bases.find((b) => b?.unavailable)?.unavailable;
    if (why) return why;
  }
  return null;
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
    case 'chatEnded':
      return 'This chat ended when privacy mode changed. Ask again to start a new one. Nothing changed.';
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

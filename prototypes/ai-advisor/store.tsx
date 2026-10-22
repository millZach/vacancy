// THROWAWAY: the prototype's whole state, in memory, and the fake Claude. Every Claude job is a timer with a canned
// answer: a short delay, a long one in the "slow" state, a failure in the "error" state, and Cancel.
import { createContext, useContext, useEffect, useMemo, useRef, useState } from 'react';
import type { Proposal, TidyProposal } from '../../src/bridge';
import {
  AI_ITEMS, DEFAULT_PLAN, EXPLAIN, PICK_CHANGES, REVIEW_FLAGS, REVIEW_FLAGS_OPEN, RULE_PROPOSALS, RULE_TIDY, answerTo, claudeTidy,
  firstAnswer, overviewWith, tileByPath, type FirstAnswer, type Privacy, type Subject,
} from './data';

/** The advisor: off (Claude Code found, signed in, not turned on), two reasons it can't be turned on, and on. "slow" and "error" are on, with replies that take long or fail. */
export type AiState = 'off' | 'notfound' | 'signedout' | 'on' | 'slow' | 'error';
export const AI_STATES: AiState[] = ['off', 'notfound', 'signedout', 'on', 'slow', 'error'];
export const isOn = (s: AiState) => s === 'on' || s === 'slow' || s === 'error';

export type Phase = 'idle' | 'working' | 'done' | 'failed' | 'cancelled';
export interface Job { phase: Phase; slow: boolean }
const IDLE: Job = { phase: 'idle', slow: false };

export interface Msg { from: 'you' | 'claude'; text: string; privacy: Privacy }
export interface Chat { subject: Subject; first: FirstAnswer | null; msgs: Msg[] }

export type JobKey = 'review' | 'pick' | 'explain' | 'tidy' | `chat:${string}`;

export interface State {
  ai: AiState;
  /** Privacy mode: on, Claude gets names, sizes, counts and dates; off, it can read files (never the hidden areas). Remembered. */
  privacy: Privacy;
  /** The confirm before privacy mode goes off is up. */
  privacyAsk: boolean;
  /** The privacy mode each finished job's request was sent with, by job key. */
  answered: Record<string, Privacy>;
  /** The advisor dialog: closed, open, or checking Claude Code after "Turn on Claude". */
  dialog: 'closed' | 'open' | 'checking';
  planOpen: boolean;
  plan: string[];
  /** Claude's own items now in the drive's proposals (unticked unless ticked since). */
  extra: string[];
  jobs: Record<string, Job>;
  /** Review flags Zach dismissed ("Keep it"). */
  kept: string[];
  /** The Let Claude pick diff is on screen, waiting for Accept or Keep mine. */
  pickOpen: boolean;
  pickAccepted: boolean;
  layout: Record<string, 'rule' | 'claude'>;
  chats: Record<string, Chat>;
  /** The chat on screen (A's drawer, C's card), by subject id. */
  chatOpen: string | null;
  drawer: string | null;
  focus: string | null;
  hot: string | null;
  confirm: boolean;
  ended: boolean;
  /** C: the review step is on screen. */
  step: 'map' | 'review';
  /** B: the order the conversation's entries were started in. */
  log: string[];
  /** B: the item the composer asks about. */
  about: string | null;
  /** B: the panel is folded to a rail. */
  folded: boolean;
  /** C: the explained folder whose card is open on the map. */
  card: string | null;
}

export function fresh(ai: AiState, privacy: Privacy): State {
  return {
    ai, privacy, privacyAsk: false, answered: {}, dialog: 'closed', planOpen: false, plan: [...DEFAULT_PLAN], extra: [], jobs: {}, kept: [], pickOpen: false, pickAccepted: false,
    layout: {}, chats: {}, chatOpen: null, drawer: null, focus: null, hot: null, confirm: false, ended: false, step: 'map', log: [],
    about: null, folded: false, card: null,
  };
}

/** For the proof: the reply delay, in ms. */
const proto = ((window as unknown as { __proto: { delay: number | null } }).__proto ??= { delay: null });

export function useStore(initial: AiState, privacy: Privacy) {
  const [s, setS] = useState<State>(() => fresh(initial, privacy));
  const timers = useRef(new Map<string, number[]>());
  const params = new URLSearchParams(location.search);
  const base = Number(params.get('delay') ?? 900);

  const clear = (key: string) => { for (const t of timers.current.get(key) ?? []) clearTimeout(t); timers.current.delete(key); };
  useEffect(() => () => { for (const k of [...timers.current.keys()]) clear(k); }, []);

  const set = (patch: Partial<State> | ((s: State) => Partial<State>)) => setS((x) => ({ ...x, ...(typeof patch === 'function' ? patch(x) : patch) }));
  const job = (key: string, j: Partial<Job>) => setS((x) => ({ ...x, jobs: { ...x.jobs, [key]: { ...(x.jobs[key] ?? IDLE), ...j } } }));
  const logged = (x: State, key: string) => [...x.log.filter((k) => k !== key), key];

  /** Runs a fake Claude job: working, then done (and `onDone`), failed in the error state, or long in the slow state. */
  const run = (key: JobKey, ai: AiState, onDone: (x: State) => Partial<State>, keepLog = false) => {
    clear(key);
    const pv = s.privacy;
    const delay = proto.delay ?? base;
    const long = ai === 'slow' ? Math.max(8000, delay * 6) : ai === 'error' ? Math.min(1400, delay + 400) : delay;
    setS((x) => ({ ...x, jobs: { ...x.jobs, [key]: { phase: 'working', slow: false } }, log: keepLog ? x.log : logged(x, key) }));
    const ts: number[] = [];
    if (ai === 'slow') ts.push(window.setTimeout(() => job(key, { slow: true }), Math.min(2500, long / 3)));
    ts.push(window.setTimeout(() => {
      timers.current.delete(key);
      if (ai === 'error') return job(key, { phase: 'failed', slow: false });
      setS((x) => ({ ...x, ...onDone(x), answered: { ...x.answered, [key]: pv }, jobs: { ...x.jobs, [key]: { phase: 'done', slow: false } } }));
    }, long));
    timers.current.set(key, ts);
  };

  const proposals = useMemo<Proposal[]>(() => [...RULE_PROPOSALS, ...s.extra.map((id) => AI_ITEMS[id]!).filter(Boolean)], [s.extra]);
  const overview = useMemo(() => overviewWith(s.extra.map((id) => AI_ITEMS[id]!)), [s.extra]);
  const tidy = useMemo<TidyProposal[]>(() => RULE_TIDY.map((t) => (s.layout[t.id] === 'claude' ? claudeTidy(t) : t)), [s.layout]);
  const on = isOn(s.ai);
  const jobOf = (key: string): Job => s.jobs[key] ?? IDLE;

  /** Any Claude action while AI is off opens the dialog instead. */
  const needOn = () => { if (on) return false; set({ dialog: 'open' }); return true; };

  const api = {
    s, proposals, overview, tidy, on, jobOf, set,
    setAi: (ai: AiState) => { for (const k of [...timers.current.keys()]) clear(k); set((x) => ({ ai, jobs: Object.fromEntries(Object.entries(x.jobs).map(([k, j]) => [k, j.phase === 'working' ? { phase: 'cancelled' as const, slow: false } : j])) })); },
    openDialog: () => set({ dialog: 'open' }),
    closeDialog: () => set({ dialog: 'closed' }),
    /** "Turn on Claude": a short check that Claude Code answers, then on. */
    turnOn: (after: 'open' | 'closed' = 'open') => {
      set({ dialog: 'checking' });
      clear('turnOn');
      timers.current.set('turnOn', [window.setTimeout(() => set((x) => ({ dialog: x.dialog === 'checking' ? after : x.dialog, ai: x.dialog === 'checking' ? 'on' : x.ai })), proto.delay ?? Math.max(400, base))]);
    },
    /** Privacy mode on is one click; off asks first. */
    setPrivacy: (p: Privacy) => set(p === 'on' ? { privacy: 'on', privacyAsk: false } : { privacyAsk: true }),
    confirmPrivacyOff: () => set({ privacy: 'off', privacyAsk: false }),
    cancelPrivacyOff: () => set({ privacyAsk: false }),
    cancelTurnOn: (after: 'open' | 'closed' = 'open') => { clear('turnOn'); set({ dialog: after }); },
    /** Turning off cancels what's running and forgets Claude's unaccepted answers. Items already in the tray stay, marked. */
    turnOff: () => {
      for (const k of [...timers.current.keys()]) clear(k);
      set({ ai: 'off', jobs: {}, pickOpen: false, kept: [], chats: {}, chatOpen: null, about: null, card: null, layout: {} });
    },
    togglePlan: (id: string) => set((x) => ({ plan: x.plan.includes(id) ? x.plan.filter((p) => p !== id) : [...x.plan, id] })),
    cancel: (key: string) => { clear(key); job(key, { phase: 'cancelled', slow: false }); },

    review: () => { if (needOn()) return; set({ kept: [] }); run('review', s.ai, () => ({})); },
    keepFlag: (id: string) => set((x) => ({ kept: [...x.kept, id] })),
    flags: () => (jobOf('review').phase === 'done'
      ? [...REVIEW_FLAGS, ...(s.answered.review === 'off' ? REVIEW_FLAGS_OPEN : [])].filter((f) => s.plan.includes(f.id) && !s.kept.includes(f.id))
      : []),

    pick: () => { if (needOn()) return; run('pick', s.ai, () => ({ pickOpen: true, pickAccepted: false })); },
    /** The plan Claude's pick makes, from the current plan. Its own items are added unticked. */
    pickPlan: (plan: string[]) => {
      let next = [...plan];
      for (const c of PICK_CHANGES) {
        if (c.change === 'untick') next = next.filter((id) => id !== c.id);
        if (c.change === 'tick' && !next.includes(c.id)) next.push(c.id);
      }
      return next;
    },
    acceptPick: () => set((x) => ({
      plan: api.pickPlan(x.plan),
      extra: [...new Set([...x.extra, ...PICK_CHANGES.filter((c) => c.change === 'add').map((c) => c.id)])],
      pickOpen: false, pickAccepted: true,
    })),
    discardPick: () => set({ pickOpen: false }),

    explain: () => {
      if (needOn()) return;
      run('explain', s.ai, (x) => ({ extra: [...new Set([...x.extra, ...EXPLAIN.flatMap((e) => (e.suggests ? [e.suggests] : []))])] }));
    },
    explained: () => jobOf('explain').phase === 'done',

    tidyLayouts: () => { if (needOn()) return; run('tidy', s.ai, () => ({})); },
    setLayout: (id: string, l: 'rule' | 'claude') => {
      if (l === 'claude' && jobOf('tidy').phase !== 'done') {
        if (needOn()) return;
        run('tidy', s.ai, (x) => ({ layout: { ...x.layout, [id]: 'claude' } }));
        return;
      }
      set((x) => ({ layout: { ...x.layout, [id]: l } }));
    },

    /** Opens the chat about `subject`: Claude's first answer, then follow-ups. */
    openChat: (subject: Subject, open = true) => {
      if (needOn()) return;
      const key = `chat:${subject.id}` as const;
      set((x) => ({ chatOpen: open ? subject.id : x.chatOpen, about: subject.id, log: logged(x, key), chats: x.chats[subject.id] ? x.chats : { ...x.chats, [subject.id]: { subject, first: null, msgs: [] } } }));
      const pv = s.privacy;
      if (!s.chats[subject.id]?.first) run(key, s.ai, (x) => ({ chats: { ...x.chats, [subject.id]: { ...x.chats[subject.id]!, first: firstAnswer(subject, pv) } } }), true);
    },
    ask: (id: string, question: string) => {
      const chat = s.chats[id];
      if (!chat || !question.trim()) return;
      const key = `chat:${id}` as const;
      const pv = s.privacy;
      set((x) => ({ log: logged(x, key), chats: { ...x.chats, [id]: { ...x.chats[id]!, msgs: [...x.chats[id]!.msgs, { from: 'you', text: question.trim(), privacy: pv }] } } }));
      run(key, s.ai, (x) => ({ chats: { ...x.chats, [id]: { ...x.chats[id]!, msgs: [...x.chats[id]!.msgs, { from: 'claude', text: answerTo(chat.subject, question, pv), privacy: pv }] } } }), true);
    },
  };
  return api;
}
export type Store = ReturnType<typeof useStore>;

export const StoreCtx = createContext<Store | null>(null);
export const useS = () => useContext(StoreCtx)!;

/** The subject for a proposal or a map tile. */
export function proposalSubject(p: Proposal): Subject {
  return { kind: 'proposal', id: p.id, name: p.title, proposal: p };
}
export function folderSubject(path: string): Extract<Subject, { kind: 'folder' }> | null {
  const t = tileByPath(path);
  return t ? { kind: 'folder', id: t.id, name: t.name, path: t.path, bytes: t.bytes } : null;
}

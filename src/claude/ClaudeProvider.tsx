import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  useBridge,
  type ClaudeAbout,
  type ClaudeEvent,
  type ClaudeJob,
  type ClaudeStatus,
  type TidyProposal,
} from '../bridge';
import type { ReadyDrives } from '../drives/context';
import type { DriveScan } from '../drives/session';
import { errorMessage } from '../errors';
import {
  ClaudeContext,
  isEnabled,
  pickOffer,
  type ChatEntry,
  type Claude,
  type Conversation,
  type Entry,
  type JobEntry,
  type JobKind,
  type Layouts,
  type PickCard,
  type Scope,
  type StatusState,
  type Where,
} from './context';

/** What a chat's first question asks; the panel shows it as "About <name>". */
export const FIRST_QUESTION = 'What is it, what would I lose by deleting it, and does it come back?';

/** Where the panel's open or folded choice is kept, once the person has made one. */
export const PANEL_KEY = 'vacancy.claudePanel';

function readPanel(): 'open' | 'folded' | null {
  try {
    const v = window.localStorage.getItem(PANEL_KEY);
    return v === 'open' || v === 'folded' ? v : null;
  } catch {
    return null;
  }
}

function keepPanel(v: 'open' | 'folded') {
  try {
    window.localStorage.setItem(PANEL_KEY, v);
  } catch {
    // Not kept: the panel still opens and folds, only not across launches.
  }
}

const EMPTY: Conversation = { entries: [], about: null, layouts: {} };

const keyOf = (w: Where) => `${w.mountPoint}#${w.run}`;
const sameAbout = (a: ClaudeAbout, b: ClaudeAbout) =>
  a.kind === 'proposal' ? b.kind === 'proposal' && a.id === b.id : b.kind === 'folder' && a.path === b.path;
const sameSet = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((id) => b.includes(id));

/**
 * One attempt at a request: its own token (a retry is a new attempt), its request id once the core gives it, and
 * whether Cancel came before the id did.
 */
interface Attempt {
  token: number;
  id: string | null;
  cancelled: boolean;
}

let seq = 0;

/**
 * Live Claude for everything below it (`useClaude()`), ADR 0008: Claude's status and its two switches (on/off, and
 * privacy mode with its confirm), and each drive scan's conversation in the Claude panel. Claude proposes only:
 * every answer arrives checked by the core, and what it changes (its own items, unticked; Let Claude pick's ticks;
 * a layout picked) goes into the drive's plan, which still runs only after the one confirmation. Turning Claude off,
 * or changing privacy mode, ends the conversations and keeps what is in the plan.
 */
export function ClaudeProvider({ drives, children }: { drives: ReadyDrives | null; children: ReactNode }) {
  const bridge = useBridge();
  const [status, setStatus] = useState<StatusState>({ phase: 'loading' });
  const [settingError, setSettingError] = useState<string | null>(null);
  const [turningOn, setTurningOn] = useState(false);
  const [checking, setChecking] = useState(false);
  const [panel, setPanel] = useState(readPanel);
  const [privacyAsk, setPrivacyAsk] = useState(false);
  const [ended, setEnded] = useState<boolean | null>(null);
  const [convos, setConvos] = useState<Record<string, Conversation>>({});
  const drivesRef = useRef(drives);
  useEffect(() => {
    drivesRef.current = drives;
  });
  /**
   * Each running request's attempt, by `drive#run/entry`. A request not in it, or an event or id from an attempt
   * that isn't the one in it, is no longer wanted and is ignored.
   */
  const requests = useRef(new Map<string, Attempt>());
  const turnOnToken = useRef(0);

  const ready = (s: ClaudeStatus) => {
    setStatus({ phase: 'ready', status: s });
    setSettingError(null);
  };
  const setting = (p: Promise<ClaudeStatus>) =>
    p.then(ready, (error: unknown) => setSettingError(errorMessage(error)));

  useEffect(() => {
    let live = true;
    bridge.claudeStatus().then(
      (s) => live && setStatus({ phase: 'ready', status: s }),
      (error: unknown) => live && setStatus({ phase: 'error', message: errorMessage(error) }),
    );
    return () => {
      live = false;
    };
  }, [bridge]);

  const edit = useCallback((w: Where, change: (c: Conversation) => Conversation) => {
    setConvos((all) => ({ ...all, [keyOf(w)]: change(all[keyOf(w)] ?? EMPTY) }));
  }, []);
  const editEntry = useCallback(
    <E extends Entry>(w: Where, id: string, change: (e: E) => E) =>
      edit(w, (c) => ({ ...c, entries: c.entries.map((e) => (e.id === id ? change(e as E) : e)) })),
    [edit],
  );

  const recheck = useCallback(() => {
    setChecking(true);
    bridge
      .claudeStatus()
      .then(ready, (error: unknown) => setStatus({ phase: 'error', message: errorMessage(error) }))
      .finally(() => setChecking(false));
  }, [bridge]);

  /** The attempt `token` is still the one wanted for `rkey`: not cancelled, not ended, not retried since. */
  const current = (rkey: string, token: number) => {
    const r = requests.current.get(rkey);
    return r !== undefined && r.token === token && !r.cancelled;
  };
  /** An event belongs to the wanted attempt: its token, and its request id once known. */
  const wanted = (rkey: string, token: number, event: ClaudeEvent) => {
    const r = requests.current.get(rkey);
    return current(rkey, token) && (r?.id === null || r?.id === event.requestId);
  };
  /**
   * Starts attempt `token` of a request: keeps its id for Cancel once it comes, or cancels it then if Cancel (or a
   * newer attempt) came first.
   */
  const track = (rkey: string, token: number, begin: () => Promise<string>, failed: (message: string) => void) => {
    requests.current.set(rkey, { token, id: null, cancelled: false });
    begin().then(
      (requestId) => {
        const r = requests.current.get(rkey);
        if (r && r.token === token && !r.cancelled) {
          requests.current.set(rkey, { ...r, id: requestId });
          return;
        }
        if (r?.token === token) requests.current.delete(rkey);
        void bridge.claudeCancel(requestId);
      },
      (error: unknown) => {
        if (!current(rkey, token)) return;
        requests.current.delete(rkey);
        failed(errorMessage(error));
      },
    );
  };

  /** Attempt `token` ended: forget it, and look again at Claude Code if it went missing or signed out. */
  const finished = (rkey: string, token: number, event: ClaudeEvent) => {
    if (requests.current.get(rkey)?.token === token) requests.current.delete(rkey);
    if (event.kind === 'failed' && (event.failure.kind === 'notFound' || event.failure.kind === 'signedOut')) recheck();
  };

  /** Stops every request running and forgets every conversation (turning Claude off, or privacy mode changing). */
  const endAll = () => {
    for (const r of requests.current.values()) if (r.id) void bridge.claudeCancel(r.id);
    requests.current.clear();
    const had = Object.values(convos).some((c) => c.entries.length > 0);
    setConvos({});
    return had;
  };

  const statusNow = status.phase === 'ready' ? status.status : null;

  /** The drive's scan `w` as it is now, if it is still that scan and finished. */
  const scanOf = (w: Where): Extract<DriveScan, { phase: 'done' }> | null => {
    const scan = drivesRef.current?.list.find((d) => d.volume.mountPoint === w.mountPoint)?.scan;
    return scan?.phase === 'done' && scan.run === w.run ? scan : null;
  };

  const start = (scope: Scope, job: JobKind) => {
    if (!statusNow) return;
    setEnded(null);
    const id = `job-${++seq}`;
    const token = ++seq;
    const entry: JobEntry = {
      kind: 'job', id, job, phase: 'working', answer: null, failure: null, privacy: statusNow.privacy, kept: [], pick: null,
    };
    edit(scope, (c) => ({ ...c, entries: [...c.entries, entry] }));
    const arg: ClaudeJob =
      job === 'explainFolders'
        ? { job }
        : job === 'tidyLayout'
          ? { job, ids: scope.tidyIds }
          : { job, ids: [...scope.plan] };
    const sent = scope.planVersion;
    const rkey = `${keyOf(scope)}/${id}`;
    const onEvent = (event: ClaudeEvent) => {
      if (event.kind === 'working' || !wanted(rkey, token, event)) return;
      if (event.kind === 'slow') return editEntry<JobEntry>(scope, id, (e) => (e.phase === 'working' ? { ...e, phase: 'slow' } : e));
      finished(rkey, token, event);
      if (event.kind === 'failed')
        return editEntry<JobEntry>(scope, id, (e) => (e.phase === 'done' || e.phase === 'failed' ? e : { ...e, phase: 'failed', failure: event.failure }));
      const answer = event.answer;
      const d = drivesRef.current;
      if (answer.result.kind === 'pick') return picked(scope, id, sent, answer);
      editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'done', answer, pick: null }));
      // Claude's changes go into the drive's scan: its own items (unticked) and its layouts.
      if (answer.refreshed) d?.claudeApplied(scope.mountPoint, scope.run, answer.refreshed);
      if (answer.result.kind === 'tidy') {
        const all = [...(answer.refreshed?.tidy ?? []), ...(answer.refreshed?.tidyOthers ?? []), ...answer.result.layouts];
        const layouts: Record<string, Layouts> = {};
        for (const mine of answer.result.layouts) {
          const builtin = all.find((t) => t.id === mine.id && t.layout === 'builtin') ?? currentTidy(scope, mine.id);
          const claude = all.find((t) => t.id === mine.id && t.layout === 'claude') ?? mine;
          if (builtin) layouts[mine.id] = { builtin, claude, busy: false, error: null };
        }
        edit(scope, (c) => ({ ...c, layouts: { ...c.layouts, ...layouts } }));
      }
    };
    track(
      rkey,
      token,
      () => bridge.claudeStart(scope.mountPoint, arg, onEvent),
      (message) => editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'failed', failure: { kind: 'error', message } })),
    );
  };

  /**
   * Let Claude pick answered about the plan at version `sent`. If the plan is still there, Claude's ticks go in at
   * once (never over an edit made meanwhile) with its own items unticked, and the card offers Keep mine. If it
   * changed while Claude was picking, the answer is set aside: no tick changes, and its own items are taken out.
   */
  const picked = (scope: Scope, id: string, sent: number, answer: Extract<ClaudeEvent, { kind: 'done' }>['answer']) => {
    if (answer.result.kind !== 'pick') return;
    const r = answer.result;
    const d = drivesRef.current;
    const added = r.changes.filter((c) => c.change === 'add').map((c) => c.id);
    const scan = scanOf(scope);
    const card = (p: Partial<PickCard>): PickCard => ({
      before: scan ? [...scan.plan] : [...scope.plan], added, choice: 'open', version: sent, after: [], busy: false, error: null, ...p,
    });
    if (!scan || scan.planVersion !== sent) {
      editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'done', answer, pick: card({ choice: 'stale', busy: added.length > 0 }) }));
      if (added.length === 0) return;
      const setPick = (p: Partial<PickCard>) => editEntry<JobEntry>(scope, id, (e) => (e.pick ? { ...e, pick: { ...e.pick, ...p } } : e));
      bridge.claudeRemoveItems(scope.mountPoint, added).then(
        (refreshed) => {
          drivesRef.current?.claudeApplied(scope.mountPoint, scope.run, refreshed);
          setPick({ busy: false });
        },
        (error: unknown) => {
          // Claude's items stay in the tray, unticked, like any of its suggestions; nothing is ticked.
          if (answer.refreshed) drivesRef.current?.claudeApplied(scope.mountPoint, scope.run, answer.refreshed);
          setPick({ busy: false, error: errorMessage(error) });
        },
      );
      return;
    }
    const known = (x: string) => scan.proposals.some((p) => p.id === x) || scan.tidy.some((t) => t.id === x);
    const after = [...new Set(r.plan)].filter(known);
    const version = sameSet(after, scan.plan) ? sent : sent + 1;
    editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'done', answer, pick: card({ version, after }) }));
    // Claude's changes go into the drive's scan: the ticks first (only over the plan as it was sent), then its own
    // items, unticked.
    d?.setPlan(scope.mountPoint, scope.run, r.plan, sent);
    if (answer.refreshed) d?.claudeApplied(scope.mountPoint, scope.run, answer.refreshed);
  };

  /** The drive's sort in use, for a layout answer that didn't send the built-in one again. */
  const currentTidy = (w: Where, id: string): TidyProposal | undefined => scanOf(w)?.tidy.find((t) => t.id === id && t.layout === 'builtin');

  /** Sends a chat's turns so far; Claude answers the last. */
  const send = (scope: Scope, chat: ChatEntry) => {
    const rkey = `${keyOf(scope)}/${chat.id}`;
    const token = ++seq;
    const onEvent = (event: ClaudeEvent) => {
      if (event.kind === 'working' || !wanted(rkey, token, event)) return;
      if (event.kind === 'slow')
        return editEntry<ChatEntry>(scope, chat.id, (e) => (e.phase === 'working' ? { ...e, phase: 'slow' } : e));
      finished(rkey, token, event);
      editEntry<ChatEntry>(scope, chat.id, (e) => {
        if (e.phase !== 'working' && e.phase !== 'slow') return e;
        if (event.kind === 'failed') return { ...e, phase: 'failed', failure: event.failure };
        const r = event.answer.result;
        if (r.kind !== 'ask') return { ...e, phase: 'failed', failure: { kind: 'error', message: 'Claude answered something else.' } };
        const basis = event.answer.basis;
        return {
          ...e,
          phase: 'done',
          failure: null,
          turns: [...e.turns, { from: 'claude', text: r.reply, privacy: basis.privacy, basis, followUps: r.followUps }],
        };
      });
    };
    const messages = chat.turns.map((t) => ({ from: t.from, text: t.text, privacy: t.privacy }));
    track(
      rkey,
      token,
      () => bridge.claudeChat(scope.mountPoint, chat.about, messages, onEvent),
      (message) => editEntry<ChatEntry>(scope, chat.id, (e) => ({ ...e, phase: 'failed', failure: { kind: 'error', message } })),
    );
  };

  const ask = (scope: Scope, about: ClaudeAbout, name: string, text?: string) => {
    if (!statusNow) return;
    setEnded(null);
    const privacy = statusNow.privacy;
    const c = convos[keyOf(scope)] ?? EMPTY;
    const had = c.entries.find((e): e is ChatEntry => e.kind === 'chat' && sameAbout(e.about, about));
    const busy = had?.phase === 'working' || had?.phase === 'slow';
    let chat: ChatEntry;
    let go: boolean;
    if (!had) {
      chat = { kind: 'chat', id: `chat-${++seq}`, about, name, turns: [{ from: 'person', text: FIRST_QUESTION, privacy }], phase: 'working', failure: null };
      go = true;
    } else if (text !== undefined && text.trim() && !busy) {
      chat = { ...had, turns: [...had.turns, { from: 'person', text: text.trim(), privacy }], phase: 'working', failure: null };
      go = true;
    } else if (text === undefined && had.phase === 'failed') {
      // Try again: the same turns once more.
      chat = { ...had, phase: 'working', failure: null };
      go = true;
    } else {
      chat = had;
      go = false;
    }
    edit(scope, (cv) => {
      const others = cv.entries.filter((e) => e.id !== chat.id);
      // A chat opened again comes to the bottom of the log, where the composer is.
      const entries = !had || text === undefined ? [...others, chat] : cv.entries.map((e) => (e.id === chat.id ? chat : e));
      return { ...cv, entries, about: chat.id };
    });
    if (go) send(scope, chat);
  };

  const cancel = (w: Where, entryId: string) => {
    const rkey = `${keyOf(w)}/${entryId}`;
    const r = requests.current.get(rkey);
    if (r?.id) {
      requests.current.delete(rkey);
      void bridge.claudeCancel(r.id);
    } else if (r) requests.current.set(rkey, { ...r, cancelled: true });
    editEntry<Entry>(w, entryId, (e) =>
      e.phase === 'working' || e.phase === 'slow' ? { ...e, phase: 'failed', failure: { kind: 'cancelled' } } : e,
    );
  };

  /** A pick's card, if it may still offer Keep mine and Use Claude's plan for the plan as it is now. */
  const openPick = (scope: Scope, entryId: string): JobEntry | null => {
    const c = convos[keyOf(scope)] ?? EMPTY;
    const entry = c.entries.find((e): e is JobEntry => e.id === entryId && e.kind === 'job');
    if (!entry?.pick || entry.pick.busy) return null;
    return pickOffer(c, entry, scope.plan, scope.planVersion) === 'open' ? entry : null;
  };

  const keepMine = (scope: Scope, entryId: string) => {
    const pick = openPick(scope, entryId)?.pick;
    if (!pick) return;
    const setPick = (p: Partial<PickCard>) =>
      editEntry<JobEntry>(scope, entryId, (e) => (e.pick ? { ...e, pick: { ...e.pick, ...p } } : e));
    setPick({ busy: true, error: null });
    const back = () => {
      // Only over the plan as Claude's answer left it: an edit made meanwhile stays.
      const moved = scanOf(scope)?.planVersion !== pick.version;
      drivesRef.current?.setPlan(scope.mountPoint, scope.run, pick.before, pick.version);
      setPick({ busy: false, choice: 'kept', error: moved ? 'Your plan changed meanwhile, so Vacancy left your ticks as they are.' : null });
    };
    if (pick.added.length === 0) return back();
    bridge.claudeRemoveItems(scope.mountPoint, pick.added).then(
      (refreshed) => {
        back();
        drivesRef.current?.claudeApplied(scope.mountPoint, scope.run, refreshed);
      },
      (error: unknown) => setPick({ busy: false, error: errorMessage(error) }),
    );
  };

  const chooseLayout: Claude['chooseLayout'] = (scope, tidyId, layout) => {
    const setL = (p: Partial<Layouts>) =>
      edit(scope, (c) => {
        const l = c.layouts[tidyId];
        return l ? { ...c, layouts: { ...c.layouts, [tidyId]: { ...l, ...p } } } : c;
      });
    setL({ busy: true, error: null });
    bridge.claudeChooseLayout(scope.mountPoint, tidyId, layout).then(
      (tidy) => {
        drivesRef.current?.setTidy(scope.mountPoint, scope.run, tidy);
        setL({ busy: false });
      },
      (error: unknown) => setL({ busy: false, error: errorMessage(error) }),
    );
  };

  /** Privacy mode is changing to `on`: the conversations so far end, so nothing from one mode is sent in the other. */
  const privacyChanging = (on: boolean) => {
    if (statusNow?.privacy === on) return;
    if (endAll()) setEnded(on);
  };

  // Folded while Claude is off (the map gets the room); open once it's turned on. A choice the person made wins.
  const folded = panel ? panel === 'folded' : !(statusNow && isEnabled(statusNow.state));

  const value: Claude = {
    status,
    settingError,
    turningOn,
    checking,
    folded,
    setFolded: (f) => {
      const v = f ? 'folded' : 'open';
      setPanel(v);
      keepPanel(v);
    },
    privacyAsk,
    ended,
    turnOn: () => {
      const token = ++turnOnToken.current;
      setTurningOn(true);
      setSettingError(null);
      bridge.claudeSetEnabled(true).then(
        (s) => {
          if (token !== turnOnToken.current) return;
          setTurningOn(false);
          ready(s);
        },
        (error: unknown) => {
          if (token !== turnOnToken.current) return;
          setTurningOn(false);
          setSettingError(errorMessage(error));
        },
      );
    },
    cancelTurnOn: () => {
      turnOnToken.current++;
      setTurningOn(false);
      void setting(bridge.claudeSetEnabled(false));
    },
    turnOff: () => {
      endAll();
      setEnded(null);
      setPrivacyAsk(false);
      void setting(bridge.claudeSetEnabled(false));
    },
    recheck,
    setPrivacy: (on) => {
      if (!on) return setPrivacyAsk(true);
      privacyChanging(true);
      void setting(bridge.claudeSetPrivacy(true));
    },
    confirmPrivacyOff: () => {
      setPrivacyAsk(false);
      privacyChanging(false);
      void setting(bridge.claudeSetPrivacy(false));
    },
    cancelPrivacyOff: () => setPrivacyAsk(false),
    conversation: (w) => convos[keyOf(w)] ?? EMPTY,
    start,
    cancel,
    ask,
    clearAbout: (w) => edit(w, (c) => ({ ...c, about: null })),
    keepFlag: (w, entryId, id) =>
      editEntry<JobEntry>(w, entryId, (e) => (e.kept.includes(id) ? e : { ...e, kept: [...e.kept, id] })),
    keepMine,
    useClaudes: (scope, entryId) => {
      if (!openPick(scope, entryId)) return;
      editEntry<JobEntry>(scope, entryId, (e) => (e.pick ? { ...e, pick: { ...e.pick, choice: 'used' } } : e));
    },
    chooseLayout,
  };

  return <ClaudeContext value={value}>{children}</ClaudeContext>;
}

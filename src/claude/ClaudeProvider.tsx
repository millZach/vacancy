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
import { errorMessage } from '../errors';
import {
  ClaudeContext,
  type ChatEntry,
  type Claude,
  type Conversation,
  type Entry,
  type JobEntry,
  type JobKind,
  type Layouts,
  type Scope,
  type StatusState,
  type Where,
} from './context';

/** What a chat's first question asks; the panel shows it as "About <name>". */
export const FIRST_QUESTION = 'What is it, what would I lose by deleting it, and does it come back?';

/** A request's place in `requests` once Cancel came before its id. */
const CANCEL = '\u0000cancel';

const EMPTY: Conversation = { entries: [], about: null, layouts: {} };

const keyOf = (w: Where) => `${w.mountPoint}#${w.run}`;
const sameAbout = (a: ClaudeAbout, b: ClaudeAbout) =>
  a.kind === 'proposal' ? b.kind === 'proposal' && a.id === b.id : b.kind === 'folder' && a.path === b.path;

let seq = 0;

/**
 * Live Claude for everything below it (`useClaude()`), ADR 0008: Claude's status and its two switches (on/off, and
 * privacy mode with its confirm), and each drive scan's conversation in the Claude panel. Claude proposes only:
 * every answer arrives checked by the core, and what it changes (its own items, unticked; Let Claude pick's ticks;
 * a layout picked) goes into the drive's plan, which still runs only after the one confirmation. Turning Claude off
 * clears the conversations and keeps what is in the plan.
 */
export function ClaudeProvider({ drives, children }: { drives: ReadyDrives | null; children: ReactNode }) {
  const bridge = useBridge();
  const [status, setStatus] = useState<StatusState>({ phase: 'loading' });
  const [settingError, setSettingError] = useState<string | null>(null);
  const [turningOn, setTurningOn] = useState(false);
  const [checking, setChecking] = useState(false);
  const [folded, setFolded] = useState(false);
  const [privacyAsk, setPrivacyAsk] = useState(false);
  const [convos, setConvos] = useState<Record<string, Conversation>>({});
  const drivesRef = useRef(drives);
  useEffect(() => {
    drivesRef.current = drives;
  });
  /**
   * Each running request's id, by `drive#run/entry`, for Cancel: '' until the id comes, and CANCEL when Cancel came
   * first. A request not in it is no longer wanted, and its events are ignored.
   */
  const requests = useRef(new Map<string, string>());
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

  /** A request still wanted: not cancelled, not ended, and Claude not turned off since. */
  const live = (rkey: string) => {
    const r = requests.current.get(rkey);
    return r !== undefined && r !== CANCEL;
  };
  /** Starts a request: keeps its id for Cancel once it comes, or cancels it then if Cancel came first. */
  const track = (rkey: string, begin: () => Promise<string>, failed: (message: string) => void) => {
    requests.current.set(rkey, '');
    begin().then(
      (requestId) => {
        const r = requests.current.get(rkey);
        if (r === CANCEL) {
          requests.current.delete(rkey);
          void bridge.claudeCancel(requestId);
        } else if (r === '') requests.current.set(rkey, requestId);
      },
      (error: unknown) => {
        if (!live(rkey)) return;
        requests.current.delete(rkey);
        failed(errorMessage(error));
      },
    );
  };

  /** A request ended: forget its id, and look again at Claude Code if it went missing or signed out. */
  const ended = (rkey: string, event: ClaudeEvent) => {
    requests.current.delete(rkey);
    if (event.kind === 'failed' && (event.failure.kind === 'notFound' || event.failure.kind === 'signedOut')) recheck();
  };

  const statusNow = status.phase === 'ready' ? status.status : null;

  const start = (scope: Scope, job: JobKind) => {
    if (!statusNow) return;
    const id = `job-${++seq}`;
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
    const rkey = `${keyOf(scope)}/${id}`;
    const onEvent = (event: ClaudeEvent) => {
      if (event.kind === 'working' || !live(rkey)) return;
      if (event.kind === 'slow') return editEntry<JobEntry>(scope, id, (e) => (e.phase === 'working' ? { ...e, phase: 'slow' } : e));
      ended(rkey, event);
      if (event.kind === 'failed')
        return editEntry<JobEntry>(scope, id, (e) => (e.phase === 'done' || e.phase === 'failed' ? e : { ...e, phase: 'failed', failure: event.failure }));
      const answer = event.answer;
      editEntry<JobEntry>(scope, id, (e) => {
        const pick =
          answer.result.kind === 'pick'
            ? {
                before: [...scope.plan],
                added: answer.result.changes.filter((c) => c.change === 'add').map((c) => c.id),
                choice: 'open' as const,
                busy: false,
                error: null,
              }
            : null;
        return { ...e, phase: 'done', answer, pick };
      });
      // Claude's changes go into the drive's scan: its own items (unticked), its layouts, and a pick's ticks.
      const d = drivesRef.current;
      if (answer.refreshed) d?.claudeApplied(scope.mountPoint, scope.run, answer.refreshed);
      if (answer.result.kind === 'pick') d?.setPlan(scope.mountPoint, scope.run, answer.result.plan);
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
      () => bridge.claudeStart(scope.mountPoint, arg, onEvent),
      (message) => editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'failed', failure: { kind: 'error', message } })),
    );
  };

  /** The drive's sort in use, for a layout answer that didn't send the built-in one again. */
  const currentTidy = (w: Where, id: string): TidyProposal | undefined => {
    const scan = drivesRef.current?.list.find((d) => d.volume.mountPoint === w.mountPoint)?.scan;
    return scan?.phase === 'done' ? scan.tidy.find((t) => t.id === id && t.layout === 'builtin') : undefined;
  };

  /** Sends a chat's turns so far; Claude answers the last. */
  const send = (scope: Scope, chat: ChatEntry) => {
    const rkey = `${keyOf(scope)}/${chat.id}`;
    const onEvent = (event: ClaudeEvent) => {
      if (event.kind === 'working' || !live(rkey)) return;
      if (event.kind === 'slow')
        return editEntry<ChatEntry>(scope, chat.id, (e) => (e.phase === 'working' ? { ...e, phase: 'slow' } : e));
      ended(rkey, event);
      editEntry<ChatEntry>(scope, chat.id, (e) => {
        if (e.phase !== 'working' && e.phase !== 'slow') return e;
        if (event.kind === 'failed') return { ...e, phase: 'failed', failure: event.failure };
        const r = event.answer.result;
        if (r.kind !== 'ask') return { ...e, phase: 'failed', failure: { kind: 'error', message: 'Claude answered something else.' } };
        return {
          ...e,
          phase: 'done',
          failure: null,
          turns: [...e.turns, { from: 'claude', text: r.reply, basis: event.answer.basis, followUps: r.followUps }],
        };
      });
    };
    const messages = chat.turns.map((t) => ({ from: t.from, text: t.text }));
    track(
      rkey,
      () => bridge.claudeChat(scope.mountPoint, chat.about, messages, onEvent),
      (message) => editEntry<ChatEntry>(scope, chat.id, (e) => ({ ...e, phase: 'failed', failure: { kind: 'error', message } })),
    );
  };

  const ask = (scope: Scope, about: ClaudeAbout, name: string, text?: string) => {
    if (!statusNow) return;
    const c = convos[keyOf(scope)] ?? EMPTY;
    const had = c.entries.find((e): e is ChatEntry => e.kind === 'chat' && sameAbout(e.about, about));
    const busy = had?.phase === 'working' || had?.phase === 'slow';
    let chat: ChatEntry;
    let go: boolean;
    if (!had) {
      chat = { kind: 'chat', id: `chat-${++seq}`, about, name, turns: [{ from: 'person', text: FIRST_QUESTION }], phase: 'working', failure: null };
      go = true;
    } else if (text !== undefined && text.trim() && !busy) {
      chat = { ...had, turns: [...had.turns, { from: 'person', text: text.trim() }], phase: 'working', failure: null };
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
    const requestId = requests.current.get(rkey);
    if (requestId === '') requests.current.set(rkey, CANCEL);
    else if (requestId && requestId !== CANCEL) {
      requests.current.delete(rkey);
      void bridge.claudeCancel(requestId);
    }
    editEntry<Entry>(w, entryId, (e) =>
      e.phase === 'working' || e.phase === 'slow' ? { ...e, phase: 'failed', failure: { kind: 'cancelled' } } : e,
    );
  };

  const keepMine = (scope: Scope, entryId: string) => {
    const c = convos[keyOf(scope)] ?? EMPTY;
    const entry = c.entries.find((e): e is JobEntry => e.id === entryId && e.kind === 'job');
    const pick = entry?.pick;
    if (!pick || pick.choice !== 'open' || pick.busy) return;
    const setPick = (p: Partial<NonNullable<JobEntry['pick']>>) =>
      editEntry<JobEntry>(scope, entryId, (e) => (e.pick ? { ...e, pick: { ...e.pick, ...p } } : e));
    setPick({ busy: true, error: null });
    const back = () => {
      drivesRef.current?.setPlan(scope.mountPoint, scope.run, pick.before);
      setPick({ busy: false, choice: 'kept' });
    };
    if (pick.added.length === 0) return back();
    bridge.claudeRemoveItems(scope.mountPoint, pick.added).then(
      (refreshed) => {
        drivesRef.current?.claudeApplied(scope.mountPoint, scope.run, refreshed);
        back();
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

  const value: Claude = {
    status,
    settingError,
    turningOn,
    checking,
    folded,
    setFolded,
    privacyAsk,
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
      requests.current.clear();
      setConvos({});
      setPrivacyAsk(false);
      void setting(bridge.claudeSetEnabled(false));
    },
    recheck,
    setPrivacy: (on) => {
      if (on) void setting(bridge.claudeSetPrivacy(true));
      else setPrivacyAsk(true);
    },
    confirmPrivacyOff: () => {
      setPrivacyAsk(false);
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
    useClaudes: (w, entryId) =>
      editEntry<JobEntry>(w, entryId, (e) => (e.pick?.choice === 'open' ? { ...e, pick: { ...e.pick, choice: 'used' } } : e)),
    chooseLayout,
  };

  return <ClaudeContext value={value}>{children}</ClaudeContext>;
}

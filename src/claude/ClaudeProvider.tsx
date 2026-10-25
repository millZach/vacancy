import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import {
  useBridge,
  type ClaudeAbout,
  type ClaudeAnswer,
  type ClaudeEvent,
  type ClaudeJob,
  type ClaudeModel,
  type ClaudeStatus,
  type TidyProposal,
} from '../bridge';
import type { ReadyDrives } from '../drives/context';
import type { DriveScan } from '../drives/session';
import { errorMessage } from '../errors';
import {
  ClaudeContext,
  chatStarting,
  isEnabled,
  pickOffer,
  readsFiles,
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

const EMPTY: Conversation = { entries: [], about: null, layouts: {}, pick: null };

const keyOf = (w: Where) => `${w.mountPoint}#${w.run}`;
const sameAbout = (a: ClaudeAbout, b: ClaudeAbout) =>
  a.kind === 'proposal'
    ? b.kind === 'proposal' && a.id === b.id
    : a.kind === 'folder'
      ? b.kind === 'folder' && a.path === b.path
      : b.kind === 'scan';
const sameSet = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((id) => b.includes(id));

/**
 * One attempt at a request: its own token (a retry is a new attempt), its request id once the core gives it,
 * whether Cancel came before the id did, and whether it is a Let Claude pick (one runs at a time).
 */
interface Attempt {
  token: number;
  id: string | null;
  cancelled: boolean;
  pick: boolean;
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
  const [verifying, setVerifying] = useState(false);
  const [panel, setPanel] = useState(readPanel);
  const [privacyAsk, setPrivacyAsk] = useState(false);
  const [privacyChanging, setPrivacyChanging] = useState(false);
  const [ended, setEnded] = useState<boolean | null>(null);
  const [convos, setConvos] = useState<Record<string, Conversation>>({});
  /** The model each alias last answered as, from Claude Code's own word ("Opus 5.5"). */
  const [names, setNames] = useState<Partial<Record<ClaudeModel, string>>>({});
  const drivesRef = useRef(drives);
  const statusRef = useRef(status);
  useEffect(() => {
    drivesRef.current = drives;
    statusRef.current = status;
  });
  /** Privacy mode is changing, at once (state comes a render later): no request starts until the core has the new mode. */
  const changing = useRef(false);
  /**
   * Each running request's attempt, by `drive#run/entry`. A request not in it, or an event or id from an attempt
   * that isn't the one in it, is no longer wanted and is ignored.
   */
  const requests = useRef(new Map<string, Attempt>());
  /**
   * Each chat's newest attempt, by `drive#run/entry`, apart from `requests`: an attempt that is answered before the
   * core says which chat it went in still gets the chat's id, so the next question goes in the same chat. Cancel, a
   * newer attempt (a retry, a new chat after one ended) or the end of every conversation drops it.
   */
  const chatAttempts = useRef(new Map<string, number>());
  /**
   * Each drive scan's newest answered Let Claude pick, by `drive#run`, kept the moment an answer is handled (the
   * conversations in state come a render later, and their mirror an effect after that). Only it may put a plan back.
   * The same answer sets `Conversation.pick`, which decides the card that offers Keep mine, so both name one pick.
   */
  const newestPick = useRef(new Map<string, string>());
  const turnOnToken = useRef(0);

  const ready = (s: ClaudeStatus) => {
    setStatus({ phase: 'ready', status: s });
    setSettingError(null);
  };

  /**
   * Goes up whenever Claude's mode changes (privacy mode either way, Claude on or off): a status read begun before
   * then answers for a mode that has ended, and is dropped.
   */
  const modeGen = useRef(0);
  /**
   * Changes of mode (privacy mode, Claude on or off) go to the core one at a time, in the order asked: each waits for
   * the one before to answer. So the last one's answer is always the core's word on the mode that holds.
   */
  const modeQueue = useRef<Promise<unknown>>(Promise.resolve());
  /** Changes of mode asked for and not answered yet. */
  const modesOut = useRef(0);
  /** Since the queue was last empty: the newest answer from a change the core kept, and the first refusal. */
  const modesKept = useRef<ClaudeStatus | null>(null);
  const modesRefused = useRef<string | null>(null);

  /**
   * Reads the status again and shows it only if no change of mode was asked for or kept meanwhile: the core's own
   * word, for when an answer that came in can't be trusted to be the newest.
   */
  const reread = useCallback(
    (after?: (s: ClaudeStatus) => void) => {
      const gen = modeGen.current;
      if (modesOut.current > 0) return;
      bridge.claudeStatus().then(
        (now) => {
          if (gen !== modeGen.current || modesOut.current > 0) return;
          setStatus({ phase: 'ready', status: now });
          after?.(now);
        },
        () => {
          // The row stays as it was; the next status read puts it right.
        },
      );
    },
    [bridge],
  );

  /**
   * Checks this Claude Code before privacy mode off lets Claude open files ("Checking Claude Code…"), when the check
   * is due or `again` (Check again). The newest check's answer wins. One that answers after the mode changed answers
   * for a mode that has ended, so the status is read again instead (the row may still say it is checking).
   */
  const verifyToken = useRef(0);
  const verify = useCallback(
    (again: boolean) => {
      // While a change of mode is out, nothing is asked: once the last has answered, the check runs if it's due.
      if (modesOut.current > 0) return;
      const token = ++verifyToken.current;
      const gen = modeGen.current;
      setVerifying(true);
      bridge.claudeCheck(again).then(
        (s) => {
          if (token !== verifyToken.current) return;
          setVerifying(false);
          if (gen !== modeGen.current) return reread();
          setStatus({ phase: 'ready', status: s });
        },
        (error: unknown) => {
          if (token !== verifyToken.current) return;
          setVerifying(false);
          if (gen === modeGen.current) setSettingError(errorMessage(error));
        },
      );
    },
    [bridge, reread],
  );
  /** With Claude on and privacy mode off, the check is due for this Claude Code: it runs now. */
  const checkIfDue = (s: ClaudeStatus) => {
    if (s.state === 'on' && !s.privacy && s.check.kind === 'unchecked') verify(false);
  };

  useEffect(() => {
    let live = true;
    bridge.claudeStatus().then(
      (s) => {
        if (!live) return;
        setStatus({ phase: 'ready', status: s });
        // At start with Claude on and privacy mode off, a Claude Code not checked yet (or changed since) is checked.
        if (s.state === 'on' && !s.privacy && s.check.kind === 'unchecked') verify(false);
      },
      (error: unknown) => live && setStatus({ phase: 'error', message: errorMessage(error) }),
    );
    return () => {
      live = false;
    };
  }, [bridge, verify]);

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
    const gen = modeGen.current;
    bridge
      .claudeStatus()
      .then(
        (s) => {
          if (gen !== modeGen.current) return;
          ready(s);
          // Back from not signed in (or a sign-in check that failed) with privacy mode off: Claude Code is checked
          // before Claude opens files, if it hasn't passed already.
          if (s.state === 'on' && !s.privacy && s.check.kind === 'unchecked') verify(false);
        },
        (error: unknown) => {
          if (gen === modeGen.current) setStatus({ phase: 'error', message: errorMessage(error) });
        },
      )
      .finally(() => setChecking(false));
  }, [bridge, verify]);

  const newMode = () => {
    modeGen.current++;
  };
  /**
   * Asks the core for a change of mode, after every change asked before it has answered. Until the queue is empty no
   * request starts and the panel shows none of the answers: each change the core keeps drops the status reads still
   * out (a refused one changes nothing), and once the last has answered, the newest kept answer is shown, then Claude
   * Code is checked if that is due, and the first refusal says why. Its promise settles when this change has answered.
   */
  const modeSetting = (ask: () => Promise<ClaudeStatus>) => {
    modesOut.current++;
    changing.current = true;
    setPrivacyChanging(true);
    const settled = modeQueue.current.then(ask).then(
      (s) => {
        newMode();
        modesKept.current = s;
      },
      (error: unknown) => {
        modesRefused.current ??= errorMessage(error);
      },
    );
    modeQueue.current = settled.then(() => {
      if (--modesOut.current > 0) return;
      changing.current = false;
      setPrivacyChanging(false);
      const s = modesKept.current;
      const refused = modesRefused.current;
      modesKept.current = null;
      modesRefused.current = null;
      if (s) {
        ready(s);
        // Privacy mode off, or Claude on with it off: Claude Code is checked first, if it hasn't passed already.
        checkIfDue(s);
      }
      if (refused) setSettingError(refused);
    });
    return modeQueue.current;
  };

  /**
   * A request with privacy mode off has ended: its own check of Claude Code may have run, found it changed, or kept
   * the call from starting, so the status is read again, quietly, when the row may no longer be right: after any
   * failure, while the check isn't passed, or when the answer came from names, sizes and dates though the row says
   * Claude reads files. A read that comes back after the mode changed is dropped.
   */
  const refreshAfter = (event: ClaudeEvent) => {
    const s = statusRef.current.phase === 'ready' ? statusRef.current.status : null;
    if (!s || s.state !== 'on' || s.privacy) return;
    const stale =
      event.kind === 'failed' ||
      s.check.kind !== 'passed' ||
      (event.kind === 'done' && event.answer.basis.privacy && readsFiles(s));
    if (!stale) return;
    const gen = modeGen.current;
    bridge.claudeStatus().then(
      (now) => {
        if (gen === modeGen.current) setStatus({ phase: 'ready', status: now });
      },
      () => {
        // The row stays as it was; the next status read puts it right.
      },
    );
  };

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
  const track = (rkey: string, token: number, begin: () => Promise<string>, failed: (message: string) => void, pick = false) => {
    requests.current.set(rkey, { token, id: null, cancelled: false, pick });
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

  /** An answer names the model that made it: the alias it was asked with answers by that name from now on. */
  const heard = (answer: ClaudeAnswer) => {
    const { model, name } = answer.made;
    if (name) setNames((n) => (n[model] === name ? n : { ...n, [model]: name }));
  };

  /** Attempt `token` ended: forget it, and look again at Claude Code if it went missing or signed out. */
  const finished = (rkey: string, token: number, event: ClaudeEvent) => {
    if (requests.current.get(rkey)?.token === token) requests.current.delete(rkey);
    if (event.kind === 'done') heard(event.answer);
    if (event.kind === 'failed' && (event.failure.kind === 'notFound' || event.failure.kind === 'signedOut')) recheck();
    else refreshAfter(event);
  };

  /** Stops every request running and forgets every conversation (turning Claude off, or privacy mode changing). */
  const endAll = () => {
    for (const r of requests.current.values()) if (r.id) void bridge.claudeCancel(r.id);
    requests.current.clear();
    chatAttempts.current.clear();
    newestPick.current.clear();
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

  /**
   * A Let Claude pick runs on the drive scan `w`, known at once (state comes a render later). One runs at a time, from
   * the chip or any card's Try again, so its answers come in the order they were asked.
   */
  const picking = (w: Where) => {
    for (const [rkey, r] of requests.current) if (r.pick && !r.cancelled && rkey.startsWith(`${keyOf(w)}/`)) return true;
    return false;
  };

  const start = (scope: Scope, job: JobKind) => {
    const scan = scanOf(scope);
    if (!statusNow || changing.current || !scan || (job === 'pickPlan' && picking(scope))) return;
    setEnded(null);
    const id = `job-${++seq}`;
    const token = ++seq;
    const entry: JobEntry = {
      kind: 'job', id, job, phase: 'working', answer: null, failure: null, privacy: !readsFiles(statusNow), kept: [], pick: null,
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
      () => bridge.claudeStart(scope.mountPoint, scan.scan, arg, onEvent),
      (message) => editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'failed', failure: { kind: 'error', message } })),
      job === 'pickPlan',
    );
  };

  /**
   * Let Claude pick answered about the plan at version `sent`. If the plan is still there, Claude's ticks go in at
   * once (never over an edit made meanwhile) with its own items unticked, and the card offers Keep mine. If it
   * changed while Claude was picking, the answer is set aside: no tick changes, and its own items are taken out of
   * that same scan. A scan that is gone (scanned again, or the drive went) is never touched.
   */
  const picked = (scope: Scope, id: string, sent: number, answer: Extract<ClaudeEvent, { kind: 'done' }>['answer']) => {
    if (answer.result.kind !== 'pick') return;
    // This answer's card is the newest pick from now on, at once: a Keep mine about an older one, coming back in
    // the same batch, sees it. The same answer decides the card that offers Keep mine.
    newestPick.current.set(keyOf(scope), id);
    edit(scope, (c) => ({ ...c, pick: id }));
    const r = answer.result;
    const d = drivesRef.current;
    const added = r.changes.filter((c) => c.change === 'add').map((c) => c.id);
    const scan = scanOf(scope);
    const card = (p: Partial<PickCard>): PickCard => ({
      before: scan ? [...scan.plan] : [...scope.plan], added, choice: 'open', version: sent, after: [], busy: false, error: null, ...p,
    });
    if (!scan || scan.planVersion !== sent) {
      const removing = scan !== null && added.length > 0;
      editEntry<JobEntry>(scope, id, (e) => ({ ...e, phase: 'done', answer, pick: card({ choice: 'stale', busy: removing }) }));
      if (!removing) return;
      const setPick = (p: Partial<PickCard>) => editEntry<JobEntry>(scope, id, (e) => (e.pick ? { ...e, pick: { ...e.pick, ...p } } : e));
      bridge.claudeRemoveItems(scope.mountPoint, scan.scan, added).then(
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

  /**
   * Sends the person's question `text` in `chat`: only its id in the core (none for a new chat) and the question; the
   * core keeps the turns and sends them. If the core says the chat ended (privacy mode changed as it was asked), the
   * question goes once more in a new chat that starts with it alone.
   */
  const send = (scope: Scope, chat: ChatEntry, text: string, restarted = false) => {
    const rkey = `${keyOf(scope)}/${chat.id}`;
    const token = ++seq;
    const scan = scanOf(scope);
    if (!scan) return;
    chatAttempts.current.set(rkey, token);
    const onEvent = (event: ClaudeEvent) => {
      if (event.kind === 'working' || !wanted(rkey, token, event)) return;
      if (event.kind === 'slow')
        return editEntry<ChatEntry>(scope, chat.id, (e) => (e.phase === 'working' ? { ...e, phase: 'slow' } : e));
      finished(rkey, token, event);
      if (event.kind === 'failed' && event.failure.kind === 'chatEnded' && !restarted) {
        const now = statusRef.current;
        const privacy = now.phase === 'ready' ? !readsFiles(now.status) : true;
        const fresh: ChatEntry = { ...chat, conversation: null, turns: [{ from: 'person', text, privacy }], phase: 'working', failure: null };
        editEntry<ChatEntry>(scope, chat.id, (e) => (e.phase === 'working' || e.phase === 'slow' ? fresh : e));
        return send(scope, fresh, text, true);
      }
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
          turns: [
            ...e.turns,
            { from: 'claude', text: r.reply, privacy: basis.privacy, basis, made: event.answer.made, followUps: r.followUps },
          ],
        };
      });
    };
    track(
      rkey,
      token,
      () =>
        bridge.claudeChat(scope.mountPoint, scan.scan, chat.conversation, chat.about, text, onEvent).then((started) => {
          // Still this chat's newest attempt, running or answered already (its Done can come first): the chat's id.
          if (chatAttempts.current.get(rkey) === token)
            editEntry<ChatEntry>(scope, chat.id, (e) => ({ ...e, conversation: started.conversation }));
          return started.requestId;
        }),
      (message) => editEntry<ChatEntry>(scope, chat.id, (e) => ({ ...e, phase: 'failed', failure: { kind: 'error', message } })),
    );
  };

  const ask = (scope: Scope, about: ClaudeAbout, name: string, text?: string) => {
    if (!statusNow || changing.current || !scanOf(scope)) return;
    setEnded(null);
    const privacy = !readsFiles(statusNow);
    const c = convos[keyOf(scope)] ?? EMPTY;
    const had = c.entries.find((e): e is ChatEntry => e.kind === 'chat' && sameAbout(e.about, about));
    const busy = had?.phase === 'working' || had?.phase === 'slow';
    let chat: ChatEntry;
    let question: string | null;
    if (!had) {
      // A row's or folder's chat opens with what it is; the whole scan's with the person's own question.
      const first = about.kind === 'scan' ? text?.trim() : FIRST_QUESTION;
      if (!first) return;
      chat = {
        kind: 'chat', id: `chat-${++seq}`, about, name, conversation: null, turns: [{ from: 'person', text: first, privacy }], phase: 'working', failure: null,
      };
      question = first;
    } else if (text !== undefined && text.trim() && !busy && !chatStarting(had)) {
      chat = { ...had, turns: [...had.turns, { from: 'person', text: text.trim(), privacy }], phase: 'working', failure: null };
      question = text.trim();
    } else if (text === undefined && had.phase === 'failed') {
      // Try again: the question that got no answer once more. The core kept nothing of it.
      chat = { ...had, phase: 'working', failure: null };
      question = [...had.turns].reverse().find((t) => t.from === 'person')?.text ?? FIRST_QUESTION;
    } else {
      chat = had;
      question = null;
    }
    edit(scope, (cv) => {
      const others = cv.entries.filter((e) => e.id !== chat.id);
      // A chat opened again comes to the bottom of the log, where the composer is; so does the whole scan's chat
      // each time it is asked, since jobs' answers come in between.
      const bottom = !had || text === undefined || about.kind === 'scan';
      const entries = bottom ? [...others, chat] : cv.entries.map((e) => (e.id === chat.id ? chat : e));
      // Asking about the whole scan leaves the composer on it: no row or folder to stop asking about.
      return { ...cv, entries, about: about.kind === 'scan' ? null : chat.id };
    });
    if (question !== null) send(scope, chat, question);
  };

  const cancel = (w: Where, entryId: string) => {
    const rkey = `${keyOf(w)}/${entryId}`;
    const r = requests.current.get(rkey);
    chatAttempts.current.delete(rkey);
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

  /**
   * Keep mine: takes the pick's own items back out of the scan it was made on, then puts the plan back. The plan goes
   * back only if, when that is done, this card is still the newest pick (`newestPick`, kept as each answer is
   * handled) and the plan is still as its answer left it: at its version with its ticks, which the drive's reducer
   * checks as it puts the plan back. A newer pick meanwhile, or an edit, stays as it is.
   */
  const keepMine = (scope: Scope, entryId: string) => {
    const pick = openPick(scope, entryId)?.pick;
    const scan = scanOf(scope);
    if (!pick || !scan) return;
    const setPick = (p: Partial<PickCard>) =>
      editEntry<JobEntry>(scope, entryId, (e) => (e.pick ? { ...e, pick: { ...e.pick, ...p } } : e));
    setPick({ busy: true, error: null });
    const back = () => {
      const newest = newestPick.current.get(keyOf(scope)) === entryId;
      // Only the newest pick puts the plan back, and the reducer does so only at the pick's version with its ticks.
      if (newest) drivesRef.current?.setPlan(scope.mountPoint, scope.run, pick.before, pick.version, pick.after);
      // What the card says; the plan is decided above. The plan as last drawn tells an edit made meanwhile.
      const now = scanOf(scope);
      const unchanged = now !== null && now.planVersion === pick.version && sameSet(now.plan, pick.after);
      setPick({
        busy: false,
        choice: 'kept',
        error: !newest
          ? 'Claude picked again meanwhile, so Vacancy left your plan as it is.'
          : unchanged
            ? null
            : 'Your plan changed meanwhile, so Vacancy left your ticks as they are.',
      });
    };
    if (pick.added.length === 0) return back();
    bridge.claudeRemoveItems(scope.mountPoint, scan.scan, pick.added).then(
      (refreshed) => {
        back();
        // An older map than the one shown (a later answer's) is ignored.
        drivesRef.current?.claudeApplied(scope.mountPoint, scope.run, refreshed);
      },
      (error: unknown) => setPick({ busy: false, error: errorMessage(error) }),
    );
  };

  const chooseLayout: Claude['chooseLayout'] = (scope, tidyId, layout) => {
    const scan = scanOf(scope);
    if (!scan) return;
    const setL = (p: Partial<Layouts>) =>
      edit(scope, (c) => {
        const l = c.layouts[tidyId];
        return l ? { ...c, layouts: { ...c.layouts, [tidyId]: { ...l, ...p } } } : c;
      });
    setL({ busy: true, error: null });
    bridge.claudeChooseLayout(scope.mountPoint, scan.scan, tidyId, layout).then(
      (refreshed) => {
        drivesRef.current?.claudeApplied(scope.mountPoint, scope.run, refreshed);
        setL({ busy: false });
      },
      (error: unknown) => setL({ busy: false, error: errorMessage(error) }),
    );
  };

  /**
   * Sets privacy mode `on`. Changing it ends the conversations so far, here and in the core (which keeps the new
   * mode and ends its chats in one step), so nothing from one mode is sent in the other; and until the core has
   * answered, nothing new is asked, so no request starts in the mode that is ending.
   */
  const changePrivacy = (on: boolean) => {
    if (statusNow?.privacy !== on && endAll()) setEnded(on);
    if (on) {
      // A check still running answers for privacy mode off: it no longer decides what the row shows.
      verifyToken.current++;
      setVerifying(false);
    }
    void modeSetting(() => bridge.claudeSetPrivacy(on));
  };

  // Folded while Claude is off (the map gets the room); open once it's turned on. A choice the person made wins.
  const folded = panel ? panel === 'folded' : !(statusNow && isEnabled(statusNow.state));

  const value: Claude = {
    status,
    settingError,
    turningOn,
    checking,
    verifying,
    verify: () => verify(true),
    folded,
    setFolded: (f) => {
      const v = f ? 'folded' : 'open';
      setPanel(v);
      keepPanel(v);
    },
    privacyAsk,
    privacyChanging,
    ended,
    turnOn: () => {
      const token = ++turnOnToken.current;
      setTurningOn(true);
      setSettingError(null);
      void modeSetting(() => bridge.claudeSetEnabled(true)).finally(() => {
        if (token === turnOnToken.current) setTurningOn(false);
      });
    },
    // The turning on still out answers first (asking Claude Code whether it's signed in takes up to 30 seconds),
    // then Claude is turned off; until then nothing is asked.
    cancelTurnOn: () => {
      turnOnToken.current++;
      setTurningOn(false);
      void modeSetting(() => bridge.claudeSetEnabled(false));
    },
    turnOff: () => {
      endAll();
      setEnded(null);
      setPrivacyAsk(false);
      verifyToken.current++;
      setVerifying(false);
      void modeSetting(() => bridge.claudeSetEnabled(false));
    },
    recheck,
    setPrivacy: (on) => {
      if (!on) return setPrivacyAsk(true);
      changePrivacy(true);
    },
    confirmPrivacyOff: () => {
      setPrivacyAsk(false);
      changePrivacy(false);
    },
    cancelPrivacyOff: () => setPrivacyAsk(false),
    // In line with the changes of mode, so a question asked after it is prepared with it; it ends no conversation.
    setModel: (model, effort) => {
      if (statusNow?.model === model && statusNow.effort === effort) return;
      void modeSetting(() => bridge.claudeSetModel(model, effort));
    },
    modelName: (model) => names[model] ?? null,
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

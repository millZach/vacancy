import {
  ArrowRight,
  Check,
  ChevronRight,
  CircleCheck,
  LoaderCircle,
  MessageCircle,
  Minus,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Send,
  ShieldX,
  Sparkles,
  TriangleAlert,
  X,
} from 'lucide-react';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import type { ClaudeAbout, ClaudeStatus, Overview, Proposal, TidyProposal } from '../bridge';
import { formatBytes, formatCount } from '../format';
import { RiskChip, SourceTag } from '../plan/bits';
import { activeMoves, kindFolders } from '../tidy/model';
import { Basis, Dropped, HiddenFold, JobLine, Mark, PrivacyNotice, PrivacyRow } from './bits';
import { FIRST_QUESTION } from './ClaudeProvider';
import { pickOffer, readsFiles, tileAt, useClaude, type ChatEntry, type Claude, type Conversation, type JobEntry, type JobKind, type Scope } from './context';
import './claude.css';

interface Props {
  /** The drive scan on the map, and its plan now. */
  scope: Scope;
  proposals: Proposal[];
  /** The sorts in use (Windows; none on macOS, so no layouts to suggest). */
  tidy: TidyProposal[];
  overview: Overview;
  /** Shows a proposal: the tray opens, its row scrolls into view and its tile lights. */
  onShow: (id: string) => void;
  /** Lights a folder's tile on the map. */
  onShowTile: (tileId: string) => void;
  /** Opens a sort's drawer, with its before and after trees. */
  onOpenTidy: (id: string) => void;
  /** Takes a proposal out of the plan (Untick it, on a flag). */
  onUntick: (id: string) => void;
}

const ASKED: Record<JobKind, string> = {
  reviewPlan: 'Review my plan',
  pickPlan: 'Let Claude pick',
  explainFolders: 'Explain big folders',
  tidyLayout: 'Suggest Tidy up layouts',
};
const WORKING: Record<JobKind, string> = {
  reviewPlan: 'Claude is reviewing your plan…',
  pickPlan: 'Claude is building a plan…',
  explainFolders: 'Claude is looking at the map…',
  tidyLayout: 'Claude is sorting the file names…',
};

/**
 * Variant B's Claude panel (prototype/ai-advisor), docked left of the map: while Claude is off, the turn-on step
 * itself, with the privacy notice and what always stays hidden; once on, privacy mode's switch, a running log of
 * the jobs and chats with Claude's checked answers (their item names light the map and the tray), the four job
 * chips and a composer for the chat it's on. Every answer says what it was based on. Claude proposes only: what
 * it adds is in the tray unticked, and nothing runs until the one confirmation.
 */
export function ClaudePanel(props: Props) {
  const claude = useClaude();
  if (!claude) return null;
  const status = claude.status.phase === 'ready' ? claude.status.status : null;
  if (claude.folded)
    return (
      <aside className="cp is-folded" aria-label="Claude" data-no-ripple>
        <button
          type="button"
          className="cp-rail"
          onClick={() => claude.setFolded(false)}
          aria-label="Show Claude"
          title={status?.state === 'on' ? 'Show Claude' : 'Claude is off. Show Claude to turn it on.'}
        >
          <PanelLeftOpen size={16} aria-hidden />
          <Mark size={16} />
          <span>{status?.state === 'on' ? 'Claude' : 'Claude · off'}</span>
        </button>
      </aside>
    );
  const c = claude.conversation(props.scope);
  return (
    <aside className="cp" aria-label="Claude" data-no-ripple>
      <div className="cp-head">
        <div>
          <h2>
            <Mark size={15} /> Claude
          </h2>
          <p>{headLine(status, c)}</p>
        </div>
        <button type="button" className="icon-btn" onClick={() => claude.setFolded(true)} aria-label="Fold the Claude panel">
          <PanelLeftClose size={16} aria-hidden />
        </button>
      </div>
      {claude.status.phase === 'loading' && <p className="cp-wait">Looking for Claude Code…</p>}
      {claude.status.phase === 'error' && (
        <div className="cp-off-body">
          <p role="alert">Vacancy couldn’t check Claude Code: {claude.status.message}</p>
          <button type="button" className="btn cp-on" onClick={claude.recheck} disabled={claude.checking}>
            {claude.checking ? 'Checking…' : 'Check again'}
          </button>
        </div>
      )}
      {status && status.state !== 'on' && <PanelOff claude={claude} status={status} />}
      {status && status.state === 'on' && <PanelOn claude={claude} status={status} c={c} {...props} />}
    </aside>
  );
}

function headLine(status: ClaudeStatus | null, c: Conversation): string {
  if (!status) return 'Checking';
  if (status.state === 'notFound') return 'Not found';
  if (status.state === 'off') return 'Off';
  if (status.state === 'signedOut') return 'On · not signed in';
  if (status.state === 'checkTimedOut') return 'On · Claude Code didn’t answer';
  if (status.state === 'checkFailed') return 'On · couldn’t check Claude Code';
  if (c.entries.some((e) => e.phase === 'slow')) return 'On · replies are slow';
  const last = c.entries.at(-1);
  if (last?.phase === 'failed' && last.failure?.kind !== 'cancelled') return 'On · the last request failed';
  return 'On · through your Claude Code';
}

/** Off, not found or not signed in: the panel is the turn-on step, with the privacy notice. */
function PanelOff({ claude, status }: { claude: Claude; status: ClaudeStatus }) {
  const checking = claude.checking || claude.turningOn;
  const [title, intro, row] =
    status.state === 'notFound'
      ? ['Claude Code isn’t on this PC.', 'Vacancy uses your own Claude Code, so it has to be installed here. Built-in cleanups still work.', 'Not found in this check']
      : status.state === 'signedOut'
        ? ['Sign in to Claude Code first.', 'Claude Code is installed but not signed in. Open a terminal, run claude, and sign in. Then check again.', 'Found · not signed in']
        : status.state === 'checkTimedOut'
          ? ['Claude Code didn’t answer.', 'Vacancy asked Claude Code whether it’s signed in, and it took too long. Check again in a moment.', 'Found · didn’t answer the check']
          : status.state === 'checkFailed'
            ? ['Vacancy couldn’t check Claude Code.', 'Asking Claude Code whether it’s signed in failed. Check again, or turn Claude off.', 'Found · the check failed']
            : ['Turn on Claude?', 'Claude can explain folders, review your plan and suggest layouts. It stays off until you turn it on.', 'Found on this PC · off'];
  const retry = status.state === 'notFound' || status.state === 'signedOut' || status.state === 'checkTimedOut' || status.state === 'checkFailed';
  return (
    <div className="cp-off-body">
      <h3>{title}</h3>
      <p>{intro}</p>
      <div className="cl-provider cp-provider">
        <span className="cl-provider-icon" aria-hidden>
          ✳
        </span>
        <div>
          <b>Claude Code</b>
          <span>{checking ? 'Checking Claude Code…' : row}</span>
        </div>
      </div>
      {status.message && (status.state === 'checkTimedOut' || status.state === 'checkFailed') && (
        <p className="cl-job is-error" role="alert">
          <TriangleAlert size={13} aria-hidden /> <span>{status.message}</span>
        </p>
      )}
      <PrivacyNotice privacy={status.privacy} compact />
      <HiddenFold hidden={status.hidden} />
      {claude.settingError && (
        <p className="cl-job is-error" role="alert">
          <TriangleAlert size={13} aria-hidden /> <span>{claude.settingError}</span>
        </p>
      )}
      {status.state === 'off' &&
        (claude.turningOn ? (
          <p className="cl-job cp-checking" role="status">
            <LoaderCircle size={13} className="tray-spin" aria-hidden />
            <span>Checking that Claude Code answers…</span>
            <button type="button" className="link-button" onClick={claude.cancelTurnOn}>
              Cancel
            </button>
          </p>
        ) : (
          <button type="button" className="cta cp-on" onClick={claude.turnOn}>
            <Mark size={13} /> Turn on Claude
          </button>
        ))}
      {retry && (
        <button type="button" className="btn cp-on" onClick={claude.recheck} disabled={checking}>
          {checking ? 'Checking…' : 'Check again'}
        </button>
      )}
      {retry && status.state !== 'notFound' && (
        <button type="button" className="link-button cp-off" onClick={claude.turnOff}>
          Turn off Claude
        </button>
      )}
    </div>
  );
}

type OnProps = Props & { claude: Claude; status: ClaudeStatus; c: Conversation };

function PanelOn(p: OnProps) {
  const { claude, status, c, scope } = p;
  const log = useRef<HTMLDivElement>(null);
  const last = c.entries.at(-1);
  const turns = last?.kind === 'chat' ? last.turns.length : 0;
  useEffect(() => {
    const el = log.current;
    el?.scrollTo?.({ top: el.scrollHeight, behavior: 'smooth' });
  }, [c.entries.length, last?.phase, turns]);
  const about = c.entries.find((e): e is ChatEntry => e.kind === 'chat' && e.id === c.about) ?? null;
  const running = (job: JobKind) => c.entries.some((e) => e.kind === 'job' && e.job === job && (e.phase === 'working' || e.phase === 'slow'));
  const chip = (job: JobKind) => (
    <button type="button" className="cl-chip" onClick={() => claude.start(scope, job)} disabled={running(job) || claude.privacyChanging}>
      {ASKED[job]}
    </button>
  );
  const aboutBusy = about?.phase === 'working' || about?.phase === 'slow';
  return (
    <>
      <div className="cp-status">
        <PrivacyRow claude={claude} privacy={status.privacy} fixture={status.fixture} unavailable={status.readsUnavailable} />
        <button type="button" className="link-button cp-off" onClick={claude.turnOff}>
          Turn off Claude
        </button>
        {claude.settingError && (
          <p className="cl-job is-error" role="alert">
            <TriangleAlert size={13} aria-hidden /> <span>{claude.settingError}</span>
          </p>
        )}
      </div>
      <div className="cp-log" ref={log} aria-label="Claude’s answers" aria-live="polite">
        <div className="cp-msg is-claude">
          <p>
            <Mark size={12} />{' '}
            {!readsFiles(status)
              ? 'I can see the names, sizes, counts and dates from this scan. Nothing inside your files.'
              : 'I can see this scan, and I can open files on this PC, except the places that always stay hidden.'}{' '}
            I only propose: nothing changes until you confirm.
          </p>
        </div>
        {claude.ended !== null && (
          <p className="cp-quiet cp-ended" role="status">
            Privacy mode is {claude.ended ? 'on' : 'off'} now, so Vacancy ended the conversation so far. Nothing from it is sent
            to Claude again.
          </p>
        )}
        {c.entries.map((e) => (e.kind === 'job' ? <JobItem key={e.id} e={e} {...p} /> : <ChatItem key={e.id} e={e} {...p} />))}
      </div>
      <div className="cp-foot">
        <div className="cp-acts">
          {chip('reviewPlan')}
          {chip('pickPlan')}
          {chip('explainFolders')}
          {p.tidy.length > 0 && chip('tidyLayout')}
        </div>
        <Composer
          key={about?.id ?? 'none'}
          disabled={!about || about.turns.length < 2 || aboutBusy || claude.privacyChanging}
          placeholder={about ? `Ask about ${about.name}` : 'Ask Claude about a row or a folder first'}
          onSend={(q) => about && claude.ask(scope, about.about, about.name, q)}
        >
          {about && (
            <span className="cp-about">
              <MessageCircle size={11} aria-hidden /> <span className="cp-about-n">{about.name}</span>
              <button type="button" aria-label="Stop asking about it" onClick={() => claude.clearAbout(scope)}>
                <X size={11} aria-hidden />
              </button>
            </span>
          )}
        </Composer>
      </div>
    </>
  );
}

function Composer({
  onSend,
  disabled,
  placeholder,
  children,
}: {
  onSend: (q: string) => void;
  disabled: boolean;
  placeholder: string;
  children?: ReactNode;
}) {
  const [q, setQ] = useState('');
  return (
    <form
      className="cl-compose"
      onSubmit={(e) => {
        e.preventDefault();
        if (!q.trim() || disabled) return;
        onSend(q);
        setQ('');
      }}
    >
      {children}
      <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={placeholder} aria-label={placeholder} />
      <button type="submit" className="icon-btn" aria-label="Send" disabled={disabled || !q.trim()}>
        <Send size={14} aria-hidden />
      </button>
    </form>
  );
}

/** A proposal's or folder's name in an answer: a link that lights it on the map (and its row in the tray). */
function ItemLink({ about, name, p }: { about: ClaudeAbout; name: string; p: OnProps }) {
  const tile = about.kind === 'folder' ? tileAt(p.overview, about.path) : null;
  const go = () => (about.kind === 'proposal' ? p.onShow(about.id) : tile && p.onShowTile(tile));
  if (about.kind === 'folder' && !tile) return <b className="cp-name">{name}</b>;
  return (
    <button type="button" className="cp-link" onClick={go}>
      {name}
      <ChevronRight size={11} aria-hidden />
    </button>
  );
}

function JobItem({ e, ...p }: OnProps & { e: JobEntry }) {
  const { claude, scope } = p;
  const a = e.answer;
  return (
    <div className="cp-entry" data-job={e.job}>
      <p className="cp-you">{ASKED[e.job]}</p>
      <JobLine
        phase={e.phase}
        failure={e.failure}
        working={WORKING[e.job]}
        onCancel={() => claude.cancel(scope, e.id)}
        onRetry={() => claude.start(scope, e.job)}
      />
      {e.phase === 'done' && a && (
        <div className="cp-msg is-claude">
          {a.result.kind === 'review' && <ReviewReply e={e} {...p} />}
          {a.result.kind === 'pick' && <PickReply e={e} {...p} />}
          {a.result.kind === 'explain' && <ExplainReply e={e} {...p} />}
          {a.result.kind === 'tidy' && <TidyReply e={e} {...p} />}
          {a.result.kind !== 'pick' && a.result.kind !== 'explain' && <Dropped items={a.dropped} />}
          <Basis basis={a.basis} />
        </div>
      )}
    </div>
  );
}

function ReviewReply({ e, ...p }: OnProps & { e: JobEntry }) {
  const r = e.answer?.result;
  const [why, setWhy] = useState(false);
  if (r?.kind !== 'review') return null;
  return (
    <>
      <p>
        <Mark size={12} /> {r.summary}
      </p>
      {r.flags.map((f) => {
        const inPlan = p.scope.plan.includes(f.id);
        const kept = e.kept.includes(f.id);
        return (
          <div key={f.id} className={inPlan && !kept ? 'cl-flag' : 'cl-flag is-settled'}>
            <p>
              <TriangleAlert size={12} aria-hidden />{' '}
              <span>
                <ItemLink about={{ kind: 'proposal', id: f.id }} name={f.title} p={p} /> <RiskChip risk={f.risk} /> {f.reason}
              </span>
            </p>
            <div className="cl-flag-acts">
              {!inPlan ? (
                <span className="cl-settled">Out of the plan.</span>
              ) : kept ? (
                <span className="cl-settled">You kept it.</span>
              ) : (
                <>
                  <button type="button" className="link-button" onClick={() => p.onUntick(f.id)}>
                    Untick it
                  </button>
                  <button type="button" className="link-button" onClick={() => p.claude.keepFlag(p.scope, e.id, f.id)}>
                    Keep it
                  </button>
                </>
              )}
            </div>
          </div>
        );
      })}
      {r.fine.length > 0 && (
        <>
          <button type="button" className="link-button cp-more" aria-expanded={why} onClick={() => setWhy(!why)}>
            {why ? 'Hide why the rest look fine' : `Why the other ${formatCount(r.fine.length)} look fine`}
          </button>
          {why && (
            <ul className="cp-fine">
              {r.fine.map((f) => (
                <li key={f.id}>
                  <Check size={11} strokeWidth={3} aria-hidden />
                  <span>
                    <ItemLink about={{ kind: 'proposal', id: f.id }} name={f.title} p={p} /> {f.note}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
      <p className="cp-quiet">A review only flags; it adds nothing. To add things, use Let Claude pick.</p>
    </>
  );
}

const CHANGE_LABEL = { untick: 'Unticked', tick: 'Ticked', add: 'Added, unticked' } as const;

function PickReply({ e, ...p }: OnProps & { e: JobEntry }) {
  const r = e.answer?.result;
  const pick = e.pick;
  if (r?.kind !== 'pick' || !pick) return null;
  const known = new Set(p.proposals.map((x) => x.id));
  const stay = pick.before.filter((id) => known.has(id) && r.plan.includes(id)).length;
  if (pick.choice === 'stale')
    return (
      <>
        <p>
          <Mark size={12} /> Your plan changed while I was picking, so Vacancy set my answer aside: nothing was ticked or unticked
          {pick.added.length > 0 ? (pick.busy ? '. Taking my own items back out…' : pick.error ? '.' : ', and my own items are out of the tray again.') : '.'}
        </p>
        {pick.error && (
          <p className="cl-job is-error" role="alert">
            <TriangleAlert size={13} aria-hidden /> <span>{pick.error} My items stay in the tray, unticked.</span>
          </p>
        )}
        <button type="button" className="link-button cl-again" onClick={() => p.claude.start(p.scope, 'pickPlan')}>
          Let Claude pick again
        </button>
      </>
    );
  if (pick.choice === 'kept')
    return (
      <>
        <p>
          <Mark size={12} /> You kept your plan. My own items are out of the tray again.
        </p>
        {pick.error && <p className="cp-quiet">{pick.error}</p>}
      </>
    );
  if (pick.choice === 'used')
    return (
      <p>
        <Mark size={12} /> You’re using my plan.{' '}
        {r.changes.some((c) => c.change === 'add') ? 'My own items are in the tray, unticked: tick the ones you want.' : ''}
      </p>
    );
  const offer = pickOffer(p.c, e, p.scope.plan, p.scope.planVersion);
  return (
    <div className={offer === 'open' ? 'cl-diff' : 'cl-diff is-settled'} aria-label="Claude’s plan">
      <p>
        <Mark size={12} /> {r.summary}
      </p>
      <p className="cl-diff-total">
        <span>
          Your plan <b>{formatBytes(r.bytesBefore)}</b>
        </span>
        <ArrowRight size={13} aria-hidden />
        <span>
          Claude’s <b>{formatBytes(r.bytesAfter)}</b>
        </span>
      </p>
      {offer === 'open' && <p className="cp-quiet">These changes are in your plan now. Keep mine puts yours back.</p>}
      <ul>
        {r.changes.map((ch) => (
          <li key={ch.id} className={`is-${ch.change}`}>
            <span className="cl-diff-sign" aria-hidden>
              {ch.change === 'untick' ? <Minus size={11} /> : ch.change === 'tick' ? <Check size={11} strokeWidth={3} /> : <Plus size={11} />}
            </span>
            <div>
              <span className="cl-diff-k">{CHANGE_LABEL[ch.change]}</span>
              {known.has(ch.id) ? <ItemLink about={{ kind: 'proposal', id: ch.id }} name={ch.title} p={p} /> : <b>{ch.title}</b>}{' '}
              <span className="cl-diff-size">{formatBytes(ch.bytes)}</span>
              <p>{ch.why}</p>
              {ch.change === 'add' && <SourceTag source="claude" agent="Claude" />}
            </div>
          </li>
        ))}
        {(e.answer?.dropped ?? []).map((d) => (
          <li key={`${d.name}:${d.path}`} className="is-refused">
            <span className="cl-diff-sign" aria-hidden>
              <ShieldX size={11} />
            </span>
            <div>
              <span className="cl-diff-k">Left out by Vacancy</span>
              <b>{d.name}</b>
              <p>
                {d.reason} It isn’t in Claude’s plan.
              </p>
            </div>
          </li>
        ))}
      </ul>
      <p className="cl-diff-same">
        {formatCount(stay)} of your ticked {stay === 1 ? 'item stays' : 'items stay'} as {stay === 1 ? 'it is' : 'they are'}.
        {p.tidy.length > 0 && ' Tidy up isn’t changed.'}
      </p>
      {pick.error && (
        <p className="cl-job is-error" role="alert">
          <TriangleAlert size={13} aria-hidden /> <span>{pick.error}</span>
        </p>
      )}
      {offer === 'open' ? (
        <div className="cl-diff-actions">
          <button type="button" className="btn" onClick={() => p.claude.keepMine(p.scope, e.id)} disabled={pick.busy}>
            {pick.busy ? 'Putting it back…' : 'Keep mine'}
          </button>
          <button type="button" className="cta" onClick={() => p.claude.useClaudes(p.scope, e.id)} disabled={pick.busy}>
            Use Claude’s plan
          </button>
        </div>
      ) : (
        <p className="cl-settled cl-diff-settled">
          {offer === 'replaced'
            ? 'A newer pick replaced this one. Its changes are settled.'
            : 'Your plan changed since, so these changes are settled. Let Claude pick again to compare.'}
        </p>
      )}
    </div>
  );
}

function ExplainReply({ e, ...p }: OnProps & { e: JobEntry }) {
  const r = e.answer?.result;
  if (r?.kind !== 'explain') return null;
  const dropped = e.answer?.dropped ?? [];
  const inFolder = (path: string) => dropped.filter((d) => isInside(d.path, path));
  const loose = dropped.filter((d) => !r.folders.some((f) => isInside(d.path, f.path)));
  if (r.folders.length === 0)
    return (
      <p>
        <Mark size={12} /> I didn’t find big folders here that the rules don’t already know.
      </p>
    );
  return (
    <>
      <p>
        <Mark size={12} /> {r.folders.length} big {r.folders.length === 1 ? 'folder' : 'folders'} the rules don’t recognize:
      </p>
      <ul className="cp-list">
        {r.folders.map((f) => {
          const about: ClaudeAbout = { kind: 'folder', path: f.path };
          return (
            <li key={f.path}>
              <span className="cp-list-h">
                <ItemLink about={about} name={f.name} p={p} /> <span>{formatBytes(f.bytes)}</span>
              </span>
              <span>{f.short}</span>
              {f.suggested.map((id) => {
                const s = p.proposals.find((x) => x.id === id);
                return s ? (
                  <span key={id} className="cp-sug">
                    <Sparkles size={11} aria-hidden /> Suggests{' '}
                    <ItemLink about={{ kind: 'proposal', id }} name={s.title} p={p} />, in the tray unticked
                  </span>
                ) : null;
              })}
              {inFolder(f.path).map((d) => (
                <span key={`${d.name}:${d.path}`} className="cp-ref">
                  <ShieldX size={11} aria-hidden /> Suggestion left out: {d.name}. {d.reason}
                </span>
              ))}
              <button type="button" className="link-button cl-ask" onClick={() => p.claude.ask(p.scope, about, f.name)}>
                <MessageCircle size={11} aria-hidden /> Ask
              </button>
            </li>
          );
        })}
      </ul>
      <Dropped items={loose} />
    </>
  );
}

/** `path` is `folder` or inside it, on either platform's separators. */
function isInside(path: string, folder: string): boolean {
  const a = path.toLowerCase();
  const b = folder.toLowerCase().replace(/[\\/]+$/, '');
  return a === b || a.startsWith(`${b}\\`) || a.startsWith(`${b}/`);
}

function TidyReply({ e, ...p }: OnProps & { e: JobEntry }) {
  const r = e.answer?.result;
  if (r?.kind !== 'tidy') return null;
  const c = p.c;
  return (
    <>
      <p>
        <Mark size={12} /> Here’s how I’d sort them instead. Pick a layout per folder; the drawer shows the trees.
      </p>
      {r.layouts.map((mine) => {
        const l = c.layouts[mine.id];
        if (!l) return null;
        const inUse = p.tidy.find((t) => t.id === mine.id)?.layout ?? 'builtin';
        const claudes = inUse === 'claude';
        return (
          <div key={mine.id} className="cp-tidy">
            <div className="cp-tidy-h">
              <b>{mine.name}</b>
              <button type="button" className="link-button" onClick={() => p.onOpenTidy(mine.id)}>
                See the trees
              </button>
            </div>
            {l.claude.note && <p className="cp-quiet">{l.claude.note}</p>}
            <div className="cp-tidy-cols" role="radiogroup" aria-label={`Layout for ${mine.name}`}>
              <LayoutPick
                on={!claudes}
                busy={l.busy}
                label="Built-in"
                folders={kindFolders(activeMoves(l.builtin, p.scope.plan))}
                onClick={() => !l.busy && claudes && p.claude.chooseLayout(p.scope, mine.id, 'builtin')}
              />
              <LayoutPick
                on={claudes}
                busy={l.busy}
                label="Claude’s"
                folders={kindFolders(activeMoves(l.claude, p.scope.plan))}
                onClick={() => !l.busy && !claudes && p.claude.chooseLayout(p.scope, mine.id, 'claude')}
              />
            </div>
            {l.error && (
              <p className="cl-job is-error" role="alert">
                <TriangleAlert size={13} aria-hidden /> <span>{l.error}</span>
              </p>
            )}
          </div>
        );
      })}
    </>
  );
}

function LayoutPick({
  on,
  busy,
  onClick,
  label,
  folders,
}: {
  on: boolean;
  busy: boolean;
  onClick: () => void;
  label: string;
  folders: [string, number][];
}) {
  return (
    <button type="button" role="radio" aria-checked={on} aria-busy={busy} className={on ? 'cp-pick is-on' : 'cp-pick'} onClick={onClick}>
      <span className="cp-pick-h">
        {on ? <CircleCheck size={12} aria-hidden /> : <span className="cp-radio" />} {label}
      </span>
      {folders.slice(0, 4).map(([n, count]) => (
        <span key={n} className="cp-pick-row">
          <span>{n}</span>
          <b>{formatCount(count)}</b>
        </span>
      ))}
      {folders.length > 4 && <span className="cp-pick-more">+{folders.length - 4} more</span>}
    </button>
  );
}

function ChatItem({ e, ...p }: OnProps & { e: ChatEntry }) {
  const { claude, scope } = p;
  const asked = new Set(e.turns.filter((t) => t.from === 'person').map((t) => t.text));
  const lastClaude = [...e.turns].reverse().find((t) => t.from === 'claude');
  const chips = (lastClaude?.followUps ?? []).filter((q) => !asked.has(q));
  return (
    <div className={p.c.about === e.id ? 'cp-entry is-about' : 'cp-entry'} data-chat={e.name}>
      <p className="cp-you">
        About <ItemLink about={e.about} name={e.name} p={p} />
      </p>
      {e.turns.length > 1 && (
        <div className="cp-msg is-claude cl-chat">
          {e.turns.map((t, i) =>
            i === 0 && t.text === FIRST_QUESTION ? null : t.from === 'person' ? (
              <p key={i} className="cl-msg is-you">
                {t.text}
              </p>
            ) : (
              <div key={i} className={i === 1 ? 'cl-first' : 'cl-msg'}>
                <p>
                  <Mark size={11} /> {t.text}
                </p>
                {t.basis && i === 1 && <Basis basis={t.basis} />}
                {t.basis && i > 1 && !t.basis.privacy && <Basis basis={t.basis} />}
              </div>
            ),
          )}
          {e.phase === 'done' && chips.length > 0 && (
            <div className="cl-chips">
              {chips.map((q) => (
                <button key={q} type="button" className="cl-chip" onClick={() => claude.ask(scope, e.about, e.name, q)}>
                  {q}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      <JobLine
        phase={e.phase}
        failure={e.failure}
        working={e.turns.length === 1 ? `Claude is looking at ${e.name}…` : 'Claude is answering…'}
        onCancel={() => claude.cancel(scope, e.id)}
        onRetry={() => claude.ask(scope, e.about, e.name)}
      />
    </div>
  );
}

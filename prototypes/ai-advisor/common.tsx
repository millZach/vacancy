// THROWAWAY: pieces every variant shares. The real app's Shell, DriveMenu, AdvisorTrigger, MapView and its model,
// Legend, RiskChip, SourceTag, Checkbox, the Tidy up model and the ConfirmDialog are imported; the tray, the drawer
// and the advisor dialog are drawn here with the app's own classes, because Claude's parts need slots they don't have.
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import {
  ArrowRight, Check, ChevronDown, ChevronRight, CornerDownRight, File, Folder, FolderTree, LoaderCircle, MessageCircle,
  Eye, EyeOff, Lock, Minus, PanelRightClose, Plus, Send, ShieldCheck, ShieldX, Signpost, Sparkles, TriangleAlert, Undo2, X,
} from 'lucide-react';
import type { Bridge, PlanSummary, Proposal, TidyProposal } from '../../src/bridge';
import { BridgeProvider } from '../../src/bridge/BridgeProvider';
import { fixtures } from '../../src/bridge/fixtures';
import type { DriveItem, ReadyDrives } from '../../src/drives/context';
import { formatBytes, formatCount } from '../../src/format';
import { Legend } from '../../src/map/Legend';
import { MapView, type TileLook } from '../../src/map/MapView';
import { overviewModel, type Leaf, type Rect } from '../../src/map/model';
import { useSize } from '../../src/map/useSize';
import { Checkbox, RiskChip, SourceTag } from '../../src/plan/bits';
import { planTotals, tileOf } from '../../src/plan/totals';
import { ConfirmDialog } from '../../src/run/ConfirmDialog';
import { activeMoves, afterTree, beforeTree, foldersMade, goingInPlan, kindFolders, listOf, sampleMoves, staysByReason, tidyTotals, type TreeNode } from '../../src/tidy/model';
import { AdvisorTrigger } from '../../src/advisor/Advisor';
import {
  AI_ITEMS, BASIS_ON, HIDDEN, PICK_CHANGES, PICK_REFUSED, REFUSED, RULE_PROPOSALS, RULE_TIDY, TIDY_NOTES, VOLUME, claudeTidy, explainOf,
  followUps, fmtGb, type Explanation, type Privacy as PrivacyMode,
} from './data';
import { isOn, useS, type Job } from './store';

export const AGENT = 'Claude';

// ---------- no bridge ----------

/** Every call is counted and refused. Only `platform` is read; the proof fails on any call. */
export function NoBridge({ children }: { children: ReactNode }) {
  const refuse = (name: string) => () => {
    (window as unknown as { __bridgeCalls: string[] }).__bridgeCalls.push(name);
    return Promise.reject(new Error('Prototype: no bridge. Nothing on disk is read or changed.'));
  };
  const bridge = new Proxy({ platform: 'windows' } as Bridge, { get: (t, key) => (key === 'platform' ? t.platform : refuse(String(key))) });
  return <BridgeProvider bridge={bridge}>{children}</BridgeProvider>;
}
(window as unknown as { __bridgeCalls: string[] }).__bridgeCalls = [];

/** The drive menu's drives: C: scanned (its map on screen), D: and E: not yet. */
export function readyDrives(): ReadyDrives {
  const s = useS();
  const fx = fixtures.windows;
  const list: DriveItem[] = fx.volumes.map((v, i) => ({
    volume: v, current: i === 0,
    scan: i === 0
      ? { phase: 'done', run: 1, overview: s.overview, proposals: s.proposals, tidy: s.tidy, plan: s.s.plan, at: Date.now() - 60_000, planRun: null, duplicates: { phase: 'stopped' } as never, skippedGroups: [] }
      : { phase: 'idle' },
  }));
  const noop = () => {};
  return {
    status: 'ready', machineName: fx.machineName, fullDiskAccess: 'notApplicable', list, current: list[0]!, screen: 'map', locked: false, gone: null,
    pick: noop, switchTo: noop, openMap: noop, showScanScreen: noop, scan: noop, cancelScan: noop, setInPlan: noop, setGroupSkipped: noop,
    setRunning: noop, runPlan: noop, cancelRun: noop, showSummary: noop, summarySeen: noop, leaveRun: noop, reportUpdated: noop, dismissGone: noop,
  };
}

// ---------- Claude's marks ----------

export function Mark({ size = 13 }: { size?: number }) {
  return <span className="cl-mark" style={{ fontSize: size }} aria-hidden>✳</span>;
}

/** The header's control: today's "Rules only" while AI is off; Claude's state once it's on. */
export function Trigger({ onOpen }: { onOpen: () => void }) {
  const st = useS();
  const { s } = st;
  if (!isOn(s.ai)) return <AdvisorTrigger open={s.dialog !== 'closed'} onOpen={onOpen} />;
  const working = Object.values(s.jobs).some((j) => j.phase === 'working');
  const failed = Object.values(s.jobs).some((j) => j.phase === 'failed');
  return (
    <button type="button" className="pill advisor-trigger cl-trigger" aria-haspopup="dialog" onClick={onOpen}>
      {working ? <LoaderCircle size={12} className="tray-spin" aria-hidden /> : <Mark size={12} />}
      {working ? 'Claude is working' : failed ? 'Claude · didn’t answer' : s.privacy === 'on' ? 'Claude on' : 'Claude on · reads files'}
      {failed && !working && <i className="cl-dot is-error" aria-hidden />}
      <ChevronDown size={12} aria-hidden />
    </button>
  );
}

// ---------- the advisor dialog (layout C, prototype/advisor-status), now with a real "Turn on Claude" ----------

const PRIVACY_ON: [string, string][] = [
  ['What Claude sees', 'Folder and file names, sizes, counts and dates, from this scan.'],
  ['Where it goes', 'To Anthropic, through your own Claude Code and its account.'],
  ['Never', 'The contents of any file.'],
  ['What it can do', 'Propose only. Its suggestions pass the Guard and your review, and nothing runs until you confirm.'],
];
const PRIVACY_OFF: [string, string][] = [
  ['What Claude sees', 'The scan’s names, sizes, counts and dates, and what’s inside files it opens anywhere on this PC.'],
  ['Where it goes', 'To Anthropic, through your own Claude Code and its account, including the contents of files it reads.'],
  ['Never', 'The hidden areas. It can’t write, delete, run commands or go online.'],
  ['What it can do', 'Propose only. Its suggestions pass the Guard and your review, and nothing runs until you confirm.'],
];

export function Privacy({ compact = false, mode }: { compact?: boolean; mode?: PrivacyMode }) {
  const st = useS();
  const rows = (mode ?? st.s.privacy) === 'on' ? PRIVACY_ON : PRIVACY_OFF;
  return (
    <dl className={compact ? 'cl-privacy is-compact' : 'cl-privacy'}>
      {rows.map(([k, v]) => (
        <div key={k}><dt>{k}</dt><dd>{v}</dd></div>
      ))}
    </dl>
  );
}

/** What Claude never reads, even with privacy mode off. */
export function HiddenList() {
  return (
    <ul className="cl-hidden" aria-label="Always hidden from Claude">
      {HIDDEN.map(([k, items]) => <li key={k}><b><EyeOff size={12} aria-hidden /> {k}</b><span>{items.join(', ')}</span></li>)}
    </ul>
  );
}

/** "What always stays hidden", folded. */
export function HiddenFold({ open: start = false }: { open?: boolean }) {
  const [open, setOpen] = useState(start);
  const id = useId();
  return (
    <div className="cl-hidden-fold">
      <button type="button" className="advisor-why" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <EyeOff size={14} aria-hidden />What always stays hidden<ChevronDown size={13} aria-hidden />
      </button>
      {open && <div id={id}><p className="advisor-explanation">Even with privacy mode off, Claude can never read these. Vacancy refuses them before Claude Code is asked.</p><HiddenList /></div>}
    </div>
  );
}

/** Privacy mode's switch row: on is the default; off asks first, and is remembered. */
export function PrivacyRow({ compact = false }: { compact?: boolean }) {
  const st = useS();
  const on = st.s.privacy === 'on';
  return (
    <div className={compact ? 'advisor-provider cl-privacy-row is-compact' : 'advisor-provider cl-privacy-row'}>
      <span className="advisor-provider-icon" aria-hidden>{on ? <Lock size={14} /> : <Eye size={14} />}</span>
      <div><b>Privacy mode</b><span>{on ? 'On: names, sizes, counts and dates only' : 'Off: Claude can read files, except hidden areas. Remembered.'}</span></div>
      <button type="button" role="switch" aria-checked={on} aria-label="Privacy mode" className={on ? 'advisor-toggle cl-toggle is-on' : 'advisor-toggle cl-toggle'}
        onClick={() => st.setPrivacy(on ? 'off' : 'on')}><i /></button>
    </div>
  );
}

/** Claude's privacy mode, for wherever a variant shows Claude's status. A click flips it (off asks first). */
export function PrivacyChip() {
  const st = useS();
  if (!isOn(st.s.ai)) return null;
  const on = st.s.privacy === 'on';
  return (
    <button type="button" className={on ? 'cl-pchip' : 'cl-pchip is-off'} onClick={() => st.setPrivacy(on ? 'off' : 'on')}
      title={on ? 'Privacy mode on: Claude gets names, sizes, counts and dates. Click to let it read files.' : 'Privacy mode off: Claude can read files, except the hidden areas. Click to turn privacy mode on.'}>
      {on ? <Lock size={11} aria-hidden /> : <Eye size={11} aria-hidden />} {on ? 'Privacy mode on' : 'Privacy off · reads files'}
    </button>
  );
}

/** The short confirm before privacy mode goes off. */
export function PrivacyConfirm() {
  const st = useS();
  const keep = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { keep.current?.focus(); }, []);
  useEffect(() => {
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); st.cancelPrivacyOff(); } };
    window.addEventListener('keydown', esc, true);
    return () => window.removeEventListener('keydown', esc, true);
  }, [st]);
  return createPortal(
    <div className="scrim cl-scrim-top" data-no-ripple onClick={st.cancelPrivacyOff}>
      <div className="dialog cl-pconfirm" role="alertdialog" aria-modal="true" aria-label="Turn privacy mode off?" onClick={(e) => e.stopPropagation()}>
        <h2>Let Claude read your files?</h2>
        <p className="dialog-lead">With privacy mode off, Claude can open and read files anywhere on this PC. <b>What’s inside them goes to Anthropic through your Claude Code.</b></p>
        <p>It still can’t write, delete, run commands or go online, and it still only proposes. Nothing runs until you confirm.</p>
        <p className="dialog-quiet">These always stay hidden:</p>
        <HiddenList />
        <p className="dialog-quiet">Vacancy remembers this choice. Turn privacy mode back on any time.</p>
        <div className="dialog-actions">
          <button ref={keep} type="button" className="btn" onClick={st.cancelPrivacyOff}>Keep privacy mode on</button>
          <button type="button" className="cta" onClick={st.confirmPrivacyOff}>Turn privacy mode off</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

/** What an answer was based on: names, sizes and dates, or the files Claude opened. */
export function Basis({ privacy, text }: { privacy: PrivacyMode; text: string }) {
  return <p className={privacy === 'on' ? 'cl-basis' : 'cl-basis is-open'}>{privacy === 'on' ? <Lock size={11} aria-hidden /> : <Eye size={11} aria-hidden />} {text}</p>;
}

/** What the dialog (or B's panel) says about Claude Code in each state, and what it offers. */
export function useAdvisorCopy() {
  const st = useS();
  const { s } = st;
  const [rechecking, setRechecking] = useState(false);
  const recheck = () => { setRechecking(true); setTimeout(() => setRechecking(false), 900); };
  const checking = s.dialog === 'checking' || rechecking;
  const title = s.ai === 'notfound' ? 'Claude Code isn’t on this PC.'
    : s.ai === 'signedout' ? 'Sign in to Claude Code first.'
    : isOn(s.ai) ? 'Claude is on.'
    : 'Turn on Claude?';
  const intro = s.ai === 'notfound' ? 'Vacancy uses your own Claude Code, so it has to be installed here. Built-in cleanups still work.'
    : s.ai === 'signedout' ? 'Claude Code is installed but not signed in. Open a terminal, run claude, and sign in.'
    : s.ai === 'slow' ? 'Replies are slow right now. You can cancel any request; nothing changes until you confirm.'
    : s.ai === 'error' ? 'The last request failed: Claude Code stopped with an error. Nothing changed.'
    : isOn(s.ai) ? 'It proposes; you decide. Turn it off any time, here.'
    : 'Claude can explain folders, review your plan and suggest layouts. It stays off until you turn it on.';
  const row = checking ? 'Checking Claude Code…'
    : s.ai === 'notfound' ? 'Not found in this check'
    : s.ai === 'signedout' ? 'Found · not signed in'
    : isOn(s.ai) ? 'On · through your Claude Code'
    : 'Found · signed in · off';
  return { s, st, checking, title, intro, row, recheck };
}

export function AdvisorDialog() {
  const { s, st, checking, title, intro, row, recheck } = useAdvisorCopy();
  const id = useId();
  const close = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { close.current?.focus(); }, []);
  useEffect(() => {
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); st.closeDialog(); } };
    window.addEventListener('keydown', esc, true);
    return () => window.removeEventListener('keydown', esc, true);
  }, [st]);
  const on = isOn(s.ai);
  const ready = s.ai === 'off';
  return createPortal(
    <div className="advisor-shade" data-no-ripple onPointerDown={(e) => { if (e.target === e.currentTarget) st.closeDialog(); }}>
      <section className="advisor-dialog cl-dialog" role="dialog" aria-modal="true" aria-labelledby={`${id}-t`}>
        <div className="advisor-top">
          <span className="advisor-eyebrow">AI assistance</span>
          <button ref={close} type="button" className="advisor-close" aria-label="Close AI options" onClick={st.closeDialog}><X size={17} aria-hidden /></button>
        </div>
        <div className="advisor-dialog-body">
          <div className="advisor-dialog-symbol">{on ? <Sparkles size={27} aria-hidden /> : <ShieldCheck size={27} aria-hidden />}</div>
          <h2 id={`${id}-t`}>{title}</h2>
          <p className="advisor-intro" role={s.ai === 'error' ? 'alert' : undefined}>{intro}</p>
          <div className="advisor-providers" aria-label="AI app">
            <div className="advisor-provider">
              <span className="advisor-provider-icon" aria-hidden>✳</span>
              <div><b>Claude Code</b><span>{row}</span></div>
              <button type="button" role="switch" aria-checked={on} aria-label="Use Claude Code" disabled={checking || (!on && !ready)}
                className={on ? 'advisor-toggle cl-toggle is-on' : ready ? 'advisor-toggle cl-toggle' : 'advisor-toggle'}
                onClick={() => (on ? st.turnOff() : st.turnOn())}><i /></button>
            </div>
            <div className="advisor-provider">
              <span className="advisor-provider-icon" aria-hidden>›_</span>
              <div><b>Codex</b><span>Not used: live AI is Claude Code only</span></div>
              <button type="button" className="advisor-toggle" role="switch" aria-checked="false" aria-label="Use Codex" disabled><i /></button>
            </div>
          </div>
          {on && <div className="advisor-providers cl-providers-2"><PrivacyRow /></div>}
          <Privacy />
          <HiddenFold />
        </div>
        {ready && !checking && (
          <div className="cl-dialog-actions">
            <button type="button" className="btn" onClick={st.closeDialog}>Not now</button>
            <button type="button" className="cta" onClick={() => st.turnOn()}><Mark size={13} /> Turn on Claude</button>
          </div>
        )}
        {s.dialog === 'checking' && (
          <div className="cl-dialog-actions">
            <span className="cl-checking"><LoaderCircle size={14} className="tray-spin" aria-hidden /> Checking that Claude Code answers…</span>
            <button type="button" className="btn" onClick={() => st.cancelTurnOn()}>Cancel</button>
          </div>
        )}
        {(s.ai === 'notfound' || s.ai === 'signedout') && (
          <div className="cl-dialog-actions">
            <button type="button" className="btn" onClick={recheck} disabled={checking}>{checking ? 'Checking…' : 'Check again'}</button>
            <button type="button" className="btn" onClick={st.closeDialog}><Check size={15} aria-hidden /> Continue with built-in rules</button>
          </div>
        )}
        {on && (
          <div className="cl-dialog-actions">
            <button type="button" className="btn" onClick={st.turnOff}>Turn off Claude</button>
            <button type="button" className="cta" onClick={st.closeDialog}>Done</button>
          </div>
        )}
      </section>
    </div>,
    document.body,
  );
}

// ---------- a Claude job's status line: working, slow (with Cancel), failed, cancelled ----------

export function JobLine({ job, working, onCancel, onRetry }: { job: Job; working: string; onCancel: () => void; onRetry: () => void }) {
  if (job.phase === 'working')
    return (
      <p className={job.slow ? 'cl-job is-slow' : 'cl-job'} role="status">
        <LoaderCircle size={13} className="tray-spin" aria-hidden />
        <span>{job.slow ? <><b>Still working.</b> Claude is taking longer than usual.</> : working}</span>
        <button type="button" className="link-button" onClick={onCancel}>Cancel</button>
      </p>
    );
  if (job.phase === 'failed')
    return (
      <p className="cl-job is-error" role="alert">
        <TriangleAlert size={13} aria-hidden />
        <span>Claude Code didn’t answer: it stopped with an error. Nothing changed.</span>
        <button type="button" className="link-button" onClick={onRetry}>Try again</button>
      </p>
    );
  if (job.phase === 'cancelled')
    return (
      <p className="cl-job is-quiet" role="status">
        <X size={13} aria-hidden />
        <span>Cancelled. Nothing changed.</span>
        <button type="button" className="link-button" onClick={onRetry}>Try again</button>
      </p>
    );
  return null;
}

// ---------- the map ----------

interface StageProps {
  /** Extra controls in the toolbar. */
  toolbar?: ReactNode;
  /** A variant's extra look for a tile (merged over the plan's). */
  look?: (leaf: Leaf, rect: Rect) => TileLook | null;
  /** A variant's section for a tile's hover card. */
  status?: (leaf: Leaf) => ReactNode;
  /** A click on a tile that isn't a proposal. Return true if the variant handled it. */
  onFolder?: (leaf: Leaf) => boolean;
  /** Draws a ✳ on the folders Claude explained. */
  badges?: boolean;
  children?: ReactNode;
}

/** The explanation of a tile, once Claude has explained the map. */
export function useExplained() {
  const st = useS();
  return (leaf: Leaf): Explanation | null => (st.explained() && leaf.path ? (explainOf(leaf.path) ?? null) : null);
}

/** The map as Atlas draws it after a scan: the toolbar, the overview (a click zooms into a category), the plan's tiles. */
export function MapStage({ toolbar, look: extra, status, onFolder, badges = false, children }: StageProps) {
  const st = useS();
  const { s, overview, proposals } = st;
  const [ref, size] = useSize<HTMLDivElement>();
  const model = useMemo(() => overviewModel(overview), [overview]);
  const planning = s.planOpen;
  const byId = useMemo(() => new Map(proposals.map((p) => [p.id, p])), [proposals]);
  const explainedOf = useExplained();
  const flagged = new Set(st.flags().map((f) => f.id));
  const focusName = s.focus ? model.regionById.get(s.focus)?.name : null;

  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape' && s.focus) st.set({ focus: null }); };
    addEventListener('keydown', k);
    return () => removeEventListener('keydown', k);
  });

  const onLeaf = (leaf: Leaf) => {
    if (leaf.proposal !== null) {
      if (planning) return st.togglePlan(leaf.proposal);
      if (s.focus) { if (!s.plan.includes(leaf.proposal)) st.togglePlan(leaf.proposal); return st.set({ planOpen: true }); }
      return st.set({ focus: leaf.regionId });
    }
    if (onFolder?.(leaf)) return;
    if (!s.focus) st.set({ focus: leaf.regionId });
  };
  const look = (leaf: Leaf, rect: Rect): TileLook => {
    const more = extra?.(leaf, rect) ?? null;
    let base: TileLook = {};
    if (leaf.proposal !== null) {
      const on = s.plan.includes(leaf.proposal);
      const hot = s.hot !== null && s.hot !== leaf.id && s.hot === tileOf(leaf.proposal) ? ' is-hot' : '';
      base = !planning ? { cls: `is-reclaim${hot}`, state: 'reclaimable' }
        : on ? { cls: `is-reclaim is-selected${hot}`, badge: <Pin on />, state: 'reclaimable, in the plan', pressed: true }
        : { cls: `is-available${hot}`, badge: <Pin on={false} />, state: 'reclaimable, not in the plan', pressed: false };
      if (flagged.has(leaf.proposal)) base.cls += ' is-flagged';
    } else if (badges && explainedOf(leaf)) {
      base = { cls: 'is-explained', badge: <span className="ai-pin" aria-hidden>✳</span>, state: 'Claude explained this folder' };
    }
    if (!more) return base;
    return { ...base, ...more, cls: [base.cls, more.cls].filter(Boolean).join(' '), badge: more.badge ?? base.badge };
  };
  const leafStatus = (leaf: Leaf): ReactNode => {
    const mine = status?.(leaf) ?? null;
    const p = leaf.proposal === null ? undefined : byId.get(leaf.proposal);
    if (!p) return mine;
    const on = s.plan.includes(p.id);
    const act = planning ? (on ? 'Click to take it out of the plan.' : 'Click to add it to the plan.') : s.focus ? 'Click to add it to the plan.' : 'Click to zoom in.';
    return (
      <>
        <div className="map-tip-meta"><RiskChip risk={p.risk} /><SourceTag source={p.source} agent={AGENT} /></div>
        <p className="map-tip-sum">{p.aiNote ?? p.summary}</p>
        {mine}
        <p className="map-tip-act">{act}</p>
      </>
    );
  };

  return (
    <main className="atlas is-map">
      <div className="atlas-toolbar">
        <nav className="crumbs" aria-label="Map location">
          {s.focus ? <button type="button" className="crumb" onClick={() => st.set({ focus: null })}>{VOLUME.name}</button>
            : <span className="crumb is-here" aria-current="location">{VOLUME.name}</span>}
          {s.focus && <span className="crumb-step"><ChevronRight size={13} aria-hidden /><span className="crumb is-here" aria-current="location">{focusName}</span></span>}
        </nav>
        <span className="cl-toolbar-gap" />
        {toolbar}
        <Legend bytesInView={s.focus ? (model.regionById.get(s.focus)?.bytes ?? 0) : VOLUME.totalBytes} w={size.w} h={size.h} reclaimable />
      </div>
      <div className="atlas-stage" ref={ref}>
        <MapView model={model} width={size.w} height={size.h} focus={s.focus} interactive hot={s.hot} onHot={(h) => st.set({ hot: h })}
          onLeaf={onLeaf} onRegion={(r) => st.set({ focus: s.focus ? null : r.free ? null : r.id })}
          clickable={(l) => l.kind !== 'free' && (l.proposal !== null || !s.focus || !!onFolder)}
          leafAction={(l) => (l.kind === 'free' ? null : s.focus ? null : 'Click to zoom in.')}
          regionAction={() => (s.focus ? 'Click to zoom out.' : 'Click to zoom in.')}
          look={look} leafStatus={leafStatus} planning={planning} />
        {children}
      </div>
    </main>
  );
}

function Pin({ on }: { on: boolean }) {
  return <span className={on ? 'map-pin is-on' : 'map-pin'} aria-hidden>{on ? <Check size={11} strokeWidth={3} /> : <Plus size={11} strokeWidth={2.6} />}</span>;
}

/** "Claude says" in a hover card, for a folder Claude explained. */
export function TipSection({ e, act }: { e: Explanation; act?: string }) {
  const st = useS();
  const open = st.s.answered.explain === 'off';
  return (
    <div className="cl-tip">
      <span className="cl-tip-h"><Mark size={11} /> Claude says</span>
      <p>{open ? e.deep : e.text}</p>
      {open && e.hidden && <p className="cl-tip-hid"><EyeOff size={11} aria-hidden /> {e.hidden}</p>}
      <Basis privacy={open ? 'off' : 'on'} text={open ? 'Read files in this folder.' : 'From names, sizes and dates.'} />
      {e.suggests && <p className="cl-tip-sug"><Sparkles size={11} aria-hidden /> Suggests {AI_ITEMS[e.suggests]?.title} ({fmtGb(AI_ITEMS[e.suggests]!.bytes)}), in the tray unticked.</p>}
      {e.refused && <p className="cl-tip-ref"><ShieldX size={11} aria-hidden /> Its suggestion was refused by the Guard.</p>}
      {act && <p className="map-tip-act">{act}</p>}
    </div>
  );
}

// ---------- the plan tray, as PlanTray draws it, with slots for Claude ----------

interface TrayProps {
  /** Under the tray's head (A: Claude's buttons). */
  head?: ReactNode;
  /** Below a row's meta line (A: a flag, a diff mark). */
  rowExtra?: (p: Proposal) => ReactNode;
  rowCls?: (p: Proposal) => string | undefined;
  /** In place of whether a row is in the plan (A: Claude's plan, previewed). */
  checked?: (id: string) => boolean;
  /** "Ask Claude" in each row's meta line. */
  onAsk?: ((p: Proposal) => void) | undefined;
  /** Rows that aren't in the drive's proposals yet (A: Claude's additions, previewed). */
  ghosts?: Proposal[];
  /** In place of the foot's button. */
  cta?: ReactNode;
  /** Under the foot's button. */
  below?: ReactNode;
  /** The figure the foot shows (A: Claude's plan's). */
  plan?: string[];
}

export function Tray({ head, rowExtra, rowCls, checked, onAsk, ghosts = [], cta, below, plan: shownPlan }: TrayProps) {
  const st = useS();
  const { s, proposals, tidy } = st;
  const plan = shownPlan ?? s.plan;
  const all = [...proposals, ...ghosts];
  const deletes = all.filter((p) => p.action === 'delete');
  const hints = all.filter((p) => p.action === 'hint');
  const isIn = checked ?? ((id: string) => plan.includes(id));
  const on = deletes.filter((p) => isIn(p.id)).length;
  const t = planTotals(all, plan);
  const sorts = tidyTotals(tidy, plan);
  const reminders = hints.filter((p) => isIn(p.id)).length;
  const row = (p: Proposal) => (
    <Row key={p.id} p={p} on={isIn(p.id)} cls={rowCls?.(p)} extra={rowExtra?.(p)} onAsk={onAsk} ghost={ghosts.includes(p)} />
  );
  return (
    <aside className="tray" aria-label="Plan" data-no-ripple>
      <div className="tray-head">
        <div>
          <h2>Plan</h2>
          <p>Click glowing areas on the map, or tick them here. Nothing changes until you confirm.</p>
        </div>
        <button type="button" className="icon-btn" onClick={() => st.set({ planOpen: false, drawer: null })} aria-label="Hide the plan"><PanelRightClose size={16} aria-hidden /></button>
      </div>
      {head}
      <div className="tray-scroll">
        <section className="tray-group" aria-label="Delete">
          <h3>Delete <span>{on} of {deletes.length}</span></h3>
          {deletes.map(row)}
        </section>
        <section className="tray-group" aria-label="Tidy up">
          <h3>Tidy up <span>{tidy.filter((x) => plan.includes(x.id)).length} of {tidy.length}</span></h3>
          <p className="tray-group-note">Sorts loose files into folders. Moves only: nothing is deleted and no space is freed.</p>
          {tidy.map((x) => <TidyRow key={x.id} t={x} on={plan.includes(x.id)} plan={plan} />)}
        </section>
        {hints.length > 0 && (
          <section className="tray-group" aria-label="Left for you">
            <h3>Left for you</h3>
            <p className="tray-group-note">Vacancy can’t do these itself. Tick one to get a reminder when the run finishes. They don’t count toward the total.</p>
            {hints.map(row)}
          </section>
        )}
      </div>
      <div className="tray-foot">
        <div className="tray-total"><span className="tray-total-num">{formatBytes(t.bytes)}</span><span className="tray-total-lbl">will be freed</span></div>
        <ul className="tray-notes">
          {on > 0 ? <li>{formatCount(t.items)} {t.items === 1 ? 'item' : 'items'} in {t.places} {t.places === 1 ? 'place' : 'places'}, deleted permanently</li> : <li>No files will be deleted.</li>}
          {sorts.moves > 0 && <li className="note-moves">{formatCount(sorts.moves)} files sorted in {listOf(sorts.folders)}. Moved, not deleted: frees 0 B. Undo any time.</li>}
          {reminders > 0 && <li>{reminders} left for you, not counted</li>}
        </ul>
        {cta ?? (
          <button type="button" className="cta tray-cta" disabled={on === 0 && sorts.moves === 0 && reminders === 0} onClick={() => st.set({ confirm: true })}>
            {on > 0 ? `Free ${formatBytes(t.bytes)}` : sorts.moves > 0 ? `Move ${formatCount(sorts.moves)} files` : 'Nothing selected yet'}
          </button>
        )}
        {below}
      </div>
    </aside>
  );
}

function Row({ p, on, cls, extra, onAsk, ghost }: { p: Proposal; on: boolean; cls: string | undefined; extra: ReactNode; onAsk: ((p: Proposal) => void) | undefined; ghost: boolean }) {
  const st = useS();
  const [open, setOpen] = useState(false);
  const more = useId();
  const hint = p.action === 'hint';
  const tile = hint ? null : tileOf(p.id);
  const c = ['tray-row', on && 'is-on', st.s.hot === tile && tile && 'is-hot', p.source === 'ai' && 'is-ai', ghost && 'is-ghost', cls].filter(Boolean).join(' ');
  return (
    <div className={c} data-row={p.id} onMouseEnter={() => st.set({ hot: tile })} onMouseLeave={() => st.set({ hot: null })}>
      <Checkbox on={on} onChange={() => { if (!ghost) st.togglePlan(p.id); }} label={hint ? `Remind me: ${p.title}` : `Include ${p.title}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={() => setOpen(!open)} aria-expanded={open} aria-controls={more}>
          <span>{p.title}</span>
          <ChevronDown size={13} className={open ? 'is-open' : undefined} aria-hidden />
        </button>
        <p className="tray-row-sum">{p.summary}</p>
        <div className="tray-row-meta">
          {hint ? <span className="source"><Signpost size={11} aria-hidden /> You do this one</span> : <RiskChip risk={p.risk} />}
          <SourceTag source={p.source} agent={AGENT} />
          {onAsk && !ghost && <button type="button" className="link-button cl-ask" onClick={() => onAsk(p)}><MessageCircle size={11} aria-hidden /> Ask Claude</button>}
        </div>
        {extra}
        {open && (
          <div className="tray-row-more" id={more}>
            {p.aiNote ? <p className="cl-note"><Mark size={11} /> {p.aiNote}</p> : <p>{p.why.split('\n')[0]}</p>}
            {p.regenerable && !hint && <p className="tray-regen">Comes back on its own if you need it.</p>}
            <ul className="tray-paths" aria-label="Sample paths">{p.samplePaths.map((x) => <li key={x}>{x}</li>)}</ul>
          </div>
        )}
      </div>
      <span className="tray-row-size">{hint ? 'Advice' : formatBytes(p.bytes)}</span>
    </div>
  );
}

/** A Tidy up row as TidyGroup draws it, saying whose layout it uses. */
function TidyRow({ t, on, plan }: { t: TidyProposal; on: boolean; plan: string[] }) {
  const st = useS();
  const claude = st.s.layout[t.id] === 'claude';
  const moves = activeMoves(t, plan).length;
  const tops = kindFolders(activeMoves(t, plan)).slice(0, 4);
  const most = Math.max(1, ...tops.map(([, n]) => n));
  const open = st.s.drawer === t.id;
  const cls = ['tray-row', 'tidy-row', on && 'is-on', open && 'is-hot', claude && 'is-ai'].filter(Boolean).join(' ');
  return (
    <div className={cls} data-row={t.id}>
      <Checkbox on={on} onChange={() => st.togglePlan(t.id)} label={`Include Sort ${t.name}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={() => st.set({ drawer: open ? null : t.id, chatOpen: null, focus: null })} aria-expanded={open} aria-haspopup="dialog">
          <span>Sort {t.name}</span><ChevronRight size={13} aria-hidden />
        </button>
        <p className="tray-row-sum">{formatCount(moves)} of {formatCount(t.loose)} loose files, {claude ? 'sorted Claude’s way: by what they’re for.' : 'sorted by kind and year.'}</p>
        <button type="button" className="mini-ba" onClick={() => st.set({ drawer: t.id, chatOpen: null, focus: null })} aria-label={`Before and after: ${t.name}`}>
          <span className="mini-before"><span className="mini-pile" aria-hidden>{Array.from({ length: Math.min(28, Math.max(3, Math.round(t.loose / 7))) }, (_, i) => <i key={i} style={{ left: `${(i * 37) % 86}%`, top: `${(i * 53) % 78}%` }} />)}</span><span className="mini-cap">{formatCount(t.loose)} loose</span></span>
          <ArrowRight size={13} className="mini-arrow" aria-hidden />
          <span className="mini-after">{tops.map(([name, n]) => <span key={name} className="mini-folder"><span className="mini-name">{name}</span><span className="mini-bar"><i style={{ width: `${(n / most) * 100}%` }} /></span><span className="mini-n">{formatCount(n)}</span></span>)}</span>
        </button>
        <div className="tray-row-meta">
          <SourceTag source={claude ? 'ai' : 'rule'} agent={AGENT} />
          <span className="source"><Undo2 size={11} aria-hidden /> Undo any time</span>
        </div>
      </div>
      <span className="tray-row-size is-files">{formatCount(moves)} files</span>
    </div>
  );
}

// ---------- the Tidy up drawer, with whose layout it shows ----------

export function Drawer({ id, top, compare = false }: { id: string; top?: ReactNode; compare?: boolean }) {
  const st = useS();
  const rule = RULE_TIDY.find((x) => x.id === id)!;
  const t = st.tidy.find((x) => x.id === id)!;
  const claude = st.s.layout[id] === 'claude';
  const plan = st.s.plan;
  const on = plan.includes(id);
  const moves = activeMoves(t, plan);
  const made = foldersMade(t, moves).length;
  const samples = sampleMoves(moves);
  const stays = staysByReason(t.stays);
  const going = goingInPlan(t, plan);
  useEffect(() => {
    const k = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); st.set({ drawer: null }); } };
    window.addEventListener('keydown', k, true);
    return () => window.removeEventListener('keydown', k, true);
  }, [st]);
  return (
    <section className={compare ? 'reorg-drawer cl-drawer is-wide' : 'reorg-drawer cl-drawer'} role="dialog" aria-label={`Sort ${t.name}`} data-no-ripple>
      <header className="rd-head">
        <div><h2>Sort {t.name}</h2><p className="rd-path">{t.path}</p></div>
        <button type="button" className="icon-btn" onClick={() => st.set({ drawer: null })} aria-label="Close"><X size={16} aria-hidden /></button>
      </header>
      <div className="rd-body">
        {top}
        {claude
          ? <p className="rd-rule cl-rule"><Mark size={13} /> <span><b>Claude’s layout:</b> {TIDY_NOTES[id]}</span></p>
          : <p className="rd-rule"><FolderTree size={14} aria-hidden /> Built-in rule: sorted by kind and year.</p>}
        {compare ? (
          <div className="trees cl-trees3">
            <section aria-label="Before"><h3>Before</h3><Tree node={beforeTree(rule)} /></section>
            <section aria-label="Built-in rule" className={claude ? 'is-dim' : 'is-picked'}><h3>Built-in rule</h3><Tree node={afterTree(rule, plan)} /></section>
            <section aria-label="Claude’s layout" className={claude ? 'is-picked' : 'is-dim'}><h3><Mark size={11} /> Claude’s</h3><Tree node={afterTree(st.tidy.find((x) => x.id === id) && claude ? t : claudeOf(rule), plan)} /></section>
          </div>
        ) : (
          <div className="trees">
            <section aria-label="Before"><h3>Before</h3><Tree node={beforeTree(t)} /></section>
            <ArrowRight size={16} className="trees-arrow" aria-hidden />
            <section aria-label="After"><h3>After{claude && <span className="cl-after"> · Claude’s</span>}</h3><Tree node={afterTree(t, plan)} /></section>
          </div>
        )}
        {samples.length > 0 && (
          <>
            <h3 className="rd-h">Sample moves</h3>
            <ul className="sample-moves" aria-label="Sample moves">
              {samples.map((m) => (
                <li key={`${m.name}:${m.dest}`} className={m.renamed ? 'is-renamed' : undefined}>
                  <span className="sm-from">{m.name}</span><ArrowRight size={12} aria-hidden /><span className="sm-to">{m.dest}\{m.to}</span>
                  {m.renamed && <em>A file there already has the name, so this one gets a number. Nothing is overwritten.</em>}
                </li>
              ))}
            </ul>
          </>
        )}
        {(stays.length > 0 || going.length > 0) && (
          <>
            <h3 className="rd-h">Stays where it is</h3>
            <ul className="stays" aria-label="Stays where it is">
              {going.length > 0 && <li><CornerDownRight size={13} aria-hidden /><span><b>{going.length} {going.length === 1 ? 'file' : 'files'}</b> going in this plan’s deletes instead.</span></li>}
              {stays.map((g) => <li key={g.reason}>{g.dropped ? <TriangleAlert size={13} aria-hidden /> : <CornerDownRight size={13} aria-hidden />}<span><b>{g.names.slice(0, 3).join(', ')}{g.names.length > 3 ? ` and ${g.names.length - 3} more` : ''}</b> {g.reason}</span></li>)}
            </ul>
          </>
        )}
        <ul className="facts">
          {claude
            ? <><li>Claude named the folders, from file names, sizes and dates only. Vacancy makes the moves.</li><li>Folder names are pinned when you review: a name taken by the run is left out, never renumbered.</li></>
            : <li>Built-in rule: sorted by kind and year. No AI, offline.</li>}
          <li>Only loose files at the top of {t.name}. Subfolders stay as they are.</li>
          <li>Moves only. Nothing is deleted, so this frees 0 B.</li>
          <li className="is-undo"><Undo2 size={13} aria-hidden /> Every move is journaled. Undo any time, from the recap or Past runs.</li>
        </ul>
      </div>
      <footer className="rd-foot">
        <p>{formatCount(moves.length)} moves · makes {formatCount(made)} folders · frees 0 B</p>
        <button type="button" className={on ? 'btn' : 'cta'} onClick={() => st.togglePlan(id)}>{on ? 'Remove from plan' : 'Add to plan'}</button>
      </footer>
    </section>
  );
}

const claudeOf = (t: TidyProposal) => claudeTidy(t);

export function Tree({ node, depth = 0 }: { node: TreeNode; depth?: number }) {
  return (
    <div className="tree-node">
      <div className={`tree-row${node.tag ? ` is-${node.tag}` : ''}`} style={{ paddingLeft: depth * 15 }}>
        {node.file ? <File size={13} strokeWidth={1.8} aria-hidden /> : <Folder size={13} strokeWidth={1.8} aria-hidden />}
        <span className="tree-name">{node.name}</span>
        {node.tag === 'new' && depth > 0 && <span className="tree-tag">new</span>}
        {node.tag === 'existing' && <span className="tree-tag is-existing">yours</span>}
        {node.count !== undefined && <span className="tree-count">{formatCount(node.count)}</span>}
      </div>
      {node.children?.map((c) => <Tree key={c.name} node={c} depth={depth + 1} />)}
    </div>
  );
}

/** Built-in rules or Claude's layout, for one folder. */
export function LayoutSwitch({ id }: { id: string }) {
  const st = useS();
  const claude = st.s.layout[id] === 'claude';
  const job = st.jobOf('tidy');
  return (
    <div className="cl-layout">
      <div className="mode-seg cl-seg" role="radiogroup" aria-label="Layout">
        <button type="button" role="radio" aria-checked={!claude} className={claude ? '' : 'is-on'} onClick={() => st.setLayout(id, 'rule')}><FolderTree size={12} aria-hidden /> Built-in rules</button>
        <button type="button" role="radio" aria-checked={claude} className={claude ? 'is-on' : ''} onClick={() => st.setLayout(id, 'claude')}><Mark size={12} /> Claude’s layout</button>
      </div>
      <JobLine job={job} working="Claude is sorting the file names…" onCancel={() => st.cancel('tidy')} onRetry={() => st.setLayout(id, 'claude')} />
    </div>
  );
}

// ---------- a chat about one item: Claude's first answer, then follow-ups ----------

export function ChatThread({ id, compose = true, noWhat = false }: { id: string; compose?: boolean; noWhat?: boolean }) {
  const st = useS();
  const chat = st.s.chats[id];
  const job = st.jobOf(`chat:${id}`);
  if (!chat) return null;
  const asked = new Set(chat.msgs.filter((m) => m.from === 'you').map((m) => m.text));
  const chips = followUps(chat.subject).filter(([q]) => !asked.has(q));
  return (
    <div className="cl-chat">
      {chat.first ? (
        <dl className="cl-first">
          {!noWhat && <div><dt>What it is</dt><dd>{chat.first.what}</dd></div>}
          <div><dt>What you’d lose</dt><dd>{chat.first.lose}</dd></div>
          <div><dt>Does it come back?</dt><dd>{chat.first.back}</dd></div>
          <Basis privacy={chat.first.basis === BASIS_ON ? 'on' : 'off'} text={chat.first.basis} />
        </dl>
      ) : job.phase !== 'working' ? null : <p className="cl-wait"><Mark size={12} /> Claude is looking at {chat.subject.name}…</p>}
      {chat.msgs.map((m, i) => <p key={i} className={m.from === 'you' ? 'cl-msg is-you' : 'cl-msg'}>{m.from === 'claude' && <Mark size={11} />} {m.text}</p>)}
      <JobLine job={job.phase === 'working' && !chat.first ? { ...job, phase: job.slow ? 'working' : 'idle' } : job} working="Claude is answering…" onCancel={() => st.cancel(`chat:${id}`)}
        onRetry={() => { const last = [...chat.msgs].reverse().find((m) => m.from === 'you'); if (last && chat.first) st.ask(id, last.text); else st.openChat(chat.subject); }} />
      {chat.first && chips.length > 0 && job.phase !== 'working' && (
        <div className="cl-chips">{chips.map(([q]) => <button key={q} type="button" className="cl-chip" onClick={() => st.ask(id, q)}>{q}</button>)}</div>
      )}
      {compose && <Composer onSend={(q) => st.ask(id, q)} disabled={!chat.first || job.phase === 'working'} placeholder={`Ask about ${chat.subject.name}`} />}
    </div>
  );
}

export function Composer({ onSend, disabled, placeholder, children }: { onSend: (q: string) => void; disabled: boolean; placeholder: string; children?: ReactNode }) {
  const [q, setQ] = useState('');
  return (
    <form className="cl-compose" onSubmit={(e) => { e.preventDefault(); if (!q.trim() || disabled) return; onSend(q); setQ(''); }}>
      {children}
      <input value={q} onChange={(e) => setQ(e.target.value)} placeholder={placeholder} aria-label={placeholder} />
      <button type="submit" className="icon-btn" aria-label="Send" disabled={disabled || !q.trim()}><Send size={14} aria-hidden /></button>
    </form>
  );
}

// ---------- Let Claude pick: what changes against the current plan ----------

export interface PickRow { id: string; change: 'untick' | 'tick' | 'add'; why: string; p: Proposal }
export function pickRows(plan: string[]): PickRow[] {
  const all = [...RULE_PROPOSALS, ...Object.values(AI_ITEMS)];
  return PICK_CHANGES.flatMap((c) => {
    const p = all.find((x) => x.id === c.id);
    if (!p) return [];
    if (c.change === 'untick' && !plan.includes(c.id)) return [];
    if (c.change === 'tick' && plan.includes(c.id)) return [];
    return [{ ...c, p }];
  });
}

export function PickDiff({ actions = true }: { actions?: boolean }) {
  const st = useS();
  const plan = st.s.plan;
  const rows = pickRows(plan);
  const next = st.pickPlan(plan);
  const all = [...st.proposals, ...Object.values(AI_ITEMS)];
  const before = planTotals(st.proposals, plan).bytes;
  const after = planTotals(all, next).bytes;
  const unchanged = plan.filter((id) => RULE_PROPOSALS.some((p) => p.id === id && p.action === 'delete') && next.includes(id)).length;
  const label = { untick: 'Unticks', tick: 'Ticks', add: 'Adds, unticked' } as const;
  return (
    <div className="cl-diff" aria-label="Claude’s plan">
      <p className="cl-diff-total"><span>Your plan <b>{formatBytes(before)}</b></span><ArrowRight size={13} aria-hidden /><span>Claude’s <b>{formatBytes(after)}</b></span></p>
      <ul>
        {rows.map((r) => (
          <li key={r.id} className={`is-${r.change}`}>
            <span className="cl-diff-sign" aria-hidden>{r.change === 'untick' ? <Minus size={11} /> : r.change === 'tick' ? <Check size={11} strokeWidth={3} /> : <Plus size={11} />}</span>
            <div>
              <span className="cl-diff-k">{label[r.change]}</span>
              <b>{r.p.title}</b> <span className="cl-diff-size">{r.p.action === 'hint' ? 'reminder' : formatBytes(r.p.bytes)}</span>
              <p>{r.why}</p>
              {r.change === 'add' && <SourceTag source="ai" agent={AGENT} />}
            </div>
          </li>
        ))}
        {PICK_REFUSED.map((k) => {
          const r = REFUSED[k]!;
          return (
            <li key={k} className="is-refused">
              <span className="cl-diff-sign" aria-hidden><ShieldX size={11} /></span>
              <div><span className="cl-diff-k">Refused by the Guard</span><b>{r.title}</b> <span className="cl-diff-size">{formatBytes(r.bytes)}</span><p>{r.reason} It isn’t in Claude’s plan.</p></div>
            </li>
          );
        })}
      </ul>
      <p className="cl-diff-same">{unchanged} of your ticked items stay as they are. Tidy up isn’t changed.</p>
      {actions && (
        <div className="cl-diff-actions">
          <button type="button" className="btn" onClick={st.discardPick}>Keep mine</button>
          <button type="button" className="cta" onClick={st.acceptPick}>Use Claude’s plan</button>
        </div>
      )}
    </div>
  );
}

// ---------- confirm: the real dialog, with the line it would need about Claude ----------

export function summaryOf(proposals: Proposal[], tidy: TidyProposal[], plan: string[]): PlanSummary {
  const chosen = proposals.filter((p) => p.action === 'delete' && plan.includes(p.id));
  const sorts = tidy.filter((t) => plan.includes(t.id));
  const t = planTotals(proposals, plan);
  return {
    id: 'proto-plan', mountPoint: VOLUME.mountPoint, bytes: t.bytes, items: t.items, places: t.places,
    moves: sorts.reduce((n, x) => n + activeMoves(x, plan).length, 0),
    steps: [
      ...chosen.map((p) => ({ id: p.id, title: p.title, category: p.category, bytes: p.bytes, items: p.itemCount, places: p.placeCount, kind: 'delete' as const, moves: 0 })),
      ...sorts.map((x) => ({ id: x.id, title: `Sort ${x.name}`, category: 'downloads' as const, bytes: 0, items: 0, places: 0, kind: 'move' as const, moves: activeMoves(x, plan).length })),
    ],
    refused: [], movesRefused: [], leftForYou: proposals.filter((p) => p.action === 'hint' && plan.includes(p.id)).map((p) => p.id), duplicates: null,
  };
}

/** The lines about Claude's part in a plan, for the confirm dialog and the summary. */
export function claudeLines(proposals: Proposal[], plan: string[], layout: Record<string, string>): string[] {
  const mine = proposals.filter((p) => p.source === 'ai' && plan.includes(p.id));
  const sorts = RULE_TIDY.filter((t) => plan.includes(t.id) && layout[t.id] === 'claude').map((t) => t.name);
  const out: string[] = [];
  if (mine.length) out.push(`${mine.length === 1 ? '1 item was' : `${mine.length} items were`} suggested by Claude: ${listOf(mine.map((p) => `${p.title} (${formatBytes(p.bytes)})`))}. ${mine.length === 1 ? 'It' : 'They'} passed the Guard and you ticked ${mine.length === 1 ? 'it' : 'them'}.`);
  if (sorts.length) out.push(`${listOf(sorts)} ${sorts.length === 1 ? 'is' : 'are'} sorted into Claude’s folders. Moves are journaled, so Undo still works.`);
  return out;
}

export function Confirm() {
  const st = useS();
  const { s, proposals, tidy } = st;
  const plan = useMemo(() => summaryOf(proposals, tidy, s.plan), [proposals, tidy, s.plan]);
  const lines = claudeLines(proposals, s.plan, s.layout);
  const [host, setHost] = useState<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const dialog = document.querySelector('.scrim .dialog');
    const actions = dialog?.querySelector('.dialog-actions');
    if (!dialog || !actions || lines.length === 0) return;
    const div = document.createElement('div');
    div.className = 'cl-confirm-host';
    dialog.insertBefore(div, actions);
    setHost(div);
    return () => { div.remove(); setHost(null); };
  }, [lines.length]);
  return (
    <>
      <ConfirmDialog plan={plan} platform="windows" onCancel={() => st.set({ confirm: false })} onGo={() => st.set({ confirm: false, ended: true, planOpen: false, drawer: null, step: 'map' })} />
      {host && createPortal(
        <div className="cl-confirm" data-proposed>
          <span className="cl-proposed">Proposed line</span>
          {lines.map((l) => <p key={l}><Mark size={12} /> {l}</p>)}
        </div>, host)}
    </>
  );
}

/** Where the prototype stops: nothing runs. It says what the summary would add about Claude. */
export function Ended() {
  const st = useS();
  const { s, proposals } = st;
  const lines = claudeLines(proposals, s.plan, s.layout);
  return createPortal(
    <div className="scrim" data-no-ripple>
      <div className="dialog cl-ended" role="dialog" aria-label="Prototype ends here">
        <h2>The prototype stops here.</h2>
        <p className="dialog-lead">Nothing ran. In the app, the run and its summary come next.</p>
        <div className="cl-confirm" data-proposed>
          <span className="cl-proposed">Proposed for the summary</span>
          {lines.length ? lines.map((l) => <p key={l}><Mark size={12} /> {l.replace('passed the Guard and you ticked', 'were deleted after you ticked')}</p>) : <p>Nothing in this plan came from Claude, so the summary says nothing about it.</p>}
        </div>
        <div className="dialog-actions"><button type="button" className="btn" onClick={() => st.set({ ended: false })}>Back to the map</button></div>
      </div>
    </div>,
    document.body,
  );
}

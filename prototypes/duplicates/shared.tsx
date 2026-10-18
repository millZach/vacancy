// THROWAWAY: pieces the three duplicate-finder variants share. Production components are imported where they fit;
// these only fill the gaps a new proposal kind opens (a confirm line, a recap section, synthetic thumbnails).
import { Check, ChevronDown, Copy as CopyIcon, FileArchive, FileText, Film, Image, Music, Play, Plus, RotateCcw, TriangleAlert } from 'lucide-react';
import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { CategoryId, PlanSummary, Platform, Proposal, RunEvent, RunReport, Skip, StepReport } from '../../src/bridge';
import { formatBytes, formatCount, formatDuration } from '../../src/format';
import { Legend } from '../../src/map/Legend';
import { MapView, type TileLook } from '../../src/map/MapView';
import { freedModel, overviewModel, type AtlasModel, type Freed, type Leaf, type Rect } from '../../src/map/model';
import { useSize } from '../../src/map/useSize';
import { Checkbox, RiskChip, SourceTag } from '../../src/plan/bits';
import { tileOf } from '../../src/plan/totals';
import { foldEvents, settleRun, startRun, type RunView } from '../../src/run/progress';
import type { ShownRun } from '../../src/run/RunPanel';
import { HeroNumber } from '../../src/summary/HeroNumber';
import { countLine, freedOf, keptOf, removedOf, type DupData, type Folder, type Group, type Kind } from './data';

export const DUPS = 'dups';

// ---------- the flow's shared state ----------

export interface Totals {
  bytes: number;
  items: number;
  places: number;
  dupBytes: number;
  dupCopies: number;
  dupGroups: number;
}

export interface Ctx {
  platform: Platform;
  d: DupData;
  /** Background hashing, 0..1, and whether it has finished. */
  hash: { t: number; done: boolean };
  /** Other proposals in the plan, by id. */
  on: string[];
  toggle: (id: string) => void;
  dupsOn: boolean;
  setDupsOn: (on: boolean) => void;
  excluded: string[];
  toggleGroup: (id: string) => void;
  totals: Totals;
  onFree: () => void;
  hot: string | null;
  setHot: (id: string | null) => void;
}

export function included(d: DupData, excluded: readonly string[]): Group[] {
  return d.groups.filter((g) => !excluded.includes(g.id));
}

export function totalsOf(d: DupData, on: readonly string[], dupsOn: boolean, excluded: readonly string[]): Totals {
  const ps = d.proposals.filter((p) => on.includes(p.id));
  const gs = dupsOn ? included(d, excluded) : [];
  const dupBytes = gs.reduce((s, g) => s + freedOf(g), 0);
  const dupCopies = gs.reduce((s, g) => s + g.copies.length - 1, 0);
  return {
    bytes: ps.reduce((s, p) => s + p.bytes, 0) + dupBytes,
    items: ps.reduce((s, p) => s + p.itemCount, 0) + dupCopies,
    places: ps.reduce((s, p) => s + p.placeCount, 0) + dupCopies,
    dupBytes,
    dupCopies,
    dupGroups: gs.length,
  };
}

/** What hashing says about itself at `t`: size matches first, then partial hashes, then full ones. */
export function hashLine(d: DupData, t: number) {
  const pct = Math.round(t * 100);
  const partial = t < 0.38;
  const files = d.groups.flatMap((g) => g.copies);
  const at = files[Math.floor(t * 997) % files.length]!;
  return {
    pct,
    label: `Checking ${formatCount(d.sizeMatches)} files… ${pct}%`,
    stage: partial
      ? `${formatCount(d.sizeMatches)} files over 1 MB share a size. Reading the start of each.`
      : `${formatCount(d.partialMatches)} still match. Hashing them in full.`,
    path: at.path,
  };
}

// ---------- small bits ----------

/** The round + or tick in a reclaimable tile's corner while the plan tray is open (Atlas's, not exported). */
export function Pin({ on }: { on: boolean }) {
  return (
    <span className={on ? 'map-pin is-on' : 'map-pin'} aria-hidden>
      {on ? <Check size={11} strokeWidth={3} /> : <Plus size={11} strokeWidth={2.6} />}
    </span>
  );
}

export function KindIcon({ kind, size = 14 }: { kind: Kind; size?: number }) {
  const I = kind === 'photo' ? Image : kind === 'video' ? Film : kind === 'audio' ? Music : kind === 'archive' ? FileArchive : FileText;
  return <I size={size} aria-hidden />;
}

function seed(s: string): number {
  let h = 2166136261;
  for (const ch of s) h = Math.imul(h ^ ch.charCodeAt(0), 16777619);
  return Math.abs(h);
}

/** A synthetic preview: a made-up landscape for photos, a frame for videos, a page for documents. No file is read. */
export function Thumb({ group, gone = false, size = 'md', label }: { group: Group; gone?: boolean; size?: 'sm' | 'md' | 'lg'; label?: string }) {
  const h = seed(group.id);
  const style = { '--th-a': (h % 360).toString(), '--th-b': ((h >> 7) % 360).toString(), '--th-y': `${38 + (h % 30)}%` } as CSSProperties;
  return (
    <span className={`thumb thumb--${group.kind} thumb--${size}${gone ? ' is-gone' : ''}`} style={style} aria-hidden>
      {group.kind === 'video' && <Play size={size === 'lg' ? 22 : 12} className="thumb-play" />}
      {(group.kind === 'document' || group.kind === 'archive' || group.kind === 'audio') && <KindIcon kind={group.kind} size={size === 'lg' ? 30 : 15} />}
      {label && <em className="thumb-tag">{label}</em>}
    </span>
  );
}

/** One of the drive's other proposals, drawn like PlanTray's row (which isn't exported). */
export function ProposalRow({ p, on, onToggle, hot, setHot }: { p: Proposal; on: boolean; onToggle: () => void; hot: boolean; setHot: (id: string | null) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={['tray-row', on && 'is-on', hot && 'is-hot'].filter(Boolean).join(' ')} onMouseEnter={() => setHot(tileOf(p.id))} onMouseLeave={() => setHot(null)}>
      <Checkbox on={on} onChange={onToggle} label={`Include ${p.title}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={() => setOpen(!open)} aria-expanded={open}>
          <span>{p.title}</span>
          <ChevronDown size={13} className={open ? 'is-open' : undefined} aria-hidden />
        </button>
        <p className="tray-row-sum">{p.summary}</p>
        <div className="tray-row-meta">
          <RiskChip risk={p.risk} />
          <SourceTag source={p.source} agent={null} />
        </div>
        {open && (
          <div className="tray-row-more">
            <p>{p.why}</p>
            <ul className="tray-paths" aria-label="Sample paths">{p.samplePaths.map((s) => <li key={s}>{s}</li>)}</ul>
          </div>
        )}
      </div>
      <span className="tray-row-size">{formatBytes(p.bytes)}</span>
    </div>
  );
}

export function OtherRows({ c }: { c: Ctx }) {
  return (
    <>
      {c.d.proposals.map((p) => (
        <ProposalRow key={p.id} p={p} on={c.on.includes(p.id)} onToggle={() => c.toggle(p.id)} hot={c.hot === tileOf(p.id)} setHot={c.setHot} />
      ))}
    </>
  );
}

export function TrayHead({ title = 'Plan', sub = 'Click glowing areas on the map, or tick them here. Nothing changes until you confirm.', aside }: { title?: string; sub?: ReactNode; aside?: ReactNode }) {
  return (
    <div className="tray-head">
      <div>
        <h2>{title}</h2>
        <p>{sub}</p>
      </div>
      {aside}
    </div>
  );
}

export function TrayFoot({ c }: { c: Ctx }) {
  const t = c.totals;
  return (
    <div className="tray-foot">
      <div className="tray-total">
        <span className="tray-total-num">{formatBytes(t.bytes)}</span>
        <span className="tray-total-lbl">will be freed</span>
      </div>
      <ul className="tray-notes">
        {t.items > 0 ? <li>{formatCount(t.items)} items in {formatCount(t.places)} places, deleted permanently</li> : <li>No files will be deleted.</li>}
        {c.dupsOn && t.dupGroups > 0 && <li>One copy of each of {t.dupGroups} duplicate groups stays</li>}
      </ul>
      <button type="button" className="cta tray-cta" disabled={t.items === 0} onClick={c.onFree}>
        {t.items > 0 ? `Free ${formatBytes(t.bytes)}` : 'Nothing selected yet'}
      </button>
    </div>
  );
}

/** "Kept: oldest, in Pictures" as a chip. */
export function Reason({ g }: { g: Group }) {
  return <span className="keep-reason"><Check size={11} strokeWidth={2.6} aria-hidden />{g.reason}</span>;
}

/** Why a removed copy is the extra one, in a word or two. */
export function whyExtra(c: Group['copies'][number], g: Group): string {
  if (c.folder === 'Downloads') return 'In Downloads';
  if (/( - Copy| copy|\(\d\))/i.test(c.file)) return 'Named like a copy';
  return c.modifiedDays < keptOf(g).modifiedDays ? 'Newer copy' : 'Same age, other place';
}

export function ago(days: number): string {
  if (days < 45) return `${days} days ago`;
  if (days < 548) return `${Math.round(days / 30.4)} months ago`;
  return `${(days / 365).toFixed(1).replace(/\.0$/, '')} years ago`;
}

// ---------- the map ----------

export interface MapProps {
  c: Ctx;
  /** The run, while it runs: tiles drain. */
  run?: ShownRun | null;
  /** Extra classes and badges by tile, beyond the plan's look. */
  extraLook?: (leaf: Leaf, rect: Rect) => TileLook | null;
  /** Things in the toolbar after the breadcrumb. */
  tools?: ReactNode;
  /** Things over the stage (a HUD, link lines). Gets the stage's size and the map's rects. */
  overlay?: (size: { w: number; h: number }, model: AtlasModel) => ReactNode;
  onLeaf?: (leaf: Leaf) => void;
  status?: (leaf: Leaf) => ReactNode;
}

export function DupMap({ c, run = null, extraLook, tools, overlay, onLeaf, status }: MapProps) {
  const [ref, size] = useSize<HTMLDivElement>();
  const overview = c.hash.done ? c.d.overviewAfter : c.d.overviewBefore;
  const top = useMemo(() => overviewModel(overview), [overview]);
  const freed = useMemo(() => (run ? freedTiles(c, run.view) : null), [c, run]);
  const shown = useMemo(() => (run && freed ? freedModel(top, freed, run.view.freedBytes) : top), [top, run, freed]);
  const byId = useMemo(() => new Map(c.d.proposals.map((p) => [p.id, p])), [c.d]);
  const look = (leaf: Leaf, rect: Rect): TileLook => {
    if (leaf.proposal === null) return extraLook?.(leaf, rect) ?? {};
    if (run) return runLook(run.view, leaf, c.excluded, c.d);
    const on = leaf.proposal === DUPS ? c.dupsOn : c.on.includes(leaf.proposal);
    const base: TileLook = on
      ? { cls: 'is-reclaim is-selected', badge: <Pin on />, state: 'reclaimable, in the plan', pressed: true }
      : { cls: 'is-available', badge: <Pin on={false} />, state: 'reclaimable, not in the plan', pressed: false };
    const extra = extraLook?.(leaf, rect);
    return extra ? { ...base, ...extra, cls: [base.cls, extra.cls].filter(Boolean).join(' ') } : base;
  };
  const leafStatus = (leaf: Leaf): ReactNode => {
    if (leaf.proposal === null) return null;
    const own = status?.(leaf);
    if (own) return own;
    const p = byId.get(leaf.proposal);
    if (!p) return null;
    return (
      <>
        <div className="map-tip-meta"><RiskChip risk={p.risk} /><SourceTag source={p.source} agent={null} /></div>
        <p className="map-tip-sum">{p.summary}</p>
        <p className="map-tip-act">{c.on.includes(p.id) ? 'Click to take it out of the plan.' : 'Click to add it to the plan.'}</p>
      </>
    );
  };
  const click = (leaf: Leaf) => {
    if (run) return;
    if (onLeaf) return onLeaf(leaf);
    if (leaf.proposal === DUPS) c.setDupsOn(!c.dupsOn);
    else if (leaf.proposal) c.toggle(leaf.proposal);
  };
  return (
    <main className="atlas is-map">
      <div className="atlas-toolbar">
        <nav className="crumbs" aria-label="Map location">
          <span className="crumb is-here" aria-current="location">{overview.volume.name}</span>
        </nav>
        {tools}
        <Legend bytesInView={overview.volume.totalBytes} w={size.w} h={size.h} reclaimable={!run} vacant={!!run} />
      </div>
      <div className="atlas-stage" ref={ref}>
        <MapView
          model={shown}
          {...(run ? { base: top } : {})}
          width={size.w}
          height={size.h}
          focus={null}
          interactive
          hot={c.hot}
          onHot={c.setHot}
          onLeaf={click}
          clickable={(l) => !run && l.proposal !== null}
          leafAction={() => (run ? null : 'Folders don’t open in this prototype.')}
          look={look}
          leafStatus={leafStatus}
          planning={!run}
        />
        {overlay?.(size, top)}
      </div>
    </main>
  );
}

/** What each tile has freed so far. The duplicates step's freed bytes are shared across its tiles by what they hold. */
function freedTiles(c: Ctx, view: RunView): Map<string, Freed> {
  const m = new Map<string, Freed>();
  for (const s of view.steps) {
    if (s.id !== DUPS) {
      m.set(tileOf(s.id), { bytes: s.freedBytes, kept: s.skips.reduce((n, k) => n + k.bytes, 0) });
      continue;
    }
    const per = folderBytes(included(c.d, c.excluded));
    const all = [...per.values()].reduce((a, b) => a + b, 0);
    for (const [f, id] of Object.entries(c.d.tiles) as [Folder, string][]) {
      const mine = per.get(f) ?? 0;
      const whole = folderBytes(c.d.groups).get(f) ?? 0;
      m.set(id, { bytes: all > 0 ? (s.freedBytes * mine) / all : 0, kept: whole - mine });
    }
  }
  return m;
}

export function folderBytes(groups: Group[]): Map<Folder, number> {
  const m = new Map<Folder, number>();
  for (const g of groups) for (const r of removedOf(g)) m.set(r.folder, (m.get(r.folder) ?? 0) + g.bytes);
  return m;
}

function runLook(view: RunView, leaf: Leaf, excluded: readonly string[], d: DupData): TileLook {
  const step = view.steps.find((s) => s.id === leaf.proposal);
  if (!step) return {};
  if (step.status === 'pending') return { cls: 'is-queued', state: 'waiting to be deleted' };
  if (step.status === 'running') return { cls: 'is-running', state: 'being deleted' };
  if (step.freedBytes <= 0) return {};
  const bytes = leaf.proposal === DUPS ? share(view, leaf, excluded, d) : step.freedBytes;
  return { cls: 'is-vacant is-lit', size: <span className="map-freed">{formatBytes(bytes)}</span>, state: 'vacant' };
}

function share(view: RunView, leaf: Leaf, excluded: readonly string[], d: DupData): number {
  const f = (Object.entries(d.tiles) as [Folder, string][]).find(([, id]) => id === leaf.id)?.[0];
  const per = folderBytes(included(d, excluded));
  const all = [...per.values()].reduce((a, b) => a + b, 0);
  const s = view.steps.find((x) => x.id === DUPS)!;
  return f && all > 0 ? (s.freedBytes * (per.get(f) ?? 0)) / all : 0;
}

// ---------- confirm ----------

export function DupConfirm({ c, onCancel, onGo }: { c: Ctx; onCancel: () => void; onGo: () => void }) {
  const t = c.totals;
  const bin = c.platform === 'macos' ? 'Trash' : 'Recycle Bin';
  const left = c.d.groups.length - t.dupGroups;
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === 'Escape' && onCancel();
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  }, [onCancel]);
  return createPortal(
    <div className="scrim" onClick={onCancel}>
      <div className="dialog dup-dialog" role="alertdialog" aria-modal="true" aria-labelledby="dup-confirm" onClick={(e) => e.stopPropagation()}>
        <h2 id="dup-confirm">Permanently delete {formatBytes(t.bytes)}?</h2>
        <p className="dialog-lead">
          {formatCount(t.items)} items in {formatCount(t.places)} places will be deleted for good. They skip the {bin}, so they can’t be brought back.
        </p>
        {c.dupsOn && t.dupGroups > 0 && (
          <div className="dup-confirm">
            <CopyIcon size={16} aria-hidden />
            <div>
              <b>Duplicate files: only the extra copies go.</b>
              <p>
                {formatCount(t.dupCopies)} extra {t.dupCopies === 1 ? 'copy' : 'copies'} ({formatBytes(t.dupBytes)}) are deleted. <strong>One copy of each of the {t.dupGroups} groups stays where it is.</strong>
              </p>
              {left > 0 && <p className="dialog-quiet">{left} {left === 1 ? 'group you unticked is' : 'groups you unticked are'} left alone, every copy kept.</p>}
              <p className="dialog-quiet">Before each copy goes, Vacancy checks the kept copy is still there and still the same.</p>
            </div>
          </div>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel} autoFocus>Go back</button>
          <button type="button" className="cta cta--danger" onClick={onGo}>Delete {formatBytes(t.bytes)} permanently</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ---------- the simulated run ----------

interface SimStep {
  id: string;
  title: string;
  why: string;
  category: CategoryId;
  regenerable: boolean;
  bytes: number;
  items: number;
  places: number;
  paths: string[];
  skip: Skip | null;
}

function simSteps(c: Ctx): SimStep[] {
  const steps: SimStep[] = c.d.proposals.filter((p) => c.on.includes(p.id)).map((p) => ({
    id: p.id, title: p.title, why: p.why, category: p.category, regenerable: p.regenerable, bytes: p.bytes, items: p.itemCount, places: p.placeCount, paths: p.samplePaths, skip: null,
  }));
  if (c.dupsOn && c.totals.dupGroups > 0) {
    const gs = included(c.d, c.excluded);
    const changed = gs.find((g) => g.kind === 'document');
    const skip: Skip | null = changed ? { path: removedOf(changed)[0]!.path, reason: 'It changed since the scan', kind: 'changed', bytes: changed.bytes, items: 1 } : null;
    steps.push({
      id: DUPS, title: 'Duplicate files', why: 'Exact copies, byte for byte. One copy of each stays.', category: 'media', regenerable: false,
      bytes: c.totals.dupBytes, items: c.totals.dupCopies, places: c.totals.dupCopies, paths: gs.flatMap((g) => removedOf(g).map((r) => r.path)), skip,
    });
  }
  return steps.sort((a, b) => b.bytes - a.bytes);
}

export interface SimRun {
  run: ShownRun;
  report: RunReport | null;
}

/** Starts a mock run of the plan: events arrive over `ms`, folded by the real run view. */
export function useSimRun(c: Ctx, started: boolean, ms: number): SimRun | null {
  const [state, setState] = useState<SimRun | null>(null);
  const ctx = useRef(c);
  ctx.current = c;
  useEffect(() => {
    if (!started) {
      setState(null);
      return;
    }
    const steps = simSteps(ctx.current);
    const plan: PlanSummary = {
      id: 'plan-proto', mountPoint: ctx.current.d.overviewAfter.volume.mountPoint, bytes: ctx.current.totals.bytes, items: ctx.current.totals.items,
      places: ctx.current.totals.places, moves: 0, steps: steps.map((s) => ({ id: s.id, title: s.title, category: s.category, bytes: s.bytes, items: s.items, places: s.places })),
      refused: [], leftForYou: [],
    };
    const reports: StepReport[] = [];
    const events: RunEvent[][] = [];
    let total = 0;
    steps.forEach((s, i) => {
      events.push([{ kind: 'stepStarted', step: i, id: s.id }]);
      const freed = s.bytes - (s.skip?.bytes ?? 0);
      const deleted = s.items - (s.skip?.items ?? 0);
      const ticks = 4;
      for (let k = 1; k <= ticks; k++) {
        const batch: RunEvent[] = [{ kind: 'progress', step: i, freedBytes: (freed * k) / ticks, deletedItems: Math.round((deleted * k) / ticks), totalFreedBytes: total + (freed * k) / ticks, currentPath: s.paths[k % Math.max(1, s.paths.length)] ?? '' }];
        const lines = s.id === DUPS ? s.paths.slice((k - 1) * 4, k * 4) : s.paths.slice(k - 1, k);
        for (const path of lines) if (path !== s.skip?.path) batch.push({ kind: 'deleted', step: i, path, bytes: freed / Math.max(1, s.paths.length), items: 1, whole: true });
        if (k === 2 && s.skip) batch.push({ kind: 'skipped', step: i, skip: s.skip });
        events.push(batch);
      }
      total += freed;
      const report: StepReport = {
        id: s.id, title: s.title, why: s.why, category: s.category, regenerable: s.regenerable, plannedBytes: s.bytes, freedBytes: freed,
        plannedItems: s.items, deletedItems: deleted, places: s.places, outcome: s.skip ? 'partial' : 'done', skipped: s.skip ? [s.skip] : [],
      };
      reports.push(report);
      events.push([{ kind: 'stepFinished', step: i, report }]);
    });
    const report = buildReport(ctx.current, plan, reports, ms);
    let view = startRun(plan);
    let at = 0;
    setState({ run: { phase: 'running', plan, view, stopping: false }, report: null });
    const every = Math.max(30, ms / Math.max(1, events.length));
    const timer = setInterval(() => {
      const batch = events[at++];
      if (!batch) {
        clearInterval(timer);
        setState({ run: { phase: 'ended', plan, view: settleRun(view, report), report }, report });
        return;
      }
      view = foldEvents(view, batch);
      setState({ run: { phase: 'running', plan, view, stopping: false }, report: null });
    }, every);
    return () => clearInterval(timer);
  }, [started, ms]);
  return state;
}

function buildReport(c: Ctx, plan: PlanSummary, steps: StepReport[], ms: number): RunReport {
  const ov = c.d.overviewAfter;
  const freedBytes = steps.reduce((s, x) => s + x.freedBytes, 0);
  const byCat = new Map<CategoryId, number>();
  for (const s of splitDups(c, steps)) byCat.set(s.category, (byCat.get(s.category) ?? 0) + s.freedBytes);
  const win = [...steps].sort((a, b) => b.freedBytes - a.freedBytes)[0];
  return {
    id: 'run-proto', planId: plan.id, mountPoint: ov.volume.mountPoint, volumeName: ov.volume.name, startedAt: Date.now(), durationMs: ms,
    cancelled: false, steps, plannedBytes: plan.bytes, freedBytes, deletedItems: steps.reduce((s, x) => s + x.deletedItems, 0), movedItems: 0,
    skippedItems: steps.reduce((s, x) => s + x.skipped.reduce((n, k) => n + k.items, 0), 0),
    freedByCategory: [...byCat].map(([id, bytes]) => ({ id, bytes })).sort((a, b) => b.bytes - a.bytes),
    totalBytes: ov.volume.totalBytes, usedBefore: ov.volume.usedBytes, usedAfter: ov.volume.usedBytes - freedBytes, freeBefore: ov.freeBytes,
    freeAfter: ov.freeBytes + freedBytes, freeChange: freedBytes, biggestWin: win?.id ?? null, leftForYou: [], reorganized: [], map: ov,
  };
}

/** The report as the summary map reads it: the duplicates step split per tile, so each tile turns vacant. */
export function splitDups(c: Ctx, steps: StepReport[]): StepReport[] {
  return steps.flatMap((s) => {
    if (s.id !== DUPS) return [s];
    const per = folderBytes(included(c.d, c.excluded));
    const all = [...per.values()].reduce((a, b) => a + b, 0);
    return [...per].map(([f, bytes]) => ({
      ...s, id: `dups:${f}`, title: `Duplicate copies in ${f}`, category: f === 'Downloads' ? 'downloads' as const : 'media' as const,
      freedBytes: all > 0 ? (s.freedBytes * bytes) / all : 0, plannedBytes: bytes,
    }));
  });
}

// ---------- recap ----------

/** The summary panel with a duplicates section (SummaryPanel's markup and styles; the section is new). */
export function RecapPanel({ c, report, onRestart }: { c: Ctx; report: RunReport; onRestart: () => void }) {
  const dup = report.steps.find((s) => s.id === DUPS);
  const gs = included(c.d, c.excluded);
  const left = c.d.groups.filter((g) => c.excluded.includes(g.id));
  const name = (id: CategoryId) => report.map.categories.find((x) => x.id === id)?.name ?? id;
  const cats = report.freedByCategory.filter((x) => x.bytes > 0).map((x) => ({ ...x, before: report.map.categories.find((k) => k.id === x.id)?.bytes ?? x.bytes }));
  const widest = Math.max(1, ...cats.map((x) => x.before));
  const pct = (b: number) => `${Math.max(0, Math.min(1, b / report.totalBytes)) * 100}%`;
  return (
    <aside className="tray summary" aria-label="What Vacancy did">
      <div className="summary-scroll">
        <section className="summary-hero">
          <h1 className="summary-num" aria-label={`${formatBytes(report.freedBytes)} freed`}><HeroNumber bytes={report.freedBytes} /></h1>
          <p className="summary-sub">back on {report.volumeName}. Free space went from {formatBytes(report.freeBefore)} to {formatBytes(report.freeAfter)}.</p>
        </section>
        <section className="diskba" aria-label="Disk before and after">
          {[['Before', report.usedBefore, 0, report.freeBefore], ['After', report.usedAfter, report.freedBytes, report.freeAfter]].map(([label, used, freed, free]) => (
            <div className="diskbar" key={label as string}>
              <span className="diskbar-label">{label}</span>
              <span className="diskbar-track" aria-hidden><i className="diskbar-used" style={{ width: pct(used as number) }} />{(freed as number) > 0 && <i className="diskbar-freed" style={{ width: pct(freed as number) }} />}</span>
              <span className="diskbar-num">{formatBytes(free as number)} free</span>
            </div>
          ))}
        </section>
        <dl className="stats">
          <div><dt>items deleted</dt><dd>{formatCount(report.deletedItems)}</dd></div>
          <div><dt>files moved</dt><dd>0</dd></div>
          <div><dt>start to finish</dt><dd>{formatDuration(report.durationMs)}</dd></div>
        </dl>
        {dup && (
          <section className="summary-sec dup-recap" aria-label="Duplicate files">
            <h3>Duplicate files</h3>
            <div className="win">
              <span className="win-num">{formatBytes(dup.freedBytes)}</span>
              <span className="win-text">
                <b>{formatCount(dup.deletedItems)} extra copies deleted</b>
                <span>One copy of each of {gs.length} groups is still where it was: {countLine(gs)} had a twin.</span>
              </span>
            </div>
            <ul className="dup-recap-pairs" aria-label="Sample pairs">
              {samples(gs).map((g) => (
                <li key={g.id}>
                  <Thumb group={g} size="sm" />
                  <span><b>{g.name}</b><small>{g.reason.replace('Kept: ', 'Kept ')} · {removedOf(g).length} {removedOf(g).length === 1 ? 'copy' : 'copies'} gone</small></span>
                </li>
              ))}
            </ul>
            {left.length > 0 && <p className="summary-note">You left {left.length === 1 ? `${left[0]!.name} alone, both copies kept` : `${left.length} groups alone, every copy kept`}.</p>}
          </section>
        )}
        {dup && dup.skipped.length > 0 && (
          <section className="summary-sec">
            <h3>Skipped</h3>
            <ul className="skips">
              {dup.skipped.map((k) => (
                <li key={k.path}><TriangleAlert size={13} aria-hidden /><span><b>{k.reason}.</b> {formatBytes(k.bytes)} stayed. Scan again to see it as it is now. Its kept copy was never touched.<code>{k.path}</code></span></li>
              ))}
            </ul>
          </section>
        )}
        {cats.length > 0 && (
          <section className="summary-sec">
            <h3>Freed by category</h3>
            <ul className="cats">
              {cats.map((x) => (
                <li key={x.id} data-cat={x.id}>
                  <span className="cat-name"><i aria-hidden />{name(x.id)}</span>
                  <span className="cat-num">{formatBytes(x.bytes)}</span>
                  <span className="cat-bar" style={{ width: `${(x.before / widest) * 100}%` }} aria-hidden>
                    <i className="cat-kept" style={{ width: `${Math.max(0, (x.before - x.bytes) / x.before) * 100}%` }} />
                    <i className="cat-freed" style={{ width: `${Math.min(1, x.bytes / x.before) * 100}%` }} />
                  </span>
                  <span className="cat-ba">{formatBytes(x.before, 0)} to {formatBytes(Math.max(0, x.before - x.bytes))}</span>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
      <div className="summary-foot">
        <button type="button" className="btn" onClick={onRestart}><RotateCcw size={14} aria-hidden /> Start over</button>
      </div>
    </aside>
  );
}

/** One photo, one video and one other group, for the recap's sample pairs. */
function samples(gs: Group[]): Group[] {
  const pick = (f: (g: Group) => boolean) => gs.find(f);
  return [pick((g) => g.kind === 'photo'), pick((g) => g.kind === 'video'), pick((g) => g.kind !== 'photo' && g.kind !== 'video')].filter((g): g is Group => !!g);
}

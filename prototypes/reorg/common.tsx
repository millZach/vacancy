// THROWAWAY: pieces every variant shares. The real app components are imported; only the reorganization parts are
// drawn here, with the app's own classes and tokens.
import { useEffect, useId, useState, type ReactNode } from 'react';
import { ArrowRight, ChevronDown, ChevronRight, CornerDownRight, File, Folder, FolderInput, History, LoaderCircle, Map as MapIcon, Play, RotateCcw, Signpost, TriangleAlert, Undo2, X } from 'lucide-react';
import type { Bridge, PlanSummary, Platform, Proposal, RunReport, Skip, StepReport } from '../../src/bridge';
import { BridgeProvider } from '../../src/bridge/BridgeProvider';
import { fixtures } from '../../src/bridge/fixtures';
import { formatBytes, formatCount, formatDuration } from '../../src/format';
import { Checkbox, RiskChip, SourceTag } from '../../src/plan/bits';
import { tileOf } from '../../src/plan/totals';
import { startRun, type RunView } from '../../src/run/progress';
import type { ShownRun } from '../../src/run/RunPanel';
import { HeroNumber } from '../../src/summary/HeroNumber';
import { formatWhen } from '../../src/summary/when';
import { foldersMade, HOME, movesOf, REORGS, sampleMoves, type Dest, type Reorg, type TreeNode } from './data';

// ---------- no bridge ----------

/** Every call is counted and refused. Only `platform` is read (by the real finish); the proof checks the count. */
export function NoBridge({ platform, children }: { platform: Platform; children: ReactNode }) {
  const refuse = (name: string) => () => {
    (window as unknown as { __bridgeCalls: string[] }).__bridgeCalls.push(name);
    return Promise.reject(new Error('Prototype: no bridge. Nothing on disk is read or changed.'));
  };
  const bridge = new Proxy({ platform } as Bridge, {
    get: (target, key) => (key === 'platform' ? target.platform : refuse(String(key))),
  });
  return <BridgeProvider bridge={bridge}>{children}</BridgeProvider>;
}
(window as unknown as { __bridgeCalls: string[] }).__bridgeCalls = [];

// ---------- plan state ----------

export type Stage = 'plan' | 'run' | 'slide' | 'recap';
export type UndoState = Record<string, 'working' | 'done'>;

export interface Flow {
  platform: Platform;
  stage: Stage;
  /** Delete proposals in the plan. */
  deletes: string[];
  /** Reorganizations in the plan, by folder id. Start empty: unchecked, like every review item. */
  picked: string[];
  /** Destinations left out (variant C's per-folder exclude). */
  excluded: string[];
  confirming: boolean;
  running: boolean;
  undo: UndoState;
  /** When the simulated run happened, for the history. */
  ranAt: number | null;
}

export interface FlowApi {
  flow: Flow;
  proposals: Proposal[];
  set: (patch: Partial<Flow>) => void;
  toggleDelete: (id: string) => void;
  togglePick: (id: string) => void;
  toggleDest: (id: string) => void;
  review: () => void;
  undo: (id: string) => void;
  reset: () => void;
}

export const deleteProposals = (platform: Platform) => fixtures[platform].proposals.filter((p) => p.action === 'delete');
export const hintProposals = (platform: Platform) => fixtures[platform].proposals.filter((p) => p.action === 'hint');
export const reclaimableOf = (platform: Platform) => deleteProposals(platform).reduce((n, p) => n + p.bytes, 0);

/** Moves a folder's sort makes, with any destinations left out. */
export function plannedMoves(r: Reorg, excluded: readonly string[]): number {
  return r.dests.filter((d) => !excluded.includes(d.id)).reduce((n, d) => n + d.count, 0);
}
export const destsIn = (r: Reorg, excluded: readonly string[]) => r.dests.filter((d) => !excluded.includes(d.id));
/** Files the simulated run leaves where they are (open, or changed after review). Only when it has files to move. */
export const runSkips = (r: Reorg, excluded: readonly string[]) => (plannedMoves(r, excluded) > 0 ? r.run.skipped : []);
export const movedFiles = (r: Reorg, excluded: readonly string[]) => plannedMoves(r, excluded) - runSkips(r, excluded).length;
/** The files Undo can't put back: only if their destination was in the plan. */
export function undoFailed(r: Reorg, excluded: readonly string[]) {
  const there = r.dests.find((d) => d.path === r.undo.failed.where);
  return there && !excluded.includes(there.id) ? r.undo.failed : { ...r.undo.failed, count: 0, names: [] };
}

export function pickedReorgs(flow: Flow): Reorg[] {
  if (flow.platform !== 'windows') return [];
  return REORGS.filter((r) => flow.picked.includes(r.id) && plannedMoves(r, flow.excluded) > 0);
}

export function totals(flow: Flow, proposals: readonly Proposal[]) {
  const chosen = proposals.filter((p) => p.action === 'delete' && flow.deletes.includes(p.id));
  const reorgs = pickedReorgs(flow);
  return {
    bytes: chosen.reduce((n, p) => n + p.bytes, 0),
    items: chosen.reduce((n, p) => n + p.itemCount, 0),
    places: chosen.reduce((n, p) => n + p.placeCount, 0),
    deletes: chosen.length,
    moves: reorgs.reduce((n, r) => n + plannedMoves(r, flow.excluded), 0),
    folders: reorgs.map((r) => r.folder),
  };
}

/** The plan as the confirm dialog gets it. Sorts are steps of their own that free 0 bytes. */
export function buildPlan(flow: Flow, proposals: readonly Proposal[]): PlanSummary {
  const t = totals(flow, proposals);
  const chosen = proposals.filter((p) => p.action === 'delete' && flow.deletes.includes(p.id));
  return {
    id: 'plan-prototype',
    mountPoint: fixtures[flow.platform].overview.volume.mountPoint,
    bytes: t.bytes,
    items: t.items,
    places: t.places,
    moves: t.moves,
    steps: [
      ...chosen.map((p) => ({ id: p.id, title: p.title, category: p.category, bytes: p.bytes, items: p.itemCount, places: p.placeCount })),
      ...pickedReorgs(flow).map((r) => ({ id: `sort-${r.id}`, title: `Sort ${r.folder}`, category: 'downloads' as const, bytes: 0, items: 0, places: 0 })),
    ],
    refused: [],
    leftForYou: [],
  };
}

const skipOf = (r: Reorg, s: Reorg['run']['skipped'][number]): Skip => ({
  path: `${r.path}\\${s.name}`,
  reason: s.reason,
  kind: /open/.test(s.reason) ? 'inUse' : 'changed',
  bytes: s.bytes,
  items: 1,
});

/** What the simulated run reports. Only deletes free space; the sorts report moved files and what stayed. */
export function buildReport(flow: Flow, proposals: readonly Proposal[]): RunReport {
  const fixture = fixtures[flow.platform];
  const overview = fixture.overview;
  const chosen = proposals.filter((p) => p.action === 'delete' && flow.deletes.includes(p.id));
  const steps: StepReport[] = chosen.map((p) => ({
    id: p.id, title: p.title, why: p.why, category: p.category, regenerable: p.regenerable,
    plannedBytes: p.bytes, freedBytes: p.bytes, plannedItems: p.itemCount, deletedItems: p.itemCount, places: p.placeCount,
    outcome: 'done', skipped: [],
  }));
  const freed = steps.reduce((n, s) => n + s.freedBytes, 0);
  const byCat = new Map<StepReport['category'], number>();
  for (const s of steps) byCat.set(s.category, (byCat.get(s.category) ?? 0) + s.freedBytes);
  const reorgs = pickedReorgs(flow);
  const win = [...steps].sort((a, b) => b.freedBytes - a.freedBytes)[0];
  return {
    id: 'run-prototype', planId: 'plan-prototype',
    mountPoint: overview.volume.mountPoint, volumeName: overview.volume.name,
    startedAt: flow.ranAt ?? Date.now(), durationMs: 38_400 + reorgs.length * 4_100, cancelled: false,
    steps, plannedBytes: freed, freedBytes: freed,
    deletedItems: steps.reduce((n, s) => n + s.deletedItems, 0),
    movedItems: reorgs.reduce((n, r) => n + movedFiles(r, flow.excluded), 0),
    skippedItems: reorgs.reduce((n, r) => n + runSkips(r, flow.excluded).length, 0),
    freedByCategory: [...byCat].map(([id, bytes]) => ({ id, bytes })).sort((a, b) => b.bytes - a.bytes),
    totalBytes: overview.volume.totalBytes, usedBefore: overview.volume.usedBytes, usedAfter: overview.volume.usedBytes - freed,
    freeBefore: overview.freeBytes, freeAfter: overview.freeBytes + freed, freeChange: freed,
    biggestWin: win?.id ?? null,
    leftForYou: [],
    reorganized: reorgs.map((r) => ({ id: r.id, folder: r.folder, movedItems: movedFiles(r, flow.excluded) })),
    map: overview,
  };
}

/** The run on the real run panel: running a moment, then done, with each sort's skips on its row and in the log. */
export function buildRun(flow: Flow, proposals: readonly Proposal[], running: boolean): ShownRun {
  const plan = buildPlan(flow, proposals);
  const start = startRun(plan);
  if (running) {
    const steps = start.steps.map((s, i) => (i === 0 ? { ...s, status: 'running' as const, freedBytes: s.plannedBytes * 0.4 } : s));
    return { phase: 'running', plan, view: { ...start, steps, freedBytes: steps[0]?.freedBytes ?? 0 }, stopping: false };
  }
  const report = buildReport(flow, proposals);
  const reorgs = pickedReorgs(flow);
  let n = 0;
  const log: RunView['log'] = [];
  const steps = start.steps.map((s) => {
    const r = reorgs.find((x) => `sort-${x.id}` === s.id);
    if (!r) {
      const rs = report.steps.find((x) => x.id === s.id);
      log.push({ id: n++, kind: 'sum', text: `${s.title}: ${formatBytes(rs?.freedBytes ?? 0)} freed` });
      return { ...s, status: 'done' as const, outcome: 'done' as const, freedBytes: rs?.freedBytes ?? 0, deletedItems: rs?.deletedItems ?? 0 };
    }
    const skips = runSkips(r, flow.excluded).map((k) => skipOf(r, k));
    for (const k of skips) log.push({ id: n++, kind: 'skip', text: `skipped ${k.path} (${k.reason.toLowerCase()})` });
    log.push({ id: n++, kind: 'sum', text: `Sort ${r.folder}: ${formatCount(movedFiles(r, flow.excluded))} files moved, nothing deleted, every move recorded` });
    return { ...s, status: 'done' as const, outcome: (skips.length ? 'partial' : 'done') as 'partial' | 'done', skips };
  });
  return {
    phase: 'ended', plan, report,
    view: { steps, current: steps.length - 1, freedBytes: report.freedBytes, skips: steps.reduce((x, s) => x + s.skips.length, 0), log, written: n },
  };
}

// ---------- the map's tiles for the three folders ----------

/** The scan's tile for a folder, so a tidy row and its tile light together. */
export function folderTile(platform: Platform, folder: string): string | null {
  const path = `${HOME}\\${folder}`;
  for (const c of fixtures[platform].overview.categories) {
    const item = c.items.find((i) => i.path === path || i.path.startsWith(`${path}\\`) && i.kind === 'files');
    if (item) return item.id;
  }
  return null;
}

// ---------- rows ----------

/** A delete proposal, as today's plan tray draws it. */
export function DeleteRow({ p, on, hot, onToggle, onHot }: { p: Proposal; on: boolean; hot: boolean; onToggle: () => void; onHot: (id: string | null) => void }) {
  const [open, setOpen] = useState(false);
  const more = useId();
  return (
    <div className={['tray-row', on && 'is-on', hot && 'is-hot'].filter(Boolean).join(' ')} onMouseEnter={() => onHot(tileOf(p.id))} onMouseLeave={() => onHot(null)}>
      <Checkbox on={on} onChange={onToggle} label={`Include ${p.title}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={() => setOpen(!open)} aria-expanded={open} aria-controls={more}>
          <span>{p.title}</span>
          <ChevronDown size={13} className={open ? 'is-open' : undefined} aria-hidden />
        </button>
        <p className="tray-row-sum">{p.summary}</p>
        <div className="tray-row-meta">
          <RiskChip risk={p.risk} />
          <SourceTag source={p.source} agent={null} />
        </div>
        {open && (
          <div className="tray-row-more" id={more}>
            <p>{p.why}</p>
            <ul className="tray-paths" aria-label="Sample paths">{p.samplePaths.map((s) => <li key={s}>{s}</li>)}</ul>
          </div>
        )}
      </div>
      <span className="tray-row-size">{formatBytes(p.bytes)}</span>
    </div>
  );
}

export function DeleteGroup({ api, hot, onHot, collapsed = false }: { api: FlowApi; hot: string | null; onHot: (id: string | null) => void; collapsed?: boolean }) {
  const [open, setOpen] = useState(!collapsed);
  const deletes = deleteProposals(api.flow.platform);
  const on = deletes.filter((p) => api.flow.deletes.includes(p.id)).length;
  return (
    <section className="tray-group" aria-label="Delete">
      <h3>
        {collapsed ? (
          <button type="button" className="group-toggle" onClick={() => setOpen(!open)} aria-expanded={open}>
            <ChevronRight size={13} className={open ? 'is-open' : undefined} aria-hidden /> Delete
          </button>
        ) : 'Delete'}{' '}
        <span>{on} of {deletes.length}</span>
      </h3>
      {open && deletes.map((p) => (
        <DeleteRow key={p.id} p={p} on={api.flow.deletes.includes(p.id)} hot={hot === tileOf(p.id)} onToggle={() => api.toggleDelete(p.id)} onHot={onHot} />
      ))}
    </section>
  );
}

export function HintGroup({ platform }: { platform: Platform }) {
  const hints = hintProposals(platform);
  if (!hints.length) return null;
  return (
    <section className="tray-group" aria-label="Left for you">
      <h3>Left for you <span>{hints.length}</span></h3>
      <p className="tray-group-note">Vacancy can’t do these itself. Shown as context; not part of this prototype.</p>
      {hints.slice(0, 2).map((p) => (
        <div className="tray-row context-row" key={p.id}>
          <button className="check" type="button" aria-label={`${p.title} · not part of this prototype`} disabled />
          <div className="tray-row-main">
            <div className="tray-row-title"><span>{p.title}</span></div>
            <p className="tray-row-sum">{p.summary}</p>
            <span className="source"><Signpost size={11} aria-hidden /> You do this one</span>
          </div>
          <span className="tray-row-size">Advice</span>
        </div>
      ))}
    </section>
  );
}

/** The plan's foot: what it frees (deletes only), and what the moves do, said apart so they never read as savings. */
export function PlanFoot({ api }: { api: FlowApi }) {
  const t = totals(api.flow, api.proposals);
  const nothing = t.deletes === 0 && t.moves === 0;
  return (
    <div className="tray-foot">
      <div className="tray-total">
        <span className="tray-total-num">{formatBytes(t.bytes)}</span>
        <span className="tray-total-lbl">will be freed</span>
      </div>
      <ul className="tray-notes">
        {t.deletes > 0 ? (
          <li>{formatCount(t.items)} items in {t.places} places, deleted permanently</li>
        ) : (
          <li>No files will be deleted.</li>
        )}
        {t.moves > 0 && (
          <li className="note-moves">
            {formatCount(t.moves)} files sorted in {listOf(t.folders)}. Moved, not deleted: frees 0 B. Undo any time.
          </li>
        )}
      </ul>
      <button type="button" className="cta tray-cta" disabled={nothing} onClick={api.review}>
        {t.deletes > 0 ? `Free ${formatBytes(t.bytes)}` : t.moves > 0 ? `Move ${formatCount(t.moves)} files` : 'Nothing selected yet'}
      </button>
    </div>
  );
}

export function listOf(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  return `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

// ---------- trees and facts ----------

export function Tree({ node, depth = 0 }: { node: TreeNode; depth?: number }) {
  const folder = !node.file;
  return (
    <div className="tree-node">
      <div className={`tree-row${node.tag ? ` is-${node.tag}` : ''}`} style={{ paddingLeft: depth * 15 }}>
        {folder ? <Folder size={13} strokeWidth={1.8} aria-hidden /> : <File size={13} strokeWidth={1.8} aria-hidden />}
        <span className="tree-name">{node.name}</span>
        {node.tag === 'new' && depth > 0 && <span className="tree-tag">new</span>}
        {node.tag === 'existing' && <span className="tree-tag is-existing">yours</span>}
        {node.count !== undefined && <span className="tree-count">{formatCount(node.count)}</span>}
      </div>
      {node.children?.map((c) => <Tree key={c.name} node={c} depth={depth + 1} />)}
    </div>
  );
}

export function SampleMoves({ r, excluded = [] }: { r: Reorg; excluded?: readonly string[] }) {
  const moves = sampleMoves(r).filter((m) => !r.dests.some((d) => excluded.includes(d.id) && m.to.startsWith(`${d.path}\\`)));
  return (
    <ul className="sample-moves" aria-label="Sample moves">
      {moves.map((m) => (
        <li key={m.to} className={m.renamed ? 'is-renamed' : undefined}>
          <span className="sm-from">{m.from}</span>
          <ArrowRight size={12} aria-hidden />
          <span className="sm-to">{m.to}</span>
          {m.renamed && <em>A file there already has the name, so this one gets “(1)”. Nothing is overwritten.</em>}
        </li>
      ))}
    </ul>
  );
}

export function Stays({ r }: { r: Reorg }) {
  return (
    <ul className="stays" aria-label="Stays where it is">
      {r.stays.map((s) => (
        <li key={s.name}>
          {s.dropped ? <TriangleAlert size={13} aria-hidden /> : <CornerDownRight size={13} aria-hidden />}
          <span><b>{s.name}</b> {s.why}</span>
        </li>
      ))}
    </ul>
  );
}

/** What a sort promises, in the drawer and the review sheet. */
export function Facts({ r }: { r: Reorg }) {
  return (
    <ul className="facts">
      <li>Built-in rule: sorted by kind and year. No AI, offline.</li>
      <li>Only loose files at the top of {r.folder}. Subfolders stay as they are.</li>
      <li>Moves only. Nothing is deleted, so this frees 0 B.</li>
      <li>Stays on this drive. Files in your Delete list aren’t moved.</li>
      <li>A file that’s open or changed when its turn comes is skipped and reported.</li>
      <li className="is-undo"><Undo2 size={13} aria-hidden /> Undo any time, from the recap or Past runs.</li>
    </ul>
  );
}

/** The kind folders a sort fills (Images, Documents…), biggest first: what the folder shows at its top after. */
export function kindFolders(r: Reorg, excluded: readonly string[] = []): [string, number][] {
  const m = new Map<string, number>();
  for (const d of r.dests.filter((x) => !excluded.includes(x.id))) {
    const top = d.path.split('\\')[0]!;
    m.set(top, (m.get(top) ?? 0) + d.count);
  }
  return [...m].sort((a, b) => b[1] - a[1]);
}

export function destLabel(r: Reorg, d: Dest) {
  return `${r.folder}\\${d.path}`;
}

// ---------- after the run ----------

/** What Undo did for a folder, as the recap and the history say it. */
export function UndoResult({ r, excluded }: { r: Reorg; excluded: readonly string[] }) {
  const moved = movedFiles(r, excluded);
  const failed = undoFailed(r, excluded);
  const back = moved - failed.count;
  const made = foldersMade(r, excluded) - (failed.count ? r.undo.keptFolders.length : 0);
  return (
    <div className="undo-result" role="status">
      <p className="undo-head">
        <RotateCcw size={13} aria-hidden />
        {failed.count === 0 ? `All ${formatCount(moved)} files are back in ${r.folder}.` : `${formatCount(back)} of ${formatCount(moved)} files are back in ${r.folder}.`}
      </p>
      {failed.count > 0 && (
        <p className="undo-warn">
          <TriangleAlert size={13} aria-hidden />
          <span>
            <b>{failed.count} files couldn’t go back: {failed.reason}.</b> They’re still in {r.folder}\{failed.where}:{' '}
            {failed.names.join(', ')}.
          </span>
        </p>
      )}
      <p className="undo-quiet">
        Removed the {made} empty {made === 1 ? 'folder' : 'folders'} Vacancy made.
        {failed.count > 0 && ` Kept ${listOf(r.undo.keptFolders)}, which still hold those files.`}
      </p>
    </div>
  );
}

/** The Undo button and its states. */
export function UndoButton({ r, api, small = true }: { r: Reorg; api: FlowApi; small?: boolean }) {
  const st = api.flow.undo[r.id];
  const stuck = undoFailed(r, api.flow.excluded).count;
  if (st === 'done') return <span className={stuck ? 'undone is-partial' : 'undone'}>{stuck ? `Moved back · ${stuck} stayed` : 'Moved back'}</span>;
  return (
    <button type="button" className={small ? 'btn btn--sm' : 'btn'} disabled={st === 'working'} onClick={() => api.undo(r.id)} aria-label={`Undo ${r.folder}`}>
      {st === 'working' ? <LoaderCircle size={13} className="tray-spin" aria-hidden /> : <Undo2 size={13} aria-hidden />}
      {st === 'working' ? 'Moving back' : `Undo ${r.folder}`}
    </button>
  );
}

/** Wrapped's Moved slide, with Undo per folder. The deck's other slides are the app's own. */
export function MovedSlideUndo({ api, onNext }: { api: FlowApi; onNext: () => void }) {
  const reorgs = pickedReorgs(api.flow);
  const moved = reorgs.reduce((n, r) => n + movedFiles(r, api.flow.excluded), 0);
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') onNext(); };
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  });
  return (
    <div className="wrap wrap--moved" data-no-ripple>
      <div className="wrap-bg" aria-hidden />
      <div className="wrap-haze" aria-hidden />
      <div className="wrap-deck" role="region" aria-roledescription="carousel" aria-label="Highlights of this run">
        <div className="wrap-stage">
          <section className="slide slide--moved" role="group" aria-roledescription="slide" aria-label="5 of 7: Moved folders">
            <div className="sl">
              <h1 className="sl-title">{formatCount(moved)} files found a home.</h1>
              <p className="sl-sub">Nothing was deleted, so no space was freed. Every move was recorded: Undo puts each file back where it was.</p>
              <ul className="moved">
                {reorgs.map((r) => (
                  <li key={r.id} className="moved-card">
                    <b>{r.folder}</b>
                    <span>
                      {formatCount(movedFiles(r, api.flow.excluded))} files into {kindFolders(r, api.flow.excluded).length} folders
                      {runSkips(r, api.flow.excluded).length > 0 && `, ${runSkips(r, api.flow.excluded).length} stayed`}
                    </span>
                    <span className="moved-dests">{kindFolders(r, api.flow.excluded).map(([k]) => k).join(' · ')}</span>
                    <div className="moved-undo">
                      <UndoButton r={r} api={api} />
                    </div>
                    {api.flow.undo[r.id] === 'done' && <UndoResult r={r} excluded={api.flow.excluded} />}
                  </li>
                ))}
              </ul>
            </div>
          </section>
        </div>
        <nav className="wrap-nav" aria-label="Highlight slides">
          <button type="button" className="round" disabled aria-label="Previous slide"><ChevronRight size={18} style={{ transform: 'rotate(180deg)' }} aria-hidden /></button>
          <div className="dots" aria-hidden>
            {[0, 1, 2, 3, 4, 5, 6].map((k) => <span key={k} className={k === 4 ? 'dot is-on' : k < 4 ? 'dot is-past' : 'dot'} />)}
          </div>
          <button type="button" className="round" onClick={onNext} aria-label="Next slide"><ChevronRight size={18} aria-hidden /></button>
        </nav>
        <button type="button" className="wrap-skip" onClick={onNext}>Skip to recap</button>
      </div>
    </div>
  );
}

/** The recap panel's frame: what the run freed and did, then the variant's reorganization section. */
export function RecapPanel({ api, report, children, onReplay }: { api: FlowApi; report: RunReport; children: ReactNode; onReplay: () => void }) {
  return (
    <aside className="tray summary" aria-label="What Vacancy did" data-no-ripple>
      <div className="summary-scroll">
        <section className="summary-hero">
          <h1 className="summary-num" aria-label={`${formatBytes(report.freedBytes)} freed`}>
            <HeroNumber bytes={report.freedBytes} />
          </h1>
          <p className="summary-sub">
            back on {report.volumeName}. Free space went from {formatBytes(report.freeBefore)} to {formatBytes(report.freeAfter)}.
            {report.movedItems > 0 && ' The moved files freed nothing: they’re only in new folders.'}
          </p>
        </section>
        <dl className="stats">
          <div><dt>items deleted</dt><dd>{formatCount(report.deletedItems)}</dd></div>
          <div><dt>files moved</dt><dd>{formatCount(report.movedItems)}</dd></div>
          <div><dt>start to finish</dt><dd>{formatDuration(report.durationMs)}</dd></div>
        </dl>
        {children}
      </div>
      <div className="summary-foot">
        <button type="button" className="replay" onClick={onReplay}><Play size={12} strokeWidth={2.4} aria-hidden /> Replay highlights</button>
        <button type="button" className="btn" onClick={api.reset}><RotateCcw size={14} aria-hidden /> Back to plan</button>
      </div>
    </aside>
  );
}

/** What stayed during the run, said once per folder. */
export function RunSkipped({ r, excluded }: { r: Reorg; excluded: readonly string[] }) {
  const s = runSkips(r, excluded);
  if (!s.length) return null;
  return (
    <ul className="skips run-skips">
      {s.map((k) => (
        <li key={k.name}>
          <TriangleAlert size={13} aria-hidden />
          <span><b>{k.reason}.</b> It stayed where it was. <code>{r.path}\{k.name}</code></span>
        </li>
      ))}
    </ul>
  );
}

// ---------- history ----------

interface PastRow { id: string; when: number; volume: string; freed: number; moved: number }

/** "Past runs" in the header, the app's look; this run first, with Undo per folder it sorted. */
export function PastRuns({ api, onOpen }: { api: FlowApi; onOpen?: () => void }) {
  const [open, setOpen] = useState(false);
  const fixture = fixtures[api.flow.platform];
  const all = fixture.proposals;
  const past: PastRow[] = fixture.history.map((h, i) => ({
    id: `past-${i}`,
    when: startOfToday() - h.daysAgo * 86_400_000 + h.hour * 3_600_000,
    volume: fixture.volumes.find((v) => v.mountPoint === h.mountPoint)?.name ?? h.mountPoint,
    freed: all.filter((p) => h.proposals.includes(p.id)).reduce((n, p) => n + p.bytes, 0) * (h.mountPoint === fixture.volumes[0]?.mountPoint ? 1 : 0.4),
    moved: 0,
  }));
  const report = api.flow.ranAt !== null ? buildReport(api.flow, api.proposals) : null;
  const reorgs = report ? pickedReorgs(api.flow) : [];
  const rows: PastRow[] = report ? [{ id: report.id, when: report.startedAt, volume: report.volumeName, freed: report.freedBytes, moved: report.movedItems }, ...past] : past;
  const drives = [...new Set(rows.map((r) => r.volume))];
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => { if (!(e.target as HTMLElement).closest('.history')) setOpen(false); };
    document.addEventListener('pointerdown', down, true);
    return () => document.removeEventListener('pointerdown', down, true);
  }, [open]);
  return (
    <div className="history">
      <button type="button" className={open ? 'history-button is-open' : 'history-button'} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <History size={14} aria-hidden /> Past runs
      </button>
      {open && (
        <div className="history-pop reorg-history" role="menu" aria-label="Past runs">
          <div className="history-head" aria-hidden>Past runs</div>
          {drives.map((d) => (
            <div key={d} className="history-drive" role="group" aria-label={d}>
              <div className="history-drive-name" aria-hidden>{d}</div>
              {rows.filter((r) => r.volume === d).map((r) => (
                <div key={r.id}>
                  <button type="button" role="menuitem" className="history-row" onClick={() => { if (r.id === report?.id && onOpen) { setOpen(false); onOpen(); } }}>
                    <span className="history-when">{formatWhen(r.when)}</span>
                    {r.moved > 0 && <span className="history-tag is-moved">{formatCount(r.moved)} moved</span>}
                    <span className="history-freed"><b>{formatBytes(r.freed)}</b> freed</span>
                  </button>
                  {r.id === report?.id && reorgs.map((x) => (
                    <div key={x.id} className="history-undo">
                      <FolderInput size={13} aria-hidden />
                      <span className="hu-text"><b>{x.folder}</b> {formatCount(movedFiles(x, api.flow.excluded))} files sorted</span>
                      <UndoButton r={x} api={api} />
                    </div>
                  ))}
                </div>
              ))}
            </div>
          ))}
          {reorgs.length > 0 && <p className="history-foot">Undo works from here any time. A file that changed, or whose old name is taken, stays where it is, and Vacancy says which.</p>}
        </div>
      )}
    </div>
  );
}

function startOfToday() {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

// ---------- small things ----------

export function Close({ onClick, label = 'Close' }: { onClick: () => void; label?: string }) {
  return <button type="button" className="icon-btn" onClick={onClick} aria-label={label}><X size={16} aria-hidden /></button>;
}

export const MapIconSmall = () => <MapIcon size={14} aria-hidden />;
export { movesOf, REORGS };

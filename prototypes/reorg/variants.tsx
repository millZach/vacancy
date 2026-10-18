// THROWAWAY: three places for folder reorganization in the Atlas flow.
// A: a "Tidy up" group in the plan tray, a mini before→after per folder, and a side drawer with both trees.
// B: a Tidy mode on the map: the three folders as tiles you scrub from loose to sorted, include and undo on the map.
// C: a review sheet over the map listing every destination folder, each with its own include box.
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';
import { ArrowRight, Check, ChevronRight, Eye, FolderInput, FolderTree, LayoutGrid, ListChecks, Plus, TriangleAlert, Undo2 } from 'lucide-react';
import type { Platform } from '../../src/bridge';
import { fixtures } from '../../src/bridge/fixtures';
import { formatCount } from '../../src/format';
import { Legend } from '../../src/map/Legend';
import { MapView, type TileLook } from '../../src/map/MapView';
import { layoutAtlas, overviewModel, regionKey, type AtlasModel, type Leaf, type Region } from '../../src/map/model';
import { useSize } from '../../src/map/useSize';
import { Checkbox, SourceTag } from '../../src/plan/bits';
import { tileOf } from '../../src/plan/totals';
import { SummaryMap } from '../../src/summary/Summary';
import {
  Close, DeleteGroup, kindFolders, destsIn, Facts, folderTile, HintGroup, PlanFoot, plannedMoves, RecapPanel, RunSkipped, SampleMoves, Stays, Tree,
  UndoButton, UndoResult, buildReport, movedFiles, pickedReorgs, runSkips, undoFailed, type FlowApi,
} from './common';
import { afterTree, beforeTree, foldersMade, REORGS, type Reorg } from './data';

export interface VariantProps {
  api: FlowApi;
  hot: string | null;
  onHot: (id: string | null) => void;
}

// ---------- the space map, as today ----------

export function SpaceMap({ platform, hot, onHot, look, extra, children, note }: {
  platform: Platform; hot: string | null; onHot: (id: string | null) => void; look: (l: Leaf) => TileLook;
  extra?: ReactNode; children?: ReactNode; note?: ReactNode;
}) {
  const [ref, size] = useSize<HTMLDivElement>();
  const model = useMemo(() => overviewModel(fixtures[platform].overview), [platform]);
  return (
    <main className="atlas is-map">
      <div className="atlas-toolbar">
        <div className="crumbs"><span className="crumb is-here">{fixtures[platform].overview.volume.name}</span></div>
        {extra}
        <Legend bytesInView={model.totalBytes} w={size.w} h={size.h} reclaimable />
      </div>
      <div className="atlas-stage" ref={ref}>
        <MapView model={model} width={size.w} height={size.h} focus={null} interactive hot={hot} onHot={onHot} planning look={look} clickable={() => false} />
      </div>
      {note && <p className="map-note">{note}</p>}
      {children}
    </main>
  );
}

export function planLook(api: FlowApi, glow: string[] = []) {
  return (l: Leaf): TileLook => {
    if (glow.includes(l.id)) return { cls: 'is-tidy-hot', state: 'loose files Vacancy can sort' };
    if (!l.proposal) return {};
    const on = api.flow.deletes.includes(l.proposal);
    return { cls: on ? 'is-reclaim is-selected' : 'is-reclaim is-available', state: on ? 'in the plan' : 'reclaimable' };
  };
}

export function runLook(api: FlowApi) {
  return (l: Leaf): TileLook => (l.proposal && api.flow.deletes.includes(l.proposal) ? { cls: 'is-vacant is-lit', state: 'vacant', fill: 0.12 } : {});
}

function TrayHead({ title, note, children }: { title: string; note: string; children?: ReactNode }) {
  return (
    <div className="tray-head">
      <div><h2>{title}</h2><p>{note}</p></div>
      {children}
    </div>
  );
}

// ======================================================================================================
// A · Tidy up in the tray
// ======================================================================================================

function MiniBA({ r, onOpen }: { r: Reorg; onOpen: () => void }) {
  const tops = kindFolders(r);
  const shown = tops.slice(0, 4);
  const most = Math.max(1, ...shown.map((t) => t[1]));
  const dots = Math.min(28, Math.round(r.loose / 7));
  return (
    <button type="button" className="mini-ba" onClick={onOpen} aria-label={`Before and after: ${r.folder}`}>
      <span className="mini-before">
        <span className="mini-pile" aria-hidden>
          {Array.from({ length: dots }, (_, i) => <i key={i} style={{ left: `${(i * 37) % 86}%`, top: `${(i * 53) % 78}%` }} />)}
        </span>
        <span className="mini-cap">{formatCount(r.loose)} loose</span>
      </span>
      <ArrowRight size={13} className="mini-arrow" aria-hidden />
      <span className="mini-after">
        {shown.map(([name, n]) => (
          <span key={name} className="mini-folder">
            <span className="mini-name">{name}</span>
            <span className="mini-bar"><i style={{ width: `${(n / most) * 100}%` }} /></span>
            <span className="mini-n">{n}</span>
          </span>
        ))}
        {tops.length > shown.length && <span className="mini-more">+{tops.length - shown.length} more folders</span>}
      </span>
    </button>
  );
}

function TidyRow({ r, api, open, onOpen, hot, onHot }: { r: Reorg; api: FlowApi; open: boolean; onOpen: () => void; hot: boolean; onHot: (id: string | null) => void }) {
  const on = api.flow.picked.includes(r.id);
  const tile = folderTile(api.flow.platform, r.folder);
  return (
    <div className={['tray-row', 'tidy-row', on && 'is-on', (hot || open) && 'is-hot'].filter(Boolean).join(' ')} onMouseEnter={() => onHot(tile)} onMouseLeave={() => onHot(null)}>
      <Checkbox on={on} onChange={() => api.togglePick(r.id)} label={`Include Sort ${r.folder}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={onOpen} aria-expanded={open}>
          <span>Sort {r.folder}</span>
          <ChevronRight size={13} aria-hidden />
        </button>
        <p className="tray-row-sum">{formatCount(plannedMoves(r, []))} of {formatCount(r.loose)} loose files, sorted by kind and year.</p>
        <MiniBA r={r} onOpen={onOpen} />
        <div className="tray-row-meta">
          <SourceTag source="rule" agent={null} />
          <span className="source"><Undo2 size={11} aria-hidden /> Undo any time</span>
        </div>
      </div>
      <span className="tray-row-size is-files">{formatCount(plannedMoves(r, []))} files</span>
    </div>
  );
}

function DrawerA({ r, api, onClose }: { r: Reorg; api: FlowApi; onClose: () => void }) {
  const on = api.flow.picked.includes(r.id);
  return (
    <section className="reorg-drawer" role="dialog" aria-label={`Sort ${r.folder}`} data-no-ripple>
      <header className="rd-head">
        <div>
          <h2>Sort {r.folder}</h2>
          <p className="rd-path">{r.path}</p>
        </div>
        <Close onClick={onClose} />
      </header>
      <div className="rd-body">
        <p className="rd-rule"><FolderTree size={14} aria-hidden /> Built-in rule: sorted by kind and year.</p>
        <div className="trees">
          <section><h3>Before</h3><Tree node={beforeTree(r)} /></section>
          <ArrowRight size={16} className="trees-arrow" aria-hidden />
          <section><h3>After</h3><Tree node={afterTree(r)} /></section>
        </div>
        <h3 className="rd-h">Sample moves</h3>
        <SampleMoves r={r} />
        <h3 className="rd-h">Stays where it is</h3>
        <Stays r={r} />
        <Facts r={r} />
      </div>
      <footer className="rd-foot">
        <p>{formatCount(plannedMoves(r, []))} moves · makes {foldersMade(r)} folders · frees 0 B</p>
        <button type="button" className={on ? 'btn' : 'cta'} onClick={() => api.togglePick(r.id)}>{on ? 'Remove from plan' : 'Add to plan'}</button>
      </footer>
    </section>
  );
}

export function VariantA({ api, hot, onHot }: VariantProps) {
  const [drawer, setDrawer] = useState<string | null>(null);
  const r = REORGS.find((x) => x.id === drawer);
  const glow = r ? [folderTile(api.flow.platform, r.folder)].filter((x): x is string => !!x) : [];
  return (
    <>
      <SpaceMap platform={api.flow.platform} hot={hot} onHot={onHot} look={planLook(api, glow)}>
        {r && <DrawerA key={r.id} r={r} api={api} onClose={() => setDrawer(null)} />}
      </SpaceMap>
      <aside className="tray" aria-label="Plan" data-no-ripple>
        <TrayHead title="Plan" note="Click glowing areas on the map, or tick them here. Nothing changes until you confirm." />
        <div className="tray-scroll">
          <DeleteGroup api={api} hot={hot} onHot={onHot} />
          <section className="tray-group" aria-label="Tidy up">
            <h3>Tidy up <span>{api.flow.picked.length} of {REORGS.length}</span></h3>
            <p className="tray-group-note">Sorts loose files into folders. Moves only: nothing is deleted and no space is freed.</p>
            {REORGS.map((x) => (
              <TidyRow key={x.id} r={x} api={api} open={drawer === x.id} onOpen={() => setDrawer(drawer === x.id ? null : x.id)} hot={!!hot && hot === folderTile(api.flow.platform, x.folder)} onHot={onHot} />
            ))}
          </section>
          <HintGroup platform={api.flow.platform} />
        </div>
        <PlanFoot api={api} />
      </aside>
    </>
  );
}

export function RecapA({ api, hot, onHot, onReplay }: VariantProps & { onReplay: () => void }) {
  const report = buildReport(api.flow, api.proposals);
  const model = useMemo(() => overviewModel(report.map), [report.map]);
  return (
    <>
      <main className="atlas is-map finish-map"><SummaryMap model={model} report={report} hot={hot} onHot={onHot} /></main>
      <RecapPanel api={api} report={report} onReplay={onReplay}>
        <section className="summary-sec reorg-recap" aria-label="Reorganized">
          <h3>Reorganized <span className="sec-aside">moved, not deleted · 0 B freed</span></h3>
          {pickedReorgs(api.flow).map((r) => (
            <div key={r.id} className="recap-reorg">
              <div className="rr-head">
                <FolderInput size={14} aria-hidden />
                <span className="rr-text"><b>{r.folder}.</b> {formatCount(movedFiles(r, []))} of {formatCount(plannedMoves(r, []))} files moved into {kindFolders(r).length} folders.</span>
                <UndoButton r={r} api={api} />
              </div>
              <RunSkipped r={r} excluded={[]} />
              {api.flow.undo[r.id] === 'done' && <UndoResult r={r} excluded={[]} />}
            </div>
          ))}
          <p className="summary-note">Every move was recorded. Undo also lives in Past runs.</p>
        </section>
      </RecapPanel>
    </>
  );
}

// ======================================================================================================
// B · Tidy mode on the map
// ======================================================================================================

const HUE: Record<Reorg['id'], Region['category']> = { downloads: 'downloads', desktop: 'cloud', documents: 'apps' };
type FolderState = 'loose' | 'sorted' | 'undone';

function leafB(id: string, regionId: string, kind: Leaf['kind'], name: string, n: number): Leaf {
  return { id, regionId, kind, name, path: '', bytes: Math.max(0, n), lastUsed: null, browsable: false, note: null, proposal: null, shade: 0, vacant: 0 };
}

/**
 * The three folders as a map whose area is file count, not bytes. `t` runs from as it is (0) to sorted (1): the
 * loose-files tile shrinks while each folder it fills grows out of it. Every tile is always there, so laid out against
 * `base` (all of them at full size) nothing reshuffles; they only grow and shrink where they stand.
 */
function tidyModel(t: number, stateOf: (r: Reorg) => FolderState, recap: boolean, base = false): AtlasModel {
  const regions: Region[] = REORGS.map((r) => {
    const st = stateOf(r);
    const held = recap ? runSkips(r, []).length : 0;
    const failed = undoFailed(r, []);
    const merged = (name: string) => r.dests.some((d) => d.path === name);
    // How far each destination fills at this t.
    const fill = (d: Reorg['dests'][number]) => {
      if (base) return d.count;
      if (st === 'loose') return 0;
      if (st === 'undone') return d.path === failed.where ? failed.count * t : 0;
      return (d.count - (d.path === 'Documents\\2026' ? held : 0)) * t;
    };
    const leaving = r.dests.reduce((n, d) => n + fill(d), 0);
    const loose = base ? r.loose : r.loose - leaving;
    const leaves: Leaf[] = [
      leafB(`${r.id}:loose`, r.id, 'files', t >= 0.999 && st === 'sorted' ? `${formatCount(Math.round(loose))} stay loose` : `${formatCount(Math.round(loose))} loose files`, loose),
      ...r.dests.map((d) => {
        const sub = r.subfolders.find((s) => s.name === d.path);
        const failedHere = st === 'undone' && d.path === failed.where;
        return leafB(`${r.id}:${d.id}`, r.id, 'folder', failedHere ? `${d.path}: ${failed.count} couldn’t go back` : d.path, (sub?.files ?? 0) + fill(d));
      }),
      ...r.subfolders.filter((s) => !merged(s.name)).map((s) => leafB(`${r.id}:sub:${s.name}`, r.id, 'folder', s.name, s.files)),
    ];
    const shaded = leaves.map((l, i) => ({ ...l, shade: i % 3 }));
    return {
      id: r.id, category: HUE[r.id], categoryName: r.path, name: r.folder, bytes: shaded.reduce((n, l) => n + l.bytes, 0),
      protected: false, blurb: '', free: false, pending: false, leaves: shaded,
    };
  });
  const leaves = regions.flatMap((r) => r.leaves);
  const total = leaves.reduce((n, l) => n + l.bytes, 0);
  return { regions, leaves, leafById: new Map(leaves.map((l) => [l.id, l])), regionById: new Map(regions.map((r) => [r.id, r])), totalBytes: total, usedBytes: total, freeBytes: 0 };
}

const reduced = () => window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

/**
 * The tidy map: one scrubber from "As it is" to "Sorted" (the summary's Before/After switch, plus a track to drag),
 * the loose tile turning into its folders as it moves. A chip on each folder adds it to the plan, or in the recap
 * undoes it.
 */
function TidyMap({ api, mode, focus, onFocus, recap, stateOf }: {
  api: FlowApi; mode: ReactNode; focus: string | null; onFocus: (id: string | null) => void; recap: boolean; stateOf: (r: Reorg) => FolderState;
}) {
  const [stageRef, size] = useSize<HTMLDivElement>();
  const track = useRef<HTMLDivElement>(null);
  const [t, setT] = useState(0);
  const now = useRef(0);
  const raf = useRef(0);
  const set = useCallback((v: number) => { now.current = v; setT(v); }, []);
  const base = useMemo(() => tidyModel(1, stateOf, recap, true), [stateOf, recap]);
  const model = useMemo(() => tidyModel(t, stateOf, recap), [t, stateOf, recap]);
  const rects = useMemo(() => layoutAtlas(model, size.w, size.h, focus, base), [model, base, size.w, size.h, focus]);
  const glide = useCallback((to: number, ms = 1100) => {
    cancelAnimationFrame(raf.current);
    if (reduced()) return set(to);
    const from = now.current;
    const start = performance.now();
    const step = () => {
      const k = Math.min(1, (performance.now() - start) / ms);
      const e = k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2;
      set(from + (to - from) * e);
      if (k < 1) raf.current = requestAnimationFrame(step);
    };
    raf.current = requestAnimationFrame(step);
  }, [set]);
  // Opening the mode, or a folder, plays the sort once, as the summary sweeps from Before to After.
  const ready = size.w > 0;
  useEffect(() => {
    if (!ready) return;
    now.current = 0;
    glide(1, 1600);
  }, [focus, ready, glide]);
  useEffect(() => () => cancelAnimationFrame(raf.current), []);
  const drag = (e: PointerEvent<HTMLButtonElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId) || !track.current) return;
    const box = track.current.getBoundingClientRect();
    set(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)));
  };
  const key = (e: KeyboardEvent) => {
    const to = e.key === 'ArrowLeft' ? t - 0.1 : e.key === 'ArrowRight' ? t + 0.1 : e.key === 'Home' ? 0 : e.key === 'End' ? 1 : null;
    if (to === null) return;
    e.preventDefault();
    e.stopPropagation();
    cancelAnimationFrame(raf.current);
    set(Math.min(1, Math.max(0, to)));
  };
  const look = (l: Leaf): TileLook => {
    const r = REORGS.find((x) => x.id === l.regionId)!;
    const live = recap ? stateOf(r) !== 'loose' : api.flow.picked.includes(r.id);
    const n = Math.round(l.bytes);
    const count = <span className="tidy-count">{formatCount(n)} {n === 1 ? 'file' : 'files'}</span>;
    const fresh = !l.id.includes(':sub:') && !l.id.endsWith(':loose');
    const failed = recap && stateOf(r) === 'undone' && l.name.includes('couldn’t go back');
    const cls = [
      l.id.endsWith(':loose') && live && t < 0.999 && 'is-tidy-src',
      fresh && live && !failed && stateOf(r) === 'sorted' && 'is-tidy-new',
      fresh && !live && 'is-tidy-ghost',
      failed && 'is-tidy-failed',
    ].filter(Boolean).join(' ');
    return cls ? { size: count, cls } : { size: count };
  };
  const onRegion = (r: Region) => onFocus(focus === r.id ? null : r.id);
  const label = t < 0.02 ? 'As it is' : t > 0.98 ? 'Sorted' : `${Math.round(t * 100)}% sorted`;
  return (
    <main className="atlas is-map tidy-map">
      <div className="atlas-toolbar">
        {mode}
        <div className="seg" role="group" aria-label="Compare">
          <button type="button" className={t < 0.5 ? 'is-on' : undefined} onClick={() => glide(0, 900)}>As it is</button>
          <button type="button" className={t >= 0.5 ? 'is-on' : undefined} onClick={() => glide(1, 900)}>Sorted</button>
        </div>
        <div className="scrub" ref={track}>
          <i style={{ width: `${t * 100}%` }} />
          <button type="button" className="scrub-knob" role="slider" aria-label="As it is to sorted" aria-valuemin={0} aria-valuemax={100}
            aria-valuenow={Math.round(t * 100)} aria-valuetext={label} style={{ left: `${t * 100}%` }}
            onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); cancelAnimationFrame(raf.current); }} onPointerMove={drag} onKeyDown={key} />
        </div>
        <span className="toolbar-hint">{focus ? 'Click the folder’s name again to see all three.' : 'Drag to watch the files move. Click a folder’s name to zoom in.'}</span>
      </div>
      <div className="atlas-stage tidy-stage" ref={stageRef}>
        <MapView model={model} base={base} width={size.w} height={size.h} focus={focus} interactive hot={null} onHot={() => {}} onRegion={onRegion} look={look} />
        {model.regions.map((reg) => {
          const rect = rects.get(regionKey(reg.id));
          if (!rect || (focus && focus !== reg.id)) return null;
          const r = REORGS.find((x) => x.id === reg.id)!;
          const on = api.flow.picked.includes(r.id);
          return (
            <div key={reg.id} className="tidy-chip" style={{ left: rect.x + rect.w - 4, top: rect.y + 2 }}>
              {recap ? (
                on && <UndoButton r={r} api={api} />
              ) : (
                <button type="button" className={on ? 'chip is-on' : 'chip'} onClick={() => api.togglePick(r.id)} aria-pressed={on} aria-label={`${on ? 'Remove' : 'Add'} Sort ${r.folder}`}>
                  {on ? <Check size={12} strokeWidth={3} aria-hidden /> : <Plus size={12} strokeWidth={2.6} aria-hidden />}
                  {on ? 'In the plan' : `Sort ${r.folder}`}
                </button>
              )}
            </div>
          );
        })}
        {recap && REORGS.map((r) => {
          const failed = undoFailed(r, []);
          const d = r.dests.find((x) => x.path === failed.where);
          const rect = d && rects.get(`${r.id}:${d.id}`);
          if (stateOf(r) !== 'undone' || !failed.count || !rect || (focus && focus !== r.id)) return null;
          // Pinned just above the tile that still holds the files, right-aligned to it.
          return (
            <p key={r.id} className="tidy-callout" style={{ left: rect.x + rect.w, top: rect.y - 6 }}>
              <TriangleAlert size={12} aria-hidden /> {failed.count} files couldn’t go back: a file now has the original name. They’re in {failed.where}, the outlined tile.
            </p>
          );
        })}
      </div>
      <p className="map-note">Tile area is the number of files, not space: sorting frees none. Built-in rule, by kind and year; only loose files at the top of each folder move.</p>
    </main>
  );
}

function ModeSeg({ mode, onMode, moves }: { mode: 'space' | 'tidy'; onMode: (m: 'space' | 'tidy') => void; moves: number }) {
  return (
    <div className="seg mode-seg" role="group" aria-label="Map">
      <button type="button" className={mode === 'space' ? 'is-on' : undefined} onClick={() => onMode('space')}><LayoutGrid size={12} aria-hidden /> Space</button>
      <button type="button" className={mode === 'tidy' ? 'is-on' : undefined} onClick={() => onMode('tidy')}><FolderTree size={12} aria-hidden /> Tidy{moves > 0 ? ` · ${formatCount(moves)}` : ''}</button>
    </div>
  );
}

export function VariantB({ api, hot, onHot }: VariantProps) {
  const [mode, setMode] = useState<'space' | 'tidy'>('space');
  const [focus, setFocus] = useState<string | null>(null);
  const moves = pickedReorgs(api.flow).reduce((n, r) => n + plannedMoves(r, []), 0);
  const seg = <ModeSeg mode={mode} onMode={(m) => { setMode(m); setFocus(null); }} moves={moves} />;
  const stateOf = useCallback(() => 'sorted' as FolderState, []);
  const r = REORGS.find((x) => x.id === focus);
  return (
    <>
      {mode === 'space' ? (
        <SpaceMap platform={api.flow.platform} hot={hot} onHot={onHot} look={planLook(api)} extra={seg} />
      ) : (
        <TidyMap api={api} mode={seg} focus={focus} onFocus={setFocus} recap={false} stateOf={stateOf} />
      )}
      <aside className="tray" aria-label="Plan" data-no-ripple>
        <TrayHead title="Plan" note={mode === 'space' ? 'Click glowing areas on the map, or tick them here. Nothing changes until you confirm.' : 'Sort the loose files in your folders. Scrub the map to see them before and after.'} />
        <div className="tray-scroll">
          {mode === 'space' ? (
            <>
              <DeleteGroup api={api} hot={hot} onHot={onHot} />
              <section className="tray-group" aria-label="Tidy up">
                <h3>Tidy up <span>{api.flow.picked.length} of {REORGS.length}</span></h3>
                <button type="button" className="tidy-teaser" onClick={() => setMode('tidy')}>
                  <FolderTree size={16} aria-hidden />
                  <span><b>{formatCount(REORGS.reduce((n, x) => n + x.loose, 0))} loose files</b> in Downloads, Desktop and Documents could be sorted into folders.</span>
                  <span className="tt-go">See them on the map <ArrowRight size={12} aria-hidden /></span>
                </button>
              </section>
              <HintGroup platform={api.flow.platform} />
            </>
          ) : (
            <>
              <section className="tray-group" aria-label="Tidy up">
                <h3>Tidy up <span>{api.flow.picked.length} of {REORGS.length}</span></h3>
                <p className="tray-group-note">Moves only: nothing is deleted and no space is freed.</p>
                {REORGS.map((x) => (
                  <div key={x.id} className={['tray-row', 'tidy-row-b', api.flow.picked.includes(x.id) && 'is-on', focus === x.id && 'is-hot'].filter(Boolean).join(' ')}>
                    <Checkbox on={api.flow.picked.includes(x.id)} onChange={() => api.togglePick(x.id)} label={`Include Sort ${x.folder}`} />
                    <div className="tray-row-main">
                      <button type="button" className="tray-row-title" onClick={() => setFocus(focus === x.id ? null : x.id)} aria-expanded={focus === x.id}>
                        <span>Sort {x.folder}</span><Eye size={13} aria-hidden />
                      </button>
                      <p className="tray-row-sum">{formatCount(plannedMoves(x, []))} files into {kindFolders(x).length} folders.</p>
                    </div>
                    <span className="tray-row-size is-files">{formatCount(plannedMoves(x, []))} files</span>
                  </div>
                ))}
              </section>
              {r && (
                <section className="tray-group focus-detail" aria-label={`Sort ${r.folder}`}>
                  <h3>{r.folder} up close</h3>
                  <h4 className="rd-h">Sample moves</h4>
                  <SampleMoves r={r} />
                  <h4 className="rd-h">Stays where it is</h4>
                  <Stays r={r} />
                  <Facts r={r} />
                </section>
              )}
              {!r && <p className="tray-group-note pad">Click a folder’s name on the map, or the eye here, to see its moves up close.</p>}
              <DeleteGroup api={api} hot={hot} onHot={onHot} collapsed />
            </>
          )}
        </div>
        <PlanFoot api={api} />
      </aside>
    </>
  );
}

export function RecapB({ api, hot, onHot, onReplay }: VariantProps & { onReplay: () => void }) {
  const report = buildReport(api.flow, api.proposals);
  const model = useMemo(() => overviewModel(report.map), [report.map]);
  const [mode, setMode] = useState<'space' | 'tidy'>('tidy');
  const [focus, setFocus] = useState<string | null>(null);
  const undo = api.flow.undo;
  const picked = api.flow.picked;
  const stateOf = useCallback((r: Reorg): FolderState => (!picked.includes(r.id) ? 'loose' : undo[r.id] === 'done' ? 'undone' : 'sorted'), [picked, undo]);
  const seg = <ModeSeg mode={mode} onMode={(m) => { setMode(m); setFocus(null); }} moves={0} />;
  return (
    <>
      {mode === 'space' ? (
        <main className="atlas is-map finish-map">
          <div className="recap-mode">{seg}</div>
          <SummaryMap model={model} report={report} hot={hot} onHot={onHot} />
        </main>
      ) : (
        <TidyMap api={api} mode={seg} focus={focus} onFocus={setFocus} recap stateOf={stateOf} />
      )}
      <RecapPanel api={api} report={report} onReplay={onReplay}>
        <section className="summary-sec reorg-recap" aria-label="Reorganized">
          <h3>Reorganized <span className="sec-aside">0 B freed</span></h3>
          <p className="summary-note">Undo sits on each folder in the Tidy map{mode === 'space' ? ' (switch to Tidy above the map)' : ''}.</p>
          {pickedReorgs(api.flow).map((r) => (
            <div key={r.id} className="recap-reorg">
              <div className="rr-head">
                <FolderInput size={14} aria-hidden />
                <span className="rr-text"><b>{r.folder}.</b> {formatCount(movedFiles(r, []))} files moved.</span>
                {undo[r.id] ? <UndoButton r={r} api={api} /> : <span className="rr-quiet">Undo it on the map</span>}
              </div>
              <RunSkipped r={r} excluded={[]} />
              {undo[r.id] === 'done' && <UndoResult r={r} excluded={[]} />}
            </div>
          ))}
        </section>
      </RecapPanel>
    </>
  );
}

// ======================================================================================================
// C · A review sheet per destination
// ======================================================================================================

function SheetC({ api, onClose }: { api: FlowApi; onClose: () => void }) {
  const [tab, setTab] = useState<Reorg['id']>('downloads');
  const r = REORGS.find((x) => x.id === tab)!;
  const on = api.flow.picked.includes(r.id);
  const ex = api.flow.excluded;
  const groups = useMemo(() => {
    const m = new Map<string, typeof r.dests>();
    for (const d of r.dests) {
      const top = d.kind === 'Documents' && !d.path.includes('\\') ? 'By year' : d.path.split('\\')[0]!;
      m.set(top, [...(m.get(top) ?? []), d]);
    }
    return [...m];
  }, [r]);
  return (
    <section className="review-sheet" role="dialog" aria-label="Review sorting" data-no-ripple>
      <header className="rs-head">
        <div>
          <h2>Review sorting</h2>
          <p><FolderTree size={13} aria-hidden /> Built-in rule: sorted by kind and year. Untick any folder you’d rather leave out.</p>
        </div>
        <Close onClick={onClose} label="Close the review" />
      </header>
      <nav className="rs-tabs" role="tablist" aria-label="Folders">
        {REORGS.map((x) => {
          const n = plannedMoves(x, ex);
          const inPlan = api.flow.picked.includes(x.id);
          return (
            <button key={x.id} type="button" role="tab" aria-selected={tab === x.id} className={tab === x.id ? 'rs-tab is-on' : 'rs-tab'} onClick={() => setTab(x.id)}>
              <b>{x.folder}</b>
              <span>{inPlan ? `${formatCount(n)} of ${formatCount(plannedMoves(x, []))} files` : 'Not in the plan'}</span>
            </button>
          );
        })}
      </nav>
      <div className="rs-body">
        <label className="rs-all">
          <Checkbox on={on} onChange={() => api.togglePick(r.id)} label={`Include Sort ${r.folder}`} />
          <span><b>Sort {r.folder}</b> {r.path}</span>
        </label>
        <div className="rs-cols">
          <div className="rs-dests">
            {groups.map(([name, ds]) => (
              <div key={name} className="rs-group">
                <h3>{name === 'Sort later' ? 'Sort later' : name}{ds[0]!.existing && name !== 'By year' && <span className="tree-tag is-existing">your folder</span>}</h3>
                {name === 'Sort later' && <p className="rs-note">Vacancy isn’t sure what these are, so they wait in one folder for you.</p>}
                {ds.map((d) => {
                  const inc = on && !ex.includes(d.id);
                  return (
                    <div key={d.id} className={inc ? 'rs-dest is-on' : 'rs-dest'}>
                      <Checkbox on={inc} onChange={() => { if (!on) api.togglePick(r.id); api.toggleDest(d.id); }} label={`Include ${r.folder}\\${d.path}`} />
                      <div className="rs-dest-main">
                        <div className="rs-dest-top">
                          <span className="rs-path">{r.folder}\<b>{d.path}</b></span>
                          {!d.existing && <span className="tree-tag">new</span>}
                          <span className="rs-n">{formatCount(d.count)} files</span>
                        </div>
                        <p className="rs-samples">{d.samples.join(' · ')}{d.count > 3 ? ` · ${formatCount(d.count - 3)} more` : ''}</p>
                        {d.renamed.map((x) => (
                          <p key={x.from} className="rs-renamed">
                            <TriangleAlert size={12} aria-hidden />
                            <span>{x.from} → <b>{x.to}</b>: a file there already has the name. Nothing is overwritten.</span>
                          </p>
                        ))}
                      </div>
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
          <aside className="rs-side">
            <h3>Stays where it is</h3>
            <Stays r={r} />
            {r.subfolders.length > 0 && <p className="rs-note">Your subfolders ({r.subfolders.map((s) => s.name).join(', ')}) stay as they are.</p>}
            <Facts r={r} />
          </aside>
        </div>
      </div>
      <footer className="rs-foot">
        <p>
          <b>{on ? `${formatCount(plannedMoves(r, ex))} files into ${destsIn(r, ex).length} folders` : `${r.folder} isn’t in the plan`}</b>
          <span>Nothing deleted · 0 B freed · same drive · undo any time</span>
        </p>
        <button type="button" className="cta" onClick={onClose}>Done</button>
      </footer>
    </section>
  );
}

export function VariantC({ api, hot, onHot }: VariantProps) {
  const [sheet, setSheet] = useState(false);
  const moves = pickedReorgs(api.flow).reduce((n, r) => n + plannedMoves(r, api.flow.excluded), 0);
  return (
    <>
      <SpaceMap platform={api.flow.platform} hot={hot} onHot={onHot} look={planLook(api)}>
        {sheet && <SheetC api={api} onClose={() => setSheet(false)} />}
      </SpaceMap>
      <aside className="tray" aria-label="Plan" data-no-ripple>
        <TrayHead title="Plan" note="Click glowing areas on the map, or tick them here. Nothing changes until you confirm." />
        <div className="tray-scroll">
          <section className="tray-group tidy-card" aria-label="Tidy up">
            <h3>Tidy up <span>{moves > 0 ? `${formatCount(moves)} files` : 'nothing yet'}</span></h3>
            <p className="tray-group-note">Sort loose files into folders by kind and year. Moves only: frees 0 B.</p>
            <ul className="tidy-list">
              {REORGS.map((x) => (
                <li key={x.id} className={api.flow.picked.includes(x.id) ? 'is-on' : undefined}>
                  <FolderInput size={13} aria-hidden />
                  <span className="tl-name">{x.folder}</span>
                  <span className="tl-n">{api.flow.picked.includes(x.id) ? `${formatCount(plannedMoves(x, api.flow.excluded))} of ${formatCount(plannedMoves(x, []))}` : `${formatCount(plannedMoves(x, []))} to sort`}</span>
                </li>
              ))}
            </ul>
            <button type="button" className="btn tidy-review" onClick={() => setSheet(true)}>
              <ListChecks size={14} aria-hidden /> Review sorting
            </button>
          </section>
          <DeleteGroup api={api} hot={hot} onHot={onHot} />
          <HintGroup platform={api.flow.platform} />
        </div>
        <PlanFoot api={api} />
      </aside>
    </>
  );
}

export function RecapC({ api, hot, onHot, onReplay }: VariantProps & { onReplay: () => void }) {
  const report = buildReport(api.flow, api.proposals);
  const model = useMemo(() => overviewModel(report.map), [report.map]);
  const ex = api.flow.excluded;
  return (
    <>
      <main className="atlas is-map finish-map"><SummaryMap model={model} report={report} hot={hot} onHot={onHot} /></main>
      <RecapPanel api={api} report={report} onReplay={onReplay}>
        <section className="summary-sec reorg-recap" aria-label="Moves">
          <h3>Moves <span className="sec-aside">nothing deleted · 0 B freed</span></h3>
          {pickedReorgs(api.flow).map((r) => {
            const done = api.flow.undo[r.id] === 'done';
            const failed = undoFailed(r, ex);
            return (
              <div key={r.id} className="ledger">
                <div className="ledger-head">
                  <b>{r.folder}</b>
                  <span>{formatCount(movedFiles(r, ex))} moved</span>
                  <UndoButton r={r} api={api} />
                </div>
                <table className="ledger-table">
                  <tbody>
                    {destsIn(r, ex).map((d) => {
                      const stuck = done && failed.count > 0 && d.path === failed.where;
                      return (
                        <tr key={d.id} className={stuck ? 'is-stuck' : done ? 'is-back' : undefined}>
                          <td>{done ? (stuck ? <TriangleAlert size={12} aria-hidden /> : <Undo2 size={12} aria-hidden />) : <ArrowRight size={12} aria-hidden />}</td>
                          <td className="lt-path">{d.path}</td>
                          <td className="lt-n">{done ? (stuck ? `${failed.count} stayed` : 'back') : formatCount(d.count)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                <RunSkipped r={r} excluded={ex} />
                {done && <UndoResult r={r} excluded={ex} />}
              </div>
            );
          })}
        </section>
      </RecapPanel>
    </>
  );
}

export { tileOf };

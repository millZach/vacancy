import { Check, ChevronRight, LoaderCircle, Lock, Plus, RotateCcw, ShieldAlert, Zap } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  useBridge,
  type DuplicateGroup,
  type FastFallback,
  type LockedGroup,
  type MapFolder,
  type Overview,
  type PlanSummary,
  type ScanMethod,
  type TidyProposal,
  type VolumeInfo,
} from '../bridge';
import { DotField, type DotFieldHandle } from '../dotfield/DotField';
import type { ReadyDrives } from '../drives/context';
import { GoneNotice } from '../drives/GoneNotice';
import { errorMessage } from '../errors';
import { formatBytes, formatCount } from '../format';
import { Legend } from '../map/Legend';
import { DuplicateLinks, DuplicatesPill } from '../map/Duplicates';
import { MapView, type TileLook } from '../map/MapView';
import {
  LOCKED_ID,
  folderModel,
  freedModel,
  lockedTotals,
  overviewModel,
  scanModel,
  type Freed,
  type Leaf,
  type Rect,
  type Region,
} from '../map/model';
import { useSize } from '../map/useSize';
import { RiskChip, SourceTag } from '../plan/bits';
import { DUPLICATES, sortable } from '../plan/duplicates';
import { DuplicatesPanel } from '../plan/DuplicatesPanel';
import { PlanTray } from '../plan/PlanTray';
import { tileOf } from '../plan/totals';
import { ConfirmDialog } from '../run/ConfirmDialog';
import { TipSection } from '../claude/bits';
import { ClaudePanel } from '../claude/ClaudePanel';
import { claudeNoteOf, explainedFolders, isOn, liveFlags, useClaude, type Scope } from '../claude/context';
import type { StepView } from '../run/progress';
import { RunPanel, type ShownRun } from '../run/RunPanel';
import { fallbackLabel, isAsking, scanProgress } from '../scan/fast';
import { ScanHud } from '../scan/ScanHud';
import { ScanPlate } from '../scan/ScanPlate';
import { TidyDrawer } from '../tidy/TidyDrawer';
import { Finish } from '../wrapped/Finish';
import './atlas.css';

interface Props {
  /** Every drive, and the drive on screen with its scan. */
  drives: ReadyDrives;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
  /** Read-only advisor status note on the scan plate. */
  advisorNote: ReactNode;
  /** A late plan check waits until the advisor releases modal ownership. */
  advisorOpen: boolean;
  /** The plan tray is open beside the map. */
  planOpen: boolean;
  onPlanOpen: (open: boolean) => void;
  /** Undoes one folder's Tidy up of a kept run (its report updates once it has). */
  onUndo: (runId: string, tidyId: string) => Promise<unknown>;
}

/**
 * Where the map is zoomed: a region (a category, or a fast scan's Locked folders), then folders opened inside a
 * category.
 */
type Level = { kind: 'category'; id: string } | { kind: 'folder'; folder: MapFolder };

const NO_GROUPS: readonly DuplicateGroup[] = [];
const NO_IDS: readonly string[] = [];

/**
 * Variant D's map screen, for the drive on screen. Before a scan: the plate with the drives on the living dot
 * field. While scanning: the HUD over the map as it fills in. After: the overview treemap, where a click zooms into
 * a category, then opens its folders, with a breadcrumb back out; hovering shows what a tile is, how big and when it
 * was last used. Reclaimable tiles (the drive's proposals) glow; with the plan tray open, clicking one puts it in
 * the drive's plan or takes it out. "Free X GB" checks the plan and asks once; while it runs the tray is the run
 * panel and the plan's tiles drain to vacant ground. After the run the drive's screen is its finish: the highlight
 * slides, then the summary (the before/after map and what changed), which the drive keeps until it is scanned again.
 */
export function Atlas({ drives, agent, advisorNote, advisorOpen, planOpen, onPlanOpen, onUndo }: Props) {
  const bridge = useBridge();
  const field = useRef<DotFieldHandle>(null);
  const [calm, setCalm] = useState<HTMLDivElement | null>(null);
  const [stageRef, size] = useSize<HTMLDivElement>();
  const [levels, setLevels] = useState<Level[]>([]);
  const [hot, setHot] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [browseError, setBrowseError] = useState<string | null>(null);

  const drive = drives.current;
  const state = drive.scan;
  const overview = state.phase === 'done' && drives.screen === 'map' ? state.overview : null;
  const scanning = state.phase === 'scanning' ? state : null;
  // A fast scan waiting on Windows' admin prompt: nothing is scanning yet, so the plate stays.
  const asking = !!scanning && isAsking(scanning, drive.volume);
  const proposals = useMemo(() => (state.phase === 'done' ? state.proposals : []), [state]);
  const plan = state.phase === 'done' ? state.plan : [];
  const byId = useMemo(() => new Map(proposals.map((p) => [p.id, p])), [proposals]);
  const planRun = state.phase === 'done' ? state.planRun : null;
  // The run on the run panel, until "See what changed" turns it into the summary.
  const run: ShownRun | null = overview && planRun && planRun.phase !== 'summary' ? planRun : null;
  const summary = overview && planRun?.phase === 'summary' ? planRun : null;
  const planning = !!overview && planOpen && !planRun;
  /** The run's steps by their proposal's id. */
  const runSteps = useMemo(() => new Map<string, StepView>(run?.view.steps.map((s) => [s.id, s])), [run]);
  /** What the run has freed from each tile so far. */
  const runFreed = useMemo(() => (run ? freedBy(run) : null), [run]);
  // The duplicate search that runs after the scan, and the groups it found.
  const search = state.phase === 'done' ? state.duplicates : null;
  const skipped = state.phase === 'done' ? state.skippedGroups : NO_IDS;
  const found = search?.phase === 'done' ? search.found : null;
  const groups = found?.groups ?? NO_GROUPS;
  const choice = useMemo(() => (found ? { groups: found.groups, skipped } : null), [found, skipped]);
  // The sorts as this plan would run them: an unticked duplicate group's extra copies stay sortable (delete wins over
  // sort only while a delete that takes the file is in the plan).
  const tidy = useMemo(
    () => (state.phase === 'done' ? sortable(state.tidy, groups, skipped) : []),
    [state, groups, skipped],
  );
  const [dupPanel, setDupPanel] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [hoverGroup, setHoverGroup] = useState<string | null>(null);
  const showPanel = planning && dupPanel && groups.length > 0;
  // "Free X GB": the plan being checked, then the plan to confirm.
  const [checking, setChecking] = useState(false);
  const [confirm, setConfirm] = useState<PlanSummary | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  // The Tidy up proposal whose drawer is open over the map, while planning.
  const [drawerId, setDrawerId] = useState<string | null>(null);
  // Claude (ADR 0008): its panel beside the map, the folders it explained (a ✳ on their tiles and a "Claude says" in
  // their hover cards, a click asks about them) and the plan items its review flags.
  const claude = useClaude();
  const claudeOn = isOn(claude);
  const scope: Scope | null =
    overview && state.phase === 'done'
      ? { mountPoint: overview.volume.mountPoint, run: state.run, plan, planVersion: state.planVersion, tidyIds: state.tidy.map((t) => t.id) }
      : null;
  const talk = claude && scope ? claude.conversation(scope) : null;
  const flagged = new Set(talk && claudeOn ? liveFlags(talk, plan).map((f) => f.id) : []);
  const explained = useMemo(
    () => new Map(talk && claudeOn ? explainedFolders(talk).map((f) => [f.path.toLowerCase(), f]) : []),
    [talk, claudeOn],
  );
  const explainedOf = (leaf: Leaf) => (leaf.proposal === null && leaf.path ? (explained.get(leaf.path.toLowerCase()) ?? null) : null);
  const askAbout = (leaf: Leaf) => {
    if (!claude || !scope) return;
    claude.setFolded(false);
    claude.ask(scope, { kind: 'folder', path: leaf.path }, leaf.name);
  };
  // Which scan's map this is. The duplicate search redraws the map when it is done; that is still the same map, and
  // nothing the user was doing on it is lost.
  const mapKey = overview && state.phase === 'done' ? `${overview.volume.mountPoint}#${state.run}` : null;
  const [seen, setSeen] = useState(mapKey);
  if (seen !== mapKey) {
    // A new map starts at the whole disk.
    setSeen(mapKey);
    setLevels([]);
    setHot(null);
    setOpening(null);
    setBrowseError(null);
    setChecking(false);
    setConfirm(null);
    setPlanError(null);
    setDupPanel(false);
    setPicked(null);
    setHoverGroup(null);
    setDrawerId(null);
  }
  // Which folder opening is still wanted. Any move on the map, or a new map, makes a pending one stale, so a
  // folder that arrives late never pulls the map back to where the user was. Plans likewise: a change to the plan,
  // or a new map, drops one still being checked.
  const request = useRef(0);
  const planRequest = useRef(0);
  useEffect(() => {
    request.current++;
    planRequest.current++;
  }, [mapKey]);

  const top = useMemo(() => (overview ? overviewModel(overview) : null), [overview]);
  const level = levels.at(-1);
  const model = useMemo(() => {
    if (scanning) return asking ? null : scanModel(drive.volume, scanning.preview, scanning.update?.bytes ?? 0);
    if (!overview || !top) return null;
    if (level?.kind === 'folder') {
      const cat = overview.categories.find((c) => c.id === level.folder.category);
      return folderModel(level.folder, { name: cat?.name ?? '', protected: cat?.protected ?? false }, overview.volume);
    }
    return top;
  }, [scanning, asking, drive.volume, overview, top, level]);
  const focus = level?.kind === 'category' ? level.id : null;
  // While a run frees space its tiles shrink by what their steps freed and free space grows by it all, laid out
  // against the map the run started from so nothing moves but the sizes. The run holds the map at the whole disk.
  const view = run?.view;
  const freeing = view && top && model === top ? top : null;
  const shown = useMemo(
    () => (freeing && view && runFreed ? freedModel(freeing, runFreed, view.freedBytes) : model),
    [freeing, view, runFreed, model],
  );

  const progress = scanning && !asking ? scanProgress(scanning.update, drive.volume.usedBytes) : 0;

  const navigate = useCallback((next: Level[] | ((levels: Level[]) => Level[])) => {
    request.current++;
    setOpening(null);
    setBrowseError(null);
    setHot(null);
    setLevels(next);
  }, []);

  // Folders open from this map's own drive.
  const mapDrive = overview?.volume.mountPoint ?? null;
  const open = useCallback(
    (leaf: Leaf) => {
      if (mapDrive === null) return;
      const id = ++request.current;
      setOpening(leaf.id);
      setBrowseError(null);
      bridge.browse(mapDrive, leaf.id).then(
        (folder) => {
          if (request.current !== id) return;
          setOpening(null);
          setHot(null);
          setLevels((ls) => [...ls, { kind: 'folder', folder }]);
        },
        (error: unknown) => {
          if (request.current !== id) return;
          setOpening(null);
          setBrowseError(errorMessage(error));
        },
      );
    },
    [bridge, mapDrive],
  );

  const up = useCallback(() => navigate((ls) => ls.slice(0, -1)), [navigate]);
  useEffect(() => {
    // The finish has its own keys: Escape there skips the slides.
    if (!overview || summary) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') up();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [overview, summary, up]);

  const inPlan = (leaf: Leaf) => leaf.proposal !== null && plan.includes(leaf.proposal);
  /** Changing the plan drops one still being checked, and an old reason it couldn't be made. */
  const planChanged = () => {
    planRequest.current++;
    setChecking(false);
    setPlanError(null);
  };
  const setInPlan = (id: string, on: boolean) => {
    planChanged();
    drives.setInPlan(id, on);
  };
  const toggleGroup = (id: string) => {
    planChanged();
    drives.setGroupSkipped(id, !skipped.includes(id));
  };
  /** Opens the duplicates panel beside the whole map, a group picked: `tile`'s, or the first. */
  const openDuplicates = (tile?: string) => {
    if (groups.length === 0) return;
    onPlanOpen(true);
    setDupPanel(true);
    const inTile = tile ? groups.find((g) => g.extras.some((e) => e.tile === tile)) : undefined;
    setPicked((p) => inTile?.id ?? p ?? groups[0]?.id ?? null);
    if (levels.length > 0) navigate([]);
  };
  // The group whose links the map draws: the one under the pointer in the panel, or the one picked.
  const lit = showPanel ? (groups.find((g) => g.id === (hoverGroup ?? picked)) ?? null) : null;
  const goneTiles = useMemo(() => new Set(lit?.extras.map((e) => e.tile) ?? []), [lit]);
  const onLeaf = (leaf: Leaf) => {
    if (run) return;
    // A folder Claude explained: the click asks Claude about it in the panel (zoom from the region's header).
    if (explainedOf(leaf)) return askAbout(leaf);
    if (leaf.proposal === DUPLICATES && planning && groups.length > 0) return openDuplicates(leaf.id);
    if (leaf.proposal !== null) {
      // With the tray open a reclaimable tile goes in or out of the plan; zoomed in, it goes in and opens the tray.
      if (planning) return setInPlan(leaf.proposal, !inPlan(leaf));
      if (levels.length > 0) {
        setInPlan(leaf.proposal, true);
        return onPlanOpen(true);
      }
    }
    if (levels.length === 0) navigate([{ kind: 'category', id: leaf.regionId }]);
    else if (leaf.browsable) open(leaf);
  };
  const onRegion = (region: Region) => {
    if (run) return;
    if (levels.length === 0 && !region.free) navigate([{ kind: 'category', id: region.id }]);
    else up();
  };
  // Reclaimable tiles always react; other tiles zoom in at the top and open folders below. A run holds the map still.
  const clickable = (leaf: Leaf) =>
    !run && (leaf.proposal !== null || !!explainedOf(leaf) || (levels.length === 0 ? leaf.kind !== 'free' : leaf.browsable));
  const drawer = planning && levels.length === 0 ? (tidy.find((t) => t.id === drawerId) ?? null) : null;
  const drawerTile = drawer && overview ? folderTileOf(overview, drawer) : null;
  /** A tile the picked duplicate group links: where its kept copy lives, or where its extra copies go from. */
  const linkLook = (leaf: Leaf, rect: Rect): TileLook | null => {
    if (!lit || levels.length > 0) return null;
    if (goneTiles.has(leaf.id)) return { cls: 'is-linked' };
    if (leaf.id !== lit.keep.tile) return null;
    // The tag only where it clears the tile's name; a small tile keeps the white edge and the link's white end.
    return rect.w >= 150 && rect.h >= 50
      ? { cls: 'is-keeper', badge: <span className="dup-kept-tag">Kept here</span>, state: 'the kept copy is here' }
      : { cls: 'is-keeper', state: 'the kept copy is here' };
  };
  const look = (leaf: Leaf, rect: Rect): TileLook => {
    if (leaf.kind === 'locked') return lockedLook(leaf, rect, levels.length > 0);
    if (leaf.id === drawerTile) return { cls: 'is-tidy-hot', state: 'loose files Vacancy can sort' };
    const linked = linkLook(leaf, rect);
    if (leaf.proposal === null) {
      if (!run && explainedOf(leaf))
        return {
          ...linked,
          cls: ['is-explained', linked?.cls].filter(Boolean).join(' '),
          badge: linked?.badge ?? (
            <span className="ai-pin" aria-hidden>
              ✳
            </span>
          ),
          state: 'Claude explained this folder',
        };
      return linked ?? {};
    }
    if (run) return runLook(runSteps.get(leaf.proposal), runFreed?.get(leaf.id));
    // A proposal drawn as several tiles lights whole when its row in the tray is under the pointer.
    const together = hot !== null && hot !== leaf.id && hot === tileOf(leaf.proposal) ? ' is-hot' : '';
    const on = inPlan(leaf);
    const base: TileLook = !planning
      ? { cls: 'is-reclaim', state: 'reclaimable' }
      : on
        ? { cls: 'is-reclaim is-selected', badge: <Pin on />, state: 'reclaimable, in the plan', pressed: true }
        : { cls: 'is-available', badge: <Pin on={false} />, state: 'reclaimable, not in the plan', pressed: false };
    const cls = [base.cls, together.trim(), linked?.cls, flagged.has(leaf.proposal) && 'is-flagged'].filter(Boolean).join(' ');
    return { ...base, ...linked, cls, badge: linked?.badge ?? base.badge };
  };
  const leafStatus = (leaf: Leaf): ReactNode => {
    const p = leaf.proposal === null ? undefined : byId.get(leaf.proposal);
    const note = !run ? explainedOf(leaf) : null;
    if (note)
      return (
        <TipSection
          note={note}
          suggested={proposals.filter((x) => note.suggested.includes(x.id))}
          act="Click to ask Claude about it in the panel."
        />
      );
    if (!p) return null;
    if (run) return runStatus(runSteps.get(leaf.proposal ?? ''), runFreed?.get(leaf.id));
    if (p.id === DUPLICATES)
      return (
        <>
          <div className="map-tip-meta">
            <RiskChip risk={p.risk} />
            <SourceTag source={p.source} agent={agent} />
          </div>
          <p className="map-tip-sum">
            {leaf.note ? `${leaf.note}, of files kept elsewhere.` : 'Extra copies of files kept elsewhere.'} One copy of
            each stays.
          </p>
          <p className="map-tip-act">
            {planning ? 'Click to see the groups.' : levels.length === 0 ? 'Click to zoom in.' : 'Click to add it to the plan.'}
          </p>
        </>
      );
    const on = inPlan(leaf);
    const act = planning
      ? on
        ? 'Click to take it out of the plan.'
        : 'Click to add it to the plan.'
      : levels.length === 0
        ? 'Click to zoom in.'
        : on
          ? 'In the plan. Click to see it there.'
          : 'Click to add it to the plan.';
    return (
      <>
        <div className="map-tip-meta">
          <RiskChip risk={p.risk} />
          <SourceTag source={p.source} agent={agent} />
        </div>
        <p className="map-tip-sum">{claudeNoteOf(p.claudeNote) ?? p.summary}</p>
        <p className="map-tip-act">{act}</p>
      </>
    );
  };
  const leafAction = (leaf: Leaf) =>
    run
      ? null
      : levels.length === 0
        ? leaf.kind === 'free'
          ? null
          : 'Click to zoom in.'
        : leaf.browsable
          ? 'Click to open.'
          : null;
  const regionAction = () => (run ? null : levels.length === 0 ? 'Click to zoom in.' : 'Click to zoom out.');

  // "Free X GB": the core checks every place again and makes the plan; the dialog shows its numbers.
  const free = () => {
    if (!overview) return;
    const id = ++planRequest.current;
    setChecking(true);
    setPlanError(null);
    bridge.buildPlan(overview.volume.mountPoint, plan, [...skipped]).then(
      (made) => {
        if (planRequest.current !== id) return;
        setChecking(false);
        setConfirm(made);
      },
      (error: unknown) => {
        if (planRequest.current !== id) return;
        setChecking(false);
        setPlanError(errorMessage(error));
      },
    );
  };
  const runConfirmed = (made: PlanSummary) => {
    if (advisorOpen) return;
    setConfirm(null);
    onPlanOpen(false);
    navigate([]);
    drives.runPlan(made);
  };
  const freedSoFar = run?.view.freedBytes ?? 0;
  /** Claude's notes on its items and layouts, for the confirm dialog. */
  const claudeNotes = useMemo(() => {
    const notes: Record<string, string> = {};
    for (const p of proposals) {
      const n = claudeNoteOf(p.claudeNote);
      if (p.source === 'claude' && n) notes[p.id] = n;
    }
    for (const t of tidy) if (t.layout === 'claude' && t.note) notes[t.id] = t.note;
    return notes;
  }, [proposals, tidy]);

  const startScan = (button: HTMLButtonElement, options?: { fast: boolean }) => {
    const b = button.getBoundingClientRect();
    field.current?.drop(b.left + b.width / 2, b.top + b.height / 2, 'splash');
    drives.scan(options);
  };

  const onPlate = !overview && (!scanning || asking);
  const bytesInView =
    level?.kind === 'folder'
      ? level.folder.item.bytes
      : (focus && top?.regionById.get(focus)?.bytes) || (overview?.volume.totalBytes ?? 0);

  /** From the Claude panel: a proposal in the tray (opened, its row in view) and its tile lit. */
  const showItem = (id: string) => {
    if (levels.length > 0) navigate([]);
    onPlanOpen(true);
    setDupPanel(false);
    setHot(tileOf(id));
    requestAnimationFrame(() =>
      [...document.querySelectorAll<HTMLElement>('[data-row]')]
        .find((el) => el.dataset.row === id)
        ?.scrollIntoView?.({ block: 'nearest', behavior: 'smooth' }),
    );
  };
  const showTile = (tile: string) => {
    if (levels.length > 0) navigate([]);
    setHot(tile);
  };
  const panel = overview && scope && !summary && !run && (
    <ClaudePanel
      scope={scope}
      proposals={proposals}
      tidy={state.phase === 'done' ? state.tidy : []}
      overview={overview}
      onShow={showItem}
      onShowTile={showTile}
      onOpenTidy={(id) => {
        if (levels.length > 0) navigate([]);
        onPlanOpen(true);
        setDupPanel(false);
        setDrawerId(id);
      }}
      onUntick={(id) => setInPlan(id, false)}
    />
  );

  return (
    <>
      {panel}
      {/* Kept while the finish is up (hidden), so the map's size is still followed when it comes back. */}
      <main className={overview ? 'atlas is-map' : 'atlas is-scan'} hidden={!!summary}>
        {!overview && (
          <DotField
            ref={field}
            calm={onPlate ? calm : null}
            scan={scanning && !asking ? progress : null}
            out={progress > 0.85}
          />
        )}
        <div className="atlas-toolbar">
          <nav className="crumbs" aria-label="Map location">
            {levels.length === 0 || !overview ? (
              <span className="crumb is-here" aria-current="location">
                {(overview?.volume ?? drive.volume).name}
              </span>
            ) : (
              <button type="button" className="crumb" onClick={() => navigate([])}>
                {overview.volume.name}
              </button>
            )}
            {overview &&
              levels.map((l, i) => {
                const last = i === levels.length - 1;
                const name = l.kind === 'category' ? (top?.regionById.get(l.id)?.name ?? l.id) : l.folder.item.name;
                return (
                  <span key={i} className="crumb-step">
                    <ChevronRight size={13} aria-hidden />
                    {last ? (
                      <span className="crumb is-here" aria-current="location">
                        {name}
                      </span>
                    ) : (
                      <button type="button" className="crumb" onClick={() => navigate(levels.slice(0, i + 1))}>
                        {name}
                      </button>
                    )}
                  </span>
                );
              })}
            {overview && level && (
              <span className="crumb-blurb">
                {level.kind === 'category' ? top?.regionById.get(level.id)?.blurb : level.folder.item.path}
              </span>
            )}
            {scanning && !asking && (
              <span className="crumb-blurb">
                {scanning.update?.method === 'fast' ? 'Mapping from the file index' : 'Mapping as it scans'}
              </span>
            )}
            {opening && <LoaderCircle className="crumb-spin" size={13} aria-label="Opening" />}
            {browseError && (
              <span className="crumb-error" role="alert">
                {browseError}
              </span>
            )}
          </nav>
          {overview && !run && (
            <DuplicatesPill
              search={search}
              groups={groups}
              open={showPanel}
              onToggle={() => (showPanel ? setDupPanel(false) : openDuplicates())}
            />
          )}
          {overview && (
            <>
              <ScanNotes
                volume={overview.volume}
                method={overview.scan.method}
                fallback={overview.scan.fallback}
                durationMs={overview.scan.durationMs}
                locked={levels.length === 0 ? overview.scan.locked : []}
                lockedBlurb={top?.regionById.get(LOCKED_ID)?.blurb ?? ''}
                unreadable={overview.scan.unreadable}
                samples={overview.scan.unreadableSamples}
                overCount={overview.scan.overCountBytes}
                os={bridge.platform === 'macos' ? 'macOS' : 'Windows'}
                missingAccess={drives.fullDiskAccess === 'missing'}
                onOpenAccess={() => void bridge.openFullDiskAccessSettings()}
              />
              <Legend
                bytesInView={bytesInView}
                w={size.w}
                h={size.h}
                reclaimable={proposals.length > 0 && !run}
                vacant={!!run && run.phase !== 'failed'}
              />
              {(!run || run.phase === 'failed') && (
                <button type="button" className="btn btn--sm" onClick={drives.showScanScreen}>
                  <RotateCcw size={12} aria-hidden /> Scan again
                </button>
              )}
            </>
          )}
        </div>
        <div className={onPlate ? 'atlas-stage is-empty' : 'atlas-stage'} ref={stageRef}>
          {shown && !summary && (
            <MapView
              freeLabel={
                freedSoFar > 0 && top ? <span className="free-grow">{formatBytes(top.freeBytes + freedSoFar)}</span> : undefined
              }
              model={shown}
              {...(freeing ? { base: freeing } : {})}
              width={size.w}
              height={size.h}
              focus={focus}
              interactive={!!overview}
              hot={hot}
              onHot={setHot}
              onLeaf={onLeaf}
              {...(run ? {} : { onRegion })}
              clickable={clickable}
              leafAction={leafAction}
              regionAction={regionAction}
              look={look}
              leafStatus={leafStatus}
              planning={planning}
            />
          )}
          {lit && top && model === top && levels.length === 0 && !summary && (
            <DuplicateLinks model={top} width={size.w} height={size.h} group={lit} />
          )}
          {onPlate && (
            <ScanPlate
              drives={drives}
              onOpenFullDiskAccess={() => void bridge.openFullDiskAccessSettings()}
              advisorNote={advisorNote}
              failure={state.phase === 'failed' ? state.message : null}
              offerFast={bridge.platform === 'windows'}
              asking={asking}
              onScan={startScan}
              contentRef={setCalm}
            />
          )}
          {scanning && !asking && (
            <ScanHud update={scanning.update} volume={drive.volume} progress={progress} onCancel={drives.cancelScan} />
          )}
          {overview && focus === LOCKED_ID && !summary && <LockedCard groups={overview.scan.locked} />}
          {drives.gone && <GoneNotice gone={drives.gone} onDismiss={drives.dismissGone} />}
        </div>
        {drawer && (
          <TidyDrawer
            key={`${drawer.id}:${drawer.layout}`}
            tidy={drawer}
            plan={plan}
            proposals={proposals}
            onToggle={() => setInPlan(drawer.id, !plan.includes(drawer.id))}
            onClose={() => setDrawerId(null)}
          />
        )}
      </main>
      {showPanel && found && (
        <DuplicatesPanel
          found={found}
          skipped={skipped}
          inPlan={plan.includes(DUPLICATES)}
          platform={bridge.platform}
          picked={lit?.id ?? picked}
          onPick={setPicked}
          onHover={setHoverGroup}
          onToggleGroup={toggleGroup}
          onClose={() => {
            setDupPanel(false);
            setHoverGroup(null);
          }}
          onAdd={() => {
            setInPlan(DUPLICATES, true);
            setDupPanel(false);
            setHoverGroup(null);
          }}
        />
      )}
      {planning && !showPanel && (
        <PlanTray
          key={overview.volume.mountPoint}
          mountPoint={overview.volume.mountPoint}
          proposals={proposals}
          tidy={tidy}
          plan={plan}
          drawer={drawer?.id ?? null}
          onDrawer={(id) => {
            if (id !== null && levels.length > 0) navigate([]);
            setDrawerId(id);
          }}
          tidyTile={(t) => folderTileOf(overview, t)}
          agent={agent}
          hot={hot}
          onHot={setHot}
          {...(claude && claudeOn && scope
            ? {
                onAsk: (p: { id: string; title: string }) => {
                  claude.setFolded(false);
                  claude.ask(scope, { kind: 'proposal', id: p.id }, p.title);
                },
              }
            : {})}
          onToggle={(id) => setInPlan(id, !plan.includes(id))}
          onClose={() => onPlanOpen(false)}
          onFree={free}
          checking={checking}
          error={planError}
          duplicates={choice}
          onOpenDuplicates={() => openDuplicates()}
        />
      )}
      {run && (
        <RunPanel
          run={run}
          hot={hot}
          onHot={setHot}
          onStop={drives.cancelRun}
          onSeeChanges={drives.showSummary}
          onBack={() => {
            drives.leaveRun();
            onPlanOpen(true);
          }}
          onScanAgain={drives.showScanScreen}
        />
      )}
      {summary && (
        <Finish
          key={summary.report.id}
          report={summary.report}
          start={summary.fresh ? 'slides' : 'rest'}
          onSeen={drives.summarySeen}
          onScanAgain={drives.showScanScreen}
          onUndo={(tidyId) => onUndo(summary.report.id, tidyId)}
        />
      )}
      {planning && confirm && !advisorOpen && (
        <ConfirmDialog
          plan={confirm}
          platform={bridge.platform}
          notes={claudeNotes}
          onCancel={() => setConfirm(null)}
          onGo={() => runConfirmed(confirm)}
        />
      )}
    </>
  );
}

/** The map tile of the folder a Tidy up sorts, if the overview has one: the folder's own tile. */
function folderTileOf(overview: Overview, t: TidyProposal): string | null {
  const key = t.path.toLowerCase();
  for (const c of overview.categories) {
    const item = c.items.find((i) => i.path.toLowerCase() === key);
    if (item) return item.id;
  }
  return null;
}

/**
 * What each step has freed from its tile so far, and left in place, by tile. The duplicates step has a tile per
 * folder: while it runs each gets its share of what the step has freed, by what the plan takes from it; once the run
 * has ended, what the report says it freed there.
 */
function freedBy(run: ShownRun): Map<string, Freed> {
  const out = new Map<string, Freed>();
  for (const s of run.view.steps) {
    const kept = s.skips.reduce((sum, k) => sum + k.bytes, 0);
    const tiles = s.id === DUPLICATES ? (run.plan.duplicates?.tiles ?? []) : [];
    if (tiles.length === 0) {
      out.set(tileOf(s.id), { bytes: s.freedBytes, kept });
      continue;
    }
    const done = run.phase === 'ended' ? run.report.duplicates?.tiles : undefined;
    const total = tiles.reduce((sum, t) => sum + t.bytes, 0);
    for (const t of tiles) {
      const exact = done?.find((d) => d.id === t.id);
      const share = total > 0 ? t.bytes / total : 0;
      out.set(
        t.id,
        exact
          ? { bytes: exact.freedBytes, kept: Math.max(0, exact.plannedBytes - exact.freedBytes) }
          : { bytes: s.freedBytes * share, kept: kept * share },
      );
    }
  }
  return out;
}

/**
 * A plan tile while its plan runs: queued, then shrinking as its step frees space (what is left of it at the
 * bottom, the vacant ground it keeps above), then a sliver of vacant ground showing what it freed. The tile is the
 * freed map's, so its size and fill already say how far the step has got.
 */
function runLook(step: StepView | undefined, tile?: Freed): TileLook {
  if (!step) return {};
  if (step.status === 'pending') return { cls: 'is-queued', state: 'waiting to be deleted' };
  if (step.status === 'running') return { cls: 'is-running', state: 'being deleted' };
  // A step that freed nothing (it was stopped first, or everything in it was in use) leaves its tile as it was.
  const freed = tile?.bytes ?? step.freedBytes;
  if (freed <= 0) return {};
  return {
    cls: 'is-vacant is-lit',
    size: <span className="map-freed">{formatBytes(freed)}</span>,
    state: 'vacant',
  };
}

/** What a plan tile's hover card says while its plan runs. */
function runStatus(step: StepView | undefined, tile?: Freed): string {
  if (!step) return 'Not in this plan.';
  if (step.status === 'pending') return 'Waiting its turn.';
  if (step.status === 'running')
    return `Being deleted now: ${formatBytes(step.freedBytes)} of ${formatBytes(step.plannedBytes)} freed.`;
  if ((tile?.bytes ?? step.freedBytes) <= 0) return 'Nothing was deleted here.';
  const left = step.skips.length;
  return `Vacant now. ${formatBytes(step.freedBytes)} freed${left > 0 ? `; ${formatCount(left)} ${left === 1 ? 'thing' : 'things'} left in place` : ''}.`;
}

/**
 * A tile of the Locked folders block: a lock, and where it has room its name and total. It has nothing else to show:
 * no names from inside, nothing to open, nothing to propose.
 */
function lockedLook(leaf: Leaf, rect: Rect, zoomed: boolean): TileLook {
  const roomy = rect.w >= 110 && rect.h >= 64;
  return {
    label: (
      <div className="locked-label">
        <Lock size={zoomed ? 16 : 12} strokeWidth={2.2} aria-hidden />
        {roomy && <span className="locked-name">{leaf.name}</span>}
        {roomy && <span className="locked-size">{formatBytes(leaf.bytes)} · totals only</span>}
      </div>
    ),
    state: 'locked, totals only',
  };
}

/** The opened Locked folders block: what it is, said once, beside its totals. */
function LockedCard({ groups }: { groups: LockedGroup[] }) {
  const { bytes, folders } = lockedTotals(groups);
  return (
    <aside className="locked-card" aria-label="About locked folders" data-no-ripple>
      <p className="locked-card-h">
        <Lock size={14} aria-hidden /> {formatBytes(bytes)} in {formatCount(folders)} locked{' '}
        {folders === 1 ? 'folder' : 'folders'}
      </p>
      <p>
        Your account can’t open these folders, even though fast scan could count them. So Vacancy shows only how much
        space they take: no names, nothing to propose, nothing to delete.
      </p>
      <p className="locked-card-quiet">
        To clean up another account’s files, sign in to Windows as that account and run Vacancy there. Admin from a
        fast scan is never used to open them.
      </p>
    </aside>
  );
}

/** The round + or tick in a reclaimable tile's corner while the plan tray is open. */
function Pin({ on }: { on: boolean }) {
  return (
    <span className={on ? 'map-pin is-on' : 'map-pin'} aria-hidden>
      {on ? <Check size={11} strokeWidth={3} /> : <Plus size={11} strokeWidth={2.6} />}
    </span>
  );
}

interface NotesProps {
  volume: VolumeInfo;
  /** How the drive was read, and why a fast scan asked for wasn't, if it wasn't. */
  method: ScanMethod;
  fallback: FastFallback | null;
  durationMs: number;
  /** A fast scan's locked folders, to note at the top level (none when zoomed in). */
  locked: LockedGroup[];
  /** The Locked folders block's blurb, for the note's tooltip. */
  lockedBlurb: string;
  unreadable: number;
  samples: string[];
  /** Bytes the scan counted beyond what the OS reports in use. */
  overCount: number;
  /** "Windows" or "macOS", for the over-count note. */
  os: string;
  missingAccess: boolean;
  onOpenAccess: () => void;
}

/**
 * After a scan: a fast scan says so, how long it took, and how much is in locked folders; a normal scan that ran in
 * place of a fast one says why. Then how many folders could not be read; if the scan counted more than the OS says
 * is in use (files that grew or appeared while it ran), by how much, since the map then holds more than the
 * header's used figure; and on a Mac without Full Disk Access, the way to fix it.
 */
function ScanNotes(p: NotesProps) {
  const fast = p.method === 'fast';
  const locked = lockedTotals(p.locked).bytes;
  if (!fast && !p.fallback && p.unreadable === 0 && p.overCount <= 0 && !p.missingAccess) return null;
  return (
    <div className="scan-notes">
      {fast && (
        <span className="scan-note is-fast" title="Read from the drive’s file index, with admin, then admin was given back.">
          <Zap size={11} aria-hidden /> Fast scan, {Math.max(1, Math.round(p.durationMs / 1000))} s
        </span>
      )}
      {fast && locked > 0 && (
        <span className="scan-note" title={p.lockedBlurb}>
          <Lock size={11} aria-hidden /> {formatBytes(locked, 0)} in locked folders, totals only
        </span>
      )}
      {!fast && p.fallback && <span className="scan-note">{fallbackLabel(p.fallback, p.volume)}</span>}
      {p.overCount > 0 && (
        <span
          className="scan-note"
          title={`The map adds up to more than ${p.os} reports as used. Files changed while the scan ran; scan again for exact figures.`}
        >
          Counted {formatBytes(p.overCount)} more than {p.os} says is in use: files changed during the scan
        </span>
      )}
      {p.unreadable > 0 && (
        <span className="scan-note" title={`Couldn’t read:\n${p.samples.join('\n')}`}>
          {formatCount(p.unreadable)} {p.unreadable === 1 ? 'folder' : 'folders'} couldn’t be read
        </span>
      )}
      {p.missingAccess && (
        <button
          type="button"
          className="scan-note is-action"
          onClick={p.onOpenAccess}
          title="Allow Full Disk Access"
          aria-label="Allow Full Disk Access"
        >
          <ShieldAlert size={12} aria-hidden /> <span className="scan-note-text">Allow Full Disk Access</span>
        </button>
      )}
    </div>
  );
}

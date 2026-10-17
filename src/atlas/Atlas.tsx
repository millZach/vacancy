import { Check, ChevronRight, LoaderCircle, Plus, RotateCcw, ShieldAlert } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useBridge, type CategoryId, type MapFolder, type PlanSummary } from '../bridge';
import { DotField, type DotFieldHandle } from '../dotfield/DotField';
import type { ReadyDrives } from '../drives/context';
import { GoneNotice } from '../drives/GoneNotice';
import { errorMessage } from '../errors';
import { formatBytes, formatCount } from '../format';
import { Legend } from '../map/Legend';
import { MapView, type TileLook } from '../map/MapView';
import { folderModel, freedModel, overviewModel, scanModel, type Freed, type Leaf, type Region } from '../map/model';
import { useSize } from '../map/useSize';
import { RiskChip, SourceTag } from '../plan/bits';
import { PlanTray } from '../plan/PlanTray';
import { tileOf } from '../plan/totals';
import { ConfirmDialog } from '../run/ConfirmDialog';
import type { StepView } from '../run/progress';
import { RunPanel, type ShownRun } from '../run/RunPanel';
import { ScanHud } from '../scan/ScanHud';
import { ScanPlate } from '../scan/ScanPlate';
import { SummaryMap, SummaryPanel } from '../summary/Summary';
import './atlas.css';

interface Props {
  /** Every drive, and the drive on screen with its scan. */
  drives: ReadyDrives;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
  /** The plan tray is open beside the map. */
  planOpen: boolean;
  onPlanOpen: (open: boolean) => void;
}

/** Where the map is zoomed: a category, then folders opened inside it. */
type Level = { kind: 'category'; id: CategoryId } | { kind: 'folder'; folder: MapFolder };

/** Scan progress shown before the scan says it is done: never quite full. */
const ALMOST = 0.98;

/**
 * Variant D's map screen, for the drive on screen. Before a scan: the plate with the drives on the living dot
 * field. While scanning: the HUD over the map as it fills in. After: the overview treemap, where a click zooms into
 * a category, then opens its folders, with a breadcrumb back out; hovering shows what a tile is, how big and when it
 * was last used. Reclaimable tiles (the drive's proposals) glow; with the plan tray open, clicking one puts it in
 * the drive's plan or takes it out. "Free X GB" checks the plan and asks once; while it runs the tray is the run
 * panel and the plan's tiles drain to vacant ground. After the run the drive's screen is its summary: the
 * before/after map and what changed.
 */
export function Atlas({ drives, agent, planOpen, onPlanOpen }: Props) {
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
  const proposals = useMemo(() => (state.phase === 'done' ? state.proposals : []), [state]);
  const plan = state.phase === 'done' ? state.plan : [];
  const byId = useMemo(() => new Map(proposals.map((p) => [p.id, p])), [proposals]);
  const planRun = state.phase === 'done' ? state.planRun : null;
  // The run on the run panel, until "See what changed" turns it into the summary.
  const run: ShownRun | null = overview && planRun && planRun.phase !== 'summary' ? planRun : null;
  const summary = overview && planRun?.phase === 'summary' ? planRun : null;
  const planning = !!overview && planOpen && !planRun;
  /** The run's steps by their tile. */
  const runSteps = useMemo(() => new Map<string, StepView>(run?.view.steps.map((s) => [tileOf(s.id), s])), [run]);
  // "Free X GB": the plan being checked, then the plan to confirm.
  const [checking, setChecking] = useState(false);
  const [confirm, setConfirm] = useState<PlanSummary | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [seen, setSeen] = useState(overview);
  if (seen !== overview) {
    // A new map starts at the whole disk.
    setSeen(overview);
    setLevels([]);
    setHot(null);
    setOpening(null);
    setBrowseError(null);
    setChecking(false);
    setConfirm(null);
    setPlanError(null);
  }
  // Which folder opening is still wanted. Any move on the map, or a new map, makes a pending one stale, so a
  // folder that arrives late never pulls the map back to where the user was. Plans likewise: a change to the plan,
  // or a new map, drops one still being checked.
  const request = useRef(0);
  const planRequest = useRef(0);
  useEffect(() => {
    request.current++;
    planRequest.current++;
  }, [overview]);

  const top = useMemo(() => (overview ? overviewModel(overview) : null), [overview]);
  const level = levels.at(-1);
  const model = useMemo(() => {
    if (scanning) return scanModel(drive.volume, scanning.preview, scanning.update?.bytes ?? 0);
    if (!overview || !top) return null;
    if (level?.kind === 'folder') {
      const cat = overview.categories.find((c) => c.id === level.folder.category);
      return folderModel(level.folder, { name: cat?.name ?? '', protected: cat?.protected ?? false }, overview.volume);
    }
    return top;
  }, [scanning, drive.volume, overview, top, level]);
  const focus = level?.kind === 'category' ? level.id : null;
  // While a run frees space its tiles shrink by what their steps freed and free space grows by it all, laid out
  // against the map the run started from so nothing moves but the sizes. The run holds the map at the whole disk.
  const view = run?.view;
  const freeing = view && top && model === top ? top : null;
  const shown = useMemo(
    () => (freeing && view ? freedModel(freeing, freedBy(view.steps), view.freedBytes) : model),
    [freeing, view, model],
  );

  const progress = scanning
    ? Math.min(ALMOST, (scanning.update?.bytes ?? 0) / Math.max(1, drive.volume.usedBytes))
    : 0;

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
    if (!overview) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') up();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [overview, up]);

  const inPlan = (leaf: Leaf) => leaf.proposal !== null && plan.includes(leaf.proposal);
  /** Changing the plan drops one still being checked, and an old reason it couldn't be made. */
  const setInPlan = (id: string, on: boolean) => {
    planRequest.current++;
    setChecking(false);
    setPlanError(null);
    drives.setInPlan(id, on);
  };
  const onLeaf = (leaf: Leaf) => {
    if (run) return;
    if (leaf.proposal !== null) {
      // With the tray open a reclaimable tile goes in or out of the plan; zoomed in, it goes in and opens the tray.
      if (planning) return setInPlan(leaf.proposal, !inPlan(leaf));
      if (levels.length > 0) {
        setInPlan(leaf.proposal, true);
        return onPlanOpen(true);
      }
    }
    if (levels.length === 0) navigate([{ kind: 'category', id: leaf.regionId as CategoryId }]);
    else if (leaf.browsable) open(leaf);
  };
  const onRegion = (region: Region) => {
    if (run) return;
    if (levels.length === 0 && region.category) navigate([{ kind: 'category', id: region.category }]);
    else up();
  };
  // Reclaimable tiles always react; other tiles zoom in at the top and open folders below. A run holds the map still.
  const clickable = (leaf: Leaf) =>
    !run && (leaf.proposal !== null || (levels.length === 0 ? leaf.kind !== 'free' : leaf.browsable));
  const look = (leaf: Leaf): TileLook => {
    if (leaf.proposal === null) return {};
    if (run) return runLook(runSteps.get(leaf.id));
    if (!planning) return { cls: 'is-reclaim', state: 'reclaimable' };
    const on = inPlan(leaf);
    return on
      ? { cls: 'is-reclaim is-selected', badge: <Pin on />, state: 'reclaimable, in the plan', pressed: true }
      : { cls: 'is-available', badge: <Pin on={false} />, state: 'reclaimable, not in the plan', pressed: false };
  };
  const leafStatus = (leaf: Leaf): ReactNode => {
    const p = leaf.proposal === null ? undefined : byId.get(leaf.proposal);
    if (!p) return null;
    if (run) return runStatus(runSteps.get(leaf.id));
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
        <p className="map-tip-sum">{p.summary}</p>
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
    bridge.buildPlan(overview.volume.mountPoint, plan).then(
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
    setConfirm(null);
    onPlanOpen(false);
    navigate([]);
    drives.runPlan(made);
  };
  const freedSoFar = run?.view.freedBytes ?? 0;

  const startScan = (button: HTMLButtonElement) => {
    const b = button.getBoundingClientRect();
    field.current?.drop(b.left + b.width / 2, b.top + b.height / 2, 'splash');
    drives.scan();
  };

  const onPlate = !overview && !scanning;
  const bytesInView =
    level?.kind === 'folder'
      ? level.folder.item.bytes
      : (focus && overview?.categories.find((c) => c.id === focus)?.bytes) || (overview?.volume.totalBytes ?? 0);

  return (
    <>
      <main className={overview ? 'atlas is-map' : 'atlas is-scan'}>
        {!overview && <DotField ref={field} calm={onPlate ? calm : null} scan={scanning ? progress : null} out={progress > 0.85} />}
        {summary && top && <SummaryMap model={top} report={summary.report} hot={hot} onHot={setHot} />}
        <div className="atlas-toolbar" hidden={!!summary}>
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
                const name =
                  l.kind === 'category'
                    ? (overview.categories.find((c) => c.id === l.id)?.name ?? l.id)
                    : l.folder.item.name;
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
                {level.kind === 'category'
                  ? overview.categories.find((c) => c.id === level.id)?.blurb
                  : level.folder.item.path}
              </span>
            )}
            {scanning && <span className="crumb-blurb">Mapping as it scans</span>}
            {opening && <LoaderCircle className="crumb-spin" size={13} aria-label="Opening" />}
            {browseError && (
              <span className="crumb-error" role="alert">
                {browseError}
              </span>
            )}
          </nav>
          {overview && (
            <>
              <ScanNotes
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
        {/* Kept while the summary is up (hidden), so the map's size is still followed when it comes back. */}
        <div className={onPlate ? 'atlas-stage is-empty' : 'atlas-stage'} ref={stageRef} hidden={!!summary}>
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
          {onPlate && (
            <ScanPlate
              drives={drives}
              onOpenFullDiskAccess={() => void bridge.openFullDiskAccessSettings()}
              agent={agent}
              failure={state.phase === 'failed' ? state.message : null}
              onScan={startScan}
              contentRef={setCalm}
            />
          )}
          {scanning && <ScanHud update={scanning.update} progress={progress} onCancel={drives.cancelScan} />}
          {drives.gone && <GoneNotice gone={drives.gone} onDismiss={drives.dismissGone} />}
        </div>
      </main>
      {planning && (
        <PlanTray
          key={overview.volume.mountPoint}
          proposals={proposals}
          plan={plan}
          agent={agent}
          hot={hot}
          onHot={setHot}
          onToggle={(id) => setInPlan(id, !plan.includes(id))}
          onClose={() => onPlanOpen(false)}
          onFree={free}
          checking={checking}
          error={planError}
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
      {summary && overview && (
        <SummaryPanel
          overview={overview}
          report={summary.report}
          os={bridge.platform === 'macos' ? 'macOS' : 'Windows'}
          onHot={setHot}
          onScanAgain={drives.showScanScreen}
        />
      )}
      {planning && confirm && (
        <ConfirmDialog
          plan={confirm}
          platform={bridge.platform}
          onCancel={() => setConfirm(null)}
          onGo={() => runConfirmed(confirm)}
        />
      )}
    </>
  );
}

/** What each step has freed from its tile so far, and left in place, by tile. */
function freedBy(steps: readonly StepView[]): Map<string, Freed> {
  return new Map(
    steps.map((s) => [tileOf(s.id), { bytes: s.freedBytes, kept: s.skips.reduce((sum, k) => sum + k.bytes, 0) }]),
  );
}

/**
 * A plan tile while its plan runs: queued, then shrinking as its step frees space (what is left of it at the
 * bottom, the vacant ground it keeps above), then a sliver of vacant ground showing what it freed. The tile is the
 * freed map's, so its size and fill already say how far the step has got.
 */
function runLook(step: StepView | undefined): TileLook {
  if (!step) return {};
  if (step.status === 'pending') return { cls: 'is-queued', state: 'waiting to be deleted' };
  if (step.status === 'running') return { cls: 'is-running', state: 'being deleted' };
  // A step that freed nothing (it was stopped first, or everything in it was in use) leaves its tile as it was.
  if (step.freedBytes <= 0) return {};
  return {
    cls: 'is-vacant is-lit',
    size: <span className="map-freed">{formatBytes(step.freedBytes)}</span>,
    state: 'vacant',
  };
}

/** What a plan tile's hover card says while its plan runs. */
function runStatus(step: StepView | undefined): string {
  if (!step) return 'Not in this plan.';
  if (step.status === 'pending') return 'Waiting its turn.';
  if (step.status === 'running')
    return `Being deleted now: ${formatBytes(step.freedBytes)} of ${formatBytes(step.plannedBytes)} freed.`;
  if (step.freedBytes <= 0) return 'Nothing was deleted here.';
  const left = step.skips.length;
  return `Vacant now. ${formatBytes(step.freedBytes)} freed${left > 0 ? `; ${formatCount(left)} ${left === 1 ? 'thing' : 'things'} left in place` : ''}.`;
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
 * After a scan: how many folders could not be read; if the scan counted more than the OS says is in use (files
 * that grew or appeared while it ran), by how much, since the map then holds more than the header's used figure;
 * and on a Mac without Full Disk Access, the way to fix it.
 */
function ScanNotes(p: NotesProps) {
  if (p.unreadable === 0 && p.overCount <= 0 && !p.missingAccess) return null;
  return (
    <div className="scan-notes">
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
        <button type="button" className="scan-note is-action" onClick={p.onOpenAccess}>
          <ShieldAlert size={12} aria-hidden /> Allow Full Disk Access
        </button>
      )}
    </div>
  );
}

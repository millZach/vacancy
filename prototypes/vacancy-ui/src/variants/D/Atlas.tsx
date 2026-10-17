// PROTOTYPE variant D "Atlas + Wrapped". Copied from A "Atlas" (map-first: the disk is a treemap you explore; the plan
// and the run happen on the same map), plus a living dot field behind the scan plate, and a summary that opens with
// highlight slides (from C) and hands off to A's before/after map summary, which becomes the resting screen.
import { AnimatePresence } from 'motion/react';
import { Check, ChevronRight, HardDrive, Lock, Plus, Signpost, Sparkles } from 'lucide-react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { buildRun } from '../../shared/run';
import type { VariantProps } from '../../shared/types';
import { RiskChip, SourceTag, Wordmark } from './bits';
import { Legend, type MapMode } from './Legend';
import { MapView, useSize, type TileLook } from './MapView';
import { buildModel, gb, toggled, type AtlasModel, type Leaf, type Rect, type Region } from './model';
import { ConfirmDialog, PlanTray, ReorgDrawer, planTotals } from './PlanTray';
import { RunPanel, usePlayback } from './RunPanel';
import { ScanHud, ScanPlate } from './Scan';
import { SummaryMap, SummaryPanel } from './Summary';
import { DotField, type DotFieldHandle } from './DotField';
import { Wrapped, flyNumbers, type HandoffKind, type UndoState } from './Wrapped';
import './atlas.css';
import './vd.css';

const SCAN_MS = 4000;

/** What a drive-picker design gets to draw in the header. */
export interface HeadCtx {
  model: AtlasModel;
  mode: MapMode;
  focusRegion?: Region;
  clearFocus: () => void;
  scanning: boolean;
}

/**
 * PROTOTYPE drive picker: places each ?picker= design plugs into. Without them Atlas looks exactly as before.
 * headDrive replaces the header's drive plate, plate replaces the scan screen's plate (it gets the function that
 * starts the scan), rail sits left of the map on every step, hideCrumbs drops the map's own breadcrumb (design 3
 * moves it into the header), restSummary opens the summary at rest instead of replaying the highlights.
 */
export interface AtlasSlots {
  headDrive?: (ctx: HeadCtx) => ReactNode;
  plate?: (scan: (button: HTMLButtonElement) => void) => ReactNode;
  rail?: ReactNode;
  hideCrumbs?: boolean;
  restSummary?: boolean;
  /** Told when a scan starts and stops, so a drive switch mid-scan can cancel it. */
  onScanning?: (scanning: boolean) => void;
}

/** The header's used-space meter, with the reclaimable share lit on the map steps. `used` overrides the scan's figure. */
export function HeadMeter({ model, mode, used = model.usedBytes }: { model: AtlasModel; mode: MapMode; used?: number }) {
  return (
    <span className="va-meter va-meter--head" aria-hidden>
      <i style={{ width: `${(used / model.totalBytes) * 100}%` }} />
      {(mode === 'overview' || mode === 'plan') && (
        <b
          style={{
            left: `${((model.usedBytes - model.reclaimableBytes) / model.totalBytes) * 100}%`,
            width: `${(model.reclaimableBytes / model.totalBytes) * 100}%`,
          }}
        />
      )}
    </span>
  );
}

/**
 * The summary's stages. slides: highlights only. fly / skip: the slides make way while the map summary fades up
 * (fly also sends the receipt's numbers into the panel). rest: the map summary. replay: slides fading back in over it.
 */
type Phase = 'slides' | 'fly' | 'skip' | 'rest' | 'replay';

export function Atlas(props: VariantProps & AtlasSlots) {
  const { data, step, proposals, setProposals, reorgs, setReorgs } = props;
  const onStep = useRef(props.onStep);
  onStep.current = props.onStep;
  const go = (s: Parameters<VariantProps['onStep']>[0]) => onStep.current(s);

  const model = useMemo(() => buildModel(data), [data]);
  const result = useMemo(() => buildRun(data, proposals, reorgs), [data, proposals, reorgs]);
  const [focus, setFocus] = useState<string | null>(null);
  const [hot, setHot] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanT, setScanT] = useState(0);
  const [drawer, setDrawer] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [scanStart, setScanStart] = useState<number | null>(null);
  const field = useRef<DotFieldHandle>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const [phase, setPhase] = useState<Phase>(props.restSummary && step === 'summary' ? 'rest' : 'slides');
  const [handed, setHanded] = useState(false);
  const [undo, setUndo] = useState<UndoState>({});
  const [seenStep, setSeenStep] = useState(step);
  if (seenStep !== step) {
    // Every arrival at the summary starts with the highlights.
    setSeenStep(step);
    if (step === 'summary') {
      setPhase('slides');
      setHanded(false);
      setUndo({});
    }
  }
  const [stageRef, size] = useSize<HTMLDivElement>();
  const agent = data.scan.aiAgent;
  const pb = usePlayback(result, step === 'run');

  const mode: MapMode = step;

  useEffect(() => {
    if (step !== 'scan') {
      setScanning(false);
      setScanT(0);
      setScanStart(null);
    }
    if (step === 'scan' || step === 'run' || step === 'summary') setFocus(null);
    if (step !== 'plan') {
      setDrawer(null);
      setConfirm(false);
    }
  }, [step]);

  const onScanning = useRef(props.onScanning);
  onScanning.current = props.onScanning;
  useEffect(() => onScanning.current?.(scanning), [scanning]);

  useEffect(() => {
    if (!scanning) return;
    let raf = 0;
    let timer = 0;
    const start = performance.now();
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / SCAN_MS);
      setScanT(t);
      if (t < 1) raf = requestAnimationFrame(tick);
      else timer = window.setTimeout(() => go('overview'), 450);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      clearTimeout(timer);
    };
  }, [scanning]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (confirm) setConfirm(false);
      else if (drawer) setDrawer(null);
      else setFocus(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [confirm, drawer]);

  // Summary hand-off. The flight is measured right after the map summary mounts under the slides.
  useLayoutEffect(() => {
    if (phase !== 'fly' || !rootRef.current) return;
    let live = true;
    flyNumbers(rootRef.current).then(() => live && setPhase('rest'));
    return () => {
      live = false;
    };
  }, [phase]);
  useEffect(() => {
    if (phase !== 'skip' && phase !== 'replay') return;
    const t = window.setTimeout(() => setPhase(phase === 'skip' ? 'rest' : 'slides'), phase === 'skip' ? 520 : 420);
    return () => clearTimeout(t);
  }, [phase]);
  const handoff = (how: HandoffKind) => {
    const fly = how === 'fly' && !matchMedia('(prefers-reduced-motion: reduce)').matches;
    setHanded(fly);
    setPhase(fly ? 'fly' : 'skip');
  };
  const runUndo = (id: string) => {
    setUndo((u) => ({ ...u, [id]: 'working' }));
    window.setTimeout(() => setUndo((u) => ({ ...u, [id]: 'done' })), 1100);
  };

  // Scan reveal: regions are "discovered" one after another while the counters run.
  const regionOrder = useMemo(() => new Map(model.regions.map((r, i) => [r.id, i])), [model]);
  const visible =
    step === 'scan'
      ? (r: Region) => scanning && scanT >= 0.06 + ((regionOrder.get(r.id) ?? 0) / model.regions.length) * 0.8
      : undefined;

  const runIndex = useMemo(() => new Map(result.steps.map((s, i) => [s.id, i])), [result]);

  const look = (l: Leaf, rect: Rect): TileLook => {
    const p = l.proposal;
    const small = rect.w < 40 || rect.h < 26;
    if (mode === 'overview') {
      if (l.kind === 'reclaim') return { cls: 'is-reclaim', badge: p?.source === 'ai' && !small ? <Sparkles className="va-badge va-badge--ai" size={12} /> : undefined };
      if (l.kind === 'hint') return { cls: 'is-hint', badge: !small ? <Signpost className="va-badge" size={13} /> : undefined };
      return {};
    }
    if (mode === 'plan' && p) {
      const on = proposals.has(p.id);
      if (l.kind === 'hint') return { cls: `is-hint ${on ? 'is-hint-on' : ''}`, badge: !small ? <Signpost className="va-badge" size={13} /> : undefined };
      return {
        cls: on ? 'is-reclaim is-selected' : 'is-available',
        badge: small ? undefined : (
          <span className={`va-pin ${on ? 'is-on' : ''}`}>{on ? <Check size={11} strokeWidth={3} /> : <Plus size={11} strokeWidth={2.6} />}</span>
        ),
      };
    }
    if (mode === 'run' && p && l.kind === 'reclaim') {
      const i = runIndex.get(p.id);
      if (i === undefined) return {};
      const s = result.steps[i];
      const st = pb.states[i];
      const residual = s.plannedBytes ? 1 - s.freedBytes / s.plannedBytes : 0;
      return {
        cls: st.status === 'pending' ? 'is-queued' : st.status === 'running' ? 'is-running' : 'is-vacant is-lit',
        fill: 1 - st.p * (1 - residual),
      };
    }
    return {};
  };

  const regionCls = (r: Region) => {
    if (mode !== 'run') return undefined;
    const i = result.steps.findIndex((s) => s.kind === 'move' && s.categoryId === r.id);
    return i >= 0 && pb.states[i]?.status === 'running' ? 'is-moving' : undefined;
  };

  const tipStatus = (l: Leaf): ReactNode => {
    const r = model.regionById.get(l.regionId)!;
    if (l.kind === 'free') return 'Space you already have.';
    if (r.protected)
      return (
        <span className="va-tip-lock">
          <Lock size={12} /> Protected. Vacancy never changes anything here.
        </span>
      );
    const p = l.proposal;
    if (p && l.kind === 'reclaim') {
      const on = proposals.has(p.id);
      const act =
        mode === 'plan' ? (on ? 'Click to take it out of the plan.' : 'Click to add it to the plan.') : mode === 'overview' ? (focus ? 'Click to add it to a plan.' : 'Click to zoom in.') : null;
      return (
        <>
          <div className="va-tip-meta">
            <RiskChip risk={p.risk} />
            <SourceTag p={p} agent={agent} />
          </div>
          <p>{p.summary}</p>
          {act && <p className="va-tip-act">{act}</p>}
        </>
      );
    }
    if (p && l.kind === 'hint')
      return (
        <>
          <p>{p.summary}</p>
          <p className="va-tip-act">Advice only. Vacancy can't remove this itself.</p>
        </>
      );
    return l.rest ? 'The rest of this folder. Nothing proposed here.' : 'Nothing proposed here.';
  };

  const onLeaf = (l: Leaf) => {
    const p = l.proposal;
    if (mode === 'plan' && p) return setProposals(toggled(proposals, p.id));
    if (mode === 'overview' && focus && p) {
      if (!proposals.has(p.id)) setProposals(toggled(proposals, p.id));
      return go('plan');
    }
    setFocus(focus ? null : l.regionId);
  };
  const clickable = (l: Leaf) => (mode === 'overview' || mode === 'plan') && l.kind !== 'free';
  const onRegion = (r: Region) => (mode === 'overview' || mode === 'plan') && setFocus(focus === r.id ? null : r.id);

  const focusRegion = focus ? model.regionById.get(focus) : undefined;
  const lit = step === 'summary' || (step === 'run' && pb.done);
  const totals = planTotals(data, proposals, reorgs);
  const freeLabel = mode === 'run' && pb.freed > 0 ? <span className="va-free-grow">{gb(model.freeBytes + pb.freed)}</span> : undefined;
  const scanAgain = () => {
    setFocus(null);
    go('scan');
  };
  const startScan = (btn: HTMLButtonElement) => {
    const b = btn.getBoundingClientRect();
    field.current?.drop(b.left + b.width / 2, b.top + b.height / 2, true);
    setScanStart(performance.now());
    setScanning(true);
  };
  const slidesUp = mode === 'summary' && phase !== 'rest';
  const mapUp = mode === 'summary' && phase !== 'slides';

  return (
    <div
      ref={rootRef}
      className={`va-root vd-root va-mode-${mode} ${mode === 'summary' ? `vd-phase-${phase}` : ''} ${mode === 'summary' && handed ? 'vd-handed' : ''}`}
    >
      <header className="va-head">
        <Wordmark lit={lit} />
        {props.headDrive ? (
          props.headDrive({ model, mode, focusRegion, clearFocus: () => setFocus(null), scanning })
        ) : (
          <div className="va-head-disk">
            <HardDrive size={14} strokeWidth={1.8} />
            <span className="va-head-name">{data.disk.name}</span>
            <HeadMeter model={model} mode={mode} />
            <span className="va-head-nums">
              {gb(model.usedBytes, 0)} of {gb(model.totalBytes, 0)} used
            </span>
          </div>
        )}
        <div className="va-head-right">
          {(mode === 'overview' || mode === 'plan') && (
            <span className="va-head-reclaim">
              <b>{gb(model.reclaimableBytes)}</b> reclaimable
            </span>
          )}
          <span className={`va-agent ${agent ? '' : 'is-none'}`} title={agent ? `${agent} explains unfamiliar folders and suggests reorganizations` : 'No AI agent found'}>
            <Sparkles size={12} strokeWidth={2.2} />
            {agent ?? 'Rules only'}
          </span>
          {mode === 'overview' && (
            <button type="button" className="va-cta va-cta--sm" onClick={() => go('plan')}>
              Build a plan
            </button>
          )}
        </div>
      </header>

      <div className="va-body">
        {props.rail}
        <main className="va-main">
          {mode === 'scan' && <DotField ref={field} scanStart={scanStart} scanMs={SCAN_MS} out={scanT > 0.84} />}
          {mode === 'summary' ? (
            mapUp && <SummaryMap model={model} result={result} hot={hot} onHot={setHot} tipStatus={tipStatus} />
          ) : (
            <>
              <div className="va-toolbar">
                {props.hideCrumbs ? null : <nav className="va-crumbs" aria-label="Map location">
                  <button type="button" className={focusRegion ? '' : 'is-here'} onClick={() => setFocus(null)} disabled={!focusRegion}>
                    {data.disk.name}
                  </button>
                  {focusRegion && (
                    <>
                      <ChevronRight size={13} />
                      <span className="is-here">{focusRegion.name}</span>
                      <span className="va-crumb-blurb">{focusRegion.blurb}</span>
                    </>
                  )}
                  {mode === 'scan' && scanning && <span className="va-crumb-blurb">Mapping as it scans</span>}
                </nav>}
                {mode !== 'scan' && <Legend mode={mode} bytesInView={focusRegion?.bytes ?? model.totalBytes} w={size.w} h={size.h} />}
              </div>
              <div className={`va-stage ${mode === 'scan' && !scanning ? 'is-empty' : ''}`} ref={stageRef}>
                <MapView
                  model={model}
                  width={size.w}
                  height={size.h}
                  focus={focus}
                  look={look}
                  regionCls={regionCls}
                  visible={visible}
                  hot={hot}
                  onHot={setHot}
                  onLeaf={onLeaf}
                  onRegion={onRegion}
                  tipStatus={tipStatus}
                  clickable={clickable}
                  freeLabel={freeLabel}
                  interactive={mode !== 'scan'}
                />
                {mode === 'scan' && !scanning && (props.plate ? props.plate(startScan) : <ScanPlate data={data} onScan={startScan} />)}
                {mode === 'scan' && scanning && <ScanHud data={data} t={scanT} />}
                <AnimatePresence>
                  {drawer &&
                    (() => {
                      const r = data.reorgs.find((x) => x.id === drawer);
                      return r ? (
                        <ReorgDrawer key={r.id} r={r} agent={agent} on={reorgs.has(r.id)} onToggle={() => setReorgs(toggled(reorgs, r.id))} onClose={() => setDrawer(null)} />
                      ) : null;
                    })()}
                </AnimatePresence>
              </div>
            </>
          )}
        </main>

        {mode === 'plan' && (
          <PlanTray
            data={data}
            proposals={proposals}
            setProposals={setProposals}
            reorgs={reorgs}
            setReorgs={setReorgs}
            hot={hot}
            onHot={setHot}
            onOpenReorg={setDrawer}
            onConfirm={() => {
              setDrawer(null);
              setConfirm(true);
            }}
            onClose={() => go('overview')}
          />
        )}
        {mode === 'run' && <RunPanel result={result} pb={pb} onDone={() => go('summary')} />}
        {mapUp && (
          <SummaryPanel
            data={data}
            model={model}
            result={result}
            hot={hot}
            onHot={setHot}
            onScanAgain={scanAgain}
            undo={undo}
            onUndo={runUndo}
            onReplay={() => setPhase('replay')}
          />
        )}
      </div>

      {slidesUp && (
        <Wrapped
          data={data}
          model={model}
          result={result}
          proposals={proposals}
          undo={undo}
          onUndo={runUndo}
          leaving={phase === 'fly' || phase === 'skip' ? phase : null}
          entering={phase === 'replay'}
          onHandoff={handoff}
        />
      )}

      <AnimatePresence>
        {confirm && (
          <ConfirmDialog
            data={data}
            totals={totals}
            onCancel={() => setConfirm(false)}
            onGo={() => {
              setConfirm(false);
              go('run');
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}


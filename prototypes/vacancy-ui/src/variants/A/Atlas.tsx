// PROTOTYPE variant A "Atlas": map-first. The disk is a treemap you explore; the plan, the run and the summary
// all happen on (and next to) the same map.
import { AnimatePresence } from 'motion/react';
import { Check, ChevronRight, HardDrive, Lock, Plus, Signpost, Sparkles } from 'lucide-react';
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { buildRun } from '../../shared/run';
import type { VariantProps } from '../../shared/types';
import { RiskChip, SourceTag, Wordmark } from './bits';
import { Legend, type MapMode } from './Legend';
import { MapView, useSize, type TileLook } from './MapView';
import { buildModel, gb, toggled, type Leaf, type Rect, type Region } from './model';
import { ConfirmDialog, PlanTray, ReorgDrawer, planTotals } from './PlanTray';
import { RunPanel, usePlayback } from './RunPanel';
import { ScanHud, ScanPlate } from './Scan';
import { SummaryMap, SummaryPanel } from './Summary';
import './atlas.css';

const SCAN_MS = 4000;

export function Atlas(props: VariantProps) {
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
  const [stageRef, size] = useSize<HTMLDivElement>();
  const agent = data.scan.aiAgent;
  const pb = usePlayback(result, step === 'run');

  const mode: MapMode = step;

  useEffect(() => {
    if (step !== 'scan') {
      setScanning(false);
      setScanT(0);
    }
    if (step === 'scan' || step === 'run' || step === 'summary') setFocus(null);
    if (step !== 'plan') {
      setDrawer(null);
      setConfirm(false);
    }
  }, [step]);

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

  return (
    <div className={`va-root va-mode-${mode}`}>
      <header className="va-head">
        <Wordmark lit={lit} />
        <div className="va-head-disk">
          <HardDrive size={14} strokeWidth={1.8} />
          <span className="va-head-name">{data.disk.name}</span>
          <span className="va-meter va-meter--head" aria-hidden>
            <i style={{ width: `${(model.usedBytes / model.totalBytes) * 100}%` }} />
            {(mode === 'overview' || mode === 'plan') && (
              <b
                style={{
                  left: `${((model.usedBytes - model.reclaimableBytes) / model.totalBytes) * 100}%`,
                  width: `${(model.reclaimableBytes / model.totalBytes) * 100}%`,
                }}
              />
            )}
          </span>
          <span className="va-head-nums">
            {gb(model.usedBytes, 0)} of {gb(model.totalBytes, 0)} used
          </span>
        </div>
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
        <main className="va-main">
          {mode === 'summary' ? (
            <SummaryMap model={model} result={result} hot={hot} onHot={setHot} tipStatus={tipStatus} />
          ) : (
            <>
              <div className="va-toolbar">
                <nav className="va-crumbs" aria-label="Map location">
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
                </nav>
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
                {mode === 'scan' && !scanning && <ScanPlate data={data} onScan={() => setScanning(true)} />}
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
        {mode === 'summary' && <SummaryPanel data={data} model={model} result={result} hot={hot} onHot={setHot} onScanAgain={scanAgain} />}
      </div>

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


// PROTOTYPE variant B, "Concierge": one centered conversational feed that grows downward and ends in a printed receipt.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Check, Download, RotateCcw } from 'lucide-react';
import { defaultSelection, formatBytes, type Dataset } from '../../shared/data';
import { buildRun, type RunResult } from '../../shared/run';
import { STEPS, type Step, type VariantModule, type VariantProps } from '../../shared/types';
import { Say } from './Feed';
import { Header } from './Header';
import { Overview } from './Overview';
import { ConfirmCard, PlanSection } from './Plan';
import { Receipt, receiptText, type UndoState } from './Receipt';
import { RunRecap, RunSection, freedSoFar, stepProgress, type RunClock } from './RunSection';
import { ScanIntro, ScanRecap } from './ScanSection';
import { planTotals, playbackDurations, plural, type PlanTotals } from './util';
import './vb.css';

function useRunClock(active: boolean, result: RunResult): RunClock {
  const durations = useMemo(() => playbackDurations(result), [result]);
  const starts = useMemo(() => {
    let acc = 0;
    return durations.map((d) => {
      const s = acc;
      acc += d;
      return s;
    });
  }, [durations]);
  const total = durations.reduce((a, b) => a + b, 0);
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!active) {
      setElapsed(0);
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (now: number) => {
      const e = Math.min(total, now - t0);
      setElapsed(e);
      if (e < total) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [active, total, result]);
  return { starts, durations, total, elapsed };
}

function Concierge({ data, step, onStep, proposals, setProposals, reorgs, setReorgs }: VariantProps) {
  const si = STEPS.indexOf(step);
  const [scanning, setScanning] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [undo, setUndo] = useState<UndoState>({});
  const [saved, setSaved] = useState(false);
  const [when, setWhen] = useState(() => new Date());
  const feedRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (step !== 'scan') setScanning(false);
    if (step !== 'plan') setConfirming(false);
    if (step === 'summary') setWhen(new Date());
  }, [step]);

  const totals = useMemo(() => planTotals(data, proposals, reorgs), [data, proposals, reorgs]);
  const result = useMemo(() => buildRun(data, proposals, reorgs), [data, proposals, reorgs]);
  const clock = useRunClock(step === 'run', result);
  const runDone = step === 'run' && clock.elapsed >= clock.total;

  useEffect(() => {
    if (!runDone) return;
    const t = window.setTimeout(() => onStep('summary'), 2800);
    return () => window.clearTimeout(t);
  }, [runDone]); // eslint-disable-line react-hooks/exhaustive-deps

  const freed = step === 'run' ? freedSoFar(result, clock) : step === 'summary' ? result.freedBytes : 0;

  // Feed scrolling: jump to the newest section when the step changes, and keep live content in view.
  const scrollToAnchor = (name: string, smooth: boolean) => {
    const feed = feedRef.current;
    if (!feed) return;
    const el = feed.querySelector<HTMLElement>(`[data-vb-anchor="${name}"]`);
    const top = el && (name === 'plan' || name === 'summary' || name === 'confirm')
      ? el.getBoundingClientRect().top - feed.getBoundingClientRect().top + feed.scrollTop - (name === 'summary' ? 34 : 28)
      : 0;
    feed.scrollTo({ top: Math.max(0, top), behavior: smooth ? 'smooth' : 'auto' });
  };
  const ensureVisible = (el: HTMLElement) => {
    const feed = feedRef.current;
    if (!feed) return;
    const fr = feed.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    const limit = fr.bottom - (step === 'scan' ? 40 : 150);
    if (r.bottom > limit) feed.scrollTo({ top: feed.scrollTop + (r.bottom - limit), behavior: 'smooth' });
  };
  const mounted = useRef(false);
  useLayoutEffect(() => {
    scrollToAnchor(step, mounted.current);
    mounted.current = true;
  }, [step]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (confirming) window.setTimeout(() => scrollToAnchor('confirm', true), 30);
  }, [confirming]); // eslint-disable-line react-hooks/exhaustive-deps

  const restart = () => {
    const def = defaultSelection(data);
    setProposals(def.proposals);
    setReorgs(def.reorgs);
    setUndo({});
    setSaved(false);
    onStep('scan');
  };
  const doUndo = (id: string) => {
    setUndo((u) => ({ ...u, [id]: 'undoing' }));
    window.setTimeout(() => setUndo((u) => ({ ...u, [id]: 'undone' })), 1100);
  };
  const save = () => {
    const blob = new Blob([receiptText(data, result, when)], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `vacancy-receipt-${when.toISOString().slice(0, 10)}.txt`;
    a.click();
    window.setTimeout(() => URL.revokeObjectURL(a.href), 1000);
    setSaved(true);
  };

  const free0 = data.disk.totalBytes - data.disk.usedBytes;
  const free1 = result.totalBytes - result.usedAfter;

  return (
    <div className="vb-root">
      <Header data={data} freed={freed} />
      <div className="vb-feed" ref={feedRef}>
        <div className="vb-col">
          {si === 0 ? (
            <ScanIntro
              data={data}
              scanning={scanning}
              onScan={() => setScanning(true)}
              onDone={() => onStep('overview')}
              ensureVisible={ensureVisible}
            />
          ) : (
            <ScanRecap data={data} />
          )}

          {(si === 1 || si === 2) && (
            <Overview
              data={data}
              proposals={proposals}
              setProposals={setProposals}
              fresh={step === 'overview'}
              showNext={si === 1}
              onNext={() => onStep('plan')}
            />
          )}
          {si === 2 && <PlanSection data={data} reorgs={reorgs} setReorgs={setReorgs} fresh={step === 'plan'} />}
          {si === 2 && confirming && (
            <ConfirmCard data={data} totals={totals} onRun={() => onStep('run')} onCancel={() => setConfirming(false)} />
          )}

          {si >= 3 && <PlanRecap totals={totals} />}
          {si === 3 && (
            <RunSection data={data} result={result} clock={clock} onSeeReceipt={() => onStep('summary')} ensureVisible={ensureVisible} />
          )}
          {si === 4 && <RunRecap result={result} />}
          {si === 4 && (
            <section className="vb-section" data-vb-anchor="summary">
              <Say avatar animate>
                <h2 className="vb-display">You got {formatBytes(result.freedBytes)} back.</h2>
                <p className="vb-voice">
                  {data.disk.name} went from {formatBytes(free0)} free to {formatBytes(free1)} free. Here's your receipt.
                </p>
              </Say>
              <Receipt data={data} result={result} when={when} undo={undo} onUndo={doUndo} />
            </section>
          )}
        </div>
      </div>

      {si >= 1 && (
        <Composer
          data={data}
          step={step}
          totals={totals}
          confirming={confirming}
          onNext={() => onStep('plan')}
          onReview={() => setConfirming(true)}
          onEdit={() => setConfirming(false)}
          result={result}
          clock={clock}
          freed={freed}
          saved={saved}
          onSave={save}
          onRestart={restart}
        />
      )}
    </div>
  );
}

function PlanRecap({ totals }: { totals: PlanTotals }) {
  return (
    <div className="vb-recap">
      <Check size={14} strokeWidth={2.2} aria-hidden />
      <span>
        You approved {plural(totals.cleanups.length, 'cleanup')} ({formatBytes(totals.bytes)})
        {totals.reorgCount ? ` and ${plural(totals.reorgCount, 'reorganization')}` : ''}.
      </span>
    </div>
  );
}

function Composer(p: {
  data: Dataset;
  step: Step;
  totals: PlanTotals;
  confirming: boolean;
  onNext: () => void;
  onReview: () => void;
  onEdit: () => void;
  result: RunResult;
  clock: RunClock;
  freed: number;
  saved: boolean;
  onSave: () => void;
  onRestart: () => void;
}) {
  const { step, totals, result, clock } = p;
  let body: ReactNode;
  if (step === 'run') {
    const done = clock.elapsed >= clock.total;
    const active = result.steps.findIndex((_, i) => stepProgress(clock, i) < 1);
    body = (
      <>
        <div className="vb-comp-live" aria-live="off">
          <span className="vb-comp-livenum">{formatBytes(p.freed)}</span>
          <span className="vb-comp-livelabel">{done ? 'freed' : 'freed so far'}</span>
        </div>
        <div className="vb-comp-progress" aria-hidden>
          <i style={{ transform: `scaleX(${clock.total ? clock.elapsed / clock.total : 1})` }} />
        </div>
        <span className="vb-comp-step">{done ? 'Finished' : `Step ${active + 1} of ${result.steps.length}`}</span>
      </>
    );
  } else if (step === 'summary') {
    body = (
      <>
        <div className="vb-comp-stats vb-comp-done">
          <Check size={16} strokeWidth={2.4} aria-hidden />
          <span><b>{formatBytes(result.freedBytes)}</b> freed</span>
          <span className="vb-comp-sep" aria-hidden />
          <span><b>{formatBytes(result.totalBytes - result.usedAfter)}</b> free now</span>
        </div>
        <button type="button" className="vb-secondary" onClick={p.onRestart}>
          <RotateCcw size={14} strokeWidth={2} aria-hidden />Start again
        </button>
        <button type="button" className="vb-primary" onClick={p.onSave}>
          {p.saved ? <Check size={15} strokeWidth={2.4} aria-hidden /> : <Download size={15} strokeWidth={2} aria-hidden />}
          {p.saved ? 'Receipt saved' : 'Save receipt'}
        </button>
      </>
    );
  } else {
    const nothing = totals.bytes === 0 && totals.moves === 0;
    body = (
      <>
        <div className="vb-comp-stats">
          <span><b>{totals.cleanups.length}</b> {totals.cleanups.length === 1 ? 'cleanup' : 'cleanups'}</span>
          <span className="vb-comp-sep" aria-hidden />
          <span><b>{formatBytes(totals.bytes)}</b> to free</span>
          <span className="vb-comp-sep" aria-hidden />
          <span><b>{totals.reorgCount}</b> {totals.reorgCount === 1 ? 'reorganization' : 'reorganizations'}</span>
          {totals.hints.length > 0 && (
            <>
              <span className="vb-comp-sep" aria-hidden />
              <span><b>{totals.hints.length}</b> for you</span>
            </>
          )}
        </div>
        {step === 'overview' ? (
          <button type="button" className="vb-primary" onClick={p.onNext}>Next: your folders</button>
        ) : p.confirming ? (
          <button type="button" className="vb-secondary" onClick={p.onEdit}>Edit plan</button>
        ) : (
          <button type="button" className="vb-primary" onClick={p.onReview} disabled={nothing}>Review & run</button>
        )}
      </>
    );
  }
  return (
    <div className="vb-composer-wrap">
      <div className={`vb-composer vb-composer-${step}`}>{body}</div>
    </div>
  );
}

export const variant: VariantModule = {
  key: 'B',
  name: 'Concierge',
  frame: { bg: '#F3E6E0', fg: '#1D2742', border: '#E2D0C7' },
  Component: Concierge,
};

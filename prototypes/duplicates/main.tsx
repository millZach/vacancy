// THROWAWAY: three ways to show the duplicate finder in Atlas. All data is synthetic and in memory: no bridge calls,
// no file access. Real Shell, map, row controls, run panel, summary map and design tokens are imported.
import '../../src/design/fonts';
import '../../src/design/tokens.css';
import '../../src/design/base.css';
import '../../src/design/controls.css';
import '../../src/atlas/atlas.css';
import '../../src/plan/plan.css';
import '../../src/run/run.css';
import '../../src/scan/scan.css';
import '../../src/summary/summary.css';
import '../../src/wrapped/wrapped.css';
import './prototype.css';
import { StrictMode, useCallback, useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowLeft, ArrowRight, HardDrive, RotateCcw } from 'lucide-react';
import type { Platform } from '../../src/bridge';
import { formatBytes } from '../../src/format';
import { overviewModel } from '../../src/map/model';
import { RunPanel } from '../../src/run/RunPanel';
import { Shell } from '../../src/shell/Shell';
import { SummaryMap } from '../../src/summary/Summary';
import { data, freedOf } from './data';
import { DupConfirm, DupMap, RecapPanel, splitDups, totalsOf, useSimRun, type Ctx } from './shared';
import { VariantA, VariantB, VariantC } from './variants';

const names = { A: 'Plan row', B: 'Map links', C: 'Compare sheet' };
type Variant = keyof typeof names;
type Stage = 'plan' | 'confirm' | 'run' | 'recap';

const params = new URLSearchParams(location.search);
const HASH_MS = Number(params.get('hash') ?? 6000);
const RUN_MS = Number(params.get('run') ?? 4500);

function Prototype() {
  const initial = params.get('variant');
  const [variant, setVariant] = useState<Variant>(initial === 'B' || initial === 'C' ? initial : 'A');
  const [platform, setPlatform] = useState<Platform>(params.get('platform') === 'macos' ? 'macos' : 'windows');
  const d = data[platform];
  const [started, setStarted] = useState(() => performance.now());
  const [t, setT] = useState(0);
  const [on, setOn] = useState<string[]>(() => d.proposals.filter((p) => p.defaultOn).map((p) => p.id));
  const [dupsOn, setDupsOn] = useState(false);
  const [excluded, setExcluded] = useState<string[]>([]);
  const [stage, setStage] = useState<Stage>('plan');
  const [hot, setHot] = useState<string | null>(null);

  // Hashing runs in the background from the moment the scan finished.
  useEffect(() => {
    let timer = 0;
    const tick = () => {
      const v = HASH_MS <= 0 ? 1 : Math.min(1, (performance.now() - started) / HASH_MS);
      setT(v);
      if (v < 1) timer = window.setTimeout(tick, 100);
    };
    tick();
    return () => clearTimeout(timer);
  }, [started]);

  const restart = useCallback((next: Platform = platform) => {
    setStarted(performance.now());
    setT(0);
    setOn(data[next].proposals.filter((p) => p.defaultOn).map((p) => p.id));
    setDupsOn(false);
    setExcluded([]);
    setStage('plan');
    setHot(null);
  }, [platform]);

  const totals = useMemo(() => totalsOf(d, on, dupsOn, excluded), [d, on, dupsOn, excluded]);
  const c: Ctx = useMemo(() => ({
    platform, d, hash: { t, done: t >= 1 }, on,
    toggle: (id) => setOn((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id])),
    dupsOn, setDupsOn, excluded,
    toggleGroup: (id) => setExcluded((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id])),
    totals, onFree: () => setStage('confirm'), hot, setHot,
  }), [platform, d, t, on, dupsOn, excluded, totals, hot]);

  const sim = useSimRun(c, stage === 'run' || stage === 'recap', RUN_MS);

  function changeVariant(next: Variant) {
    setVariant(next);
    const url = new URL(location.href);
    url.searchParams.set('variant', next);
    history.replaceState(null, '', url);
  }
  function changePlatform(next: Platform) {
    setPlatform(next);
    restart(next);
    const url = new URL(location.href);
    url.searchParams.set('platform', next);
    history.replaceState(null, '', url);
  }
  function cycle(delta: number) {
    const keys: Variant[] = ['A', 'B', 'C'];
    changeVariant(keys[(keys.indexOf(variant) + delta + 3) % 3]!);
  }
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      // The compare sheet and dialogs own the arrow keys while they are open.
      if ((e.target as HTMLElement).closest('input,textarea,[contenteditable]') || document.querySelector('.dupC-sheet, .scrim')) return;
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        cycle(e.key === 'ArrowLeft' ? -1 : 1);
      }
    };
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  });

  const ov = d.overviewAfter;
  const reclaimable = d.proposals.reduce((s, p) => s + p.bytes, 0) + (c.hash.done ? d.groups.reduce((s, g) => s + freedOf(g), 0) : 0);
  const recapModel = useMemo(() => overviewModel(ov), [ov]);
  const report = sim?.report ?? null;

  let body;
  if (stage === 'recap' && report) {
    body = (
      <div className="finish is-rest">
        <main className="atlas is-map finish-map">
          <SummaryMap model={recapModel} report={{ ...report, steps: splitDups(c, report.steps) }} hot={hot} onHot={setHot} />
        </main>
        <RecapPanel c={c} report={report} onRestart={() => restart()} />
      </div>
    );
  } else if (stage === 'run' && sim) {
    body = (
      <>
        <DupMap c={c} run={sim.run} />
        <RunPanel run={sim.run} hot={hot} onHot={setHot} onStop={() => undefined} onSeeChanges={() => setStage('recap')} onBack={() => setStage('plan')} onScanAgain={() => restart()} />
      </>
    );
  } else {
    body = variant === 'A' ? <VariantA key={platform} c={c} /> : variant === 'B' ? <VariantB key={platform} c={c} /> : <VariantC key={platform} c={c} />;
  }

  const readout =
    stage === 'recap' && report
      ? `Recap · ${formatBytes(report.freedBytes)} freed`
      : stage === 'run'
        ? sim?.run.phase === 'ended' ? 'Run done' : `Running · ${formatBytes(sim?.run.view.freedBytes ?? 0)}`
        : !c.hash.done
          ? `Hashing ${Math.round(t * 100)}%`
          : `${stage === 'confirm' ? 'Confirm' : 'Ready'} · ${dupsOn ? `${totals.dupGroups} of ${d.groups.length} groups in plan` : `${d.groups.length} groups, off`}`;

  return (
    <>
      <Shell
        platform={platform}
        drive={
          <div className="head-disk">
            <HardDrive size={15} aria-hidden />
            <span className="head-disk-name">{ov.volume.name}</span>
            <span className="head-disk-nums">{formatBytes(ov.volume.usedBytes, 0)} of {formatBytes(ov.volume.totalBytes, 0)} used</span>
          </div>
        }
        advisor={null}
        reclaimable={stage === 'recap' ? null : reclaimable}
        lit={stage === 'recap'}
      >
        {body}
      </Shell>
      {stage === 'confirm' && <DupConfirm c={c} onCancel={() => setStage('plan')} onGo={() => setStage('run')} />}
      {(import.meta.env.DEV || params.get('review') === '1') && (
        <div className="prototype-switcher" data-no-ripple>
          <span className="prototype-label">THROWAWAY</span>
          <button type="button" aria-label="Previous variant" onClick={() => cycle(-1)}><ArrowLeft size={14} /></button>
          <span className="variant-label">{variant} · {names[variant]}{variant === 'A' && ' · Recommended'}</span>
          <button type="button" aria-label="Next variant" onClick={() => cycle(1)}><ArrowRight size={14} /></button>
          <span className="switcher-divider" />
          <button type="button" className={platform === 'windows' ? 'on' : ''} onClick={() => changePlatform('windows')}>Windows</button>
          <button type="button" className={platform === 'macos' ? 'on' : ''} onClick={() => changePlatform('macos')}>macOS</button>
          <span className="switcher-divider" />
          <button type="button" aria-label="Start over" title="Start over: hash again" onClick={() => restart()}><RotateCcw size={13} /></button>
          <span className="state-label" role="status">{readout}</span>
        </div>
      )}
    </>
  );
}

createRoot(document.getElementById('root')!).render(<StrictMode><Prototype /></StrictMode>);

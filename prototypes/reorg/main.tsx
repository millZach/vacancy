// THROWAWAY: three placements for folder reorganization (built-in sorting, Windows only) in the Atlas flow.
import '../../src/design/fonts';
import '../../src/design/tokens.css';
import '../../src/design/base.css';
import '../../src/design/controls.css';
import '../../src/atlas/atlas.css';
import '../../src/plan/plan.css';
import '../../src/run/run.css';
import '../../src/summary/summary.css';
import '../../src/history/history.css';
import '../../src/wrapped/wrapped.css';
import './prototype.css';
import { StrictMode, useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowLeft, ArrowRight, HardDrive } from 'lucide-react';
import type { Platform } from '../../src/bridge';
import { fixtures } from '../../src/bridge/fixtures';
import { formatBytes, formatCount } from '../../src/format';
import { PlanTray } from '../../src/plan/PlanTray';
import { ConfirmDialog } from '../../src/run/ConfirmDialog';
import { RunPanel } from '../../src/run/RunPanel';
import { Shell } from '../../src/shell/Shell';
import { Finish } from '../../src/wrapped/Finish';
import {
  buildPlan, buildReport, buildRun, MovedSlideUndo, NoBridge, PastRuns, pickedReorgs, reclaimableOf, totals, undoFailed,
  type Flow, type FlowApi,
} from './common';
import { RecapA, RecapB, RecapC, runLook, SpaceMap, VariantA, VariantB, VariantC, planLook } from './variants';

const names = { A: 'Tidy up in the tray', B: 'Tidy mode on the map', C: 'Review sheet' };
type Variant = keyof typeof names;
const RECOMMENDED: Variant = 'A';

function fresh(platform: Platform): Flow {
  return {
    platform,
    stage: 'plan',
    deletes: fixtures[platform].proposals.filter((p) => p.action === 'delete' && p.defaultOn).map((p) => p.id),
    picked: [],
    excluded: [],
    confirming: false,
    running: false,
    undo: {},
    ranAt: null,
  };
}

const toggled = (list: readonly string[], id: string) => (list.includes(id) ? list.filter((x) => x !== id) : [...list, id]);

function Prototype() {
  const params = new URLSearchParams(location.search);
  const initial = params.get('variant');
  const [variant, setVariant] = useState<Variant>(initial === 'B' || initial === 'C' ? initial : 'A');
  const [flow, setFlow] = useState<Flow>(() => fresh(params.get('platform') === 'macos' ? 'macos' : 'windows'));
  const [hot, setHot] = useState<string | null>(null);
  const timers = useRef<number[]>([]);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  const later = (ms: number, fn: () => void) => { timers.current.push(window.setTimeout(fn, ms)); };
  const platform = flow.platform;
  const proposals = fixtures[platform].proposals;

  const api: FlowApi = {
    flow,
    proposals,
    set: (patch) => setFlow((f) => ({ ...f, ...patch })),
    toggleDelete: (id) => setFlow((f) => ({ ...f, deletes: toggled(f.deletes, id) })),
    togglePick: (id) => setFlow((f) => ({ ...f, picked: toggled(f.picked, id) })),
    toggleDest: (id) => setFlow((f) => ({ ...f, excluded: toggled(f.excluded, id) })),
    review: () => setFlow((f) => ({ ...f, confirming: true })),
    undo: (id) => {
      setFlow((f) => ({ ...f, undo: { ...f.undo, [id]: 'working' } }));
      later(900, () => setFlow((f) => ({ ...f, undo: { ...f.undo, [id]: 'done' } })));
    },
    reset: () => setFlow((f) => ({ ...fresh(f.platform), deletes: f.deletes, picked: f.picked, excluded: f.excluded })),
  };

  const go = () => {
    setFlow((f) => ({ ...f, confirming: false, stage: 'run', running: true, ranAt: Date.now(), undo: {} }));
    later(1100, () => setFlow((f) => ({ ...f, running: false })));
  };

  function changeVariant(next: Variant) {
    setVariant(next);
    const url = new URL(location.href);
    url.searchParams.set('variant', next);
    history.replaceState(null, '', url);
  }
  function changePlatform(next: Platform) {
    setFlow(fresh(next));
    setHot(null);
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
      const t = e.target as HTMLElement;
      if (t.closest('input,textarea,[contenteditable],[role="slider"]') || document.querySelector('.wrap, .scrim')) return;
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault();
        cycle(e.key === 'ArrowLeft' ? -1 : 1);
      }
    };
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  });

  const switcher = import.meta.env.DEV || params.get('review') === '1';
  useEffect(() => { document.body.classList.toggle('has-switcher', switcher); }, [switcher]);
  const t = totals(flow, proposals);
  const plan = useMemo(() => buildPlan(flow, proposals), [flow, proposals]);
  const props = { api, hot, onHot: setHot };
  const toRecap = () => setFlow((f) => ({ ...f, stage: 'recap' }));
  const replay = () => setFlow((f) => ({ ...f, stage: 'slide' }));

  let body;
  if (platform === 'macos') {
    body = <MacFlow api={api} hot={hot} onHot={setHot} />;
  } else if (flow.stage === 'plan') {
    body = variant === 'A' ? <VariantA {...props} /> : variant === 'B' ? <VariantB {...props} /> : <VariantC {...props} />;
  } else if (flow.stage === 'run') {
    body = (
      <>
        <SpaceMap platform={platform} hot={hot} onHot={setHot} look={flow.running ? planLook(api) : runLook(api)} />
        <RunPanel run={buildRun(flow, proposals, flow.running)} hot={hot} onHot={setHot} onStop={() => {}} onBack={api.reset}
          onScanAgain={api.reset} onSeeChanges={() => setFlow((f) => ({ ...f, stage: pickedReorgs(f).length ? 'slide' : 'recap' }))} />
      </>
    );
  } else {
    const Recap = variant === 'A' ? RecapA : variant === 'B' ? RecapB : RecapC;
    body = (
      <>
        <Recap {...props} onReplay={replay} />
        {flow.stage === 'slide' && <MovedSlideUndo api={api} onNext={toRecap} />}
      </>
    );
  }

  const reorgs = pickedReorgs(flow);
  const undone = reorgs.filter((r) => flow.undo[r.id] === 'done');
  const stuck = undone.reduce((n, r) => n + undoFailed(r, flow.excluded).count, 0);
  const stage = { plan: flow.confirming ? 'Confirm' : 'Plan', run: flow.running ? 'Running' : 'Run done', slide: 'Moved slide', recap: 'Recap' }[flow.stage];
  const state = platform === 'macos'
    ? `${stage} · no reorganization on macOS · ${formatBytes(t.bytes)}`
    : `${stage} · ${reorgs.length} ${reorgs.length === 1 ? 'folder' : 'folders'} · ${formatCount(t.moves)} moves · ${formatBytes(t.bytes)} freed${undone.length ? ` · ${undone.length} undone${stuck ? `, ${stuck} stayed` : ''}` : ''}`;

  return (
    <NoBridge platform={platform}>
      <Shell
        platform={platform}
        drive={<div className="head-disk"><HardDrive size={15} aria-hidden /><span className="head-disk-name">{fixtures[platform].overview.volume.name}</span><span className="head-disk-nums">Synthetic drive</span></div>}
        advisor={null}
        reclaimable={flow.stage === 'plan' ? reclaimableOf(platform) : null}
        history={<PastRuns api={api} onOpen={() => setFlow((f) => ({ ...f, stage: 'recap' }))} />}
        lit={flow.stage !== 'plan' && !flow.running}
      >
        {body}
      </Shell>
      {flow.confirming && <ConfirmDialog plan={plan} platform={platform} onCancel={() => api.set({ confirming: false })} onGo={go} />}
      {switcher && (
        <div className="prototype-switcher">
          <span className="prototype-label">THROWAWAY</span>
          <button aria-label="Previous variant" onClick={() => cycle(-1)}><ArrowLeft size={14} /></button>
          <span className="variant-label">{variant} · {names[variant]}{variant === RECOMMENDED && ' · Recommended'}</span>
          <button aria-label="Next variant" onClick={() => cycle(1)}><ArrowRight size={14} /></button>
          <span className="switcher-divider" />
          <button className={platform === 'windows' ? 'on' : ''} onClick={() => changePlatform('windows')}>Windows</button>
          <button className={platform === 'macos' ? 'on' : ''} onClick={() => changePlatform('macos')}>macOS</button>
          <span className="state-label">{state}</span>
        </div>
      )}
    </NoBridge>
  );
}

/** macOS: no reorganization at all. The app's own tray, confirm, run panel and finish, exactly as today. */
function MacFlow({ api, hot, onHot }: { api: FlowApi; hot: string | null; onHot: (id: string | null) => void }) {
  const { flow } = api;
  if (flow.stage === 'plan')
    return (
      <>
        <SpaceMap platform="macos" hot={hot} onHot={onHot} look={planLook(api)} />
        <PlanTray mountPoint="/" proposals={api.proposals} plan={flow.deletes} agent={null} hot={hot} onHot={onHot}
          onToggle={api.toggleDelete} onClose={() => {}} onFree={api.review} checking={false} error={null} />
      </>
    );
  if (flow.stage === 'run')
    return (
      <>
        <SpaceMap platform="macos" hot={hot} onHot={onHot} look={flow.running ? planLook(api) : runLook(api)} />
        <RunPanel run={buildRun(flow, api.proposals, flow.running)} hot={hot} onHot={onHot} onStop={() => {}} onBack={api.reset}
          onScanAgain={api.reset} onSeeChanges={() => api.set({ stage: 'recap' })} />
      </>
    );
  return <Finish report={buildReport(flow, api.proposals)} start="rest" onScanAgain={api.reset} />;
}

createRoot(document.getElementById('root')!).render(<StrictMode><Prototype /></StrictMode>);

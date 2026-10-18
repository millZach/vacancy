// THROWAWAY: three ways to offer "Fast scan (asks for admin)" on Windows' scan screen, switchable with ?variant=A|B|C.
// macOS (?platform=macos) shows today's scan screen, HUD and map, unchanged.
import '../../src/design/fonts';
import '../../src/design/tokens.css';
import '../../src/design/base.css';
import '../../src/design/controls.css';
import '../../src/atlas/atlas.css';
import '../../src/scan/scan.css';
import '../../src/plan/plan.css';
import './prototype.css';
import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import type { Platform } from '../../src/bridge';
import { AdvisorTrigger } from '../../src/advisor/Advisor';
import { DriveMenu } from '../../src/drives/DriveMenu';
import { reclaimableOf } from '../../src/drives/session';
import { formatBytes } from '../../src/format';
import { ScanHud } from '../../src/scan/ScanHud';
import { ScanPlate } from '../../src/scan/ScanPlate';
import { Shell } from '../../src/shell/Shell';
import { driveScanOf, freshFlow, MapScreen, NoBridge, readyDrives, ScanScreen, UacPrompt, volumesOf, type Flow, type FlowApi } from './common';
import { DURATION_MS, FILE_SYSTEM, isNtfs, lockedTotal, updateAt } from './data';
import { Advisor, VariantA, VariantB, VariantC } from './variants';

const names = { A: 'Second button on the plate', B: 'Scan mode in the drive picker', C: 'Switch from the scan HUD' };
type Variant = keyof typeof names;
const RECOMMENDED: Variant = 'A';
const TICK = 100;

/** For the proof: freeze a scan at a point, or let it run. */
const proto = ((window as unknown as { __proto: { hold: number | null } }).__proto = { hold: null as number | null });

function Prototype() {
  const params = new URLSearchParams(location.search);
  const initial = params.get('variant');
  const [variant, setVariant] = useState<Variant>(initial === 'B' || initial === 'C' ? initial : 'A');
  const [flow, setFlow] = useState<Flow>(() => freshFlow(params.get('platform') === 'macos' ? 'macos' : 'windows'));
  const platform = flow.platform;
  const volumes = volumesOf(platform);
  const volume = volumes.find((v) => v.mountPoint === flow.drive)!;

  // The scan runs in prototype time. A switch prompt (C) never lets the normal scan finish under it.
  useEffect(() => {
    if (flow.screen !== 'scanning') return;
    const id = setInterval(() => {
      setFlow((f) => {
        if (f.screen !== 'scanning') return f;
        if (proto.hold !== null) return f.progress === proto.hold ? f : { ...f, progress: proto.hold };
        const progress = f.progress + TICK / DURATION_MS[f.mode];
        if (progress < 1) return { ...f, progress };
        if (f.uac) return { ...f, progress: 0.97 };
        return { ...f, screen: 'map', progress: 1, done: { ...f.done, [f.drive]: { mode: f.mode, note: f.note, at: Date.now() } } };
      });
    }, TICK);
    return () => clearInterval(id);
  }, [flow.screen]);

  const start = { screen: 'scanning' as const, progress: 0, offer: 'open' as const, uac: null };
  const api: FlowApi = {
    flow, volumes, volume,
    set: (patch) => setFlow((f) => ({ ...f, ...patch })),
    pick: (drive) => setFlow((f) => ({ ...f, drive })),
    scan: (mode) => setFlow((f) => {
      if (mode === 'fast' && f.platform === 'windows') {
        if (!isNtfs(f.drive)) return { ...f, ...start, mode: 'normal', note: 'notNtfs' };
        return { ...f, uac: 'start' };
      }
      return { ...f, ...start, mode: 'normal', note: null };
    }),
    uacYes: () => setFlow((f) => (f.uac === 'switch'
      ? { ...f, uac: null, mode: 'fast', progress: 0, offer: 'switched', note: null }
      : { ...f, ...start, mode: 'fast', note: null })),
    uacNo: () => setFlow((f) => (f.uac === 'switch'
      ? { ...f, uac: null, offer: 'declined', note: 'declined' }
      : { ...f, ...start, mode: 'normal', note: 'declined' })),
    cancel: () => setFlow((f) => ({ ...f, screen: 'plate', progress: 0, uac: null, note: null, choice: 'normal' })),
    switchToFast: () => setFlow((f) => ({ ...f, uac: 'switch' })),
    openMap: () => setFlow((f) => ({ ...f, screen: 'map' })),
    scanAgain: () => setFlow((f) => ({ ...f, screen: 'plate', note: null, choice: 'normal', progress: 0 })),
  };

  function changeVariant(next: Variant) {
    setVariant(next);
    const url = new URL(location.href);
    url.searchParams.set('variant', next);
    history.replaceState(null, '', url);
  }
  function changePlatform(next: Platform) {
    setFlow(freshFlow(next));
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
      if (t.closest('input,textarea,[contenteditable],[role="radio"],[role="menu"],.uac-desktop')) return;
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
  const drives = readyDrives(api);

  let body;
  if (flow.screen === 'map') body = <MapScreen key={flow.drive} api={api} />;
  else if (platform === 'macos') body = <MacScan api={api} />;
  else body = variant === 'A' ? <VariantA api={api} /> : variant === 'B' ? <VariantB api={api} /> : <VariantC api={api} />;

  const pct = Math.round(Math.min(0.98, flow.progress) * 100);
  const why = flow.note === 'declined' ? ' · fell back: admin declined' : flow.note === 'notNtfs' ? ` · fell back: ${FILE_SYSTEM[flow.drive]}` : '';
  const done = flow.done[flow.drive];
  const state = flow.uac
    ? `Windows prompt (stand-in) · ${flow.uac === 'switch' ? 'the normal scan keeps going behind it' : 'nothing is scanning yet'}`
    : flow.screen === 'plate'
      ? `Scan screen · ${volume.name}${platform === 'windows' ? ` (${FILE_SYSTEM[flow.drive]})` : ''}${variant === 'B' && platform === 'windows' ? ` · mode: ${flow.choice}` : ''}`
      : flow.screen === 'scanning'
        ? `${flow.mode === 'fast' ? 'Fast' : 'Normal'} scan ${pct}%${why}`
        : `Map · ${done?.mode === 'fast' ? `fast scan · Locked folders ${formatBytes(lockedTotal(volume))}, totals only` : 'normal scan'}${done?.note === 'declined' ? ' · admin declined' : done?.note === 'notNtfs' ? ` · ${FILE_SYSTEM[flow.drive]}` : ''}`;

  return (
    <NoBridge platform={platform}>
      <Shell
        platform={platform}
        drive={<DriveMenu drives={drives} />}
        advisor={<AdvisorTrigger open={false} onOpen={() => {}} />}
        reclaimable={flow.screen === 'map' ? reclaimableOf(driveScanOf(api, volume)) : null}
      >
        {body}
      </Shell>
      {flow.uac && <UacPrompt api={api} />}
      {switcher && (
        <div className="prototype-switcher">
          <span className="prototype-label">THROWAWAY</span>
          <button aria-label="Previous variant" onClick={() => cycle(-1)}><ArrowLeft size={14} /></button>
          <span className="variant-label">{variant} · {names[variant]}{variant === RECOMMENDED && ' · Recommended'}</span>
          <button aria-label="Next variant" onClick={() => cycle(1)}><ArrowRight size={14} /></button>
          <span className="switcher-divider" />
          <button className={platform === 'windows' ? 'on' : ''} onClick={() => changePlatform('windows')}>Windows</button>
          <button className={platform === 'macos' ? 'on' : ''} onClick={() => changePlatform('macos')}>macOS</button>
          <span className="switcher-divider" />
          {flow.screen === 'scanning' && !flow.uac && <button onClick={() => { proto.hold = null; setFlow((f) => ({ ...f, progress: 0.999 })); }}>Finish scan</button>}
          <button onClick={() => { proto.hold = null; setFlow(freshFlow(platform)); }}>Start over</button>
          <span className="state-label">{state}</span>
        </div>
      )}
    </NoBridge>
  );
}

/** macOS: today's plate and HUD, exactly. There is no fast scan on a Mac. */
function MacScan({ api }: { api: FlowApi }) {
  const drives = readyDrives(api);
  const { update } = updateAt(api.volume, 'normal', api.flow.progress);
  return (
    <ScanScreen
      api={api}
      plate={({ calmRef, onScan }) => (
        <ScanPlate drives={drives} onOpenFullDiskAccess={() => {}} advisorNote={<Advisor api={api} />} failure={null}
          onScan={(b) => onScan(b, 'normal')} contentRef={calmRef} />
      )}
      hud={<ScanHud update={update} progress={Math.min(0.98, api.flow.progress)} onCancel={api.cancel} />}
    />
  );
}

createRoot(document.getElementById('root')!).render(<StrictMode><Prototype /></StrictMode>);

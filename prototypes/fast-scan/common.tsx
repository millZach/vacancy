// THROWAWAY: pieces every variant shares. The real app components (Shell, DriveMenu, ScanPlate, ScanHud, DotField,
// MapView and its model, Legend) are imported; only the fast-scan parts are drawn here, with the app's classes.
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import { Check, ChevronRight, Info, Lock, RotateCcw, ShieldCheck, Zap } from 'lucide-react';
import type { Bridge, MapFolder, Platform, VolumeInfo } from '../../src/bridge';
import { BridgeProvider } from '../../src/bridge/BridgeProvider';
import { fixtures } from '../../src/bridge/fixtures';
import { DotField, type DotFieldHandle } from '../../src/dotfield/DotField';
import { DriveIcon, DriveStatus, DriveTags, UsageBar } from '../../src/drives/bits';
import type { DriveItem, ReadyDrives } from '../../src/drives/context';
import { reclaimableOf, type DriveScan } from '../../src/drives/session';
import { useNow } from '../../src/drives/useNow';
import { formatBytes, formatCount } from '../../src/format';
import { Legend } from '../../src/map/Legend';
import { MapView, type TileLook } from '../../src/map/MapView';
import { FREE_ID, folderModel, overviewModel, scanModel, type AtlasModel, type Leaf, type Region } from '../../src/map/model';
import { useSize } from '../../src/map/useSize';
import { RiskChip, SourceTag } from '../../src/plan/bits';
import { ESTIMATE, folderFor, isNtfs, FILE_SYSTEM, lockedFolders, lockedOf, lockedTotal, overviewFor, proposalsFor, updateAt } from './data';

// ---------- no bridge ----------

/** Every call is counted and refused. Only `platform` is read; the proof fails on any call. */
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

// ---------- flow state ----------

export type Mode = 'normal' | 'fast';
/** Why a fast scan that was asked for runs as a normal one. */
export type Note = 'declined' | 'notNtfs' | null;
export interface Done { mode: Mode; note: Note; at: number }

export interface Flow {
  platform: Platform;
  /** The drive on screen. */
  drive: string;
  screen: 'plate' | 'scanning' | 'map';
  /** The running scan's mode. */
  mode: Mode;
  note: Note;
  /** 0 to 1. */
  progress: number;
  /** Windows' prompt is up: before a fast scan starts, or (C) over a normal scan to switch it. */
  uac: null | 'start' | 'switch';
  /** B: the picker's mode. Forgotten every time the scan screen comes back. */
  choice: Mode;
  /** C: the HUD's offer. */
  offer: 'open' | 'declined' | 'switched';
  done: Record<string, Done>;
}

export interface FlowApi {
  flow: Flow;
  volumes: VolumeInfo[];
  volume: VolumeInfo;
  set: (patch: Partial<Flow>) => void;
  pick: (mountPoint: string) => void;
  /** Scan the drive on screen. Fast on a drive with no file index runs normally, with a note; no prompt. */
  scan: (mode: Mode) => void;
  uacYes: () => void;
  uacNo: () => void;
  cancel: () => void;
  /** C: from the normal scan running, ask for admin and start over fast. */
  switchToFast: () => void;
  openMap: () => void;
  scanAgain: () => void;
}

export const volumesOf = (platform: Platform): VolumeInfo[] => fixtures[platform].volumes;

export function freshFlow(platform: Platform): Flow {
  return {
    platform, drive: volumesOf(platform)[0]!.mountPoint, screen: 'plate', mode: 'normal', note: null, progress: 0,
    uac: null, choice: 'normal', offer: 'open', done: {},
  };
}

export const noteText = (note: Note, volume: VolumeInfo): ReactNode =>
  note === 'declined' ? (
    <><b>Running the normal scan.</b> Windows wasn’t given admin, so Vacancy can’t read the file index. This takes {ESTIMATE[volume.mountPoint]?.normal ?? 'a few minutes'}.</>
  ) : note === 'notNtfs' ? (
    <><b>Running the normal scan.</b> {volume.name} is {FILE_SYSTEM[volume.mountPoint]}, and fast scan only reads NTFS drives. No admin was asked for.</>
  ) : null;

/** The one line every fast-scan offer says about admin. */
export const ADMIN_LINE = 'Windows asks for admin each time, and Vacancy uses it only for that read. Deleting still happens normally, with the same checks.';

// ---------- the drives, as the real DriveMenu and ScanPlate want them ----------

export function driveScanOf(api: FlowApi, v: VolumeInfo): DriveScan {
  const { flow } = api;
  if (flow.drive === v.mountPoint && flow.screen === 'scanning') {
    const { update, preview } = updateAt(v, flow.mode, flow.progress);
    return { phase: 'scanning', run: 1, update, preview };
  }
  const done = flow.done[v.mountPoint];
  if (!done) return { phase: 'idle' };
  const proposals = proposalsFor(v);
  return { phase: 'done', overview: overviewFor(v, done.mode), proposals, plan: proposals.filter((p) => p.defaultOn).map((p) => p.id), at: done.at, planRun: null };
}

export function readyDrives(api: FlowApi): ReadyDrives {
  const list: DriveItem[] = api.volumes.map((v) => ({ volume: v, scan: driveScanOf(api, v), current: v.mountPoint === api.flow.drive }));
  const noop = () => {};
  return {
    status: 'ready',
    machineName: fixtures[api.flow.platform].machineName,
    fullDiskAccess: api.flow.platform === 'macos' ? 'granted' : 'notApplicable',
    list,
    current: list.find((d) => d.current)!,
    screen: api.flow.screen === 'map' ? 'map' : 'scan',
    locked: false,
    gone: null,
    pick: api.pick,
    switchTo: (mp) => { api.pick(mp); api.set({ screen: api.flow.done[mp] ? 'map' : 'plate' }); },
    openMap: api.openMap,
    showScanScreen: api.scanAgain,
    scan: () => api.scan('normal'),
    cancelScan: api.cancel,
    setInPlan: noop, setRunning: noop, runPlan: noop, cancelRun: noop, showSummary: noop, summarySeen: noop, leaveRun: noop, dismissGone: noop,
  };
}

// ---------- the scan screen: the dot field, the plate, then the HUD over the map filling in ----------

export interface PlateProps {
  api: FlowApi;
  /** The plate's content, which the dot field keeps calm around. */
  calmRef: (el: HTMLDivElement | null) => void;
  /** Starts a scan from a button, with the field's splash where it was pressed. */
  onScan: (button: HTMLButtonElement, mode: Mode) => void;
}

/** The scan screen (Atlas before its map): the plate on the living dot field, then the map filling in under a HUD. */
export function ScanScreen({ api, plate, hud }: { api: FlowApi; plate: (p: PlateProps) => ReactNode; hud: ReactNode }) {
  const field = useRef<DotFieldHandle>(null);
  const [calm, setCalm] = useState<HTMLDivElement | null>(null);
  const [stageRef, size] = useSize<HTMLDivElement>();
  const { flow, volume } = api;
  const scanning = flow.screen === 'scanning';
  const model = useMemo(() => {
    if (!scanning) return null;
    const { update, preview } = updateAt(volume, flow.mode, flow.progress);
    return scanModel(volume, preview, update.bytes);
  }, [scanning, volume, flow.mode, flow.progress]);
  const onScan = (button: HTMLButtonElement, mode: Mode) => {
    const b = button.getBoundingClientRect();
    field.current?.drop(b.left + b.width / 2, b.top + b.height / 2, 'splash');
    api.scan(mode);
  };
  return (
    <main className="atlas is-scan">
      <DotField ref={field} calm={scanning ? null : calm} scan={scanning ? flow.progress : null} out={flow.progress > 0.85} />
      <div className="atlas-toolbar">
        <nav className="crumbs" aria-label="Map location">
          <span className="crumb is-here" aria-current="location">{volume.name}</span>
          {scanning && <span className="crumb-blurb">{flow.mode === 'fast' ? 'Mapping from the file index' : 'Mapping as it scans'}</span>}
        </nav>
      </div>
      <div className={scanning ? 'atlas-stage' : 'atlas-stage is-empty'} ref={stageRef}>
        {model && <MapView model={model} width={size.w} height={size.h} focus={null} interactive={false} hot={null} onHot={() => {}} />}
        {!scanning && plate({ api, calmRef: setCalm, onScan })}
        {scanning && hud}
      </div>
    </main>
  );
}

/** The drive cards, as the real plate draws them, with room for a variant's line at the foot of each. */
export function DriveCards({ api, foot }: { api: FlowApi; foot?: (v: VolumeInfo, on: boolean) => ReactNode }) {
  const now = useNow();
  const ids = useId();
  return (
    <div className="drive-cards" role="radiogroup" aria-label="Drives" data-no-ripple>
      {api.volumes.map((v, i) => {
        const on = v.mountPoint === api.flow.drive;
        const scan = driveScanOf(api, v);
        return (
          <button key={v.mountPoint} type="button" role="radio" aria-checked={on} aria-label={v.name}
            aria-describedby={`${ids}-${i}-tags ${ids}-${i}-nums ${ids}-${i}-status`} tabIndex={on ? 0 : -1}
            className={on ? 'drive-card is-on' : 'drive-card'} onClick={() => api.pick(v.mountPoint)}>
            <span className="drive-card-top">
              <DriveIcon kind={v.kind} size={20} className="drive-card-icon" />
              <DriveTags volume={v} id={`${ids}-${i}-tags`} />
              {on && <span className="drive-card-tick" aria-hidden><Check size={11} strokeWidth={3} /></span>}
            </span>
            <span className="drive-card-name">{v.name}</span>
            <UsageBar volume={v} reclaim={reclaimableOf(scan)} />
            <span className="drive-card-nums" id={`${ids}-${i}-nums`}><b>{formatBytes(v.freeBytes, 0)} free</b> of {formatBytes(v.totalBytes, 0)}</span>
            <DriveStatus scan={scan} now={now} id={`${ids}-${i}-status`} />
            {foot?.(v, on)}
          </button>
        );
      })}
    </div>
  );
}

/** The plate's frame: the question, the cards, then the variant's actions and notes, then the app's fine print. */
export function PlateFrame({ api, calmRef, children, cards, above, after }: { api: FlowApi; calmRef: PlateProps['calmRef']; children: ReactNode; cards?: ReactNode; above?: ReactNode; after?: ReactNode }) {
  const v = api.volume;
  return (
    <div className="plate">
      <div className="plate-content has-cards" ref={calmRef}>
        <h1 className="plate-ask">Which drive should Vacancy scan?</h1>
        <p className="plate-cap">{api.volumes.length} drives on {fixtures[api.flow.platform].machineName}</p>
        {above}
        {cards ?? <DriveCards api={api} />}
        {children}
        {after}
        <p className="plate-fine">
          Scanning only reads. Nothing changes until you approve a plan.
          {v.kind !== 'internal' && <> Keep {v.name} plugged in until the scan finishes.</>}
        </p>
      </div>
    </div>
  );
}

/** The HUD, as the real one draws it, with a mode tag and room below for a note or an offer. */
export function Hud({ api, children }: { api: FlowApi; children?: ReactNode }) {
  const { flow, volume } = api;
  const { update } = updateAt(volume, flow.mode, flow.progress);
  return (
    <div className={flow.mode === 'fast' ? 'hud is-fast' : 'hud'} role="status" aria-label="Scan progress" data-no-ripple>
      <div className="hud-nums">
        {flow.mode === 'fast' && <span className="hud-mode"><Zap size={11} strokeWidth={2.4} aria-hidden /> Fast scan</span>}
        <span><b>{formatCount(update.files)}</b> files</span>
        <span><b>{formatCount(update.folders)}</b> folders</span>
        <button type="button" className="hud-cancel" onClick={api.cancel}>Cancel</button>
      </div>
      <div className="hud-path" title={update.currentPath}>{update.currentPath}</div>
      <div className="hud-bar" role="progressbar" aria-label="Scanned" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(flow.progress * 100)}>
        <i style={{ width: `${Math.min(0.98, flow.progress) * 100}%` }} />
      </div>
      {children}
    </div>
  );
}

/** A short line under the HUD's bar: why this is the normal scan. */
export function HudNote({ children, tone = 'review' }: { children: ReactNode; tone?: 'review' | 'quiet' | 'safe' }) {
  return (
    <p className={`hud-note is-${tone}`} role="note">
      {tone === 'safe' ? <ShieldCheck size={13} aria-hidden /> : <Info size={13} aria-hidden />}
      <span>{children}</span>
    </p>
  );
}

// ---------- Windows' prompt, drawn as a labelled stand-in ----------

/**
 * A stand-in for the User Account Control prompt. Windows draws the real one on its secure desktop; Vacancy can't
 * style it, word it or skip it. Its question ("make changes to your device") is Windows' own wording.
 */
export function UacPrompt({ api }: { api: FlowApi }) {
  const yes = useRef<HTMLButtonElement>(null);
  const [more, setMore] = useState(false);
  useEffect(() => { yes.current?.focus(); }, []);
  return (
    <div className="uac-desktop" data-no-ripple>
      <div className="uac-stand-in" role="note">
        PROTOTYPE STAND-IN for Windows’ own prompt. Windows draws it and dims the screen; Vacancy can’t change its words.
      </div>
      <div className="uac" role="dialog" aria-modal="true" aria-label="User Account Control">
        <div className="uac-top">
          <p className="uac-title">User Account Control</p>
          <p className="uac-q">Do you want to allow this app to make changes to your device?</p>
        </div>
        <div className="uac-body">
          <div className="uac-app">
            <span className="uac-icon" aria-hidden><Zap size={18} strokeWidth={2.2} /></span>
            <span className="uac-name">Vacancy scan helper</span>
          </div>
          <p className="uac-meta">Verified publisher: Vacancy (synthetic)<br />File origin: Hard drive on this computer</p>
          {more && <p className="uac-meta">Program location: "C:\Users\alex\AppData\Local\Vacancy\vacancy-index-reader.exe" --read-index {api.volume.mountPoint}</p>}
          <button type="button" className="uac-link" onClick={() => setMore(!more)}>{more ? 'Hide details' : 'Show more details'}</button>
        </div>
        <div className="uac-foot">
          <button type="button" ref={yes} className="uac-btn is-yes" onClick={api.uacYes}>Yes</button>
          <button type="button" className="uac-btn" onClick={api.uacNo}>No</button>
        </div>
      </div>
    </div>
  );
}

// ---------- the map, with the Locked folders block ----------

export const LOCKED_ID = 'locked';

/** The overview's model with the Locked folders block (fast scans only): one region, totals per reason, no names. */
export function mapModel(volume: VolumeInfo, mode: Mode): AtlasModel {
  const base = overviewModel(overviewFor(volume, mode));
  if (mode !== 'fast') return base;
  const groups = lockedOf(volume);
  const region: Region = {
    id: LOCKED_ID, category: 'system', categoryName: 'Locked folders', name: 'Locked folders', bytes: lockedTotal(volume),
    protected: true, free: false, pending: false,
    blurb: `Totals only: ${formatCount(lockedFolders(volume))} folders your account can’t open. Vacancy shows no names from inside them, and can’t propose or delete anything there.`,
    leaves: groups.map((g, i) => ({
      id: g.id, regionId: LOCKED_ID, kind: 'hidden', name: g.label, path: '', bytes: g.bytes, lastUsed: null, browsable: false,
      note: `${g.why} Fast scan counted ${formatCount(g.folders)} folders here and shows their total only.`, proposal: null, shade: i % 3, vacant: 0,
    })),
  };
  const regions = [...base.regions.filter((r) => r.id !== FREE_ID), region, ...base.regions.filter((r) => r.id === FREE_ID)];
  const leaves = regions.flatMap((r) => r.leaves);
  return { ...base, regions, leaves, leafById: new Map(leaves.map((l) => [l.id, l])), regionById: new Map(regions.map((r) => [r.id, r])) };
}

type Level = { kind: 'region'; id: string } | { kind: 'folder'; folder: MapFolder };

/** The drive's map once scanned (Atlas after a scan): breadcrumb, notes and key over the map; zoom and open. */
export function MapScreen({ api }: { api: FlowApi }) {
  const { volume } = api;
  const done = api.flow.done[volume.mountPoint]!;
  const [stageRef, size] = useSize<HTMLDivElement>();
  const [levels, setLevels] = useState<Level[]>([]);
  const [hot, setHot] = useState<string | null>(null);
  const top = useMemo(() => mapModel(volume, done.mode), [volume, done.mode]);
  const overview = useMemo(() => overviewFor(volume, done.mode), [volume, done.mode]);
  const proposals = useMemo(() => proposalsFor(volume), [volume]);
  const byId = useMemo(() => new Map(proposals.map((p) => [p.id, p])), [proposals]);
  const level = levels.at(-1);
  const model = useMemo(() => {
    if (level?.kind !== 'folder') return top;
    const cat = overview.categories.find((c) => c.id === level.folder.category);
    return folderModel(level.folder, { name: cat?.name ?? '', protected: cat?.protected ?? false }, volume);
  }, [level, top, overview, volume]);
  const focus = level?.kind === 'region' ? level.id : null;
  const go = (next: Level[]) => { setHot(null); setLevels(next); };
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') setLevels((ls) => ls.slice(0, -1)); };
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  }, []);

  const onLeaf = (leaf: Leaf) => {
    if (levels.length === 0) return go([{ kind: 'region', id: leaf.regionId }]);
    const folder = leaf.browsable ? folderFor(volume, leaf.id) : null;
    if (folder) go([...levels, { kind: 'folder', folder }]);
  };
  const onRegion = (r: Region) => (levels.length === 0 ? go([{ kind: 'region', id: r.id }]) : go(levels.slice(0, -1)));
  const clickable = (l: Leaf) => (levels.length === 0 ? l.kind !== 'free' : l.browsable && !!folderFor(volume, l.id));
  const look = (l: Leaf, rect: { w: number; h: number }): TileLook => {
    if (l.regionId === LOCKED_ID) {
      const roomy = rect.w >= 110 && rect.h >= 64;
      return {
        cls: 'is-locked-block',
        label: (
          <div className="locked-label">
            <Lock size={levels.length ? 16 : 12} strokeWidth={2.2} aria-hidden />
            {roomy && <span className="locked-name">{l.name}</span>}
            {roomy && <span className="locked-size">{formatBytes(l.bytes)} · totals only</span>}
          </div>
        ),
        state: 'locked, totals only',
      };
    }
    return l.proposal !== null ? { cls: 'is-reclaim', state: 'reclaimable' } : {};
  };
  const leafStatus = (l: Leaf): ReactNode => {
    const p = l.proposal === null ? undefined : byId.get(l.proposal);
    if (!p) return null;
    return (
      <>
        <div className="map-tip-meta"><RiskChip risk={p.risk} /><SourceTag source={p.source} agent={null} /></div>
        <p className="map-tip-sum">{p.summary}</p>
        <p className="map-tip-act">Click to zoom in.</p>
      </>
    );
  };
  const regionName = (id: string) => model.regionById.get(id)?.name ?? top.regionById.get(id)?.name ?? id;
  const blurb = level?.kind === 'region' ? top.regionById.get(level.id)?.blurb : level?.kind === 'folder' ? level.folder.item.path : null;
  const bytesInView = level?.kind === 'folder' ? level.folder.item.bytes : focus ? (top.regionById.get(focus)?.bytes ?? volume.totalBytes) : volume.totalBytes;
  const isFast = done.mode === 'fast';

  return (
    <main className="atlas is-map">
      <div className="atlas-toolbar">
        <nav className="crumbs" aria-label="Map location">
          {levels.length === 0 ? (
            <span className="crumb is-here" aria-current="location">{volume.name}</span>
          ) : (
            <button type="button" className="crumb" onClick={() => go([])}>{volume.name}</button>
          )}
          {levels.map((l, i) => {
            const last = i === levels.length - 1;
            const name = l.kind === 'region' ? regionName(l.id) : l.folder.item.name;
            return (
              <span key={i} className="crumb-step">
                <ChevronRight size={13} aria-hidden />
                {last ? <span className="crumb is-here" aria-current="location">{name}</span> : <button type="button" className="crumb" onClick={() => go(levels.slice(0, i + 1))}>{name}</button>}
              </span>
            );
          })}
          {blurb && <span className="crumb-blurb">{blurb}</span>}
        </nav>
        <div className="scan-notes">
          {isFast ? (
            <>
              <span className="scan-note is-fast" title="Read from the drive’s file index, with admin, then admin was given back."><Zap size={11} aria-hidden /> Fast scan, {Math.round(overview.scan.durationMs / 1000)} s</span>
              {levels.length === 0 && <span className="scan-note" title={top.regionById.get(LOCKED_ID)?.blurb}><Lock size={11} aria-hidden /> {formatBytes(lockedTotal(volume), 0)} in locked folders, totals only</span>}
            </>
          ) : (
            <>
              {done.note && <span className="scan-note">Normal scan: {done.note === 'declined' ? 'admin wasn’t given' : `${FILE_SYSTEM[volume.mountPoint]} drive`}</span>}
              {overview.scan.unreadable > 0 && <span className="scan-note" title={`Couldn’t read:\n${overview.scan.unreadableSamples.join('\n')}`}>{formatCount(overview.scan.unreadable)} folders couldn’t be read</span>}
            </>
          )}
        </div>
        <Legend bytesInView={bytesInView} w={size.w} h={size.h} reclaimable={proposals.length > 0} />
        <button type="button" className="btn btn--sm" onClick={api.scanAgain}><RotateCcw size={12} aria-hidden /> Scan again</button>
      </div>
      <div className="atlas-stage" ref={stageRef}>
        <MapView model={model} width={size.w} height={size.h} focus={focus} interactive hot={hot} onHot={setHot}
          onLeaf={onLeaf} onRegion={onRegion} clickable={clickable} look={look} leafStatus={leafStatus}
          leafAction={(l) => (l.regionId === LOCKED_ID ? null : levels.length === 0 ? (l.kind === 'free' ? null : 'Click to zoom in.') : l.browsable && folderFor(volume, l.id) ? 'Click to open.' : null)}
          regionAction={(r) => (r.id === LOCKED_ID && levels.length === 0 ? 'Click to see why.' : levels.length === 0 ? 'Click to zoom in.' : 'Click to zoom out.')} />
        {focus === LOCKED_ID && <LockedCard volume={volume} />}
      </div>
    </main>
  );
}

/** Opened Locked folders: what the block is, said once, beside its two totals. */
function LockedCard({ volume }: { volume: VolumeInfo }) {
  return (
    <aside className="locked-card" aria-label="About locked folders" data-no-ripple>
      <p className="locked-card-h"><Lock size={14} aria-hidden /> {formatBytes(lockedTotal(volume))} in {formatCount(lockedFolders(volume))} locked folders</p>
      <p>Your account can’t open these folders, even though fast scan could count them. So Vacancy shows only how much space they take: no names, nothing to propose, nothing to delete.</p>
      <p className="locked-card-quiet">To clean up another account’s files, sign in to Windows as that account and run Vacancy there. Admin from a fast scan is never used to open them.</p>
    </aside>
  );
}

export { isNtfs };

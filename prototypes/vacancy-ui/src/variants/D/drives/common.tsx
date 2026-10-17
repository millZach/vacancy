// PROTOTYPE drive picker (variant D, ?picker=1|2|3): what every design shares. The view of one drive, the switching
// API the host hands each design, and small pieces (icon, usage bar, scanned-state line, the scan plate's actions).
import { HardDrive, Map as MapIcon, ScanLine, Sparkles, Usb } from 'lucide-react';
import { useEffect, type RefObject } from 'react';
import type { AiAgent } from '../../../shared/data';
import type { Drive, DriveKind } from '../../../shared/drives';

export interface DriveView {
  drive: Drive;
  current: boolean;
  scanned: boolean;
  scannedAt: number | null;
  /** A plan ran on this drive (it reached the summary). */
  cleaned: boolean;
  freed: number;
  /** Live numbers: after cleaning when it was cleaned. */
  used: number;
  free: number;
  /** What the scan found to delete (sum of delete proposals). */
  reclaim: number;
}

export interface PickerApi {
  views: DriveView[];
  cur: DriveView;
  machine: string;
  agent: AiAgent | null;
  /** A run is deleting: no switching until it finishes. */
  locked: boolean;
  now: number;
  /** Header and shelf: a scanned drive opens where you left it (its map), an unscanned one opens its scan screen. */
  switchTo: (id: string) => void;
  /** Scan screen only: choose the drive the screen is about, without leaving it. */
  pick: (id: string) => void;
  /** Open the current, already scanned drive's map. */
  openMap: () => void;
}

export const LOCK_NOTE = 'Switch drives after this run finishes.';

/** Capacity in the way the OS shows it: "1 TB", "1.3 TB", "993 GB". */
export function sz(b: number): string {
  if (b >= 1e12) return `${(b / 1e12).toFixed(1).replace(/\.0$/, '')} TB`;
  return `${Math.round(b / 1e9)} GB`;
}

export const pct = (v: DriveView) => Math.round((v.used / v.drive.totalBytes) * 100);

export function ago(ms: number): string {
  const m = Math.floor(ms / 60_000);
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  return `${Math.floor(m / 60)} h ago`;
}

export function statusText(v: DriveView, now: number): string {
  if (!v.scanned || v.scannedAt === null) return 'Not scanned yet';
  if (v.cleaned) return `Cleaned, ${sz(v.freed)} freed`;
  return `Scanned ${ago(now - v.scannedAt)}, ${sz(v.reclaim)} to reclaim`;
}

/** What switching to this drive will do, in two words. */
export const goText = (v: DriveView) => (v.scanned ? 'Open map' : 'Scan');

export function DriveIcon({ kind, size = 14, className }: { kind: DriveKind; size?: number; className?: string }) {
  const Icon = kind === 'usb' ? Usb : HardDrive;
  return <Icon size={size} strokeWidth={size > 18 ? 1.4 : 1.8} className={className} aria-hidden />;
}

export function KindTag({ v }: { v: DriveView }) {
  const k = v.drive.kind;
  return <span className={`vd-kind vd-kind--${k}`}>{k === 'internal' ? 'Internal' : k === 'usb' ? 'USB' : 'External'}</span>;
}

/** Used space, with the share a scan found to reclaim lit at its end. Turns warm when the drive is 90% full. */
export function UsageBar({ v, className = '' }: { v: DriveView; className?: string }) {
  const t = v.drive.totalBytes;
  const reclaim = v.scanned && !v.cleaned ? Math.min(v.reclaim, v.used) : 0;
  return (
    <span className={`vd-use ${v.used / t >= 0.9 ? 'is-full' : ''} ${className}`} aria-hidden>
      <i style={{ width: `${(v.used / t) * 100}%` }} />
      {reclaim > 0 && <b style={{ left: `${((v.used - reclaim) / t) * 100}%`, width: `${(reclaim / t) * 100}%` }} />}
    </span>
  );
}

export function Status({ v, now }: { v: DriveView; now: number }) {
  return (
    <span className={`vd-status ${v.scanned ? 'is-scanned' : ''} ${v.cleaned ? 'is-cleaned' : ''}`}>
      <i />
      {statusText(v, now)}
    </span>
  );
}

export function AgentNote({ agent }: { agent: AiAgent | null }) {
  return (
    <div className={`va-agent-note ${agent ? '' : 'is-none'}`}>
      <Sparkles size={14} strokeWidth={2} />
      {agent ? (
        <span>
          <b>{agent} is installed.</b> Vacancy will ask it about big folders the built-in rules don't recognize, and for tidier layouts of messy
          ones.
        </span>
      ) : (
        <span>
          <b>No AI agent found.</b> Vacancy will use its built-in rules only.
        </span>
      )}
    </div>
  );
}

export function PlateFine({ v }: { v: DriveView }) {
  return (
    <p className="va-plate-fine">
      Scanning only reads. Nothing changes until you approve a plan.
      {v.drive.removable && <> Keep {v.drive.label} plugged in until the scan finishes.</>}
    </p>
  );
}

/** The plate's main action: scan this drive, or (already scanned) go back to its map without rescanning. */
export function PlateActions({ api, scan }: { api: PickerApi; scan: (b: HTMLButtonElement) => void }) {
  const v = api.cur;
  if (!v.scanned)
    return (
      <button type="button" className="va-cta va-cta--scan" onClick={(e) => scan(e.currentTarget)}>
        <ScanLine size={16} /> Scan {v.drive.label}
      </button>
    );
  return (
    <div className="vd-plate-acts">
      <button type="button" className="va-cta va-cta--scan" onClick={api.openMap}>
        <MapIcon size={16} /> Open its map
      </button>
      <button type="button" className="va-btn vd-rescan" onClick={(e) => scan(e.currentTarget)}>
        <ScanLine size={14} /> Scan again
      </button>
    </div>
  );
}

/** Closes a popover on a press outside it or on Escape. */
export function useDismiss(ref: RefObject<HTMLElement | null>, open: boolean, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    document.addEventListener('pointerdown', down, true);
    window.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('pointerdown', down, true);
      window.removeEventListener('keydown', key);
    };
  }, [open]);
}

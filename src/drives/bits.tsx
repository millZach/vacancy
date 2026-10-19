import { HardDrive, Usb } from 'lucide-react';
import type { DriveKind, VolumeInfo } from '../bridge';
import { formatBytes, timeAgo } from '../format';
import { reclaimableOf, reportOf, type DriveScan } from './session';
import './drives.css';

/** Why the drive menu is shut while a plan runs. */
export const LOCK_NOTE = 'Switch drives after this run finishes.';

/** A share this full or more turns a drive's bar warm. */
const NEARLY_FULL = 0.9;

const KIND_LABEL: Record<DriveKind, string> = { internal: 'Internal', usb: 'USB', external: 'External' };

/** A USB stick for USB drives, a hard drive for the rest. */
export function DriveIcon({ kind, size = 14, className }: { kind: DriveKind; size?: number; className?: string }) {
  const Icon = kind === 'usb' ? Usb : HardDrive;
  return <Icon size={size} strokeWidth={size > 18 ? 1.4 : 1.8} className={className} aria-hidden />;
}

/** The drive's kind as a small tag, and a "System" tag on the drive the OS runs from. */
export function DriveTags({ volume, system = true, id }: { volume: VolumeInfo; system?: boolean; id?: string }) {
  return (
    <span className="drive-tags" id={id}>
      <span className={volume.kind === 'internal' ? 'drive-tag' : 'drive-tag is-outside'}>{KIND_LABEL[volume.kind]}</span>
      {/* A space, so screen readers say "Internal System"; the flex layout ignores it. */}
      {system && volume.system && <> <span className="drive-tag">System</span></>}
    </span>
  );
}

/**
 * Used space as a bar, warm once the drive is 90% full, with the share a scan found to reclaim lit at its end. The
 * numbers beside it say the same for screen readers.
 */
export function UsageBar({
  volume,
  reclaim = 0,
  className,
}: {
  volume: VolumeInfo;
  reclaim?: number;
  className?: string;
}) {
  const total = volume.totalBytes;
  const share = total > 0 ? Math.min(1, volume.usedBytes / total) : 0;
  const lit = total > 0 ? Math.min(reclaim, volume.usedBytes) / total : 0;
  const classes = ['usage', share >= NEARLY_FULL ? 'is-full' : '', className ?? ''].filter(Boolean).join(' ');
  return (
    <span className={classes} aria-hidden>
      <i style={{ width: `${share * 100}%` }} />
      {lit > 0 && <b style={{ left: `${(share - lit) * 100}%`, width: `${lit * 100}%` }} />}
    </span>
  );
}

/**
 * "Not scanned yet", "Waiting for admin" (a fast scan's prompt), "Scanning now", "Scanned 4 min ago, 120 GB to
 * reclaim", "Cleaning up now" or "Freed 74 GB 2 min ago", with a dot that lights once the drive has a scan.
 */
export function DriveStatus({ scan, now, id }: { scan: DriveScan; now: number; id?: string }) {
  const lit = scan.phase === 'done' ? ' is-scanned' : scan.phase === 'scanning' ? ' is-scanning' : '';
  return (
    <span className={`drive-status${lit}`} id={id}>
      <i aria-hidden />
      {statusText(scan, now)}
    </span>
  );
}

function statusText(scan: DriveScan, now: number): string {
  if (scan.phase === 'scanning') {
    // A fast scan asked for, before the core has read anything: Windows' admin prompt is up.
    const asking = scan.update ? scan.update.stage === 'asking' : scan.fast;
    return asking ? 'Waiting for admin' : 'Scanning now';
  }
  if (scan.phase !== 'done') return 'Not scanned yet';
  if (scan.planRun?.phase === 'running') return 'Cleaning up now';
  const report = reportOf(scan);
  if (report) return `Freed ${formatBytes(report.freedBytes, 0)} ${timeAgo(report.startedAt + report.durationMs, now)}`;
  const reclaim = reclaimableOf(scan);
  return `Scanned ${timeAgo(scan.at, now)}${reclaim > 0 ? `, ${formatBytes(reclaim, 0)} to reclaim` : ''}`;
}

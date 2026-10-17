import type { CategoryTotal, Overview, ScanOutcome, ScanUpdate, VolumeInfo } from '../bridge';

/**
 * The drives for this session, one at a time on screen, each with its own scan and map (and, as later tickets add
 * them, its own plan and summary), so switching never mixes two drives' data. This file is the pure part: the state
 * and how each event changes it. `DrivesProvider` wires it to the bridge.
 */
export interface Session {
  /** The drives to offer, the system drive first, from the latest listing. */
  drives: VolumeInfo[];
  /** The drive on screen, by mount point. */
  current: string;
  /** Whether the drive on screen shows its scan screen or (once it has one) its map. */
  screen: Screen;
  /** Each drive's scan, by mount point. A drive with no entry hasn't been scanned. */
  scans: Record<string, DriveScan>;
  /** A plan is running: switching drives waits until it ends. */
  locked: boolean;
  /** The drive on screen went away, for a short notice. */
  gone: Gone | null;
}

export type Screen = 'scan' | 'map';

/** Where one drive's scan is. `run` tells a scan's events apart from those of a scan that came before it. */
export type DriveScan =
  | { phase: 'idle' }
  | {
      phase: 'scanning';
      run: number;
      update: ScanUpdate | null;
      /** The categories so far, from the last update that carried a preview. */
      preview: CategoryTotal[];
    }
  | { phase: 'done'; overview: Overview; /** When it finished, in ms since the epoch. */ at: number }
  | { phase: 'failed'; message: string };

/** A drive that went away while it was on screen, and what went with it. */
export interface Gone {
  name: string;
  lost: 'map' | 'scan' | 'nothing';
}

export type Action =
  /** A fresh listing of the drives. Drives no longer in it leave, and their data with them. */
  | { type: 'listed'; drives: VolumeInfo[] }
  /** The scan screen's drive cards: choose the drive to scan, staying on the scan screen. */
  | { type: 'pick'; mountPoint: string }
  /** The header's drive menu: a scanned drive opens its map, an unscanned one its scan screen. */
  | { type: 'switch'; mountPoint: string }
  /** Back from the scan screen to the drive's map. */
  | { type: 'openMap' }
  /** From the map to the drive's scan screen ("Scan again"). */
  | { type: 'showScanScreen' }
  | { type: 'scanStarted'; mountPoint: string; run: number }
  | { type: 'scanUpdate'; mountPoint: string; run: number; update: ScanUpdate }
  | { type: 'scanEnded'; mountPoint: string; run: number; outcome: ScanOutcome; at: number }
  | { type: 'scanFailed'; mountPoint: string; run: number; message: string }
  /** The user stopped the scan of this drive. */
  | { type: 'scanCancelled'; mountPoint: string }
  | { type: 'lock'; locked: boolean }
  | { type: 'dismissGone' };

const IDLE: DriveScan = { phase: 'idle' };

/** Before the first listing: no drives yet. */
export const EMPTY: Session = { drives: [], current: '', screen: 'scan', scans: {}, locked: false, gone: null };

export function scanOf(s: Session, mountPoint: string): DriveScan {
  return s.scans[mountPoint] ?? IDLE;
}

/**
 * The same drive as before: same place, same volume id. A different stick plugged in where the last one was is a
 * different drive, and the last one's map must not carry over to it. A name or kind that reads differently once
 * (the OS was slow to answer) doesn't make it another drive.
 */
export function sameDrive(a: VolumeInfo, b: VolumeInfo): boolean {
  return a.mountPoint === b.mountPoint && a.id === b.id;
}

/** The figures to show for a drive: those its map was drawn against once it has one, so they agree with the map. */
export function shownVolume(s: Session, drive: VolumeInfo): VolumeInfo {
  const scan = scanOf(s, drive.mountPoint);
  return scan.phase === 'done' ? scan.overview.volume : drive;
}

/** The screen a drive opens at: its map once it has one, its scan screen until then. */
function screenFor(s: Session, mountPoint: string): Screen {
  return scanOf(s, mountPoint).phase === 'done' ? 'map' : 'scan';
}

/** Puts `mountPoint` on screen. A scan running on the drive being left stops (the provider cancels it). */
function goTo(s: Session, mountPoint: string, screen: Screen): Session {
  if (s.locked || !s.drives.some((d) => d.mountPoint === mountPoint)) return s;
  let scans = s.scans;
  if (mountPoint !== s.current && scanOf(s, s.current).phase === 'scanning') {
    scans = { ...scans };
    delete scans[s.current];
  }
  return { ...s, scans, current: mountPoint, screen, gone: null };
}

/** Applies `change` to the drive's scan only if it is the scan `run` (a later scan, or none, ignores it). */
function onRun(
  s: Session,
  mountPoint: string,
  run: number,
  change: (scan: Extract<DriveScan, { phase: 'scanning' }>) => DriveScan,
): Session {
  const scan = scanOf(s, mountPoint);
  if (scan.phase !== 'scanning' || scan.run !== run) return s;
  return { ...s, scans: { ...s.scans, [mountPoint]: change(scan) } };
}

export function reduce(s: Session, a: Action): Session {
  switch (a.type) {
    case 'listed':
      return listed(s, a.drives);
    case 'pick':
      return goTo(s, a.mountPoint, 'scan');
    case 'switch':
      return goTo(s, a.mountPoint, screenFor(s, a.mountPoint));
    case 'openMap':
      return screenFor(s, s.current) === 'map' ? { ...s, screen: 'map' } : s;
    case 'showScanScreen':
      return { ...s, screen: 'scan' };
    case 'scanStarted':
      if (!s.drives.some((d) => d.mountPoint === a.mountPoint)) return s;
      return {
        ...s,
        scans: { ...s.scans, [a.mountPoint]: { phase: 'scanning', run: a.run, update: null, preview: [] } },
      };
    case 'scanUpdate':
      return onRun(s, a.mountPoint, a.run, (scan) => ({
        ...scan,
        update: a.update,
        preview: a.update.categories ?? scan.preview,
      }));
    case 'scanEnded': {
      const next = onRun(s, a.mountPoint, a.run, () =>
        a.outcome.status === 'done' ? { phase: 'done', overview: a.outcome.overview, at: a.at } : IDLE,
      );
      return next !== s && a.mountPoint === s.current && a.outcome.status === 'done' ? { ...next, screen: 'map' } : next;
    }
    case 'scanFailed':
      return onRun(s, a.mountPoint, a.run, () => ({ phase: 'failed', message: a.message }));
    case 'scanCancelled': {
      if (scanOf(s, a.mountPoint).phase !== 'scanning') return s;
      const scans = { ...s.scans };
      delete scans[a.mountPoint];
      return { ...s, scans };
    }
    case 'lock':
      return a.locked === s.locked ? s : { ...s, locked: a.locked };
    case 'dismissGone':
      return s.gone ? { ...s, gone: null } : s;
  }
}

/**
 * A fresh listing. Drives that left take their scans with them. If the drive on screen left, the system drive takes
 * its place (at its map if it has one) and `gone` says what happened.
 */
function listed(s: Session, drives: VolumeInfo[]): Session {
  const first = drives.find((d) => d.system) ?? drives[0];
  if (!first) return s; // the system drive is always listed; an empty listing is a failed one
  if (s.drives.length === 0) return { ...EMPTY, locked: s.locked, drives, current: first.mountPoint };
  if (drives.length === s.drives.length && drives.every((d, i) => sameVolume(d, s.drives[i]!))) return s;

  const left = s.drives.filter((d) => !drives.some((n) => sameDrive(n, d)));
  let scans = s.scans;
  if (left.length) {
    scans = { ...scans };
    for (const d of left) delete scans[d.mountPoint];
  }
  const next = { ...s, drives, scans };
  const onScreen = left.find((d) => d.mountPoint === s.current);
  if (!onScreen) return next;
  const scan = scanOf(s, s.current).phase;
  const lost = scan === 'done' ? 'map' : scan === 'scanning' ? 'scan' : 'nothing';
  return {
    ...next,
    current: first.mountPoint,
    screen: screenFor(next, first.mountPoint),
    gone: { name: onScreen.name, lost },
  };
}

function sameVolume(a: VolumeInfo, b: VolumeInfo): boolean {
  return (
    sameDrive(a, b) &&
    a.name === b.name &&
    a.kind === b.kind &&
    a.system === b.system &&
    a.totalBytes === b.totalBytes &&
    a.usedBytes === b.usedBytes &&
    a.freeBytes === b.freeBytes
  );
}

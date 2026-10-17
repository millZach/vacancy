import { describe, expect, it } from 'vitest';
import { fixtures, type Overview, type VolumeInfo } from '../bridge';
import { EMPTY, reclaimableBytes, reclaimableOf, reduce, scanOf, shownVolume, type Action, type Session } from './session';

const [C, D, E] = fixtures.windows.volumes as [VolumeInfo, VolumeInfo, VolumeInfo];

function mapOf(volume: VolumeInfo): Overview {
  return { ...fixtures.windows.overview, volume };
}

/** A finished scan of `volume`: its map and the fixture's proposals. */
const done = (volume: VolumeInfo) => ({ status: 'done' as const, overview: mapOf(volume), proposals: fixtures.windows.proposals });

const run = (s: Session, ...actions: Action[]) => actions.reduce(reduce, s);
const listed = (drives: VolumeInfo[] = [C, D, E]): Action => ({ type: 'listed', drives });
const scanned = (volume: VolumeInfo, id: number): Action[] => [
  { type: 'scanStarted', mountPoint: volume.mountPoint, run: id },
  { type: 'scanEnded', mountPoint: volume.mountPoint, run: id, outcome: done(volume), at: 1 },
];
const start = run(EMPTY, listed());

describe('the drive session', () => {
  it('starts on the system drive’s scan screen', () => {
    expect(start.current).toBe(C.mountPoint);
    expect(start.screen).toBe('scan');
    expect(scanOf(start, C.mountPoint).phase).toBe('idle');
  });

  it('keeps each drive’s map apart, and switching back opens it without a new scan', () => {
    const s = run(start, ...scanned(C, 1), { type: 'switch', mountPoint: D.mountPoint });
    expect(s.current).toBe(D.mountPoint);
    expect(s.screen).toBe('scan');

    const both = run(s, ...scanned(D, 2), { type: 'switch', mountPoint: C.mountPoint });
    expect(both.current).toBe(C.mountPoint);
    expect(both.screen).toBe('map');
    const c = scanOf(both, C.mountPoint);
    const d = scanOf(both, D.mountPoint);
    expect(c.phase === 'done' && c.overview.volume.name).toBe('Local Disk (C:)');
    expect(d.phase === 'done' && d.overview.volume.name).toBe('Data (D:)');
  });

  it('never lets a scan land on another drive, or an old scan on a newer one', () => {
    let s = run(start, { type: 'scanStarted', mountPoint: C.mountPoint, run: 1 });
    // Switching away stops C:'s scan; D:'s scan starts.
    s = run(s, { type: 'switch', mountPoint: D.mountPoint }, { type: 'scanStarted', mountPoint: D.mountPoint, run: 2 });
    expect(scanOf(s, C.mountPoint).phase).toBe('idle');

    // C:'s scan finishing late changes nothing, and D:'s events only touch D:.
    const late = run(s, {
      type: 'scanEnded',
      mountPoint: C.mountPoint,
      run: 1,
      outcome: done(C),
      at: 1,
    });
    expect(late).toBe(s);
    const wrongDrive = run(s, {
      type: 'scanEnded',
      mountPoint: C.mountPoint,
      run: 2,
      outcome: done(D),
      at: 1,
    });
    expect(wrongDrive).toBe(s);

    // A rescan of D: replaces the first; the first one's end is ignored.
    s = run(s, { type: 'scanStarted', mountPoint: D.mountPoint, run: 3 });
    expect(run(s, ...scanned(D, 2).slice(1))).toBe(s);
    s = run(s, ...scanned(D, 3).slice(1));
    expect(scanOf(s, D.mountPoint).phase).toBe('done');
    expect(s.screen).toBe('map');
  });

  it('picks a drive on the scan screen without leaving it', () => {
    const s = run(start, ...scanned(D, 1), { type: 'showScanScreen' }, { type: 'pick', mountPoint: D.mountPoint });
    expect(s.current).toBe(D.mountPoint);
    expect(s.screen).toBe('scan');
    expect(run(s, { type: 'openMap' }).screen).toBe('map');
    // An unscanned drive has no map to open.
    const e = run(s, { type: 'pick', mountPoint: E.mountPoint }, { type: 'openMap' });
    expect(e.screen).toBe('scan');
  });

  it('shows a scanned drive with the figures its map was drawn against', () => {
    const later = { ...C, usedBytes: C.usedBytes - 1e9, freeBytes: C.freeBytes + 1e9 };
    const s = run(start, ...scanned(C, 1), listed([later, D, E]));
    expect(shownVolume(s, later).usedBytes).toBe(C.usedBytes);
    expect(shownVolume(s, D)).toBe(D);
  });

  it('holds still while a plan runs', () => {
    const locked = run(start, ...scanned(C, 1), { type: 'lock', locked: true });
    expect(run(locked, { type: 'switch', mountPoint: D.mountPoint })).toBe(locked);
    expect(run(locked, { type: 'pick', mountPoint: D.mountPoint })).toBe(locked);
    const unlocked = run(locked, { type: 'lock', locked: false }, { type: 'switch', mountPoint: D.mountPoint });
    expect(unlocked.current).toBe(D.mountPoint);
  });

  it('drops a drive that leaves, quietly when it was not on screen', () => {
    const s = run(start, ...scanned(E, 1), { type: 'switch', mountPoint: C.mountPoint }, listed([C, D]));
    expect(s.drives.map((d) => d.mountPoint)).toEqual([C.mountPoint, D.mountPoint]);
    expect(s.scans[E.mountPoint]).toBeUndefined();
    expect(s.gone).toBeNull();
  });

  it('says so when the drive on screen leaves, and goes back to the system drive', () => {
    const withMap = run(start, ...scanned(C, 1), { type: 'switch', mountPoint: E.mountPoint }, ...scanned(E, 2));
    const s = run(withMap, listed([C, D]));
    expect(s.gone).toEqual({ name: 'SanDisk (E:)', lost: 'map' });
    expect(s.current).toBe(C.mountPoint);
    expect(s.screen).toBe('map');
    expect(run(s, { type: 'dismissGone' }).gone).toBeNull();

    const scanning = run(start, { type: 'pick', mountPoint: E.mountPoint }, { type: 'scanStarted', mountPoint: E.mountPoint, run: 3 });
    const stopped = run(scanning, listed([C, D]));
    expect(stopped.gone).toEqual({ name: 'SanDisk (E:)', lost: 'scan' });
    expect(stopped.screen).toBe('scan');
    // The scan's events after that change nothing.
    expect(run(stopped, ...scanned(E, 3).slice(1))).toBe(stopped);
  });

  it('treats a different drive in the same place as a new drive', () => {
    const s = run(start, { type: 'switch', mountPoint: E.mountPoint }, ...scanned(E, 1));
    const other = { ...E, id: 'another stick', name: 'Kingston (E:)', totalBytes: 128e9 };
    const next = run(s, listed([C, D, other]));
    expect(scanOf(next, E.mountPoint).phase).toBe('idle');
    expect(next.gone).toEqual({ name: 'SanDisk (E:)', lost: 'map' });
  });

  it('keeps a drive whose name or kind reads differently for a moment', () => {
    const s = run(start, { type: 'switch', mountPoint: E.mountPoint }, ...scanned(E, 1));
    const flicker = { ...E, name: 'USB Drive (E:)', kind: 'internal' as const };
    const next = run(s, listed([C, D, flicker]));
    expect(scanOf(next, E.mountPoint).phase).toBe('done');
    expect(next.gone).toBeNull();
    expect(next.drives[2]!.name).toBe('USB Drive (E:)');
  });

  it('ignores a listing that changed nothing, or came back empty', () => {
    expect(run(start, listed([{ ...C }, { ...D }, { ...E }]))).toBe(start);
    expect(run(start, listed([]))).toBe(start);
  });
});

describe('each drive’s plan', () => {
  const planOf = (s: Session, volume: VolumeInfo) => {
    const scan = scanOf(s, volume.mountPoint);
    return scan.phase === 'done' ? scan.plan : null;
  };
  const defaults = fixtures.windows.proposals.filter((p) => p.defaultOn).map((p) => p.id);
  const plan = (volume: VolumeInfo, id: string, on: boolean): Action => ({
    type: 'plan',
    mountPoint: volume.mountPoint,
    id,
    on,
  });

  it('starts with the proposals that are on by default: the safe ones, and old installers', () => {
    const s = run(start, ...scanned(C, 1));
    expect(planOf(s, C)).toEqual(defaults);
    expect(defaults).toContain('w-installers');
    expect(defaults).toContain('w-node');
  });

  it('adds and removes proposals on its own drive only', () => {
    let s = run(start, ...scanned(C, 1), { type: 'switch', mountPoint: D.mountPoint }, ...scanned(D, 2));
    s = run(s, plan(C, 'w-node', false), plan(C, 'w-node', false));
    expect(planOf(s, C)).toEqual(defaults.filter((id) => id !== 'w-node'));
    expect(planOf(s, D)).toEqual(defaults); // D:'s plan is its own

    s = run(s, plan(D, 'w-temp', false), plan(C, 'w-node', true));
    expect(planOf(s, C)).toContain('w-node');
    expect(planOf(s, D)).not.toContain('w-temp');
    expect(planOf(s, C)).toContain('w-temp');
  });

  it('ignores a proposal the drive doesn’t have, a drive with no map, and a change that changes nothing', () => {
    const s = run(start, ...scanned(C, 1));
    expect(run(s, plan(C, 'm-trash', true))).toBe(s);
    expect(run(s, plan(D, 'w-node', false))).toBe(s);
    expect(run(s, plan(C, 'w-node', true))).toBe(s);
  });

  it('starts afresh when the drive is scanned again', () => {
    const s = run(start, ...scanned(C, 1), plan(C, 'w-node', false), ...scanned(C, 2));
    expect(planOf(s, C)).toEqual(defaults);
  });

  it('adds up what a drive could reclaim from its delete proposals', () => {
    const total = fixtures.windows.proposals.reduce((sum, p) => sum + p.bytes, 0);
    expect(reclaimableBytes(fixtures.windows.proposals)).toBe(total);
    const s = run(start, ...scanned(C, 1));
    expect(reclaimableOf(scanOf(s, C.mountPoint))).toBe(total);
    expect(reclaimableOf(scanOf(s, D.mountPoint))).toBe(0);
  });
});

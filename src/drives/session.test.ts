import { describe, expect, it } from 'vitest';
import { fixtures, type Overview, type VolumeInfo } from '../bridge';
import { plan as samplePlan, report as sampleReport } from '../test/run';
import {
  EMPTY,
  reclaimableBytes,
  reclaimableOf,
  reduce,
  reportOf,
  scanOf,
  shownVolume,
  type Action,
  type Session,
} from './session';

const [C, D, E] = fixtures.windows.volumes as [VolumeInfo, VolumeInfo, VolumeInfo];

function mapOf(volume: VolumeInfo): Overview {
  return { ...fixtures.windows.overview, volume };
}

/** A finished scan of `volume`: its map and the fixture's proposals. */
const done = (volume: VolumeInfo) => ({ status: 'done' as const, scan: 1, overview: mapOf(volume), proposals: fixtures.windows.proposals, tidy: [] });

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

describe('each drive’s run and summary', () => {
  const runOf = (s: Session, volume: VolumeInfo) => {
    const scan = scanOf(s, volume.mountPoint);
    return scan.phase === 'done' ? scan.planRun : undefined;
  };
  const planFor = (volume: VolumeInfo, id = 'plan-1') => ({ ...samplePlan, id, mountPoint: volume.mountPoint });
  const started = (volume: VolumeInfo, id = 'plan-1'): Action => ({
    type: 'runStarted',
    mountPoint: volume.mountPoint,
    plan: planFor(volume, id),
  });
  const ended = (volume: VolumeInfo, id = 'plan-1'): Action => ({
    type: 'runEnded',
    mountPoint: volume.mountPoint,
    planId: id,
    report: { ...sampleReport, planId: id, mountPoint: volume.mountPoint },
  });
  const events = (volume: VolumeInfo, id = 'plan-1'): Action => ({
    type: 'runEvents',
    mountPoint: volume.mountPoint,
    planId: id,
    events: [
      { kind: 'stepStarted', step: 0, id: 'w-node' },
      { kind: 'progress', step: 0, freedBytes: 3e9, deletedItems: 10, totalFreedBytes: 3e9, currentPath: 'C:\\a' },
    ],
  });

  it('locks drive switching and scanning while the plan runs, and follows its events', () => {
    const s = run(start, ...scanned(C, 1), started(C), events(C));
    expect(s.locked).toBe(true);
    const r = runOf(s, C);
    expect(r?.phase).toBe('running');
    expect(r?.phase === 'running' && r.view.freedBytes).toBe(3e9);
    expect(run(s, { type: 'switch', mountPoint: D.mountPoint })).toBe(s);
    expect(run(s, { type: 'showScanScreen' })).toBe(s);
    expect(run(s, { type: 'scanStarted', mountPoint: C.mountPoint, run: 2 })).toBe(s);
    // Its proposals are spent: the plan can't change, and there is nothing left to reclaim.
    expect(run(s, { type: 'plan', mountPoint: C.mountPoint, id: 'w-node', on: false })).toBe(s);
    expect(reclaimableOf(scanOf(s, C.mountPoint))).toBe(0);
  });

  it('marks Stop, then ends with the report, unlocked, and opens the summary on "See what changed"', () => {
    let s = run(start, ...scanned(C, 1), started(C), { type: 'runStopping', mountPoint: C.mountPoint });
    const r = runOf(s, C);
    expect(r?.phase === 'running' && r.stopping).toBe(true);

    s = run(s, ended(C));
    expect(s.locked).toBe(false);
    expect(runOf(s, C)?.phase).toBe('ended');
    expect(reportOf(scanOf(s, C.mountPoint))?.freedBytes).toBe(sampleReport.freedBytes);
    // The drive's figures are its own from the end of the run.
    expect(shownVolume(s, C).freeBytes).toBe(sampleReport.freeAfter);

    s = run(s, { type: 'showSummary', mountPoint: C.mountPoint });
    expect(runOf(s, C)?.phase).toBe('summary');
    expect(s.screen).toBe('map');
  });

  it('ignores another plan’s events and ending, and a start while one runs', () => {
    const s = run(start, ...scanned(C, 1), started(C));
    expect(run(s, events(C, 'plan-9'))).toBe(s);
    expect(run(s, started(C, 'plan-2'))).toBe(s);
    const other = run(s, ended(C, 'plan-9'));
    expect(runOf(other, C)?.phase).toBe('running');
  });

  it('says why a run failed, unlocks, and goes back to the plan', () => {
    let s = run(start, ...scanned(C, 1), started(C), {
      type: 'runFailed',
      mountPoint: C.mountPoint,
      planId: 'plan-1',
      message: 'a plan is already running',
    });
    expect(s.locked).toBe(false);
    const r = runOf(s, C);
    expect(r?.phase === 'failed' && r.message).toBe('a plan is already running');
    expect(reclaimableOf(scanOf(s, C.mountPoint))).toBeGreaterThan(0);
    s = run(s, { type: 'leaveRun', mountPoint: C.mountPoint });
    expect(runOf(s, C)).toBeNull();
  });

  it('keeps a drive’s summary until it is scanned again, and other drives keep theirs', () => {
    let s = run(start, ...scanned(C, 1), started(C), ended(C), { type: 'showSummary', mountPoint: C.mountPoint });
    s = run(s, { type: 'switch', mountPoint: D.mountPoint }, ...scanned(D, 2));
    expect(runOf(s, D)).toBeNull();
    s = run(s, started(D, 'plan-2'), ended(D, 'plan-2'), { type: 'showSummary', mountPoint: D.mountPoint });

    // Back to C:: its summary is still there.
    s = run(s, { type: 'switch', mountPoint: C.mountPoint });
    expect(s.screen).toBe('map');
    expect(runOf(s, C)?.phase).toBe('summary');
    expect(runOf(s, D)?.phase).toBe('summary');

    // Scanning C: again starts afresh; D: keeps its summary.
    s = run(s, { type: 'showScanScreen' }, ...scanned(C, 3));
    expect(runOf(s, C)).toBeNull();
    expect(reclaimableOf(scanOf(s, C.mountPoint))).toBeGreaterThan(0);
    expect(runOf(s, D)?.phase).toBe('summary');
  });

  it('unlocks when a run ends even if its drive went away meanwhile', () => {
    const s = run(start, { type: 'switch', mountPoint: E.mountPoint }, ...scanned(E, 1), started(E), listed([C, D]));
    expect(s.locked).toBe(true);
    expect(run(s, ended(E)).locked).toBe(false);
  });
});

describe('the duplicate search after a scan', () => {
  const found = {
    status: 'done' as const,
    overview: { ...mapOf(C), freeBytes: 1 },
    proposals: [
      ...fixtures.windows.proposals,
      { ...fixtures.windows.proposals[0]!, id: 'duplicates', title: 'Duplicate files', defaultOn: false },
    ],
    tidy: [],
    duplicates: {
      proposal: 'duplicates',
      groups: [],
      folders: ['Pictures'],
      compared: 2,
      hardLinked: 0,
      changed: 0,
      notLocal: 0,
      cloned: 0,
    },
  };

  it('starts checking when the scan is done, and redraws the map when it ends, keeping the plan', () => {
    const s = run(start, ...scanned(C, 1));
    const scan = scanOf(s, C.mountPoint);
    expect(scan.phase === 'done' && scan.duplicates).toEqual({ phase: 'checking', progress: null });
    const plan = scan.phase === 'done' ? scan.plan : [];

    const progress = { stage: 'starts' as const, files: 10, done: 3, percent: 4 };
    const going = run(s, { type: 'dupsProgress', mountPoint: C.mountPoint, run: 1, progress });
    const g = scanOf(going, C.mountPoint);
    expect(g.phase === 'done' && g.duplicates).toEqual({ phase: 'checking', progress });

    const ended = run(going, { type: 'dupsEnded', mountPoint: C.mountPoint, run: 1, outcome: found });
    const e = scanOf(ended, C.mountPoint);
    expect(e.phase === 'done' && e.overview.freeBytes).toBe(1);
    expect(e.phase === 'done' && e.proposals.some((p) => p.id === 'duplicates')).toBe(true);
    expect(e.phase === 'done' && e.plan).toEqual(plan);
    expect(e.phase === 'done' && e.duplicates.phase).toBe('done');
  });

  it('ignores a search of an earlier scan, and one that ended after its plan ran', () => {
    const s = run(start, ...scanned(C, 1), ...scanned(C, 2));
    expect(run(s, { type: 'dupsEnded', mountPoint: C.mountPoint, run: 1, outcome: found })).toBe(s);

    const ran = run(s, { type: 'runStarted', mountPoint: C.mountPoint, plan: samplePlan });
    const r = scanOf(ran, C.mountPoint);
    expect(r.phase === 'done' && r.duplicates.phase).toBe('stopped');
    expect(run(ran, { type: 'dupsEnded', mountPoint: C.mountPoint, run: 2, outcome: found })).toBe(ran);
  });

  it('takes the sorts the search marked, and keeps a ticked sort in the plan', () => {
    const tidy = fixtures.windows.tidy;
    expect(tidy.length).toBeGreaterThan(0);
    const scanEnded = { ...done(C), tidy };
    let s = run(
      start,
      { type: 'scanStarted', mountPoint: C.mountPoint, run: 1 },
      { type: 'scanEnded', mountPoint: C.mountPoint, run: 1, outcome: scanEnded, at: 1 },
    );
    s = run(s, { type: 'plan', mountPoint: C.mountPoint, id: tidy[0]!.id, on: true });
    const marked = tidy.map((t, i) =>
      i === 0 ? { ...t, moves: t.moves.map((m) => ({ ...m, deletedBy: [...m.deletedBy, 'duplicates'] })) } : t,
    );
    const ended = run(s, { type: 'dupsEnded', mountPoint: C.mountPoint, run: 1, outcome: { ...found, tidy: marked } });
    const e = scanOf(ended, C.mountPoint);
    expect(e.phase === 'done' && e.tidy[0]!.moves.every((m) => m.deletedBy.includes('duplicates'))).toBe(true);
    expect(e.phase === 'done' && e.plan.includes(tidy[0]!.id)).toBe(true);
  });

  it('keeps the groups the user unticks, and lets them be ticked again', () => {
    const s = run(start, ...scanned(C, 1));
    const off = run(s, { type: 'skipGroup', mountPoint: C.mountPoint, id: 'g3', skipped: true });
    const o = scanOf(off, C.mountPoint);
    expect(o.phase === 'done' && o.skippedGroups).toEqual(['g3']);
    expect(run(off, { type: 'skipGroup', mountPoint: C.mountPoint, id: 'g3', skipped: true })).toBe(off);
    const on = run(off, { type: 'skipGroup', mountPoint: C.mountPoint, id: 'g3', skipped: false });
    const n = scanOf(on, C.mountPoint);
    expect(n.phase === 'done' && n.skippedGroups).toEqual([]);
  });
});

describe('Claude’s answers on a drive’s scan', () => {
  const planOf = (s: Session) => {
    const scan = scanOf(s, C.mountPoint);
    return scan.phase === 'done' ? scan.plan : null;
  };
  const claudeItem = { ...fixtures.windows.proposals[0]!, id: 'claude-x', source: 'claude' as const, defaultOn: false };
  const refreshed = {
    revision: 1,
    overview: mapOf(C),
    proposals: [...fixtures.windows.proposals.filter((p) => p.id !== 'w-temp'), claudeItem],
    tidy: [],
    tidyOthers: [],
  };

  it('shows the proposals Claude’s answer sent, unticked, and the plan keeps what is still there', () => {
    const before = run(start, ...scanned(C, 1));
    const s = run(before, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed });
    const scan = scanOf(s, C.mountPoint);
    expect(scan.phase === 'done' && scan.proposals.some((p) => p.id === 'claude-x')).toBe(true);
    expect(planOf(s)).not.toContain('claude-x');
    expect(planOf(s)).toEqual(planOf(before)!.filter((id) => id !== 'w-temp'));
  });

  it('sets the whole plan (Let Claude pick, Keep mine), leaving out what the scan doesn’t have', () => {
    const s = run(start, ...scanned(C, 1), { type: 'setPlan', mountPoint: C.mountPoint, run: 1, ids: ['w-node', 'w-node', 'nope'] });
    expect(planOf(s)).toEqual(['w-node']);
  });

  it('counts each change of what is in the plan, however it changes, and nothing else', () => {
    const versionOf = (s: ReturnType<typeof run>) => {
      const scan = scanOf(s, C.mountPoint);
      return scan.phase === 'done' ? scan.planVersion : null;
    };
    const s0 = run(start, ...scanned(C, 1));
    expect(versionOf(s0)).toBe(0);
    const s1 = run(s0, { type: 'plan', mountPoint: C.mountPoint, id: 'w-node', on: false });
    expect(versionOf(s1)).toBe(1);
    // Ticking what is ticked changes nothing; neither does a whole plan with the same items in another order.
    expect(run(s1, { type: 'plan', mountPoint: C.mountPoint, id: 'w-temp', on: true })).toBe(s1);
    const same = run(s1, { type: 'setPlan', mountPoint: C.mountPoint, run: 1, ids: [...planOf(s1)!].reverse() });
    expect(versionOf(same)).toBe(1);
    const s2 = run(s1, { type: 'setPlan', mountPoint: C.mountPoint, run: 1, ids: ['w-node'] });
    expect(versionOf(s2)).toBe(2);
    // Claude's items arriving unticked change the map, not the plan; a plan item they drop does.
    const added = { ...refreshed, proposals: [...fixtures.windows.proposals, claudeItem] };
    expect(versionOf(run(s2, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed: added }))).toBe(2);
    expect(versionOf(run(s1, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed }))).toBe(2);
  });

  it('sets a whole plan from a version only while the plan is still at it: an edit made meanwhile stays', () => {
    const s0 = run(start, ...scanned(C, 1));
    const picked = run(s0, { type: 'setPlan', mountPoint: C.mountPoint, run: 1, ids: ['w-node'], from: 0 });
    expect(planOf(picked)).toEqual(['w-node']);
    const edited = run(s0, { type: 'plan', mountPoint: C.mountPoint, id: 'w-temp', on: false });
    expect(run(edited, { type: 'setPlan', mountPoint: C.mountPoint, run: 1, ids: ['w-node'], from: 0 })).toBe(edited);
  });

  it('never puts back an older map: a slow reply with a lower revision than the one shown is ignored', () => {
    const s0 = run(start, ...scanned(C, 1));
    const newer = { ...refreshed, revision: 3 };
    const shown = run(s0, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed: newer });
    const doneScan = scanOf(shown, C.mountPoint);
    expect(doneScan.phase === 'done' && doneScan.claudeRevision).toBe(3);
    // Keep mine's reply, made before the newer answer, arrives late without Claude's item: nothing changes.
    const older = { ...refreshed, revision: 2, proposals: fixtures.windows.proposals };
    expect(run(shown, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed: older })).toBe(shown);
    expect(run(shown, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed: newer })).toBe(shown);
    const latest = run(shown, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed: { ...older, revision: 4 } });
    expect(latest).not.toBe(shown);
  });

  it('ignores an answer about an earlier scan, or one that comes after the plan ran', () => {
    const later = run(start, ...scanned(C, 1), ...scanned(C, 2));
    expect(run(later, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 1, refreshed })).toBe(later);
    expect(run(later, { type: 'setPlan', mountPoint: C.mountPoint, run: 1, ids: [] })).toBe(later);
    const ran = run(later, { type: 'runStarted', mountPoint: C.mountPoint, plan: { ...samplePlan, mountPoint: C.mountPoint } });
    expect(run(ran, { type: 'claudeApplied', mountPoint: C.mountPoint, run: 2, refreshed })).toBe(ran);
  });
});

import type { Fixture, PastRun, RunSkip } from './fixtures';
import { makePlan, pastReport, play, type MockPlan, type Playback } from './mockRun';
import type {
  Bridge,
  CategoryTotal,
  MapFolder,
  Overview,
  Proposal,
  RunListing,
  RunReport,
  ScanOutcome,
  ScanUpdate,
  VolumeInfo,
} from './types';

export interface MockOptions {
  /** Answer after this many milliseconds, to see loading states. Default: next tick. */
  delayMs?: number;
  /** Make every call fail with this message, to see error states. */
  fail?: string;
  /** How long a mock scan takes. Default: 4 s, like the prototype. Zero finishes on the next tick. */
  scanMs?: number;
  /** Override the fixture's Full Disk Access answer. */
  fullDiskAccess?: Fixture['fullDiskAccess'];
  /** Unplug the last drive in the list (the USB stick on Windows) this long after the bridge is made. */
  unplugMs?: number;
  /** How long a mock run of a plan takes. Default: two seconds a step. Zero finishes on the next tick. */
  runMs?: number;
  /** How many kept runs the history says it couldn't read. Default: none. */
  unreadableRuns?: number;
  /**
   * Runs also report the fixture's reorganizations as done, to preview the finish's Moved slide before
   * reorganizations arrive (ticket 08).
   */
  moves?: boolean;
  /** Start with the fixture's past runs in the history. Default: yes. */
  pastRuns?: boolean;
}

/** A mock run's length when `runMs` isn't given, per step. */
const STEP_MS = 2000;

/** The mock bridge, plus what tests need to act out the world around the app. */
export interface MockBridge extends Bridge {
  /** Takes a drive away, as if it were unplugged: listings leave it out from now on. */
  unplug(mountPoint: string): void;
}

/** The scan's progress curve: fast at first, slowing as it nears the end, the way a real walk feels. */
const ease = (t: number) => 1 - Math.pow(1 - t, 2.4);

/**
 * A bridge that answers from fixture data. Used in a plain browser (`npm run dev`), in tests and in screenshots.
 * Its scan replays the fixture's map over `scanMs`: counts climb, the path ticker moves, and categories are found
 * one after another, the way the prototype's scan reveals them, then it answers with the map and the proposals.
 * Another drive gets the system drive's map and proposals scaled to its size, with their paths moved onto it. Like
 * the app, it keeps a map per drive and runs one scan at a time.
 *
 * Its plans come from a drive's proposals, and a run plays one back over `runMs` with the events the core sends:
 * the fixture's skips happen on the way (a place changed since the scan, which planning leaves out; one in use),
 * Stop is honoured, and the report's free-space figures are a little off what was freed, as other apps write too.
 * Each scan runs at most one plan; a run's report goes in the history and the drive's figures change. The history
 * starts with the fixture's past runs.
 */
export function createMockBridge(fixture: Fixture, options: MockOptions = {}): MockBridge {
  const answer = <T>(value: T) =>
    new Promise<T>((resolve, reject) => {
      setTimeout(() => {
        if (options.fail !== undefined) reject(new Error(options.fail));
        else resolve(structuredClone(value));
      }, options.delayMs ?? 0);
    });
  /** Fails after the delay, like every answer when `fail` is set. */
  const failure = <T>(message: string) =>
    new Promise<T>((_, reject) => setTimeout(() => reject(new Error(message)), options.delayMs ?? 0));
  const system = fixture.volumes[0]!;
  let volumes = [...fixture.volumes];
  /** Drives with a finished scan, which `browse` can open, with the map, proposals and skips their runs work from. */
  const mapped = new Map<string, { map: Overview; proposals: Proposal[]; skips: RunSkip[] }>();
  let running: { mountPoint: string; timer: number; finish: (outcome: ScanOutcome) => void } | null = null;
  /** Plans made and not yet run, by id. A new scan of their drive drops them. */
  const plans = new Map<string, MockPlan & { mountPoint: string }>();
  /** Drives whose scan has run its plan. */
  const ran = new Set<string>();
  let playing: Playback | null = null;
  const history: RunReport[] = options.pastRuns === false ? [] : fixture.history.map((past, i) => pastRun(fixture, past, i));
  let made = 0;
  const forget = (mountPoint: string) => {
    mapped.delete(mountPoint);
    ran.delete(mountPoint);
    for (const [id, p] of plans) if (p.mountPoint === mountPoint) plans.delete(id);
  };

  const unplug = (mountPoint: string) => {
    volumes = volumes.filter((v) => v.mountPoint !== mountPoint);
  };
  if (options.unplugMs !== undefined) {
    const last = fixture.volumes.at(-1);
    if (last && !last.system) setTimeout(() => unplug(last.mountPoint), options.unplugMs);
  }

  return {
    platform: fixture.platform,
    machineName: () => answer(fixture.machineName),
    listVolumes: () => answer(volumes),
    forgetDrive: (mountPoint) => {
      if (running?.mountPoint === mountPoint) running.finish({ status: 'cancelled' });
      forget(mountPoint);
      return answer(undefined);
    },
    fullDiskAccess: () => answer(options.fullDiskAccess ?? fixture.fullDiskAccess),
    openFullDiskAccessSettings: () => answer(undefined),
    scan: (mountPoint, onUpdate) => {
      if (options.fail !== undefined) return answer<ScanOutcome>({ status: 'cancelled' });
      if (playing) return Promise.reject(new Error('a plan is running: scan once it has finished'));
      running?.finish({ status: 'cancelled' });
      forget(mountPoint);
      const volume = volumes.find((v) => v.mountPoint === mountPoint);
      if (!volume) return Promise.reject(new Error(`could not scan ${mountPoint}: the drive is not connected`));
      const map = onDrive(structuredClone(fixture.overview), system, volume);
      const proposals = proposalsOnDrive(structuredClone(fixture.proposals), system, volume);
      const skips = skipsOnDrive(structuredClone(fixture.runSkips), system, volume);
      const paths = map.categories.flatMap((c) => c.items.map((i) => i.path)).filter(Boolean);
      // Interleaved, so the ticker jumps around the disk the way a real walk does.
      const ticker = paths.map((p, i) => paths[(i * 7) % paths.length] ?? p);
      const total = options.scanMs ?? 4000;
      const started = Date.now();
      return new Promise<ScanOutcome>((resolve) => {
        const finish = (outcome: ScanOutcome) => {
          if (running) clearInterval(running.timer);
          running = null;
          if (outcome.status === 'done') mapped.set(mountPoint, { map: structuredClone(map), proposals: structuredClone(proposals), skips });
          resolve(outcome);
        };
        const tick = () => {
          const t = total <= 0 ? 1 : Math.min(1, (Date.now() - started) / total);
          onUpdate(update(map, ticker, t, Date.now() - started));
          if (t >= 1) finish({ status: 'done', overview: map, proposals });
        };
        running = { mountPoint, timer: window.setInterval(tick, total <= 0 ? 0 : 100), finish };
      });
    },
    cancelScan: () => {
      running?.finish({ status: 'cancelled' });
      return answer(undefined);
    },
    browse: (mountPoint, id) => {
      if (!mapped.has(mountPoint)) return Promise.reject(new Error(`there is no finished scan of ${mountPoint} to explore`));
      const folder = fixture.folders[id];
      const volume = volumes.find((v) => v.mountPoint === mountPoint) ?? system;
      return folder ? answer(folderOnDrive(folder, system, volume)) : Promise.reject(new Error(`there is no folder ${id} in this scan`));
    },
    buildPlan: (mountPoint, ids) => {
      if (options.fail !== undefined) return failure(options.fail);
      const scan = mapped.get(mountPoint);
      if (!scan) return failure(`there is no finished scan of ${mountPoint} to plan from`);
      if (ran.has(mountPoint)) return failure(`this scan of ${mountPoint} already ran a plan: scan again first`);
      let plan: MockPlan;
      try {
        plan = makePlan(`plan-${++made}`, mountPoint, scan.map, scan.proposals, ids, scan.skips);
      } catch (error) {
        return failure(error instanceof Error ? error.message : String(error));
      }
      plans.set(plan.summary.id, { ...plan, mountPoint });
      return answer(plan.summary);
    },
    runPlan: (mountPoint, planId, onEvent) => {
      if (options.fail !== undefined) return failure(options.fail);
      const plan = plans.get(planId);
      if (!plan || plan.mountPoint !== mountPoint) return Promise.reject(new Error(`there is no plan ${planId} for ${mountPoint}`));
      if (playing) return Promise.reject(new Error('a plan is already running'));
      if (ran.has(mountPoint)) return Promise.reject(new Error(`this scan of ${mountPoint} already ran a plan: scan again first`));
      const volume = volumes.find((v) => v.mountPoint === mountPoint);
      if (!volume) return Promise.reject(new Error(`could not run the plan: ${mountPoint} is not connected`));
      ran.add(mountPoint);
      for (const [id, p] of plans) if (p.mountPoint === mountPoint) plans.delete(id);
      const totalMs = options.runMs ?? STEP_MS * plan.steps.length;
      return new Promise<RunReport>((resolve) => {
        setTimeout(() => {
          const moves = options.moves ? structuredClone(fixture.moves) : [];
          const playback = play({ id: `run-${++made}`, plan, volume, totalMs, moves }, onEvent);
          playing = playback;
          void playback.done.then((report) => {
            playing = null;
            history.unshift(report);
            volumes = volumes.map((v) =>
              v.mountPoint === mountPoint ? { ...v, usedBytes: report.usedAfter, freeBytes: report.freeAfter } : v,
            );
            resolve(structuredClone(report));
          });
        }, options.delayMs ?? 0);
      });
    },
    cancelRun: () => {
      playing?.cancel();
      return answer(undefined);
    },
    listRuns: () => answer({ runs: history.map(listing), unreadable: options.unreadableRuns ?? 0 }),
    openRun: (id) => {
      const run = history.find((r) => r.id === id);
      return run ? answer(run) : failure(`This run can’t be opened: the history has no run ${id}.`);
    },
    // Browser fixtures never open host apps or claim an external launch succeeded.
    openHint: () => failure('Steam can’t be opened from this fixture preview. No app was launched.'),
    unplug,
  };
}

/** A run in the history list, as the core lists it. */
function listing(r: RunReport): RunListing {
  return {
    id: r.id,
    mountPoint: r.mountPoint,
    volumeName: r.volumeName,
    volumeId: r.map.volume.id,
    startedAt: r.startedAt,
    freedBytes: r.freedBytes,
    deletedItems: r.deletedItems,
    movedItems: r.movedItems,
    cancelled: r.cancelled,
  };
}

/** One of the fixture's past runs, as its kept report: its drive's map and proposals, run to the end. */
function pastRun(fixture: Fixture, past: PastRun, i: number): RunReport {
  const system = fixture.volumes[0]!;
  const volume = fixture.volumes.find((v) => v.mountPoint === past.mountPoint) ?? system;
  const map = onDrive(structuredClone(fixture.overview), system, volume);
  const proposals = proposalsOnDrive(structuredClone(fixture.proposals), system, volume);
  const skips = skipsOnDrive(structuredClone(fixture.runSkips), system, volume).filter((s) => s.when === 'run');
  const plan = makePlan(`plan-past-${i + 1}`, volume.mountPoint, map, proposals, past.proposals, skips);
  const day = new Date();
  day.setHours(past.hour, 0, 0, 0);
  const startedAt = day.getTime() - past.daysAgo * 86_400_000 + i * 7 * 60_000;
  return pastReport(`run-past-${i + 1}`, plan, volume, startedAt, past.durationMs);
}

/** The system drive's skips as another drive's: scaled like its proposals, their paths moved onto it. */
function skipsOnDrive(skips: RunSkip[], system: VolumeInfo, volume: VolumeInfo): RunSkip[] {
  if (volume.mountPoint === system.mountPoint) return skips;
  const scale = volume.usedBytes / system.usedBytes;
  for (const s of skips) {
    s.skip.bytes *= scale;
    s.skip.path = moved(s.skip.path, system, volume);
  }
  return skips;
}

/** The system drive's map as another drive's: scaled to that drive's used space, its paths moved onto it. */
function onDrive(map: ScanOutcomeMap, system: VolumeInfo, volume: VolumeInfo): ScanOutcomeMap {
  if (volume.mountPoint === system.mountPoint) return map;
  const scale = volume.usedBytes / system.usedBytes;
  map.volume = volume;
  map.freeBytes = volume.freeBytes;
  for (const c of map.categories) {
    c.bytes *= scale;
    for (const i of c.items) {
      i.bytes *= scale;
      i.path = moved(i.path, system, volume);
    }
  }
  return map;
}

/** The system drive's proposals as another drive's: scaled like its map, their sample paths moved onto it. */
function proposalsOnDrive(proposals: Proposal[], system: VolumeInfo, volume: VolumeInfo): Proposal[] {
  if (volume.mountPoint === system.mountPoint) return proposals;
  const scale = volume.usedBytes / system.usedBytes;
  for (const p of proposals) {
    p.bytes *= scale;
    p.samplePaths = p.samplePaths.map((s) => moved(s, system, volume));
  }
  return proposals;
}

function folderOnDrive(folder: MapFolder, system: VolumeInfo, volume: VolumeInfo): MapFolder {
  if (volume.mountPoint === system.mountPoint) return folder;
  const scale = volume.usedBytes / system.usedBytes;
  const out = structuredClone(folder);
  for (const i of [out.item, ...out.children]) {
    i.bytes *= scale;
    i.path = moved(i.path, system, volume);
  }
  return out;
}

/** `C:\Users\alex` on D: is `D:\Users\alex`; `/Users/alex` on the Samsung T7 is `/Volumes/Samsung T7/Users/alex`. */
function moved(path: string, system: VolumeInfo, volume: VolumeInfo): string {
  if (!path.startsWith(system.mountPoint)) return path;
  const sep = volume.mountPoint.endsWith('\\') || volume.mountPoint.endsWith('/') ? '' : '/';
  return volume.mountPoint + sep + path.slice(system.mountPoint.length);
}

/** The HUD's numbers `t` of the way through a replayed scan. */
function update(map: ScanOutcomeMap, ticker: string[], t: number, elapsedMs: number): ScanUpdate {
  const e = ease(t);
  const n = map.categories.length;
  const categories: CategoryTotal[] = map.categories
    .map((c, i) => {
      // Each category is found in turn, then fills up.
      const found = 0.06 + (i / n) * 0.8;
      const share = t < found ? 0 : Math.min(1, (t - found) / 0.14 + 0.15);
      const hidden = c.items.filter((item) => item.kind === 'hidden').reduce((s, item) => s + item.bytes, 0);
      return { id: c.id, name: c.name, protected: c.protected, bytes: (c.bytes - hidden) * share };
    })
    .filter((c) => c.bytes > 0);
  return {
    files: Math.round(e * map.scan.files),
    folders: Math.round(e * map.scan.folders),
    bytes: categories.reduce((s, c) => s + c.bytes, 0),
    unreadable: Math.round(e * map.scan.unreadable),
    currentPath: ticker[Math.floor(t * 70) % Math.max(1, ticker.length)] ?? '',
    elapsedMs: Math.round(elapsedMs),
    categories,
  };
}

type ScanOutcomeMap = Extract<ScanOutcome, { status: 'done' }>['overview'];

import type { Fixture, PastRun, RunSkip } from './fixtures';
import { createMockClaude, type MockClaudeOptions, type MockClaudeScan } from './mockClaude';
import { coverDuplicates, DUPLICATES, mockDuplicates, type MockDuplicates } from './mockDuplicates';
import { askingUpdate, fallbackFor, fastOverview, fastUpdate, UAC_MS, type MockFast } from './mockFast';
import { makePlan, mockUndo, pastReport, play, type MockPlan, type Playback } from './mockRun';
import type {
  Bridge,
  AdvisorStatus,
  CategoryTotal,
  DuplicateProgress,
  FastFallback,
  DuplicatesOutcome,
  MapFolder,
  Proposal,
  RunListing,
  RunReport,
  ScanOutcome,
  ScanUpdate,
  VolumeInfo,
} from './types';

export interface MockOptions extends MockClaudeOptions {
  /** Advisor-only browser preview: native fixture bypass, or a failed read. Never enables a provider. */
  advisor?: 'notChecked' | 'error';
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
  /**
   * How long the mock duplicate finder takes after a scan, in the background. Default: 6 s. Zero finishes on the
   * next tick.
   */
  dupsMs?: number;
  /** How many kept runs the history says it couldn't read. Default: none. */
  unreadableRuns?: number;
  /**
   * Cleanup runs whose plan has no Tidy up of its own also report the fixture's preview reorganizations as done, to
   * see the finish's Moved slide. Reminder-only runs never inject moves.
   */
  moves?: boolean;
  /** Start with the fixture's past runs in the history. Default: yes. */
  pastRuns?: boolean;
  /**
   * How a fast scan of an NTFS drive goes: `ok` reads the file index (the default); `declined` (Windows' admin prompt
   * is answered No), `failed` (the helper doesn't start) and `unreadable` (the index can't be read) run the normal
   * scan, saying why. A drive that isn't NTFS always gets the normal scan, with no prompt.
   */
  fast?: MockFast;
  /** How long the mock's admin prompt is up before a fast scan. Default: 1.2 s. */
  uacMs?: number;
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
  /**
   * Drives with a finished scan, which `browse` can open, with the map, proposals, sorts and skips their runs work
   * from, and the duplicates found on them once the mock finder is done.
   */
  const mapped = new Map<
    string,
    MockClaudeScan & {
      skips: RunSkip[];
      dups: MockDuplicates | null;
    }
  >();
  /** The last scan id given out, on any drive: each finished scan gets the next, as the core's generations go. */
  let scans = 0;
  // Claude answers from the drive's scan `scan` only, and changes it the way the core does (its items, its layouts).
  const claude = createMockClaude(fixture.platform, options, (mountPoint, scan) => {
    const s = mapped.get(mountPoint);
    return ran.has(mountPoint) || s?.scan !== scan ? null : s;
  }, () => {
    for (const s of mapped.values()) for (const c of s.chats.values()) c.ended = true;
  });
  /** The mock duplicate finder running now, on one drive. */
  let finding: { mountPoint: string; timer: number; finish: (outcome: DuplicatesOutcome) => void } | null = null;
  const stopFinding = (mountPoint?: string) => {
    if (finding && (mountPoint === undefined || finding.mountPoint === mountPoint)) finding.finish({ status: 'cancelled' });
  };
  let running: { mountPoint: string; timer: number; finish: (outcome: ScanOutcome) => void } | null = null;
  /** Plans made and not yet run, by id. A new scan of their drive drops them. */
  const plans = new Map<string, MockPlan & { mountPoint: string }>();
  /** Drives whose scan has run its plan. */
  const ran = new Set<string>();
  let playing: Playback | null = null;
  const history: RunReport[] = options.pastRuns === false ? [] : fixture.history.map((past, i) => pastRun(fixture, past, i));
  let made = 0;
  const forget = (mountPoint: string) => {
    stopFinding(mountPoint);
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
    advisorStatus: () => options.advisor === 'error' ? failure<AdvisorStatus>('Fixture app availability is unavailable.') : answer({
      ...fixture.advisorStatus,
      enabled: false,
      discovery: 'fixture' as const,
      providers: options.advisor === 'notChecked'
        ? fixture.advisorStatus.providers.map((p) => ({ ...p, availability: { state: 'notChecked' as const } }))
        : fixture.advisorStatus.providers,
    }),
    listVolumes: () => answer(volumes),
    forgetDrive: (mountPoint) => {
      if (running?.mountPoint === mountPoint) running.finish({ status: 'cancelled' });
      forget(mountPoint);
      return answer(undefined);
    },
    fullDiskAccess: () => answer(options.fullDiskAccess ?? fixture.fullDiskAccess),
    openFullDiskAccessSettings: () => answer(undefined),
    scan: (mountPoint, onUpdate, scanOptions) => {
      if (options.fail !== undefined) return answer<ScanOutcome>({ status: 'cancelled' });
      if (playing) return Promise.reject(new Error('a plan is running: scan once it has finished'));
      running?.finish({ status: 'cancelled' });
      stopFinding();
      forget(mountPoint);
      const volume = volumes.find((v) => v.mountPoint === mountPoint);
      if (!volume) return Promise.reject(new Error(`could not scan ${mountPoint}: the drive is not connected`));
      const normal = onDrive(structuredClone(fixture.overview), system, volume);
      // A fast scan asked for: Windows' prompt first (NTFS only), then the index, or the normal scan saying why.
      const fallback: FastFallback | null = scanOptions?.fast ? fallbackFor(volume, options.fast ?? 'ok') : null;
      const fast = !!scanOptions?.fast && fallback === null;
      const asking = scanOptions?.fast && fallback !== 'notNtfs' ? (options.uacMs ?? UAC_MS) : 0;
      const map = fast ? fastOverview(normal, volume.usedBytes / system.usedBytes) : normal;
      if (fallback) map.scan.fallback = fallback;
      const proposals = proposalsOnDrive(structuredClone(fixture.proposals), system, volume);
      const skips = skipsOnDrive(structuredClone(fixture.runSkips), system, volume);
      // Tidy up sorts the home's folders: only the system drive has them.
      const tidy = volume.mountPoint === system.mountPoint ? structuredClone(fixture.tidy) : [];
      const paths = map.categories.flatMap((c) => c.items.map((i) => i.path)).filter(Boolean);
      // Interleaved, so the ticker jumps around the disk the way a real walk does.
      const ticker = paths.map((p, i) => paths[(i * 7) % paths.length] ?? p);
      // A fast scan takes half the time of a normal one here, so its stages can still be watched.
      const total = (options.scanMs ?? 4000) * (fast ? 0.5 : 1);
      const started = Date.now();
      const scan = ++scans;
      return new Promise<ScanOutcome>((resolve) => {
        const finish = (outcome: ScanOutcome) => {
          if (running) clearInterval(running.timer);
          running = null;
          if (outcome.status === 'done')
            mapped.set(mountPoint, {
              scan,
              map: structuredClone(map),
              proposals: structuredClone(proposals),
              skips,
              tidy,
              dups: null,
              others: [],
              revision: 0,
              chats: new Map(),
            });
          resolve(outcome);
        };
        const tick = () => {
          const elapsed = Date.now() - started;
          if (elapsed < asking) {
            onUpdate(askingUpdate(elapsed));
            return;
          }
          const t = total <= 0 ? 1 : Math.min(1, (elapsed - asking) / total);
          onUpdate(fast ? fastUpdate(map, t, elapsed) : { ...update(map, ticker, t, elapsed), fallback });
          if (t >= 1) finish({ status: 'done', scan, overview: map, proposals, tidy });
        };
        running = { mountPoint, timer: window.setInterval(tick, total <= 0 && asking <= 0 ? 0 : 100), finish };
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
    findDuplicates: (mountPoint, onProgress) => {
      if (options.fail !== undefined) return failure(options.fail);
      const scan = mapped.get(mountPoint);
      if (!scan || ran.has(mountPoint)) return answer<DuplicatesOutcome>({ status: 'cancelled' });
      stopFinding();
      const total = options.dupsMs ?? 6000;
      const started = Date.now();
      const files = 1_204;
      return new Promise<DuplicatesOutcome>((resolve) => {
        const finish = (outcome: DuplicatesOutcome) => {
          if (finding) clearInterval(finding.timer);
          finding = null;
          resolve(structuredClone(outcome));
        };
        const tick = () => {
          const t = total <= 0 ? 1 : Math.min(1, (Date.now() - started) / total);
          const percent = Math.round(t * 100);
          const progress: DuplicateProgress =
            percent < 15
              ? { stage: 'starts', files, done: Math.round((files * percent) / 15), percent }
              : { stage: 'contents', files: 236, done: Math.round((236 * (percent - 15)) / 85), percent };
          onProgress(progress);
          if (t < 1) return;
          // Only the system drive has copies to find; elsewhere the search finds none.
          const found = mountPoint === system.mountPoint ? mockDuplicates(fixture.platform, scan.map, fixtureHome(fixture)) : null;
          if (found) {
            scan.map = found.overview;
            scan.proposals = [...scan.proposals.filter((p) => p.id !== DUPLICATES), found.proposal].sort((a, b) => b.bytes - a.bytes);
            scan.dups = found;
            // Delete wins over sort: a sort's move of an extra copy is marked as deleted by duplicates.
            scan.tidy = coverDuplicates(scan.tidy, found);
          }
          finish({
            status: 'done',
            overview: scan.map,
            proposals: scan.proposals,
            tidy: scan.tidy,
            duplicates: found?.duplicates ?? {
              proposal: null,
              groups: [],
              folders: [],
              compared: 0,
              hardLinked: 0,
              changed: 0,
              notLocal: 0,
              cloned: 0,
            },
          });
        };
        finding = { mountPoint, timer: window.setInterval(tick, total <= 0 ? 0 : 100), finish };
      });
    },
    buildPlan: (mountPoint, ids, skippedGroups = []) => {
      if (options.fail !== undefined) return failure(options.fail);
      const scan = mapped.get(mountPoint);
      if (!scan) return failure(`there is no finished scan of ${mountPoint} to plan from`);
      if (ran.has(mountPoint)) return failure(`this scan of ${mountPoint} already ran a plan: scan again first`);
      let plan: MockPlan;
      try {
        plan = makePlan(`plan-${++made}`, mountPoint, scan.map, scan.proposals, ids, scan.skips, scan.tidy, fixture.tidyScripts, {
          found: scan.dups,
          skipped: skippedGroups,
        });
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
      stopFinding(mountPoint);
      for (const [id, p] of plans) if (p.mountPoint === mountPoint) plans.delete(id);
      const totalMs = options.runMs ?? STEP_MS * plan.steps.length;
      return new Promise<RunReport>((resolve) => {
        setTimeout(() => {
          const moves = options.moves && plan.steps.length > 0 ? structuredClone(fixture.moves) : [];
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
    undoTidy: (runId, tidyId) => {
      if (options.fail !== undefined) return failure(options.fail);
      if (fixture.platform !== 'windows') return failure('Tidy up isn’t available on this computer.');
      if (playing) return failure('A plan is running. Undo when it has finished.');
      const run = history.find((r) => r.id === runId);
      if (!run) return failure(`This run can’t be opened: the history has no run ${runId}.`);
      try {
        mockUndo(run, tidyId, fixture.tidyScripts);
      } catch (error) {
        return failure(error instanceof Error ? error.message : String(error));
      }
      return new Promise((resolve) => setTimeout(() => resolve(structuredClone(run)), options.delayMs ?? 400));
    },
    // Browser fixtures never open host apps or claim an external launch succeeded.
    openHint: () => failure('External advice can’t be opened from this fixture preview. No app was launched.'),
    openGuide: () => failure('Vendor references can’t be opened from this fixture preview. No app was launched.'),
    claudeStatus: () => (options.fail !== undefined ? failure(options.fail) : claude.status()),
    claudeSetEnabled: (on) => claude.setEnabled(on),
    claudeSetPrivacy: (on) => claude.setPrivacy(on),
    claudeStart: (mountPoint, scan, job, onEvent) => claude.start(mountPoint, scan, job, onEvent),
    claudeChat: (mountPoint, scan, conversation, about, text, onEvent) =>
      claude.chat(mountPoint, scan, conversation, about, text, onEvent),
    claudeCancel: (requestId) => claude.cancel(requestId),
    claudeChooseLayout: (mountPoint, scan, tidyId, layout) => claude.chooseLayout(mountPoint, scan, tidyId, layout),
    claudeRemoveItems: (mountPoint, scan, ids) => claude.removeItems(mountPoint, scan, ids),
    unplug,
  };
}

/** The fixture's home folder, from its Downloads tile: `C:\Users\alex`, `/Users/alex`. */
function fixtureHome(fixture: Fixture): string {
  const downloads = fixture.overview.categories.flatMap((c) => c.items).find((i) => i.kind === 'folder' && i.name === 'Downloads');
  const path = downloads?.path ?? '';
  return path.slice(0, Math.max(0, path.length - '/Downloads'.length));
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
    claudeSteps: r.steps.filter((s) => s.source === 'claude').length,
    tidy: r.reorganized.map((m) => ({
      id: m.id,
      folder: m.folder,
      movedItems: m.movedItems,
      undoable: m.undoable,
      back: m.undo?.back ?? null,
      stuck: m.undo?.stuck.length ?? 0,
      interrupted: m.interrupted,
    })),
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
  proposals = proposals.filter(p => !p.hintAction?.kind.startsWith('windows'));
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
    method: 'normal',
    fallback: null,
    stage: null,
    index: null,
  };
}

type ScanOutcomeMap = Extract<ScanOutcome, { status: 'done' }>['overview'];

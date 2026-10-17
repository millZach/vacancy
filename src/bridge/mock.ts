import type { Fixture } from './fixtures';
import type { Bridge, CategoryTotal, ScanOutcome, ScanUpdate } from './types';

export interface MockOptions {
  /** Answer after this many milliseconds, to see loading states. Default: next tick. */
  delayMs?: number;
  /** Make every call fail with this message, to see error states. */
  fail?: string;
  /** How long a mock scan takes. Default: 4 s, like the prototype. Zero finishes on the next tick. */
  scanMs?: number;
  /** Override the fixture's Full Disk Access answer. */
  fullDiskAccess?: Fixture['fullDiskAccess'];
}

/** The scan's progress curve: fast at first, slowing as it nears the end, the way a real walk feels. */
const ease = (t: number) => 1 - Math.pow(1 - t, 2.4);

/**
 * A bridge that answers from fixture data. Used in a plain browser (`npm run dev`), in tests and in screenshots.
 * Its scan replays the fixture's map over `scanMs`: counts climb, the path ticker moves, and categories are found
 * one after another, the way the prototype's scan reveals them.
 */
export function createMockBridge(fixture: Fixture, options: MockOptions = {}): Bridge {
  const answer = <T>(value: T) =>
    new Promise<T>((resolve, reject) => {
      setTimeout(() => {
        if (options.fail !== undefined) reject(new Error(options.fail));
        else resolve(structuredClone(value));
      }, options.delayMs ?? 0);
    });
  let running: { timer: number; finish: (outcome: ScanOutcome) => void } | null = null;

  return {
    platform: fixture.platform,
    systemVolume: () => answer(fixture.systemVolume),
    machineName: () => answer(fixture.machineName),
    listVolumes: () => answer(fixture.volumes),
    fullDiskAccess: () => answer(options.fullDiskAccess ?? fixture.fullDiskAccess),
    openFullDiskAccessSettings: () => answer(undefined),
    scan: (mountPoint, onUpdate) => {
      if (options.fail !== undefined) return answer<ScanOutcome>({ status: 'cancelled' });
      running?.finish({ status: 'cancelled' });
      const volume = fixture.volumes.find((v) => v.mountPoint === mountPoint) ?? fixture.systemVolume;
      const map = structuredClone(fixture.overview);
      map.volume = volume;
      map.freeBytes = volume.totalBytes - volume.usedBytes;
      const scale = map.volume.usedBytes / fixture.overview.volume.usedBytes;
      for (const c of map.categories) {
        c.bytes *= scale;
        for (const i of c.items) i.bytes *= scale;
      }
      const paths = map.categories.flatMap((c) => c.items.map((i) => i.path)).filter(Boolean);
      // Interleaved, so the ticker jumps around the disk the way a real walk does.
      const ticker = paths.map((p, i) => paths[(i * 7) % paths.length] ?? p);
      const total = options.scanMs ?? 4000;
      const started = Date.now();
      return new Promise<ScanOutcome>((resolve) => {
        const finish = (outcome: ScanOutcome) => {
          if (running) clearInterval(running.timer);
          running = null;
          resolve(outcome);
        };
        const tick = () => {
          const t = total <= 0 ? 1 : Math.min(1, (Date.now() - started) / total);
          onUpdate(update(map, ticker, t, Date.now() - started));
          if (t >= 1) finish({ status: 'done', overview: map });
        };
        running = { timer: window.setInterval(tick, total <= 0 ? 0 : 100), finish };
      });
    },
    cancelScan: () => {
      running?.finish({ status: 'cancelled' });
      return answer(undefined);
    },
    browse: (id) => {
      const folder = fixture.folders[id];
      return folder ? answer(folder) : Promise.reject(new Error(`there is no folder ${id} in this scan`));
    },
  };
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

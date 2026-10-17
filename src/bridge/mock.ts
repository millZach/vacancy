import type { Fixture } from './fixtures';
import type { Bridge, CategoryTotal, MapFolder, ScanOutcome, ScanUpdate, VolumeInfo } from './types';

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
}

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
 * one after another, the way the prototype's scan reveals them. Another drive gets the system drive's map scaled to
 * its size, with its paths moved onto it. Like the app, it keeps a map per drive and runs one scan at a time.
 */
export function createMockBridge(fixture: Fixture, options: MockOptions = {}): MockBridge {
  const answer = <T>(value: T) =>
    new Promise<T>((resolve, reject) => {
      setTimeout(() => {
        if (options.fail !== undefined) reject(new Error(options.fail));
        else resolve(structuredClone(value));
      }, options.delayMs ?? 0);
    });
  const system = fixture.volumes[0]!;
  let volumes = [...fixture.volumes];
  /** Drives with a finished scan, which `browse` can open. */
  const mapped = new Set<string>();
  let running: { mountPoint: string; timer: number; finish: (outcome: ScanOutcome) => void } | null = null;

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
      mapped.delete(mountPoint);
      return answer(undefined);
    },
    fullDiskAccess: () => answer(options.fullDiskAccess ?? fixture.fullDiskAccess),
    openFullDiskAccessSettings: () => answer(undefined),
    scan: (mountPoint, onUpdate) => {
      if (options.fail !== undefined) return answer<ScanOutcome>({ status: 'cancelled' });
      running?.finish({ status: 'cancelled' });
      mapped.delete(mountPoint);
      const volume = volumes.find((v) => v.mountPoint === mountPoint);
      if (!volume) return Promise.reject(new Error(`could not scan ${mountPoint}: the drive is not connected`));
      const map = onDrive(structuredClone(fixture.overview), system, volume);
      const paths = map.categories.flatMap((c) => c.items.map((i) => i.path)).filter(Boolean);
      // Interleaved, so the ticker jumps around the disk the way a real walk does.
      const ticker = paths.map((p, i) => paths[(i * 7) % paths.length] ?? p);
      const total = options.scanMs ?? 4000;
      const started = Date.now();
      return new Promise<ScanOutcome>((resolve) => {
        const finish = (outcome: ScanOutcome) => {
          if (running) clearInterval(running.timer);
          running = null;
          if (outcome.status === 'done') mapped.add(mountPoint);
          resolve(outcome);
        };
        const tick = () => {
          const t = total <= 0 ? 1 : Math.min(1, (Date.now() - started) / total);
          onUpdate(update(map, ticker, t, Date.now() - started));
          if (t >= 1) finish({ status: 'done', overview: map });
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
    unplug,
  };
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

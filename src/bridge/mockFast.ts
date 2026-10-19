// The mock bridge's fast scan (Windows, NTFS): Windows' admin prompt, then the file index read, then the sort into
// categories, ending with a fast scan's map. The numbers are synthetic, like the fast-scan prototype's.
import type { CategoryTotal, FastFallback, LockedGroup, MapItem, Overview, ScanUpdate, VolumeInfo } from './types';

const GB = 1e9;

/** How the mock's fast scan goes: `ok` reads the index; the others run the normal scan, saying why. */
export type MockFast = 'ok' | 'declined' | 'failed' | 'unreadable';

/** How long the mock's admin prompt is up when `uacMs` isn't given. */
export const UAC_MS = 1200;

/** The share of a mock fast scan spent reading the index; sorting takes the rest. */
const READING = 0.5;

/** A fast scan's time on the map, as on a big drive's first scan. */
const FAST_MS = 9_400;

/** The system drive's locked folders: another account's files, and folders only Windows opens. */
const LOCKED_GB = { otherAccounts: 21.4, windowsOnly: 4.3 };
const LOCKED_COUNTS = {
  otherAccounts: { files: 18_406, folders: 1_204 },
  windowsOnly: { files: 2_177, folders: 83 },
};
/** The file index's own records on the system drive. */
const RECORDS_GB = 1.3;

const RECORDS_NOTE =
  "The drive's own file index, its log, folder indexes and the other records NTFS keeps beside files. Fast scan reads them; Windows keeps them up to date.";

/** Why the mock runs the normal scan where a fast one was asked for, or null if it reads the index. */
export function fallbackFor(volume: VolumeInfo, mode: MockFast): FastFallback | null {
  if (volume.fileSystem.toUpperCase() !== 'NTFS') return 'notNtfs';
  if (mode === 'declined') return 'declined';
  if (mode === 'failed') return 'helperFailed';
  if (mode === 'unreadable') return 'indexUnreadable';
  return null;
}

/**
 * A fast scan's map, from the normal scan's map of the same drive (`map`, already on `volume`): the space a normal
 * scan can't see goes, the file index's records join System, and the folders this account can't open become locked
 * groups, totals only. The space they take comes out of the biggest personal folder (Videos), as in the prototype,
 * so the drive still adds up: categories, locked folders and free space make its size. `scale` is the drive's used
 * space against the system drive's.
 */
export function fastOverview(map: Overview, scale: number): Overview {
  const out = structuredClone(map);
  const locked: LockedGroup[] = (['otherAccounts', 'windowsOnly'] as const).map((kind) => ({
    kind,
    bytes: LOCKED_GB[kind] * GB * scale,
    files: Math.round(LOCKED_COUNTS[kind].files * scale),
    folders: LOCKED_COUNTS[kind].folders,
  }));
  const lockedBytes = locked.reduce((n, g) => n + g.bytes, 0);
  const records = RECORDS_GB * GB * scale;
  let hidden = 0;
  for (const c of out.categories) {
    const unseen = c.items.find((i) => i.kind === 'hidden');
    if (!unseen) continue;
    hidden = unseen.bytes;
    c.items = c.items.filter((i) => i !== unseen);
    c.items.push(recordsItem(records));
    c.bytes += records - unseen.bytes;
  }
  // What the fast scan sees beyond the normal one comes out of the biggest personal folder.
  const take = lockedBytes + records - hidden;
  const media = out.categories.find((c) => c.id === 'media');
  const from = media?.items.find((i) => i.name === 'Videos') ?? media?.items[0];
  if (media && from && from.bytes > take) {
    from.bytes -= take;
    media.bytes -= take;
  }
  for (const c of out.categories) c.items.sort((a, b) => b.bytes - a.bytes);
  out.categories.sort((a, b) => b.bytes - a.bytes);
  out.scan = {
    ...out.scan,
    files: out.scan.files + Math.round(106_653 * scale),
    folders: out.scan.folders + Math.round(17_523 * scale),
    scannedBytes: out.categories.reduce((n, c) => n + c.bytes, 0),
    hiddenBytes: 0,
    unreadable: 0,
    unreadableSamples: [],
    durationMs: FAST_MS,
    method: 'fast',
    fallback: null,
    locked,
    recordsBytes: records,
  };
  return out;
}

function recordsItem(bytes: number): MapItem {
  return {
    id: 'records',
    name: 'File system records',
    path: '',
    bytes,
    files: 0,
    count: 0,
    lastUsed: null,
    kind: 'records',
    browsable: false,
    note: RECORDS_NOTE,
    proposal: null,
  };
}

/** What the core says while Windows' admin prompt is up: a fast scan, asking, nothing counted. */
export function askingUpdate(elapsedMs: number): ScanUpdate {
  return {
    files: 0,
    folders: 0,
    bytes: 0,
    unreadable: 0,
    currentPath: '',
    elapsedMs: Math.round(elapsedMs),
    categories: null,
    method: 'fast',
    fallback: null,
    stage: 'asking',
    index: null,
  };
}

/**
 * A fast scan's update `t` of the way through, after the prompt: reading the index (records read, files and
 * folders found, no map yet), then sorting it into categories (the map fills in, every category at once).
 */
export function fastUpdate(map: Overview, t: number, elapsedMs: number): ScanUpdate {
  const base = { unreadable: 0, currentPath: '', elapsedMs: Math.round(elapsedMs), method: 'fast' as const, fallback: null };
  const records = map.scan.files + map.scan.folders;
  if (t < READING) {
    const read = t / READING;
    return {
      ...base,
      files: Math.round(map.scan.files * read),
      folders: Math.round(map.scan.folders * read),
      bytes: 0,
      categories: null,
      stage: 'reading',
      index: { done: Math.round(records * read), total: records },
    };
  }
  const share = Math.min(1, (t - READING) / (1 - READING));
  const categories: CategoryTotal[] = map.categories.map((c) => ({
    id: c.id,
    name: c.name,
    protected: c.protected,
    bytes: c.bytes * share,
  }));
  return {
    ...base,
    files: map.scan.files,
    folders: map.scan.folders,
    bytes: categories.reduce((n, c) => n + c.bytes, 0),
    categories: categories.filter((c) => c.bytes > 0),
    stage: 'sorting',
    index: null,
  };
}

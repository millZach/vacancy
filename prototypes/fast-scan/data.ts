// THROWAWAY: synthetic drives, maps and timings for the fast-scan prototype. Nothing here is read from a disk.
import type { CategoryTotal, MapFolder, Overview, Proposal, ScanUpdate, VolumeInfo } from '../../src/bridge';
import { fixtures } from '../../src/bridge/fixtures';

export const GB = 1e9; // as the app's fixtures and formatBytes count

/** What each drive's file system is. Fast scan reads an NTFS drive's file index (the MFT); nothing else has one. */
export const FILE_SYSTEM: Record<string, 'NTFS' | 'exFAT'> = { 'C:\\': 'NTFS', 'D:\\': 'NTFS', 'E:\\': 'exFAT' };
export const isNtfs = (mountPoint: string) => FILE_SYSTEM[mountPoint] === 'NTFS';

/** Rough first-scan times, as a person would read them. Real: about 3 min for C: normally, 5 to 15 s fast. */
export const ESTIMATE: Record<string, { normal: string; fast: string | null }> = {
  'C:\\': { normal: 'about 3 min', fast: 'about 10 s' },
  'D:\\': { normal: 'about 5 min', fast: 'about 15 s' },
  'E:\\': { normal: 'about 40 s', fast: null },
  '/': { normal: 'about 2 min', fast: null },
  '/Volumes/Samsung T7': { normal: 'about 1 min', fast: null },
};

/** Prototype time: scans are compressed so a person can walk the flow. The HUD never shows a clock. */
export const DURATION_MS = { normal: 14_000, fast: 3_200 };

const win = fixtures.windows;
const C = win.volumes[0]!;

/**
 * Space a normal scan can't see on C:. Bigger than the app fixture's 4 GB because this machine has a second account
 * (21.4 GB) a normal scan can't open. Videos give up the difference so the drive's used figure stays the same.
 */
const HIDDEN_GB = 27;
const LOCKED_ACCOUNTS_GB = 21.4;
const LOCKED_SYSTEM_GB = 4.3;
const RECORDS_GB = HIDDEN_GB - LOCKED_ACCOUNTS_GB - LOCKED_SYSTEM_GB; // the file index itself, which fast scan counts

const NORMAL_NOTE =
  "Windows counts this as used, but a normal scan can't see it: 111 folders your account isn't allowed to open (another account's files, System Volume Information, WindowsApps) and the file system's own records. A fast scan counts this space, as totals only.";

/** C:'s map from a normal scan: the app's fixture, with the bigger unseen share. */
function normalC(): Overview {
  const o = structuredClone(win.overview);
  for (const c of o.categories) {
    for (const i of c.items) {
      if (i.kind === 'hidden') {
        c.bytes += HIDDEN_GB * GB - i.bytes;
        i.bytes = HIDDEN_GB * GB;
        i.note = NORMAL_NOTE;
      }
      if (i.name === 'Videos') {
        c.bytes -= (HIDDEN_GB - 4) * GB;
        i.bytes -= (HIDDEN_GB - 4) * GB;
      }
    }
    c.items.sort((a, b) => b.bytes - a.bytes);
  }
  o.categories.sort((a, b) => b.bytes - a.bytes);
  o.scan = { ...o.scan, hiddenBytes: HIDDEN_GB * GB, durationMs: 178_000 };
  return o;
}

/**
 * C:'s map from a fast scan. The file index accounts for every byte: the unseen tile goes, the index's own records
 * join Windows & system, and folders the account can't open become the Locked folders block (`LOCKED`, below).
 */
function fastC(): Overview {
  const o = normalC();
  for (const c of o.categories) {
    const hidden = c.items.find((i) => i.kind === 'hidden');
    if (!hidden) continue;
    c.items = c.items.filter((i) => i !== hidden);
    c.items.push({
      id: 'fx-system-records', name: 'File system records', path: 'C:\\$MFT and other NTFS records', bytes: RECORDS_GB * GB,
      files: 9, count: 0, lastUsed: Date.now(), kind: 'file', browsable: false,
      note: 'The drive’s own file index. Fast scan reads it; Windows keeps it up to date.', proposal: null,
    });
    c.bytes -= (LOCKED_ACCOUNTS_GB + LOCKED_SYSTEM_GB) * GB;
    c.items.sort((a, b) => b.bytes - a.bytes);
  }
  o.categories.sort((a, b) => b.bytes - a.bytes);
  o.scan = { ...o.scan, hiddenBytes: 0, unreadable: 0, unreadableSamples: [], durationMs: 9_400, files: 1_391_204, folders: 228_610 };
  return o;
}

/** The Locked folders block: totals only, grouped by why they're locked. Never a name from inside them. */
export interface LockedGroup { id: string; label: string; bytes: number; folders: number; why: string }
export function lockedOf(volume: VolumeInfo): LockedGroup[] {
  const s = volume.usedBytes / C.usedBytes;
  return [
    {
      id: 'locked:accounts', label: 'Other accounts on this PC', bytes: LOCKED_ACCOUNTS_GB * GB * s, folders: 1_204,
      why: 'Files that belong to another account. Only that account, or Windows, can open them.',
    },
    {
      id: 'locked:system', label: 'Folders only Windows opens', bytes: LOCKED_SYSTEM_GB * GB * s, folders: 83,
      why: 'Restore points, app packages and other folders Windows keeps to itself.',
    },
  ];
}
export const lockedTotal = (volume: VolumeInfo) => lockedOf(volume).reduce((n, g) => n + g.bytes, 0);
export const lockedFolders = (volume: VolumeInfo) => lockedOf(volume).reduce((n, g) => n + g.folders, 0);

/** The system drive's map, scaled onto another drive (as the app's mock does), never browsable there. */
function scaled(o: Overview, volume: VolumeInfo): Overview {
  if (volume.mountPoint === o.volume.mountPoint) return o;
  const s = volume.usedBytes / o.volume.usedBytes;
  const out = structuredClone(o);
  out.volume = volume;
  out.freeBytes = volume.freeBytes;
  out.scan = { ...out.scan, files: Math.round(out.scan.files * s), folders: Math.round(out.scan.folders * s), hiddenBytes: out.scan.hiddenBytes * s };
  for (const c of out.categories) {
    c.bytes *= s;
    for (const i of c.items) {
      i.bytes *= s;
      i.browsable = false;
      i.path = i.path.replace(/^C:\\/, volume.mountPoint);
    }
  }
  return out;
}

const NORMAL_C = normalC();
const FAST_C = fastC();

export function overviewFor(volume: VolumeInfo, mode: 'normal' | 'fast'): Overview {
  if (volume.mountPoint.startsWith('/')) {
    const mac = fixtures.macos.overview;
    return volume.mountPoint === '/' ? mac : scaled(mac, volume);
  }
  return scaled(mode === 'fast' ? FAST_C : NORMAL_C, volume);
}

export function proposalsFor(volume: VolumeInfo): Proposal[] {
  if (volume.mountPoint === 'C:\\') return win.proposals;
  if (volume.mountPoint === '/') return fixtures.macos.proposals;
  return [];
}

export function folderFor(volume: VolumeInfo, id: string): MapFolder | null {
  if (volume.mountPoint === 'C:\\') return (win.folders as Record<string, MapFolder>)[id] ?? null;
  if (volume.mountPoint === '/') return (fixtures.macos.folders as Record<string, MapFolder>)[id] ?? null;
  return null;
}

const NORMAL_PATHS = [
  'C:\\Program Files (x86)\\Steam\\steamapps\\common\\Cyberpunk 2077\\archive\\pc\\content',
  'C:\\Users\\alex\\AppData\\Local\\Docker\\wsl\\disk',
  'C:\\Windows\\WinSxS\\amd64_microsoft-windows-servicingstack_31bf3856ad364e35',
  'C:\\Users\\alex\\code\\ledger\\target\\debug\\deps',
  'C:\\Users\\alex\\Pictures\\2026\\06',
  'C:\\Program Files\\Adobe\\Adobe Photoshop 2026\\Required',
  'C:\\Users\\alex\\AppData\\Local\\npm-cache\\_cacache\\content-v2',
  'C:\\Users\\alex\\Downloads\\Archives',
];

/** What the HUD says while a scan runs, `t` from 0 to 1. A fast scan reads the index first, then sorts. */
export function updateAt(volume: VolumeInfo, mode: 'normal' | 'fast', t: number): { update: ScanUpdate; preview: CategoryTotal[] } {
  const o = overviewFor(volume, mode);
  const fastIndex = mode === 'fast' ? Math.min(1, t / 0.55) : t;
  const files = Math.round(o.scan.files * fastIndex);
  const folders = Math.round(o.scan.folders * fastIndex);
  // A fast scan has every size once the index is read, so the map fills in all at once after that.
  const share = mode === 'fast' ? Math.max(0, (t - 0.45) / 0.55) : t;
  const preview: CategoryTotal[] = o.categories.map((c) => ({ id: c.id, name: c.name, protected: c.protected, bytes: c.bytes * share }));
  const paths = volume.mountPoint.startsWith('/')
    ? ['/Users/alex/Library/Developer/CoreSimulator/Devices', '/Applications/Xcode.app/Contents', '/Users/alex/Library/Caches']
    : NORMAL_PATHS.map((p) => p.replace(/^C:\\/, volume.mountPoint));
  const currentPath = mode === 'fast'
    ? t < 0.55
      ? `Reading the file index of ${volume.name}`
      : `Sorting ${volume.name} into categories`
    : paths[Math.floor(t * 40) % paths.length]!;
  return {
    update: { files, folders, bytes: preview.reduce((n, c) => n + c.bytes, 0), unreadable: mode === 'fast' ? 0 : Math.round(111 * t), currentPath, elapsedMs: 0, categories: preview },
    preview,
  };
}

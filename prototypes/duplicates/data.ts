// THROWAWAY: synthetic duplicate groups for the duplicate-finder prototype. Nothing here reads a disk.
import type { MapCategory, MapItem, Overview, Platform, Proposal } from '../../src/bridge';
import { fixtures } from '../../src/bridge/fixtures';

export type Kind = 'photo' | 'video' | 'document' | 'archive' | 'audio';
/** The five folders Zach scoped duplicates to, under their platform names. */
export type Folder = 'Pictures' | 'Videos' | 'Documents' | 'Downloads' | 'Desktop';

export interface Copy {
  id: string;
  folder: Folder;
  /** The folder as the platform names it: Videos is "Movies" on a Mac. */
  folderName: string;
  path: string;
  /** The path under the home folder, for short labels ("Pictures\2025\Lisbon"). */
  dir: string;
  file: string;
  modifiedDays: number;
}

export interface Group {
  id: string;
  kind: Kind;
  name: string;
  /** Allocated bytes of one copy. */
  bytes: number;
  copies: Copy[];
  /** The copy Vacancy keeps, by index into `copies`. */
  keep: number;
  /** "Kept: oldest, in Pictures". */
  reason: string;
}

export interface DupData {
  platform: Platform;
  home: string;
  sep: string;
  groups: Group[];
  /** Files over 1 MB that share a size with another one: hashing's first pass. */
  sizeMatches: number;
  /** Still matching after the first 64 KB were hashed. */
  partialMatches: number;
  hardLinked: number;
  cloudName: string;
  /** The extra notes this platform needs in the review. */
  notes: string[];
  overviewBefore: Overview;
  overviewAfter: Overview;
  proposals: Proposal[];
  /** Map tile ids holding removed copies, by folder. */
  tiles: Partial<Record<Folder, string>>;
  /** Map tile ids of the folders themselves, by folder (where kept copies live). */
  folderTiles: Partial<Record<Folder, string>>;
}

const MB = 1e6;
const GB = 1e9;
const COPY_NAME = /( - Copy| copy|\(\d\))(?=\.[^.]+$|$)/i;

/** How untidy a copy's place is: in Downloads, or named like a copy. Lower is tidier. */
export function mess(c: Copy): number {
  return (c.folder === 'Downloads' ? 1 : 0) + (COPY_NAME.test(c.file) ? 1 : 0);
}

/** Vacancy's rule: the tidiest copy, and among the tidiest the oldest. Says why in a few words. */
export function pickKeeper(copies: Copy[]): { keep: number; reason: string } {
  const order = copies.map((c, i) => ({ c, i })).sort((a, b) => mess(a.c) - mess(b.c) || b.c.modifiedDays - a.c.modifiedDays);
  const k = order[0]!;
  const oldest = copies.every((c) => c.modifiedDays <= k.c.modifiedDays);
  if (oldest) return { keep: k.i, reason: `Kept: oldest, in ${k.c.folderName}` };
  const older = copies.filter((c) => c.modifiedDays > k.c.modifiedDays);
  if (older.some((c) => c.folder === 'Downloads') && k.c.folder !== 'Downloads')
    return { keep: k.i, reason: `Kept: in ${k.c.folderName}, not Downloads` };
  const named = older.find((c) => COPY_NAME.test(c.file));
  const tag = named?.file.match(COPY_NAME)?.[1]?.trim() ?? 'copy';
  return { keep: k.i, reason: `Kept: original name, not ‘${tag}’` };
}

type Spec = [kind: Kind, bytes: number, copies: [rel: string, days: number][]];

/** Hand-written groups, then generated ones, paths relative to the home folder with '/'. Videos means Movies on a Mac. */
function specs(mac: boolean): Spec[] {
  const pic = mac ? 'HEIC' : 'JPG';
  const mov = mac ? 'mov' : 'mp4';
  const hand: Spec[] = [
    ['video', 3.9 * GB, [[`Videos/GoPro/2025-07 Tahoe/GX010231.MP4`, 450], [`Desktop/GX010231 - Copy.MP4`, 440]]],
    ['video', 1.84 * GB, [[`Videos/2024/Wedding highlights.${mov}`, 610], [`Downloads/Wedding highlights.${mov}`, 612]]],
    ['video', 0.74 * GB, [[`Videos/Captures/Screen Recording 2026-03-02.${mov}`, 230], [`Desktop/Screen Recording 2026-03-02.${mov}`, 229]]],
    ['photo', 6.2 * MB, [[`Pictures/2025/Lisbon/IMG_4471.${pic}`, 400], [`Downloads/IMG_4471.${pic}`, 390], [`Desktop/IMG_4471 (1).${pic}`, 120]]],
    ['photo', 28 * MB, [[`Pictures/Raw/2024-10/DSC_0912.NEF`, 380], [`Pictures/Lightroom import/DSC_0912.NEF`, 300]]],
    ['audio', 612 * MB, [[`Documents/Podcast/Ep 41/master.wav`, 160], [`Desktop/master.wav`, 158]]],
    ['document', 4.8 * MB, [[`Documents/Taxes/2025/Tax return 2025.pdf`, 190], [`Downloads/Tax return 2025.pdf`, 191], [`Documents/Taxes/2025/Tax return 2025 (1).pdf`, 100]]],
    ['archive', 212 * MB, [[`Downloads/Lightroom presets.zip`, 300], [`Downloads/Lightroom presets (1).zip`, 320]]],
    ['document', 38 * MB, [[`Documents/Work/Brand guide.pdf`, 75], [`Downloads/Brand guide.pdf`, 76]]],
  ];
  const photos: Spec[] = Array.from({ length: 22 }, (_, i): Spec => {
    const n = 5102 + i * 7;
    const size = (2.1 + ((i * 37) % 23) / 10) * MB;
    const both: [string, number][] = [[`Pictures/Camera Roll/2025/IMG_${n}.${pic}`, 300 - i], [`Pictures/Phone import 2025-08/IMG_${n}.${pic}`, 280 - i]];
    if (i % 5 === 0) both.push([`Downloads/IMG_${n} (1).${pic}`, 60]);
    return ['photo', size, both];
  });
  const videos: Spec[] = Array.from({ length: 5 }, (_, i): Spec => {
    const day = 12 + i * 3;
    return ['video', (0.22 + i * 0.13) * GB, [[`Videos/Phone/VID_202506${day}_1832.${mov}`, 330 - i * 4], [`Desktop/VID_202506${day}_1832.${mov}`, 200 - i]]];
  });
  const docs: Spec[] = [
    ['document', 12 * MB, [[`Documents/House/Inspection report.pdf`, 520], [`Downloads/Inspection report.pdf`, 522]]],
    ['document', 3.4 * MB, [[`Documents/Resume/Resume 2026.docx`, 40], [`Desktop/Resume 2026 copy.docx`, 38]]],
    ['document', 18 * MB, [[`Documents/Manuals/Camera manual.pdf`, 700], [`Downloads/Camera manual.pdf`, 700]]],
    ['archive', 96 * MB, [[`Documents/Backups/Contacts export.zip`, 260], [`Downloads/Contacts export.zip`, 255]]],
  ];
  return [...hand, ...photos, ...videos, ...docs];
}

function build(platform: Platform): DupData {
  const mac = platform === 'macos';
  const home = mac ? '/Users/alex' : 'C:\\Users\\alex';
  const sep = mac ? '/' : '\\';
  const rename = (f: Folder) => (mac && f === 'Videos' ? 'Movies' : f);
  const groups: Group[] = specs(mac).map(([kind, bytes, list], gi) => {
    const copies: Copy[] = list.map(([rel, days], ci) => {
      const parts = rel.split('/');
      const folder = parts[0] as Folder;
      parts[0] = rename(folder);
      const file = parts.at(-1)!;
      return {
        id: `g${gi}c${ci}`,
        folder,
        folderName: rename(folder),
        path: [home, ...parts].join(sep),
        dir: parts.slice(0, -1).join(sep),
        file,
        modifiedDays: days,
      };
    });
    const { keep, reason } = pickKeeper(copies);
    return { id: `g${gi}`, kind, name: copies[keep]!.file, bytes, copies, keep, reason };
  });
  // Biggest first, like every other proposal list. Photos and videos come first in the counts, not the order.
  groups.sort((a, b) => freedOf(b) - freedOf(a));

  const fx = fixtures[platform];
  const removedBy = new Map<Folder, number>();
  for (const g of groups) g.copies.forEach((c, i) => i !== g.keep && removedBy.set(c.folder, (removedBy.get(c.folder) ?? 0) + g.bytes));

  // The media category, rebuilt with the five folders (same total as the fixture's), and Downloads carved too.
  const mediaSizes: Record<Platform, [Folder, number][]> = {
    windows: [['Videos', 34], ['Pictures', 19], ['Documents', 11], ['Desktop', 10]],
    macos: [['Documents', 15], ['Desktop', 11], ['Videos', 9], ['Pictures', 6]],
  };
  const tiles: Partial<Record<Folder, string>> = {};
  const folderTiles: Partial<Record<Folder, string>> = {};
  const now = Date.now();
  const item = (id: string, name: string, path: string, bytes: number, extra: Partial<MapItem> = {}): MapItem => ({
    id, name, path, bytes, files: Math.round(bytes / 3e6), count: 0, lastUsed: now - 4 * 86_400_000, kind: 'folder',
    browsable: false, note: null, proposal: null, ...extra,
  });
  const dupTile = (f: Folder): MapItem => {
    const id = `p:dups:${f}`;
    tiles[f] = id;
    return item(id, 'Duplicate copies', '', removedBy.get(f) ?? 0, { kind: 'proposal', proposal: 'dups', lastUsed: null,
      files: groups.reduce((n, g) => n + g.copies.filter((c, i) => i !== g.keep && c.folder === f).length, 0) });
  };
  const carve = (cats: MapCategory[], withDups: boolean): MapCategory[] => cats.map((c) => {
    if (c.id === 'media') {
      const items = mediaSizes[platform].flatMap(([f, gb]) => {
        const id = `dup-folder-${f}`;
        folderTiles[f] = id;
        const removed = withDups ? (removedBy.get(f) ?? 0) : 0;
        const own = item(id, rename(f), [home, rename(f)].join(sep), gb * GB - removed);
        return withDups && removed > 0 ? [own, dupTile(f)] : [own];
      });
      return { ...c, items: items.sort((a, b) => b.bytes - a.bytes) };
    }
    if (c.id === 'downloads') {
      const items = c.items.flatMap((i) => {
        if (i.name !== 'Downloads' || i.kind !== 'folder') return [i];
        folderTiles.Downloads = i.id;
        const removed = withDups ? (removedBy.get('Downloads') ?? 0) : 0;
        const own = { ...i, bytes: i.bytes - removed, browsable: false };
        return withDups ? [own, dupTile('Downloads')] : [own];
      });
      return { ...c, items: items.sort((a, b) => b.bytes - a.bytes) };
    }
    // Folders stay closed in this prototype: there is no bridge to browse with.
    return { ...c, items: c.items.map((i) => ({ ...i, browsable: false })) };
  });
  // Mac: Pictures outside Photos Library is a plain folder; the library stays locked in its own category.
  const before = { ...fx.overview, categories: carve(fx.overview.categories, false) };
  const after = { ...fx.overview, categories: carve(fx.overview.categories, true) };

  return {
    platform,
    home,
    sep,
    groups,
    sizeMatches: mac ? 986 : 1204,
    partialMatches: mac ? 174 : 236,
    hardLinked: mac ? 9 : 18,
    cloudName: mac ? 'iCloud Drive' : 'OneDrive',
    notes: mac
      ? ['Photos Library is left alone. Photos has its own Duplicates album for it.', 'Copies in iCloud Drive are left alone, so nothing disappears from your other devices.']
      : ['Copies in OneDrive and Dropbox are left alone, so nothing disappears from your other devices.'],
    overviewBefore: before,
    overviewAfter: after,
    // Tool and store advice is another prototype's question; only cleanup proposals are context here.
    proposals: fx.proposals.filter((p) => p.action === 'delete'),
    tiles,
    folderTiles,
  };
}

export const freedOf = (g: Group) => g.bytes * (g.copies.length - 1);
export const removedOf = (g: Group) => g.copies.filter((_, i) => i !== g.keep);
export const keptOf = (g: Group) => g.copies[g.keep]!;

export const data: Record<Platform, DupData> = { windows: build('windows'), macos: build('macos') };

/** "24 photos, 8 videos and 9 other files": photos and videos counted first. */
export function countLine(groups: Group[]): string {
  const files = (k: Kind[]) => groups.filter((g) => k.includes(g.kind)).reduce((n, g) => n + g.copies.length - 1, 0);
  const photos = files(['photo']);
  const videos = files(['video']);
  const other = files(['document', 'archive', 'audio']);
  const parts = [
    photos && `${photos} ${photos === 1 ? 'photo' : 'photos'}`,
    videos && `${videos} ${videos === 1 ? 'video' : 'videos'}`,
    other && `${other} other ${other === 1 ? 'file' : 'files'}`,
  ].filter(Boolean) as string[];
  return parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}` : (parts[0] ?? 'No files');
}

export const KIND_LABEL: Record<Kind, string> = { photo: 'Photos', video: 'Videos', document: 'Documents', archive: 'Archives', audio: 'Audio' };

// The mock bridge's duplicate finder: synthetic groups of copies for each fixture's system drive, the map with their
// tiles carved out of the folders holding the extra copies, and the "Duplicate files" proposal, shaped the way the
// core answers `find_duplicates`. Nothing reads a disk; names, sizes and folders only, never contents.
import type {
  TidyProposal,
  DuplicateCopy,
  DuplicateGroup,
  Duplicates,
  FileKind,
  MapItem,
  Overview,
  Platform,
  Proposal,
} from './types';

/** The duplicates proposal's id, as the core names it. */
export const DUPLICATES = 'duplicates';

/** What the mock finder finds on a drive. */
export interface MockDuplicates {
  /** The map with the duplicates' tiles. */
  overview: Overview;
  proposal: Proposal;
  duplicates: Duplicates;
  /** A group whose kept copy a mock run finds changed: its extra copy stays, with the reason the core gives. */
  changedGroup: string | null;
}

const MB = 1e6;
const GB = 1e9;
const DAY = 86_400_000;

type Folder = 'Pictures' | 'Videos' | 'Documents' | 'Downloads' | 'Desktop';
type Spec = [kind: FileKind, bytes: number, copies: [rel: string, days: number][]];

/** The groups, paths relative to the home folder with `/`. Each platform's map shows only some folders as tiles. */
function specs(platform: Platform): Spec[] {
  if (platform === 'windows') {
    const photos: Spec[] = Array.from({ length: 14 }, (_, i): Spec => {
      const n = 5102 + i * 7;
      const copies: [string, number][] = [
        [`Pictures/Camera Roll/2025/IMG_${n}.JPG`, 300 - i],
        [`Pictures/Phone import 2025-08/IMG_${n}.JPG`, 280 - i],
      ];
      if (i % 5 === 0) copies.push([`Downloads/IMG_${n} (1).JPG`, 60]);
      return ['photo', (2.1 + ((i * 37) % 23) / 10) * MB, copies];
    });
    const videos: Spec[] = Array.from({ length: 4 }, (_, i): Spec => [
      'video',
      (0.22 + i * 0.13) * GB,
      [
        [`Videos/Phone/VID_202506${12 + i * 3}_1832.mp4`, 330 - i * 4],
        [`Downloads/VID_202506${12 + i * 3}_1832.mp4`, 200 - i],
      ],
    ]);
    return [
      ['video', 3.9 * GB, [['Videos/GoPro/2025-07 Tahoe/GX010231.MP4', 450], ['Downloads/GX010231 - Copy.MP4', 440]]],
      ['video', 1.84 * GB, [['Videos/2024/Wedding highlights.mp4', 610], ['Downloads/Wedding highlights.mp4', 612]]],
      ['video', 0.74 * GB, [['Videos/Captures/Screen Recording 2026-03-02.mp4', 230], ['Documents/Screen Recording 2026-03-02.mp4', 229]]],
      ['photo', 6.2 * MB, [['Pictures/2025/Lisbon/IMG_4471.JPG', 400], ['Downloads/IMG_4471.JPG', 390], ['Documents/IMG_4471 (1).JPG', 120]]],
      ['photo', 28 * MB, [['Pictures/Raw/2024-10/DSC_0912.NEF', 380], ['Pictures/Lightroom import/DSC_0912.NEF', 300]]],
      ['audio', 612 * MB, [['Documents/Podcast/Ep 41/master.wav', 160], ['Downloads/master.wav', 158]]],
      ['document', 4.8 * MB, [['Documents/Taxes/2025/Tax return 2025.pdf', 190], ['Downloads/Tax return 2025.pdf', 191], ['Documents/Taxes/2025/Tax return 2025 (1).pdf', 100]]],
      ['archive', 212 * MB, [['Downloads/Lightroom presets.zip', 300], ['Downloads/Lightroom presets (1).zip', 320]]],
      ['document', 38 * MB, [['Documents/Work/Brand guide.pdf', 75], ['Downloads/Brand guide.pdf', 76]]],
      ...photos,
      ...videos,
      ['document', 12 * MB, [['Documents/House/Inspection report.pdf', 520], ['Downloads/Inspection report.pdf', 522]]],
      ['archive', 96 * MB, [['Documents/Backups/Contacts export.zip', 260], ['Downloads/Contacts export.zip', 255]]],
    ];
  }
  const photos: Spec[] = Array.from({ length: 10 }, (_, i): Spec => {
    const n = 2210 + i * 9;
    return [
      'photo',
      (2.4 + ((i * 29) % 19) / 10) * MB,
      [
        [`Desktop/Phone import/IMG_${n}.HEIC`, 260 - i],
        [`Downloads/IMG_${n}.HEIC`, 90 - i],
      ],
    ];
  });
  return [
    ['video', 2.6 * GB, [['Documents/Videos/2025 Iceland/Iceland trip.mov', 420], ['Downloads/Iceland trip.mov', 400]]],
    ['video', 1.2 * GB, [['Documents/Videos/Screen Recording 2026-02-11.mov', 250], ['Desktop/Screen Recording 2026-02-11.mov', 249]]],
    ['photo', 8.4 * MB, [['Documents/Photos/2025/Lisbon/IMG_4471.HEIC', 400], ['Downloads/IMG_4471.HEIC', 390], ['Desktop/IMG_4471 copy.HEIC', 120]]],
    ['audio', 540 * MB, [['Documents/Podcast/Ep 12/master.aif', 180], ['Desktop/master.aif', 178]]],
    ['document', 5.1 * MB, [['Documents/Taxes/2025/Tax return 2025.pdf', 190], ['Downloads/Tax return 2025.pdf', 191]]],
    ['archive', 180 * MB, [['Downloads/Brushes.zip', 300], ['Downloads/Brushes copy.zip', 320]]],
    ['document', 22 * MB, [['Documents/Work/Pitch deck.key', 75], ['Desktop/Pitch deck copy.key', 70]]],
    ...photos,
  ];
}

const COPY_NAME = /( - Copy| copy| copy \d+|\(\d+\))(?=\.[^.]+$|$)/i;

interface Copy {
  folder: Folder;
  path: string;
  dir: string;
  name: string;
  days: number;
}

/** How untidy a copy's place is (the core's keeper rule): in Downloads, named like a copy. */
const mess = (c: Copy) => (c.folder === 'Downloads' ? 1 : 0) + (COPY_NAME.test(c.name) ? 1 : 0);

/** The core's keeper rule: the tidiest copy, then the oldest. Says why in a few words. */
function keeper(copies: Copy[]): { keep: number; reason: string } {
  const order = copies.map((c, i) => ({ c, i })).sort((a, b) => mess(a.c) - mess(b.c) || b.c.days - a.c.days);
  const k = order[0]!.c;
  const older = copies.filter((c) => c.days > k.days);
  if (older.length === 0) return { keep: order[0]!.i, reason: `Kept: oldest, in ${k.folder}` };
  if (older.some((c) => c.folder === 'Downloads') && k.folder !== 'Downloads')
    return { keep: order[0]!.i, reason: `Kept: in ${k.folder}, not Downloads` };
  const tag = older.map((c) => c.name.match(COPY_NAME)?.[1]?.trim()).find(Boolean) ?? 'copy';
  return { keep: order[0]!.i, reason: `Kept: original name, not ‘${tag}’` };
}

/** "24 photos, 8 videos and 9 other files": photos and videos counted first. */
function countLine(kinds: FileKind[]): string {
  const photos = kinds.filter((k) => k === 'photo').length;
  const videos = kinds.filter((k) => k === 'video').length;
  const other = kinds.length - photos - videos;
  const parts = [
    photos && `${photos.toLocaleString('en-US')} ${photos === 1 ? 'photo' : 'photos'}`,
    videos && `${videos.toLocaleString('en-US')} ${videos === 1 ? 'video' : 'videos'}`,
    other && `${other.toLocaleString('en-US')} other ${other === 1 ? 'file' : 'files'}`,
  ].filter((p): p is string => !!p);
  return parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}` : (parts[0] ?? '');
}

/**
 * What the mock finder finds on the system drive of a fixture whose map is `overview`: groups of copies in the
 * folders the map shows, their tiles carved out of those folders' tiles (one per folder holding extra copies), and
 * the proposal. `null` when the map shows none of the folders.
 */
export function mockDuplicates(platform: Platform, overview: Overview, home: string): MockDuplicates | null {
  const sep = platform === 'macos' ? '/' : '\\';
  const now = Date.now();
  const map = structuredClone(overview);
  /** Each folder's tile on the map, found by name. */
  const folderTile = new Map<Folder, { item: MapItem; category: string }>();
  for (const c of map.categories)
    for (const i of c.items)
      if (i.kind === 'folder' && ['Pictures', 'Videos', 'Documents', 'Downloads', 'Desktop'].includes(i.name))
        folderTile.set(i.name as Folder, { item: i, category: c.id });
  if (folderTile.size === 0) return null;
  const dupTile = (f: Folder) => `p:${DUPLICATES}:${folderTile.get(f)!.item.id}`;

  const groups: DuplicateGroup[] = specs(platform).map(([kind, bytes, list], gi) => {
    const copies: Copy[] = list.map(([rel, days]) => {
      const parts = rel.split('/');
      return {
        folder: parts[0] as Folder,
        path: [home, ...parts].join(sep),
        dir: parts.slice(0, -1).join(sep),
        name: parts.at(-1)!,
        days,
      };
    });
    const { keep, reason } = keeper(copies);
    const shown = (c: Copy, tile: string): DuplicateCopy => ({
      path: c.path,
      name: c.name,
      folder: c.folder,
      dir: c.dir,
      bytes,
      modified: now - c.days * DAY,
      tile,
    });
    const kept = copies[keep]!;
    const extras = copies.filter((_, i) => i !== keep);
    return {
      id: `g${gi}`,
      kind,
      name: kept.name,
      bytes,
      freed: bytes * extras.length,
      keep: shown(kept, folderTile.get(kept.folder)!.item.id),
      extras: extras.map((c) => shown(c, dupTile(c.folder))),
      reason,
    };
  });
  groups.sort((a, b) => b.freed - a.freed);

  // Carve the extra copies out of their folders' tiles, into one duplicates tile per folder.
  const removed = new Map<Folder, { bytes: number; files: number }>();
  for (const g of groups)
    for (const e of g.extras) {
      const r = removed.get(e.folder as Folder) ?? { bytes: 0, files: 0 };
      removed.set(e.folder as Folder, { bytes: r.bytes + e.bytes, files: r.files + 1 });
    }
  for (const c of map.categories) {
    const added: MapItem[] = [];
    for (const i of c.items) {
      const r = i.kind === 'folder' ? removed.get(i.name as Folder) : undefined;
      if (!r || folderTile.get(i.name as Folder)?.item.id !== i.id) continue;
      i.bytes -= r.bytes;
      i.files = Math.max(0, i.files - r.files);
      added.push({
        id: `p:${DUPLICATES}:${i.id}`,
        name: 'Duplicate copies',
        path: i.path,
        bytes: r.bytes,
        files: r.files,
        count: r.files,
        lastUsed: null,
        kind: 'proposal',
        browsable: false,
        note: `Extra copies in ${i.name}`,
        proposal: DUPLICATES,
      });
    }
    c.items = [...c.items, ...added].sort((a, b) => b.bytes - a.bytes);
  }

  const extras = groups.flatMap((g) => g.extras.map((e) => ({ e, kind: g.kind })));
  const folders = [...folderTile.keys()];
  const proposal: Proposal = {
    id: DUPLICATES,
    category: 'media',
    title: 'Duplicate files',
    summary: `${countLine(extras.map((x) => x.kind))} have an exact copy elsewhere, in ${groups.length} groups. Vacancy keeps one copy of each.`,
    why:
      `Byte-for-byte copies of files of 1 MB or more, in length and on disk, in ${folders.slice(0, -1).join(', ')} and ${folders.at(-1)}. Vacancy keeps one copy of each group: the ` +
      'tidiest (one outside Downloads that isn’t named like a copy: “copy”, “- Copy”, “(1)”), and the oldest among those. Before each extra copy goes, it ' +
      'checks the kept copy is still there and unchanged, and holds it while the copy is deleted. Untick a group to keep every copy of it. Copies in ' +
      'cloud-sync folders and in a Photos library are left alone, and so are files that aren’t all on this disk. The space is what the extra copies take ' +
      'up: up to that much comes back, less where a drive shares copied files’ data.',
    bytes: extras.reduce((s, x) => s + x.e.bytes, 0),
    itemCount: extras.length,
    placeCount: extras.length,
    risk: 'review',
    source: 'rule',
    aiNote: null,
    action: 'delete',
    regenerable: false,
    samplePaths: [...extras].sort((a, b) => b.e.bytes - a.e.bytes).slice(0, 5).map((x) => x.e.path),
    defaultOn: false,
  };
  return {
    overview: map,
    proposal,
    duplicates: {
      proposal: DUPLICATES,
      groups,
      folders: platform === 'macos' ? ['Pictures', 'Movies', 'Documents', 'Downloads', 'Desktop'] : ['Pictures', 'Videos', 'Documents', 'Downloads', 'Desktop'],
      compared: groups.reduce((n, g) => n + g.extras.length + 1, 0) + 37,
      hardLinked: platform === 'macos' ? 9 : 18,
      changed: 0,
      notLocal: 0,
      cloned: 0,
    },
    changedGroup: groups.find((g) => g.kind === 'document')?.id ?? null,
  };
}

/** The sorts again, each move of an extra copy marked as deleted by duplicates, as the core marks them. */
export function coverDuplicates(tidy: TidyProposal[], found: MockDuplicates): TidyProposal[] {
  const extras = new Set(found.duplicates.groups.flatMap((g) => g.extras.map((e) => e.path.toLowerCase())));
  return tidy.map((t) => ({
    ...t,
    moves: t.moves.map((m) => {
      const others = m.deletedBy.filter((id) => id !== DUPLICATES);
      const sep = t.path.includes('\\') ? '\\' : '/';
      const mine = extras.has(`${t.path}${sep}${m.name}`.toLowerCase());
      return { ...m, deletedBy: mine ? [...others, DUPLICATES].sort() : others };
    }),
  }));
}

/** The extra copies of the groups the user hasn't unticked, by lower-cased path: what a plan with duplicates deletes. */
export function deletedExtras(found: MockDuplicates | null, skipped: readonly string[]): Set<string> {
  return new Set(
    (found?.duplicates.groups ?? [])
      .filter((g) => !skipped.includes(g.id))
      .flatMap((g) => g.extras.map((e) => e.path.toLowerCase())),
  );
}

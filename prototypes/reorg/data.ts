// THROWAWAY: synthetic, in-memory reorganization data for the prototype. Nothing here reads or writes a file.
// The numbers follow the Windows engine's limits: loose regular files at the top of Downloads, Desktop and
// Documents only (at most 512 looked at in one go), moves on the same drive, never an overwrite (a name already
// taken gets " (1)"), nothing deleted, and Undo puts back what it can and removes only empty folders it made.
import type { Proposal } from '../../src/bridge';

export const HOME = 'C:\\Users\\alex';

/** A folder the sort moves files into, relative to the folder being sorted. */
export interface Dest {
  id: string;
  /** "Images\\2026". */
  path: string;
  kind: 'Images' | 'Videos' | 'Documents' | 'Installers' | 'Archives' | 'Audio' | 'Sort later';
  /** The year part, or null for a kind-only folder. */
  year: string | null;
  count: number;
  bytes: number;
  /** A few of its files, after their move. */
  samples: string[];
  /** Files that get a " (1)" because a file there already has the name. */
  renamed: { from: string; to: string }[];
  /** The kind folder is one the user already has. */
  existing: boolean;
}

export interface Stay {
  name: string;
  why: string;
  /** Dropped by the engine's checks (true) or left by the rule (false). */
  dropped: boolean;
  count: number;
}

export interface Reorg {
  id: 'downloads' | 'desktop' | 'documents';
  folder: string;
  path: string;
  /** Loose files at the top of the folder now. */
  loose: number;
  dests: Dest[];
  stays: Stay[];
  /** Subfolders already there, left as they are. */
  subfolders: { name: string; files: number }[];
  /** What the simulated run does. */
  run: { skipped: { name: string; reason: string; bytes: number }[] };
  /** What the simulated Undo does. */
  undo: {
    failed: { count: number; reason: string; where: string; names: string[] };
    keptFolders: string[];
  };
  /** The map tile it is on (variant A lights it), if the scan has one. */
  tile: string | null;
}

const MB = 1e6;
const GB = 1e9;

function dest(id: string, path: string, count: number, bytes: number, samples: string[], extra: Partial<Dest> = {}): Dest {
  const [kind, year = null] = path.split('\\') as [Dest['kind'], string?];
  return { id, path, kind: /^\d{4}$/.test(kind) ? 'Documents' : kind, year: /^\d{4}$/.test(kind) ? kind : year, count, bytes, samples, renamed: [], existing: false, ...extra };
}

export const REORGS: Reorg[] = [
  {
    id: 'downloads',
    folder: 'Downloads',
    path: `${HOME}\\Downloads`,
    loose: 188,
    tile: null,
    dests: [
      dest('dl-inst', 'Installers', 41, 10.8 * GB, ['ubuntu-26.04-desktop-amd64.iso', 'VSCodeUserSetup-x64-1.94.exe', 'Docker Desktop Installer (1).exe'], {
        existing: true,
        renamed: [{ from: 'Docker Desktop Installer.exe', to: 'Docker Desktop Installer (1).exe' }],
      }),
      dest('dl-img26', 'Images\\2026', 38, 0.21 * GB, ['IMG_3301.jpg', 'boarding-pass.png', 'floorplan-v3.png']),
      dest('dl-img25', 'Images\\2025', 27, 0.14 * GB, ['IMG_2041.jpg', 'logo-final.svg', 'receipt-0612.png']),
      dest('dl-doc26', 'Documents\\2026', 22, 0.09 * GB, ['statement-2026-09.pdf', 'lease-renewal.pdf', 'W-2 2025.pdf']),
      dest('dl-doc25', 'Documents\\2025', 19, 0.07 * GB, ['insurance-card.pdf', 'resume-2025.docx', 'ticket-48812.pdf']),
      dest('dl-doc24', 'Documents\\2024', 6, 0.02 * GB, ['old-lease.pdf', 'warranty.pdf', 'menu.pdf']),
      dest('dl-arch', 'Archives', 14, 1.1 * GB, ['fonts (1).zip', 'photos-export.zip', 'drivers.7z'], {
        existing: true,
        renamed: [{ from: 'fonts.zip', to: 'fonts (1).zip' }],
      }),
      dest('dl-vid', 'Videos\\2026', 8, 2.3 * GB, ['screen-recording.mp4', 'birthday.mov', 'demo-cut.mp4']),
      dest('dl-aud', 'Audio\\2025', 5, 0.04 * GB, ['voice-memo.m4a', 'podcast-ep12.mp3', 'ringtone.wav']),
      dest('dl-later', 'Sort later', 6, 0.3 * GB, ['data.bin', 'export_1734', 'download (3)']),
    ],
    stays: [
      { name: 'invoice-0425.pdf', why: 'It changed after Vacancy looked, so this idea was dropped.', dropped: true, count: 1 },
      { name: '~$budget.xlsx', why: 'An Office lock file: Excel has budget.xlsx open.', dropped: false, count: 1 },
    ],
    subfolders: [
      { name: 'Installers', files: 23 },
      { name: 'Archives', files: 61 },
    ],
    run: {
      skipped: [
        { name: 'Q3-invoice.pdf', reason: 'It was open in Acrobat when its turn came', bytes: 2.1 * MB },
        { name: 'notes-meeting.docx', reason: 'It changed after you reviewed the plan', bytes: 0.4 * MB },
      ],
    },
    undo: {
      failed: {
        count: 2,
        reason: 'a file now has the original name',
        where: 'Documents\\2026',
        names: ['statement-2026-09.pdf', 'statement-2026-10.pdf'],
      },
      keptFolders: ['Documents\\2026', 'Documents'],
    },
  },
  {
    id: 'desktop',
    folder: 'Desktop',
    path: `${HOME}\\Desktop`,
    loose: 63,
    tile: null,
    dests: [
      dest('dt-img26', 'Images\\2026', 24, 0.11 * GB, ['Screenshot 2026-10-02 141133.png', 'Screenshot 2026-09-28 090412.png', 'wallpaper.jpg']),
      dest('dt-img25', 'Images\\2025', 7, 0.03 * GB, ['Screenshot 2025-12-19 171002.png', 'cat.heic', 'map.png']),
      dest('dt-doc26', 'Documents\\2026', 9, 0.02 * GB, ['todo.txt', 'packing list.docx', 'quote.pdf']),
      dest('dt-arch', 'Archives', 3, 0.2 * GB, ['project-old.zip', 'mods.rar', 'backup.7z']),
      dest('dt-inst', 'Installers', 2, 0.3 * GB, ['OBS-Studio-31.0-Windows.exe', 'gimp-3.0.4-setup.exe']),
      dest('dt-later', 'Sort later', 4, 0.01 * GB, ['thing', 'New Text Document (2)', 'output.dat']),
    ],
    stays: [{ name: '14 shortcuts', why: 'Shortcuts stay on the Desktop, where you put them.', dropped: false, count: 14 }],
    subfolders: [],
    run: { skipped: [] },
    undo: { failed: { count: 0, reason: '', where: '', names: [] }, keptFolders: [] },
  },
  {
    id: 'documents',
    folder: 'Documents',
    path: `${HOME}\\Documents`,
    loose: 35,
    tile: null,
    dests: [
      dest('dc-26', '2026', 8, 0.03 * GB, ['tax-notes.docx', 'car-loan.pdf', 'recipes.docx']),
      dest('dc-25', '2025', 11, 0.04 * GB, ['tax-return (1).pdf', 'budget.xlsx', 'cover-letter.docx'], {
        existing: true,
        renamed: [{ from: 'tax-return.pdf', to: 'tax-return (1).pdf' }],
      }),
      dest('dc-24', '2024', 6, 0.02 * GB, ['moving-checklist.docx', 'quote-2024.pdf', 'minutes.docx']),
      dest('dc-img', 'Images\\2025', 4, 0.02 * GB, ['scan001.jpg', 'id-front.png', 'id-back.png']),
      dest('dc-later', 'Sort later', 5, 0.01 * GB, ['Untitled', 'file.tmp1', 'export']),
    ],
    stays: [{ name: 'desktop.ini', why: 'Windows uses it to show this folder.', dropped: false, count: 1 }],
    subfolders: [
      { name: '2025', files: 3 },
      { name: 'Zoom', files: 41 },
    ],
    run: { skipped: [] },
    undo: { failed: { count: 0, reason: '', where: '', names: [] }, keptFolders: [] },
  },
];

/** Folders a sort would make: new kind folders and every year folder, for the destinations in the plan. */
export function foldersMade(r: Reorg, excluded: readonly string[] = []): number {
  const made = new Set<string>();
  for (const d of r.dests.filter((x) => !excluded.includes(x.id))) {
    const [top = d.path, sub] = d.path.split('\\');
    if (!d.existing) made.add(top);
    if (sub) made.add(`${top}\\${sub}`);
  }
  return made.size;
}

export const movesOf = (r: Reorg) => r.dests.reduce((n, d) => n + d.count, 0);
export const bytesOf = (r: Reorg) => r.dests.reduce((n, d) => n + d.bytes, 0);
export const movedOf = (r: Reorg) => movesOf(r) - r.run.skipped.length;
export const undoneOf = (r: Reorg) => movedOf(r) - r.undo.failed.count;

/** Sample moves for the drawer: the first file of a few folders, and every renamed one. */
export function sampleMoves(r: Reorg): { from: string; to: string; renamed: boolean }[] {
  const renamed = r.dests.flatMap((d) => d.renamed.map((x) => ({ from: x.from, to: `${d.path}\\${x.to}`, renamed: true })));
  const plain = r.dests
    .filter((d) => d.renamed.length === 0)
    .slice(0, 5 - Math.min(2, renamed.length))
    .map((d) => ({ from: d.samples[0]!, to: `${d.path}\\${d.samples[0]!}`, renamed: false }));
  return [...plain.slice(0, 3), ...renamed.slice(0, 2), ...plain.slice(3)];
}

/** The before and after trees, one level of kind folders with their year folders inside. */
export interface TreeNode {
  name: string;
  count?: number;
  tag?: 'new' | 'existing' | 'stays' | 'later';
  children?: TreeNode[];
  file?: boolean;
}

export function beforeTree(r: Reorg): TreeNode {
  return {
    name: r.folder,
    children: [
      ...r.subfolders.map((s) => ({ name: s.name, count: s.files })),
      { name: `${r.loose} loose files`, file: true, count: r.loose },
    ],
  };
}

export function afterTree(r: Reorg): TreeNode {
  const kinds = new Map<string, TreeNode>();
  for (const d of r.dests) {
    const [top = d.path, sub] = d.path.split('\\');
    const node: TreeNode = kinds.get(top) ?? {
      name: top,
      children: [],
      count: 0,
      tag: d.existing ? 'existing' : d.kind === 'Sort later' ? 'later' : 'new',
    };
    if (sub) node.children!.push({ name: sub, count: d.count, tag: 'new' });
    node.count = (node.count ?? 0) + d.count;
    kinds.set(top, node);
  }
  const subs = r.subfolders.filter((s) => !kinds.has(s.name)).map((s) => ({ name: s.name, count: s.files }));
  const stays = r.stays.reduce((n, s) => n + s.count, 0);
  return {
    name: r.folder,
    children: [
      ...[...kinds.values()].map(({ children, ...k }) => (children?.length ? { ...k, children } : k)),
      ...subs,
      ...(stays > 0 ? [{ name: `${stays} ${stays === 1 ? 'file stays' : 'files stay'} loose`, file: true, count: stays, tag: 'stays' as const }] : []),
    ],
  };
}

/** The Delete and Left for you rows the tray shows today, as context: the fixture's own proposals. */
export type { Proposal };

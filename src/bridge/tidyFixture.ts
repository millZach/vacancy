// The mock bridge's Tidy up: what the core's built-in sorting rules would propose for the Windows fixture's
// Downloads, Desktop and Documents, file by file, and what a mock run and a mock Undo of each find. Shaped like the
// approved prototype's numbers (prototype/reorg-ui, variant A). Nothing here reads or moves a file.
import type { Skip, SkipKind, TidyKind, TidyMove, TidyProposal, TidyStay, TidySubfolder } from './types';

/** A destination folder as the fixture writes it: how many files go there, a few of their names, how big. */
interface DestSpec {
  /** Below the folder being sorted: `Images\2026`. */
  dest: string;
  kind: TidyKind;
  count: number;
  mb: number;
  /** The first files' names; the rest are made up from them. */
  samples: string[];
  /** Files that get a numbered suffix: a file there already has the name. */
  renamed?: [from: string, to: string][];
  /** Files a delete proposal takes too, and which one. */
  deletedBy?: { names: string[]; by: string };
}

interface FolderSpec {
  id: string;
  folder: TidyProposal['folder'];
  name: string;
  dests: DestSpec[];
  stays: TidyStay[];
  subfolders: TidySubfolder[];
  existing: string[];
}

/** What a mock run of one folder's sort leaves where it is, and what a mock Undo of it can't put back. */
export interface TidyScript {
  /** Files the run skips, by name, with why. */
  skips: { name: string; reason: string; kind: SkipKind }[];
  /** Files Undo can't put back, by name, with why and where they are. */
  stuck: { name: string; dest: string; reason: string }[];
  /** Folders the run made that Undo keeps, because they still hold those files. */
  kept: string[];
}

export interface TidyFixture {
  proposals: TidyProposal[];
  scripts: Record<string, TidyScript>;
}

/** `photo.jpg` as the n-th made-up name like it: `photo-12.jpg`. */
function another(sample: string, n: number): string {
  const dot = sample.lastIndexOf('.');
  return dot > 0 ? `${sample.slice(0, dot)}-${n}${sample.slice(dot)}` : `${sample}-${n}`;
}

function build(home: string, spec: FolderSpec): TidyProposal {
  const moves: TidyMove[] = [];
  for (const d of spec.dests) {
    const renamed = new Map(d.renamed ?? []);
    const each = Math.round((d.mb * 1e6) / d.count);
    for (let i = 0; i < d.count; i++) {
      const sample = d.samples[i % d.samples.length]!;
      const name = i < d.samples.length ? sample : another(sample, Math.floor(i / d.samples.length) + 1);
      const to = renamed.get(name) ?? name;
      moves.push({
        name,
        dest: d.dest,
        to,
        kind: d.kind,
        bytes: each,
        renamed: to !== name,
        deletedBy: d.deletedBy?.names.includes(name) ? [d.deletedBy.by] : [],
      });
    }
  }
  moves.sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  return {
    id: spec.id,
    folder: spec.folder,
    name: spec.name,
    path: `${home}\\${spec.name}`,
    loose: moves.length + spec.stays.length,
    moves,
    stays: spec.stays,
    subfolders: spec.subfolders,
    existing: spec.existing,
    limit: 512,
    capped: false,
    later: 0,
    layout: 'builtin',
    note: null,
  };
}

const YEAR = 2026;

/** The Windows fixture's Tidy up, for the home `home`. */
export function windowsTidy(home: string): TidyFixture {
  const specs: FolderSpec[] = [
    {
      id: 'tidy-downloads',
      folder: 'downloads',
      name: 'Downloads',
      existing: ['Archives', 'Installers'],
      subfolders: [
        { name: 'Archives', files: 61 },
        { name: 'Installers', files: 23 },
      ],
      stays: [
        { name: 'invoice-0425.pdf', reason: 'It changed after Vacancy looked. So Vacancy left this one out.', dropped: true },
        { name: 'ubuntu-26.04.iso.crdownload', reason: 'It looks like a download that hasn’t finished, or a temporary file.', dropped: false },
      ],
      dests: [
        {
          dest: 'Installers',
          kind: 'installers',
          count: 41,
          mb: 10_800,
          samples: ['VSCodeUserSetup-x64-1.94.exe', 'Docker Desktop Installer.exe', 'SteamSetup.exe', 'VSCodeUserSetup-x64-1.92.exe', 'OBS-Studio-31.0-Windows.exe', 'node-v22.9.0-x64.msi'],
          renamed: [['Docker Desktop Installer.exe', 'Docker Desktop Installer (1).exe']],
          deletedBy: { names: ['SteamSetup.exe', 'VSCodeUserSetup-x64-1.92.exe', 'node-v22.9.0-x64.msi'], by: 'w-installers' },
        },
        { dest: `Images\\${YEAR}`, kind: 'images', count: 38, mb: 210, samples: ['IMG_3301.jpg', 'boarding-pass.png', 'floorplan-v3.png'] },
        { dest: `Images\\${YEAR - 1}`, kind: 'images', count: 27, mb: 140, samples: ['IMG_2041.jpg', 'logo-final.svg', 'receipt-0612.png'] },
        { dest: `Documents\\${YEAR}`, kind: 'documents', count: 22, mb: 90, samples: ['statement-2026-09.pdf', 'lease-renewal.pdf', 'Q3-invoice.pdf', 'notes-meeting.docx', 'statement-2026-10.pdf'] },
        { dest: `Documents\\${YEAR - 1}`, kind: 'documents', count: 19, mb: 70, samples: ['insurance-card.pdf', 'resume-2025.docx', 'ticket-48812.pdf'] },
        { dest: `Documents\\${YEAR - 2}`, kind: 'documents', count: 6, mb: 20, samples: ['old-lease.pdf', 'warranty.pdf', 'menu.pdf'] },
        {
          dest: 'Archives',
          kind: 'archives',
          count: 14,
          mb: 1100,
          samples: ['fonts.zip', 'photos-export.zip', 'drivers.7z'],
          renamed: [['fonts.zip', 'fonts (1).zip']],
        },
        { dest: `Videos\\${YEAR}`, kind: 'videos', count: 8, mb: 2300, samples: ['screen-recording.mp4', 'birthday.mov', 'demo-cut.mp4'] },
        { dest: `Audio\\${YEAR - 1}`, kind: 'audio', count: 5, mb: 40, samples: ['voice-memo.m4a', 'podcast-ep12.mp3', 'ringtone.wav'] },
        { dest: 'Sort later', kind: 'later', count: 6, mb: 300, samples: ['data.bin', 'export_1734', 'download (3)'] },
      ],
    },
    {
      id: 'tidy-desktop',
      folder: 'desktop',
      name: 'Desktop',
      existing: [],
      subfolders: [],
      stays: [
        ...['Steam', 'Visual Studio Code', 'Spotify', 'Discord', 'Microsoft Edge', 'Notion', 'Zoom', 'Figma', 'GIMP', 'OBS Studio', 'Slack', 'Battle.net', 'Blender', 'Epic Games Launcher'].map(
          (n): TidyStay => ({ name: `${n}.lnk`, reason: 'Shortcuts stay where you put them.', dropped: false }),
        ),
        { name: 'desktop.ini', reason: 'Windows uses it to show this folder.', dropped: false },
      ],
      dests: [
        { dest: `Images\\${YEAR}`, kind: 'images', count: 24, mb: 110, samples: ['Screenshot 2026-10-02 141133.png', 'Screenshot 2026-09-28 090412.png', 'wallpaper.jpg'] },
        { dest: `Images\\${YEAR - 1}`, kind: 'images', count: 7, mb: 30, samples: ['Screenshot 2025-12-19 171002.png', 'cat.heic', 'map.png'] },
        { dest: `Documents\\${YEAR}`, kind: 'documents', count: 9, mb: 20, samples: ['todo.txt', 'packing list.docx', 'quote.pdf'] },
        { dest: 'Sort later', kind: 'later', count: 4, mb: 10, samples: ['thing', 'New Text Document (2)', 'output.dat'] },
        { dest: 'Archives', kind: 'archives', count: 3, mb: 200, samples: ['project-old.zip', 'mods.rar', 'backup.7z'] },
        { dest: 'Installers', kind: 'installers', count: 2, mb: 300, samples: ['OBS-Studio-31.0-Windows.exe', 'gimp-3.0.4-setup.exe'] },
      ],
    },
    {
      id: 'tidy-documents',
      folder: 'documents',
      name: 'Documents',
      existing: [String(YEAR - 1)],
      subfolders: [
        { name: String(YEAR - 1), files: 3 },
        { name: 'Zoom', files: 41 },
      ],
      stays: [{ name: 'desktop.ini', reason: 'Windows uses it to show this folder.', dropped: false }],
      dests: [
        { dest: String(YEAR), kind: 'documents', count: 8, mb: 30, samples: ['tax-notes.docx', 'car-loan.pdf', 'recipes.docx'] },
        {
          dest: String(YEAR - 1),
          kind: 'documents',
          count: 11,
          mb: 40,
          samples: ['tax-return.pdf', 'budget.xlsx', 'cover-letter.docx'],
          renamed: [['tax-return.pdf', 'tax-return (1).pdf']],
        },
        { dest: String(YEAR - 2), kind: 'documents', count: 6, mb: 20, samples: ['moving-checklist.docx', 'quote-2024.pdf', 'minutes.docx'] },
        { dest: `Images\\${YEAR - 1}`, kind: 'images', count: 4, mb: 20, samples: ['scan001.jpg', 'id-front.png', 'id-back.png'] },
        { dest: 'Sort later', kind: 'later', count: 5, mb: 10, samples: ['Untitled', 'file.tmp1', 'export'] },
      ],
    },
  ];
  return {
    proposals: specs.map((s) => build(home, s)),
    scripts: {
      'tidy-downloads': {
        skips: [
          { name: 'Q3-invoice.pdf', reason: 'It was open in another app when its turn came', kind: 'inUse' },
          { name: 'notes-meeting.docx', reason: 'It changed after you reviewed the plan', kind: 'changed' },
        ],
        stuck: [
          { name: 'statement-2026-09.pdf', dest: `Documents\\${YEAR}`, reason: 'A file now has its old name' },
          { name: 'statement-2026-10.pdf', dest: `Documents\\${YEAR}`, reason: 'A file now has its old name' },
        ],
        kept: [`Documents\\${YEAR}`, 'Documents'],
      },
      'tidy-desktop': { skips: [], stuck: [], kept: [] },
      'tidy-documents': { skips: [], stuck: [], kept: [] },
    },
  };
}

/** A skip as the core reports a move's: the file's path, why, and its size. */
export function moveSkip(path: string, reason: string, kind: SkipKind, bytes: number): Skip {
  return { path, reason, kind, bytes, items: 1 };
}

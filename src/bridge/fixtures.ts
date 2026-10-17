import type {
  CategoryId,
  DriveKind,
  FullDiskAccess,
  MapCategory,
  MapFolder,
  MapItem,
  Overview,
  Platform,
  VolumeInfo,
} from './types';

/**
 * What the mock bridge answers with. One per platform, with the prototype's datasets (variant D) and drives shaped
 * like the drive picker prototype's: the system drive, a roomy second drive and, on Windows, a USB stick.
 */
export interface Fixture {
  platform: Platform;
  machineName: string;
  /** The drives to offer, the system drive first. */
  volumes: VolumeInfo[];
  fullDiskAccess: FullDiskAccess;
  /** The map a scan of the system drive ends with. Other drives get it scaled to their size. */
  overview: Overview;
  /** What opening a folder tile shows, by tile id. */
  folders: Record<string, MapFolder>;
}

const GB = 1e9;
const DAY = 86_400_000;

/** A tile as the fixtures write it: name, path, gigabytes, days since last used, and its own children. */
interface Spec {
  name: string;
  path?: string;
  gb: number;
  days?: number;
  kind?: MapItem['kind'];
  note?: string;
  files?: number;
  children?: Spec[];
}

interface CategorySpec {
  id: CategoryId;
  name: string;
  blurb: string;
  locked?: boolean;
  items: Spec[];
}

function build(
  volume: VolumeInfo,
  scan: { files: number; folders: number; seconds: number; unreadable: string[] },
  specs: CategorySpec[],
): { overview: Overview; folders: Record<string, MapFolder> } {
  const folders: Record<string, MapFolder> = {};
  const now = Date.now();
  const item = (s: Spec, id: string, category: CategoryId): MapItem => {
    const kind = s.kind ?? 'folder';
    const children = (s.children ?? []).map((c, i) => item(c, `${id}.${i}`, category));
    const out: MapItem = {
      id,
      name: s.name,
      path: s.path ?? '',
      bytes: s.gb * GB,
      files: s.files ?? Math.round(s.gb * 2100),
      count: kind === 'more' ? (s.files ?? 0) : 0,
      lastUsed: s.days === undefined ? null : now - s.days * DAY,
      kind,
      browsable: children.length > 0,
      note: s.note ?? null,
    };
    if (children.length) folders[id] = { item: out, category, children };
    return out;
  };
  const categories: MapCategory[] = specs
    .map((c) => ({
      id: c.id,
      name: c.name,
      blurb: c.blurb,
      protected: c.locked ?? false,
      bytes: c.items.reduce((sum, s) => sum + s.gb * GB, 0),
      items: c.items.map((s, i) => item(s, `fx-${c.id}-${i}`, c.id)).sort((a, b) => b.bytes - a.bytes),
    }))
    .sort((a, b) => b.bytes - a.bytes);
  const hidden = categories.flatMap((c) => c.items).find((i) => i.kind === 'hidden')?.bytes ?? 0;
  const scanned = categories.reduce((sum, c) => sum + c.bytes, 0) - hidden;
  return {
    folders,
    overview: {
      volume,
      freeBytes: volume.freeBytes,
      scan: {
        files: scan.files,
        folders: scan.folders,
        scannedBytes: scanned,
        hiddenBytes: hidden,
        overCountBytes: 0,
        unreadable: scan.unreadable.length * 37,
        unreadableSamples: scan.unreadable,
        linksSkipped: 212,
        hardLinksMerged: 84_120,
        durationMs: scan.seconds * 1000,
      },
      categories,
    },
  };
}

/** A drive as the core reports it, from its capacity and used space in gigabytes. */
function drive(name: string, mountPoint: string, kind: DriveKind, system: boolean, totalGb: number, usedGb: number): VolumeInfo {
  const id = `fx-${name}`;
  return { name, mountPoint, id, kind, system, totalBytes: totalGb * GB, usedBytes: usedGb * GB, freeBytes: (totalGb - usedGb) * GB };
}

const winVolume = drive('Local Disk (C:)', 'C:\\', 'internal', true, 476, 431);
const home = 'C:\\Users\\alex';

const win = build(
  winVolume,
  {
    files: 1_284_551,
    folders: 211_087,
    seconds: 38,
    unreadable: ['C:\\System Volume Information', 'C:\\Program Files\\WindowsApps', 'C:\\Windows\\Temp'],
  },
  [
    {
      id: 'apps',
      name: 'Apps & games',
      blurb: 'Installed programs and games.',
      items: [
        { name: 'Cyberpunk 2077', path: 'C:\\Program Files (x86)\\Steam\\steamapps\\common\\Cyberpunk 2077', gb: 71, days: 430 },
        { name: "Baldur's Gate 3", path: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Baldurs Gate 3", gb: 25, days: 12 },
        {
          name: 'Adobe',
          path: 'C:\\Program Files\\Adobe',
          gb: 9,
          days: 41,
          children: [
            { name: 'Adobe Photoshop 2026', path: 'C:\\Program Files\\Adobe\\Adobe Photoshop 2026', gb: 4.6, days: 41 },
            { name: 'Adobe Premiere Pro 2026', path: 'C:\\Program Files\\Adobe\\Adobe Premiere Pro 2026', gb: 3.1, days: 90 },
            { name: 'Adobe Creative Cloud', path: 'C:\\Program Files\\Adobe\\Adobe Creative Cloud', gb: 1.3, days: 2 },
          ],
        },
        { name: 'Microsoft Office', path: 'C:\\Program Files\\Microsoft Office', gb: 4, days: 1 },
        { name: 'NVIDIA Corporation', path: 'C:\\Program Files\\NVIDIA Corporation', gb: 3.2, days: 6 },
        { name: 'Visual Studio Code', path: `${home}\\AppData\\Local\\Programs\\Microsoft VS Code`, gb: 1.1, days: 0 },
        { name: '212 smaller items', gb: 28.7, kind: 'more', files: 212 },
      ],
    },
    {
      id: 'dev',
      name: 'Developer',
      blurb: 'Code, project dependencies, build output, containers and SDKs.',
      items: [
        {
          name: 'code',
          path: `${home}\\code`,
          gb: 53,
          days: 96,
          children: [
            { name: 'node_modules', path: `${home}\\code\\web-shop\\node_modules`, gb: 21, days: 96 },
            { name: 'target', path: `${home}\\code\\ledger\\target`, gb: 22, days: 120 },
            { name: 'src', path: `${home}\\code\\web-shop\\src`, gb: 0.4, days: 3 },
            { name: '1,840 files in code', path: `${home}\\code`, gb: 9.6, kind: 'files', files: 1840, days: 3 },
          ],
        },
        { name: 'Docker', path: `${home}\\AppData\\Local\\Docker`, gb: 19, days: 33 },
        { name: 'npm-cache', path: `${home}\\AppData\\Local\\npm-cache`, gb: 9 },
        { name: 'Sdk', path: `${home}\\AppData\\Local\\Android\\Sdk`, gb: 7, days: 240 },
      ],
    },
    {
      id: 'media',
      name: 'Photos, videos & documents',
      blurb: 'Your personal files.',
      items: [
        { name: 'Videos', path: `${home}\\Videos`, gb: 41, days: 20 },
        { name: 'Pictures', path: `${home}\\Pictures`, gb: 21, days: 4 },
        { name: 'Documents', path: `${home}\\Documents`, gb: 12, days: 0 },
      ],
    },
    {
      id: 'system',
      name: 'Windows & system',
      blurb: 'Windows itself. Vacancy never touches this.',
      locked: true,
      items: [
        { name: 'Windows', path: 'C:\\Windows', gb: 20, days: 0 },
        { name: 'pagefile.sys', path: 'C:\\pagefile.sys', gb: 8, kind: 'file', days: 0 },
        { name: 'hiberfil.sys', path: 'C:\\hiberfil.sys', gb: 6, kind: 'file', days: 2 },
        {
          name: "Space Vacancy can't see",
          gb: 4,
          kind: 'hidden',
          note: "Windows counts this as used, but Vacancy can't see it: 111 folders it isn't allowed to open (such as System Volume Information and WindowsApps), restore points and the file system's own records.",
        },
      ],
    },
    {
      id: 'downloads',
      name: 'Downloads',
      blurb: '2,184 files you downloaded.',
      items: [
        {
          name: 'Downloads',
          path: `${home}\\Downloads`,
          gb: 36,
          days: 2,
          children: [
            { name: 'Installers', path: `${home}\\Downloads\\Installers`, gb: 14, days: 210 },
            { name: 'ubuntu-26.04-desktop-amd64.iso', path: `${home}\\Downloads\\ubuntu-26.04-desktop-amd64.iso`, gb: 6.2, kind: 'file', days: 150 },
            { name: 'Archives', path: `${home}\\Downloads\\Archives`, gb: 9, days: 150 },
            { name: '2,031 files in Downloads', path: `${home}\\Downloads`, gb: 6.8, kind: 'files', files: 2031, days: 2 },
          ],
        },
      ],
    },
    {
      id: 'caches',
      name: 'Caches & temp',
      blurb: 'Leftovers that Windows and your apps rebuild on their own.',
      items: [
        { name: 'Download', path: 'C:\\Windows\\SoftwareDistribution\\Download', gb: 8.2, days: 30 },
        { name: 'Cache', path: `${home}\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache`, gb: 6.3, days: 0 },
        { name: 'Temp', path: `${home}\\AppData\\Local\\Temp`, gb: 5.1, days: 0 },
        { name: 'DeliveryOptimization', path: 'C:\\Windows\\ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization', gb: 4, days: 9 },
        { name: 'WER', path: 'C:\\ProgramData\\Microsoft\\Windows\\WER', gb: 3.1, days: 40 },
        { name: 'Cache', path: `${home}\\AppData\\Roaming\\discord\\Cache`, gb: 2.3, days: 1 },
      ],
    },
    {
      id: 'cloud',
      name: 'OneDrive & Dropbox',
      blurb: 'Synced folders. Vacancy leaves sync roots alone so nothing disappears from your other devices.',
      locked: true,
      items: [
        { name: 'OneDrive', path: `${home}\\OneDrive`, gb: 11, days: 0 },
        { name: 'Dropbox', path: `${home}\\Dropbox`, gb: 7, days: 3 },
      ],
    },
    {
      id: 'other',
      name: 'Other',
      blurb: 'Everything else.',
      items: [
        { name: 'Package Cache', path: 'C:\\ProgramData\\Package Cache', gb: 4.1, days: 60 },
        { name: 'Public', path: 'C:\\Users\\Public', gb: 1.9, days: 300 },
      ],
    },
  ],
);

const macVolume = drive('Macintosh HD', '/', 'internal', true, 494, 460);
const mhome = '/Users/alex';

const mac = build(
  macVolume,
  {
    files: 2_031_447,
    folders: 402_118,
    seconds: 44,
    unreadable: [`${mhome}/Library/Mail`, `${mhome}/Library/Messages`, `${mhome}/Library/Safari`],
  },
  [
    {
      id: 'dev',
      name: 'Developer',
      blurb: 'Xcode, simulators, code, project dependencies and containers.',
      items: [
        {
          name: 'Developer',
          path: `${mhome}/Library/Developer`,
          gb: 71,
          days: 3,
          children: [
            { name: 'CoreSimulator', path: `${mhome}/Library/Developer/CoreSimulator`, gb: 34, days: 60 },
            { name: 'Xcode', path: `${mhome}/Library/Developer/Xcode`, gb: 37, days: 3 },
          ],
        },
        { name: 'node_modules', path: `${mhome}/code/weatherly/node_modules`, gb: 26, days: 101 },
        { name: 'com.docker.docker', path: `${mhome}/Library/Containers/com.docker.docker`, gb: 18, days: 20 },
        { name: 'homebrew', path: '/opt/homebrew', gb: 6, days: 8 },
      ],
    },
    {
      id: 'photos',
      name: 'Photos Library',
      blurb: 'Your Photos library. Vacancy never edits it; use Optimize Mac Storage in Photos instead.',
      locked: true,
      items: [{ name: 'Photos Library.photoslibrary', path: `${mhome}/Pictures/Photos Library.photoslibrary`, gb: 96, days: 0 }],
    },
    {
      id: 'apps',
      name: 'Applications',
      blurb: 'Apps and the content they install.',
      items: [
        { name: 'Logic', path: '/Library/Application Support/Logic', gb: 31, days: 300 },
        { name: 'Xcode.app', path: '/Applications/Xcode.app', gb: 14, days: 3 },
        { name: 'Adobe Creative Cloud', path: '/Applications/Adobe Creative Cloud', gb: 9, days: 55 },
        { name: '64 smaller items', gb: 10, kind: 'more', files: 64 },
      ],
    },
    {
      id: 'system',
      name: 'macOS & System Data',
      blurb: 'macOS, local Time Machine snapshots and swap. Vacancy never touches this.',
      locked: true,
      items: [
        {
          name: "Space Vacancy can't see",
          gb: 21,
          kind: 'hidden',
          note: "macOS counts this as used, but Vacancy can't see it: 111 folders it isn't allowed to open (such as Mail and Messages), swap, local Time Machine snapshots and macOS's other system volumes.",
        },
        { name: 'macOS', path: '/', gb: 14, days: 10 },
        { name: 'private', path: '/private', gb: 14, days: 0 },
        { name: 'vm', path: '/private/var/vm', gb: 9, days: 0 },
      ],
    },
    {
      id: 'media',
      name: 'Photos, videos & documents',
      blurb: 'Your personal files.',
      items: [
        { name: 'Documents', path: `${mhome}/Documents`, gb: 29, days: 0 },
        { name: 'Desktop', path: `${mhome}/Desktop`, gb: 12, days: 1 },
      ],
    },
    {
      id: 'downloads',
      name: 'Downloads',
      blurb: '1,412 files you downloaded.',
      items: [{ name: 'Downloads', path: `${mhome}/Downloads`, gb: 33, days: 4 }],
    },
    {
      id: 'caches',
      name: 'Caches, logs & Trash',
      blurb: 'Leftovers that macOS and your apps rebuild on their own.',
      items: [
        { name: 'Caches', path: `${mhome}/Library/Caches`, gb: 14, days: 0 },
        { name: 'Cache', path: `${mhome}/Library/Application Support/Slack/Cache`, gb: 6.4, days: 0 },
        { name: 'Logs', path: `${mhome}/Library/Logs`, gb: 3.1, days: 1 },
        { name: 'Mail Downloads', path: `${mhome}/Library/Containers/com.apple.mail/Data/Library/Mail Downloads`, gb: 2.2, days: 18 },
        { name: '.Trash', path: `${mhome}/.Trash`, gb: 1.3, days: 5 },
      ],
    },
    {
      id: 'cloud',
      name: 'iCloud Drive',
      blurb: 'Synced folders. Vacancy leaves sync roots alone so nothing disappears from your other devices.',
      locked: true,
      items: [{ name: 'Mobile Documents', path: `${mhome}/Library/Mobile Documents`, gb: 15, days: 0 }],
    },
    {
      id: 'other',
      name: 'Other',
      blurb: 'Everything else.',
      items: [
        { name: 'Code', path: `${mhome}/Library/Application Support/Code`, gb: 4.2, days: 2 },
        { name: 'Shared', path: '/Users/Shared', gb: 0.8, days: 120 },
      ],
    },
  ],
);

export const fixtures = {
  windows: {
    platform: 'windows',
    machineName: 'DESKTOP-ALEX',
    volumes: [
      winVolume,
      drive('Data (D:)', 'D:\\', 'internal', false, 2000, 700),
      drive('SanDisk (E:)', 'E:\\', 'usb', false, 64, 41),
    ],
    fullDiskAccess: 'notApplicable',
    overview: win.overview,
    folders: win.folders,
  },
  macos: {
    platform: 'macos',
    machineName: 'Alex’s MacBook Air',
    volumes: [macVolume, drive('Samsung T7', '/Volumes/Samsung T7', 'external', false, 1000, 380)],
    fullDiskAccess: 'missing',
    overview: mac.overview,
    folders: mac.folders,
  },
} satisfies Record<Platform, Fixture>;

export type FixtureName = keyof typeof fixtures;

export function isFixtureName(name: string | null): name is FixtureName {
  return name !== null && Object.hasOwn(fixtures, name);
}

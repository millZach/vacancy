import type {
  CategoryId,
  DriveKind,
  FullDiskAccess,
  MapCategory,
  MapFolder,
  MapItem,
  Overview,
  Platform,
  Proposal,
  Reorganized,
  Risk,
  Skip,
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
  /** The cleanups the rules propose for the system drive, biggest first. Each has a tile on the map. */
  proposals: Proposal[];
  /** What opening a folder tile shows, by tile id. */
  folders: Record<string, MapFolder>;
  /**
   * What a mock run of the system drive leaves in place, if the proposal is in the plan: a place in use when its
   * step runs, and one that changed since the scan, which planning finds and leaves out.
   */
  runSkips: RunSkip[];
  /** Runs kept in the history before this session, newest first: when, on which drive, and what they ran. */
  history: PastRun[];
  /**
   * Folders a run reorganizes, for previewing the finish's Moved slide with `?moves=1` before reorganizations
   * arrive (ticket 08). No plan can include them yet.
   */
  moves: Reorganized[];
}

/**
 * Something a mock run leaves in place, found when planning (`plan`) or while its step runs (`run`); or, for a note,
 * something it deletes whose space stays in use.
 */
export interface RunSkip {
  proposal: string;
  when: 'plan' | 'run';
  skip: Skip;
}

/** A run kept in the mock's history: `daysAgo` days before the session, at `hour`, running `proposals`. */
export interface PastRun {
  mountPoint: string;
  daysAgo: number;
  hour: number;
  durationMs: number;
  proposals: string[];
}

const GB = 1e9;
const DAY = 86_400_000;

/**
 * A tile as the fixtures write it: name, path, gigabytes, days since last used, and its own children. A tile with
 * `proposal` is that proposal's tile, the way the core carves it out of the map: named and sized like the proposal,
 * with no single path.
 */
interface Spec {
  name: string;
  path?: string;
  gb: number;
  days?: number;
  kind?: MapItem['kind'];
  note?: string;
  files?: number;
  proposal?: string;
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
    const kind = s.proposal ? 'proposal' : (s.kind ?? 'folder');
    const children = (s.children ?? []).map((c, i) => item(c, `${id}.${i}`, category));
    const out: MapItem = {
      id: s.proposal ? `p:${s.proposal}` : id,
      name: s.name,
      path: s.path ?? '',
      bytes: s.gb * GB,
      files: s.files ?? Math.round(s.gb * 2100),
      count: kind === 'more' ? (s.files ?? 0) : 0,
      lastUsed: s.days === undefined ? null : now - s.days * DAY,
      kind,
      browsable: children.length > 0,
      note: s.note ?? null,
      proposal: s.proposal ?? null,
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

/** A proposal as the fixtures write it. Safe ones are on by default; `on` overrides, as some rules do. */
function proposal(
  p: Pick<Proposal, 'id' | 'category' | 'title' | 'summary' | 'why' | 'regenerable' | 'samplePaths'> & {
    gb: number;
    items: number;
    places: number;
    risk?: Risk;
    on?: boolean;
  },
): Proposal {
  const risk = p.risk ?? 'safe';
  return {
    id: p.id,
    category: p.category,
    title: p.title,
    summary: p.summary,
    why: p.why,
    bytes: p.gb * GB,
    itemCount: p.items,
    placeCount: p.places,
    risk,
    source: 'rule',
    aiNote: null,
    action: 'delete',
    regenerable: p.regenerable,
    samplePaths: p.samplePaths,
    defaultOn: p.on ?? risk === 'safe',
  };
}

/** The proposals, biggest first, the way the core lists them. */
const biggestFirst = (ps: Proposal[]) => [...ps].sort((a, b) => b.bytes - a.bytes);

/** A proposal's tile, sized like the proposal. */
function tileOf(ps: Proposal[], id: string): Spec {
  const p = ps.find((x) => x.id === id);
  if (!p) throw new Error(`no proposal ${id}`);
  return { name: p.title, gb: p.bytes / GB, files: p.itemCount, days: 120, proposal: p.id };
}

/** A drive as the core reports it, from its capacity and used space in gigabytes. */
function drive(name: string, mountPoint: string, kind: DriveKind, system: boolean, totalGb: number, usedGb: number): VolumeInfo {
  const id = `fx-${name}`;
  return { name, mountPoint, id, kind, system, totalBytes: totalGb * GB, usedBytes: usedGb * GB, freeBytes: (totalGb - usedGb) * GB };
}

const winVolume = drive('Local Disk (C:)', 'C:\\', 'internal', true, 476, 431);
const home = 'C:\\Users\\alex';
const local = `${home}\\AppData\\Local`;
const roaming = `${home}\\AppData\\Roaming`;

const winProposals = biggestFirst([
  proposal({
    id: 'w-node',
    category: 'dev',
    title: 'node_modules in 61 idle projects',
    summary: 'Dependencies in projects you haven’t touched for 90 days or more.',
    why: 'Run npm install in a project to bring them back. Projects you used in the last 90 days are left alone.',
    gb: 21,
    items: 412_118,
    places: 61,
    regenerable: true,
    samplePaths: [
      `${home}\\code\\portfolio-2024\\node_modules`,
      `${home}\\code\\hackathon-bot\\node_modules`,
      `${home}\\code\\web-shop\\node_modules`,
    ],
  }),
  proposal({
    id: 'w-rust',
    category: 'dev',
    title: 'Rust build output in 12 projects',
    summary: 'target folders from cargo builds, in projects you haven’t touched for 90 days or more.',
    why: 'cargo build makes them again. Source code is untouched, and projects you used in the last 90 days are left alone.',
    gb: 19.8,
    items: 88_310,
    places: 12,
    regenerable: true,
    samplePaths: [`${home}\\code\\raytracer\\target`, `${home}\\code\\advent-2025\\target`],
  }),
  proposal({
    id: 'w-installers',
    category: 'downloads',
    title: 'Old installers in Downloads',
    summary: '47 .exe and .msi files older than 30 days.',
    why: 'These programs are already installed or can be downloaded again. Check the list if you keep offline installers.',
    gb: 11.9,
    items: 47,
    places: 47,
    risk: 'review',
    on: true,
    regenerable: false,
    samplePaths: [
      `${home}\\Downloads\\VSCodeUserSetup-x64-1.92.exe`,
      `${home}\\Downloads\\SteamSetup.exe`,
      `${home}\\Downloads\\Installers\\Docker Desktop Installer.exe`,
    ],
  }),
  proposal({
    id: 'w-pkgcache',
    category: 'dev',
    title: 'Package manager caches',
    summary: 'Download caches from npm, pip and NuGet.',
    why: 'Package managers download anything they need again. Installed tools and your projects keep working.',
    gb: 8.7,
    items: 135_463,
    places: 4,
    regenerable: true,
    samplePaths: [`${local}\\npm-cache\\_cacache`, `${local}\\pip\\cache\\http`, `${home}\\.nuget\\packages\\newtonsoft.json`],
  }),
  proposal({
    id: 'w-browser',
    category: 'caches',
    title: 'Browser caches',
    summary: 'Pages and images Edge and Chrome saved to load sites faster.',
    why: 'Browsers download these again as you browse. You stay signed in, and history, passwords and bookmarks are untouched.',
    gb: 6.3,
    items: 48_310,
    places: 6,
    regenerable: true,
    samplePaths: [
      `${local}\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data`,
      `${local}\\Google\\Chrome\\User Data\\Default\\Cache\\Cache_Data`,
    ],
  }),
  proposal({
    id: 'w-temp',
    category: 'caches',
    title: 'Temp folders',
    summary: '18,402 temporary files left behind by installers and apps.',
    why: 'Files in Temp are meant to be thrown away. Anything changed in the last day is left alone, and so is anything in use when the plan runs.',
    gb: 4.7,
    items: 18_402,
    places: 214,
    regenerable: true,
    samplePaths: [`${local}\\Temp\\vs_setup_bootstrapper_1234`, `${local}\\Temp\\chocolatey`],
  }),
  proposal({
    id: 'w-appcache',
    category: 'caches',
    title: 'App caches',
    summary: 'Caches that discord, Teams and Spotify rebuild on their own.',
    why: 'Apps fill these again as you use them. Your settings, sign-ins and files are elsewhere and are untouched.',
    gb: 2.3,
    items: 9_871,
    places: 9,
    regenerable: true,
    samplePaths: [`${roaming}\\discord\\Cache\\Cache_Data`, `${roaming}\\Microsoft\\Teams\\Cache`],
  }),
  proposal({
    id: 'w-dumps',
    category: 'caches',
    title: 'Crash dumps & error reports',
    summary: '46 crash dumps and error reports from apps that crashed.',
    why: 'They’re only useful for sending to a developer right after a crash.',
    gb: 0.6,
    items: 46,
    places: 46,
    regenerable: false,
    samplePaths: [`${local}\\CrashDumps\\Discord.exe.18244.dmp`],
  }),
]);

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
        { name: 'Visual Studio Code', path: `${local}\\Programs\\Microsoft VS Code`, gb: 1.1, days: 0 },
        { name: '212 smaller items', gb: 28.7, kind: 'more', files: 212 },
      ],
    },
    {
      id: 'dev',
      name: 'Developer',
      blurb: 'Code, project dependencies, build output, containers and SDKs.',
      items: [
        tileOf(winProposals, 'w-node'),
        tileOf(winProposals, 'w-rust'),
        {
          name: 'code',
          path: `${home}\\code`,
          gb: 12.2,
          days: 3,
          children: [
            { name: 'target', path: `${home}\\code\\ledger\\target`, gb: 2.2, days: 3 },
            { name: 'src', path: `${home}\\code\\web-shop\\src`, gb: 0.4, days: 3 },
            { name: '1,840 files in code', path: `${home}\\code`, gb: 9.6, kind: 'files', files: 1840, days: 3 },
          ],
        },
        { name: 'Docker', path: `${local}\\Docker`, gb: 19, days: 33 },
        tileOf(winProposals, 'w-pkgcache'),
        { name: 'npm-cache', path: `${local}\\npm-cache`, gb: 0.3, days: 0 },
        { name: 'Sdk', path: `${local}\\Android\\Sdk`, gb: 7, days: 240 },
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
        tileOf(winProposals, 'w-installers'),
        {
          name: 'Downloads',
          path: `${home}\\Downloads`,
          gb: 24.1,
          days: 2,
          children: [
            { name: 'Installers', path: `${home}\\Downloads\\Installers`, gb: 2.1, days: 12 },
            { name: 'ubuntu-26.04-desktop-amd64.iso', path: `${home}\\Downloads\\ubuntu-26.04-desktop-amd64.iso`, gb: 6.2, kind: 'file', days: 150 },
            { name: 'Archives', path: `${home}\\Downloads\\Archives`, gb: 9, days: 150 },
            { name: '1,984 files in Downloads', path: `${home}\\Downloads`, gb: 6.8, kind: 'files', files: 1984, days: 2 },
          ],
        },
      ],
    },
    {
      id: 'caches',
      name: 'Caches & temp',
      blurb: 'Leftovers that Windows and your apps rebuild on their own.',
      items: [
        { name: 'Download', path: 'C:\\Windows\\SoftwareDistribution\\Download', gb: 7.6, days: 30 },
        tileOf(winProposals, 'w-browser'),
        tileOf(winProposals, 'w-temp'),
        { name: 'Temp', path: `${local}\\Temp`, gb: 0.4, days: 0 },
        { name: 'DeliveryOptimization', path: 'C:\\Windows\\ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization', gb: 4, days: 9 },
        { name: 'WER', path: 'C:\\ProgramData\\Microsoft\\Windows\\WER', gb: 3.1, days: 40 },
        tileOf(winProposals, 'w-appcache'),
        tileOf(winProposals, 'w-dumps'),
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
const mlib = `${mhome}/Library`;

const macProposals = biggestFirst([
  proposal({
    id: 'm-derived',
    category: 'dev',
    title: 'Xcode DerivedData',
    summary: 'Build files for 87 projects you opened in Xcode.',
    why: 'Xcode rebuilds this on your next build. It’s the same as Clean Build Folder for every project. Left alone while Xcode is open.',
    gb: 27,
    items: 214_330,
    places: 87,
    regenerable: true,
    samplePaths: [`${mlib}/Developer/Xcode/DerivedData/Weatherly-abcxyz`, `${mlib}/Developer/Xcode/DerivedData/ModuleCache.noindex`],
  }),
  proposal({
    id: 'm-node',
    category: 'dev',
    title: 'node_modules in 52 idle projects',
    summary: 'Dependencies in projects you haven’t touched for 90 days or more.',
    why: 'Run npm install in a project to bring them back. Projects you used in the last 90 days are left alone.',
    gb: 21.4,
    items: 388_002,
    places: 52,
    regenerable: true,
    samplePaths: [`${mhome}/code/portfolio-2024/node_modules`, `${mhome}/code/hackathon-bot/node_modules`],
  }),
  proposal({
    id: 'm-dmgs',
    category: 'downloads',
    title: 'Old disk images in Downloads',
    summary: '38 .dmg and .pkg files older than 30 days.',
    why: 'These apps are installed or can be downloaded again.',
    gb: 11.6,
    items: 38,
    places: 38,
    risk: 'review',
    on: true,
    regenerable: false,
    samplePaths: [`${mhome}/Downloads/Docker.dmg`, `${mhome}/Downloads/googlechrome.dmg`, `${mhome}/Downloads/Figma-124.5.dmg`],
  }),
  proposal({
    id: 'm-archives',
    category: 'dev',
    title: 'Xcode archives older than a year',
    summary: '31 archives of builds you already shipped.',
    why: 'Keep any archive you might need to symbolicate crash reports for an old release.',
    gb: 8,
    items: 2_480,
    places: 31,
    risk: 'review',
    regenerable: false,
    samplePaths: [`${mlib}/Developer/Xcode/Archives/2025-03-14/Weatherly 2.1.xcarchive`],
  }),
  proposal({
    id: 'm-caches',
    category: 'caches',
    title: 'Library caches',
    summary: 'Caches in your Library, from 61 apps.',
    why: 'Apps rebuild caches as needed. Caches of apps that are running are left alone.',
    gb: 7,
    items: 61_204,
    places: 61,
    regenerable: true,
    samplePaths: [`${mlib}/Caches/com.spotify.client/Data`, `${mlib}/Caches/Google/Chrome/Default`],
  }),
  proposal({
    id: 'm-appcache',
    category: 'caches',
    title: 'App caches',
    summary: 'Caches that Slack, Spotify and discord rebuild on their own.',
    why: 'Apps fill these again as you use them. Apps that are running are left alone, and your settings and files are untouched.',
    gb: 6.4,
    items: 14_022,
    places: 5,
    regenerable: true,
    samplePaths: [`${mlib}/Application Support/Slack/Cache/Cache_Data`, `${mlib}/Application Support/discord/Cache/Cache_Data`],
  }),
  proposal({
    id: 'm-brew',
    category: 'dev',
    title: 'Homebrew & package caches',
    summary: 'Downloaded bottles and package archives from Homebrew and npm.',
    why: 'Same as brew cleanup: installed packages keep working, and package managers download anything they need again.',
    gb: 5.6,
    items: 418,
    places: 2,
    regenerable: true,
    samplePaths: [`${mlib}/Caches/Homebrew/downloads`, `${mhome}/.npm/_cacache/content-v2`],
  }),
  proposal({
    id: 'm-logs',
    category: 'caches',
    title: 'Logs & diagnostic reports',
    summary: '3,310 log files and crash reports from apps.',
    why: 'They’re only useful right after a crash. macOS and your apps keep writing new ones.',
    gb: 3.1,
    items: 3_310,
    places: 14,
    regenerable: false,
    samplePaths: [`${mlib}/Logs/DiagnosticReports`, `${mlib}/Logs/CoreSimulator`],
  }),
  proposal({
    id: 'm-mail',
    category: 'caches',
    title: 'Mail attachment downloads',
    summary: 'Copies of 912 attachments you opened from Mail.',
    why: 'The originals stay in your mailbox. Mail downloads them again when you open them.',
    gb: 2.2,
    items: 912,
    places: 1,
    regenerable: true,
    samplePaths: [`${mlib}/Containers/com.apple.mail/Data/Library/Mail Downloads`],
  }),
  proposal({
    id: 'm-trash',
    category: 'caches',
    title: 'Empty the Trash',
    summary: '214 files already in the Trash.',
    why: 'You already threw these away.',
    gb: 1.3,
    items: 214,
    places: 214,
    regenerable: false,
    samplePaths: [`${mhome}/.Trash`],
  }),
]);

const mac = build(
  macVolume,
  {
    files: 2_031_447,
    folders: 402_118,
    seconds: 44,
    unreadable: [`${mlib}/Mail`, `${mlib}/Messages`, `${mlib}/Safari`],
  },
  [
    {
      id: 'dev',
      name: 'Developer',
      blurb: 'Xcode, simulators, code, project dependencies and containers.',
      items: [
        {
          name: 'Developer',
          path: `${mlib}/Developer`,
          gb: 36,
          days: 3,
          children: [
            { name: 'CoreSimulator', path: `${mlib}/Developer/CoreSimulator`, gb: 34, days: 60 },
            { name: 'Xcode', path: `${mlib}/Developer/Xcode`, gb: 2, days: 3 },
          ],
        },
        tileOf(macProposals, 'm-derived'),
        tileOf(macProposals, 'm-node'),
        { name: 'com.docker.docker', path: `${mlib}/Containers/com.docker.docker`, gb: 18, days: 20 },
        tileOf(macProposals, 'm-archives'),
        { name: 'homebrew', path: '/opt/homebrew', gb: 6, days: 8 },
        tileOf(macProposals, 'm-brew'),
        { name: 'code', path: `${mhome}/code`, gb: 4.6, days: 2 },
        { name: 'Code', path: `${mlib}/Application Support/Code`, gb: 4.2, days: 2 },
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
      items: [tileOf(macProposals, 'm-dmgs'), { name: 'Downloads', path: `${mhome}/Downloads`, gb: 21.4, days: 4 }],
    },
    {
      id: 'caches',
      name: 'Caches, logs & Trash',
      blurb: 'Leftovers that macOS and your apps rebuild on their own.',
      items: [
        tileOf(macProposals, 'm-caches'),
        tileOf(macProposals, 'm-appcache'),
        tileOf(macProposals, 'm-logs'),
        tileOf(macProposals, 'm-mail'),
        { name: 'Caches', path: `${mlib}/Caches`, gb: 1.4, days: 0 },
        tileOf(macProposals, 'm-trash'),
      ],
    },
    {
      id: 'cloud',
      name: 'iCloud Drive',
      blurb: 'Synced folders. Vacancy leaves sync roots alone so nothing disappears from your other devices.',
      locked: true,
      items: [{ name: 'Mobile Documents', path: `${mlib}/Mobile Documents`, gb: 15, days: 0 }],
    },
    {
      id: 'other',
      name: 'Other',
      blurb: 'Everything else.',
      items: [{ name: 'Shared', path: '/Users/Shared', gb: 0.8, days: 120 }],
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
    proposals: [...winProposals, {
      id: 'w-steam-advice', category: 'apps', title: 'Cyberpunk 2077 (not played for 430 days)',
      summary: 'Use Steam to uninstall this game if you no longer play it.',
      why: 'Uninstall from Steam so its library stays consistent. Vacancy won’t delete the game or open Steam during a run.',
      bytes: 0, itemCount: 0, placeCount: 0, risk: 'review', source: 'rule', aiNote: null,
      action: 'hint', hintAction: { kind: 'steamUninstall', appId: 1091500 }, regenerable: false, samplePaths: [], defaultOn: false,
    }],
    folders: win.folders,
    runSkips: [
      {
        proposal: 'w-browser',
        when: 'run',
        skip: {
          path: `${local}\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data`,
          reason: '212 files are open in Microsoft Edge',
          kind: 'inUse',
          bytes: 0.41 * GB,
          items: 212,
        },
      },
      {
        proposal: 'w-installers',
        when: 'plan',
        skip: {
          path: `${home}\\Downloads\\Installers\\Docker Desktop Installer.exe`,
          reason: 'It changed since the scan',
          kind: 'changed',
          bytes: 0.62 * GB,
          items: 1,
        },
      },
      {
        proposal: 'w-pkgcache',
        when: 'run',
        skip: {
          path: `${local}\\npm-cache\\_cacache`,
          reason: '38 files are also kept elsewhere under other names (hard links), so their space is still in use',
          kind: 'hardLinked',
          bytes: 0.09 * GB,
          items: 38,
        },
      },
    ],
    history: [
      { mountPoint: 'C:\\', daysAgo: 12, hour: 18, durationMs: 41_600, proposals: ['w-temp', 'w-browser', 'w-dumps'] },
      { mountPoint: 'D:\\', daysAgo: 37, hour: 21, durationMs: 128_300, proposals: ['w-node', 'w-rust'] },
    ],
    moves: [
      { id: 'r-downloads', folder: 'Downloads', movedItems: 812 },
      { id: 'r-desktop', folder: 'Desktop', movedItems: 216 },
    ],
  },
  macos: {
    platform: 'macos',
    machineName: 'Alex’s MacBook Air',
    volumes: [macVolume, drive('Samsung T7', '/Volumes/Samsung T7', 'external', false, 1000, 380)],
    fullDiskAccess: 'missing',
    overview: mac.overview,
    proposals: [...macProposals, {
      id: 'm-logic-advice', category: 'apps', title: 'Logic Pro sound library',
      summary: 'Remove sounds you don’t use from inside Logic Pro.',
      why: "In Logic Pro, open Sound Library and review your downloaded content (Manage Packs in Logic Pro 12; Sound Library Manager in earlier versions). Use the app's controls for content you no longer need, or relocate the library through Logic. Existing projects may need these sounds. Shared or cloned content means the library's size is not a promise of space recovered.",
      bytes: 0, itemCount: 0, placeCount: 0, risk: 'review', source: 'rule', aiNote: null,
      action: 'hint', hintAction: { kind: 'logicInstructions' }, regenerable: false, samplePaths: [], defaultOn: false,
    }],
    folders: mac.folders,
    runSkips: [
      {
        proposal: 'm-appcache',
        when: 'run',
        skip: {
          path: `${mlib}/Application Support/Slack/Cache/Cache_Data`,
          reason: '48 files are open in Slack',
          kind: 'inUse',
          bytes: 0.58 * GB,
          items: 48,
        },
      },
      {
        proposal: 'm-dmgs',
        when: 'plan',
        skip: {
          path: `${mhome}/Downloads/Docker.dmg`,
          reason: 'It changed since the scan',
          kind: 'changed',
          bytes: 0.71 * GB,
          items: 1,
        },
      },
      {
        proposal: 'm-brew',
        when: 'run',
        skip: {
          path: `${mhome}/.npm/_cacache/content-v2`,
          reason: '14 files are also kept elsewhere under other names (hard links), so their space is still in use',
          kind: 'hardLinked',
          bytes: 0.06 * GB,
          items: 14,
        },
      },
    ],
    history: [
      { mountPoint: '/', daysAgo: 9, hour: 9, durationMs: 52_900, proposals: ['m-caches', 'm-logs'] },
      { mountPoint: '/Volumes/Samsung T7', daysAgo: 44, hour: 20, durationMs: 23_100, proposals: ['m-node'] },
    ],
    moves: [
      { id: 'r-downloads', folder: 'Downloads', movedItems: 640 },
      { id: 'r-desktop', folder: 'Desktop', movedItems: 133 },
    ],
  },
} satisfies Record<Platform, Fixture>;

export type FixtureName = keyof typeof fixtures;

export function isFixtureName(name: string | null): name is FixtureName {
  return name !== null && Object.hasOwn(fixtures, name);
}

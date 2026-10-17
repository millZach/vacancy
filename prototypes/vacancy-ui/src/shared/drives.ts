// PROTOTYPE fake drives for the drive-picker prototype (variant D, ?picker=1|2|3). Nothing here touches a real disk.
// Only internal, USB and external drives are listed: Vacancy skips network and optical drives.
// The system drive uses the full dataset from data.ts. On Windows it is grown to a nearly full 1 TB disk (the extra
// items are added to the existing categories, so the existing proposals are unchanged). The other drives get small
// stand-in datasets, enough to show that switching drives works; they are prototype data, not a design for those maps.
import { GB, datasets, type CategoryItem, type Dataset, type OS } from './data';

export type DriveKind = 'internal' | 'usb' | 'external';

export interface Drive {
  id: string;
  /** Shown wherever the drive is named, e.g. "Windows-SSD (C:)" or "Samsung T7". */
  label: string;
  /** Compact name for tabs and chips: the letter on Windows, a short name on macOS. */
  short: string;
  /** Drive letter or mount point. */
  mount: string;
  kind: DriveKind;
  /** What it physically is, for captions. */
  media: string;
  /** The drive the OS runs from. */
  system: boolean;
  removable: boolean;
  totalBytes: number;
  usedBytes: number;
  data: Dataset;
  /** Small made-up map data, labelled as such in code. */
  standIn: boolean;
}

export const KIND_LABEL: Record<DriveKind, string> = { internal: 'Internal', usb: 'USB', external: 'External' };

const TB = 1000 * GB;
const sumBytes = (d: Dataset) => d.categories.reduce((s, c) => s + c.bytes, 0);

/** The existing dataset on a bigger disk: extra items go into existing categories, category totals follow. */
function grown(d: Dataset, name: string, totalBytes: number, scan: Dataset['scan'], extra: Record<string, CategoryItem[]>): Dataset {
  const categories = d.categories.map((c) => {
    const add = extra[c.id] ?? [];
    return add.length ? { ...c, bytes: c.bytes + add.reduce((s, i) => s + i.bytes, 0), items: [...c.items, ...add] } : c;
  });
  const out: Dataset = { ...d, scan, categories, disk: { name, totalBytes, usedBytes: 0 } };
  out.disk.usedBytes = sumBytes(out);
  return out;
}

// ---------- Windows ----------

const winC = grown(
  datasets.win,
  'Windows-SSD (C:)',
  1 * TB,
  { files: 2_412_906, folders: 388_140, durationSec: 61, aiAgent: 'Claude Code' },
  {
    apps: [
      { name: 'Microsoft Flight Simulator 2024', path: 'C:\\XboxGames\\Microsoft Flight Simulator 2024', bytes: 64 * GB, lastUsedDays: 160 },
      { name: 'Call of Duty (Battle.net)', path: 'C:\\Program Files (x86)\\Call of Duty', bytes: 58 * GB, lastUsedDays: 30 },
      { name: 'Forza Horizon 5 (Xbox app)', path: 'C:\\XboxGames\\Forza Horizon 5', bytes: 58 * GB, lastUsedDays: 95 },
    ],
    media: [
      { name: 'GoPro footage', path: 'C:\\Users\\alex\\Videos\\GoPro', bytes: 62 * GB },
      { name: 'Wedding video (raw)', path: 'C:\\Users\\alex\\Videos\\Wedding raw', bytes: 54 * GB },
      { name: 'iPhone backups', path: 'C:\\Users\\alex\\Apple\\MobileSync\\Backup', bytes: 54 * GB, lastUsedDays: 300 },
      { name: 'OBS recordings', path: 'C:\\Users\\alex\\Videos\\OBS', bytes: 41 * GB },
    ],
    dev: [
      { name: 'WSL Ubuntu disk', path: 'C:\\Users\\alex\\AppData\\Local\\Packages\\CanonicalGroupLimited.Ubuntu\\LocalState\\ext4.vhdx', bytes: 52 * GB, lastUsedDays: 14 },
      { name: 'Visual Studio 2022 & SDKs', path: 'C:\\Program Files\\Microsoft Visual Studio', bytes: 38 * GB },
      { name: 'Unity Hub & editors', path: 'C:\\Program Files\\Unity\\Hub', bytes: 30 * GB, lastUsedDays: 70 },
    ],
    system: [
      { name: 'Component store (WinSxS)', path: 'C:\\Windows\\WinSxS', bytes: 14 * GB },
      { name: 'Restore points', path: 'C:\\System Volume Information', bytes: 17 * GB },
    ],
    downloads: [{ name: 'Disk images (.iso)', path: 'C:\\Users\\alex\\Downloads\\*.iso', bytes: 20 * GB, lastUsedDays: 260 }],
  },
);

/** PROTOTYPE stand-in data for a roomy second internal drive. */
const winD: Dataset = {
  os: 'win',
  machineName: 'DESKTOP-ALEX',
  userName: 'alex',
  disk: { name: 'Data (D:)', totalBytes: 2 * TB, usedBytes: 700 * GB },
  scan: { files: 412_330, folders: 38_904, durationSec: 22, aiAgent: 'Claude Code' },
  categories: [
    {
      id: 'games', name: 'Steam library', bytes: 318 * GB, hue: 265,
      blurb: 'A second Steam library folder.',
      items: [
        { name: 'Starfield', path: 'D:\\SteamLibrary\\steamapps\\common\\Starfield', bytes: 132 * GB, lastUsedDays: 410 },
        { name: 'Red Dead Redemption 2', path: 'D:\\SteamLibrary\\steamapps\\common\\Red Dead Redemption 2', bytes: 118 * GB, lastUsedDays: 60 },
        { name: 'Elden Ring', path: 'D:\\SteamLibrary\\steamapps\\common\\ELDEN RING', bytes: 49 * GB, lastUsedDays: 20 },
        { name: 'Download leftovers', path: 'D:\\SteamLibrary\\steamapps\\downloading', bytes: 19 * GB, lastUsedDays: 200 },
      ],
    },
    {
      id: 'media', name: 'Photo & video archive', bytes: 236 * GB, hue: 330,
      blurb: 'Years of camera imports.',
      items: [
        { name: 'Camera imports 2019-2024', path: 'D:\\Archive\\Photos', bytes: 142 * GB },
        { name: 'Family videos', path: 'D:\\Archive\\Video', bytes: 94 * GB },
      ],
    },
    {
      id: 'backups', name: 'Backups', bytes: 112 * GB, hue: 40,
      blurb: 'System images and File History.',
      items: [
        { name: 'Macrium system images (2023)', path: 'D:\\Backups\\Macrium', bytes: 74 * GB, lastUsedDays: 640 },
        { name: 'File History', path: 'D:\\FileHistory', bytes: 38 * GB, lastUsedDays: 1 },
      ],
    },
    {
      id: 'other', name: 'Other', bytes: 34 * GB, hue: 0,
      blurb: 'Everything else on D:.',
      items: [
        { name: 'ISO images', path: 'D:\\ISOs', bytes: 21 * GB, lastUsedDays: 380 },
        { name: 'Miscellaneous', path: 'D:\\', bytes: 13 * GB },
      ],
    },
  ],
  proposals: [
    {
      id: 'd-steamdl', categoryId: 'games', title: 'Steam download leftovers',
      summary: 'Half-finished downloads and update chunks Steam no longer needs.',
      why: 'Steam re-downloads anything it still wants.',
      bytes: 19 * GB, itemCount: 36, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['D:\\SteamLibrary\\steamapps\\downloading', 'D:\\SteamLibrary\\steamapps\\temp'], defaultOn: true,
    },
    {
      id: 'd-macrium', categoryId: 'backups', title: 'Old system images from 2023',
      summary: 'Three full images of a PC you replaced.',
      why: 'Vacancy asked Claude Code about this folder. The images belong to a different computer.',
      aiNote: 'D:\\Backups\\Macrium holds three .mrimg images named LAPTOP-OLD, last written in January 2023. This PC is DESKTOP-ALEX, so they cannot restore it. Keep them only if you still want files from that old laptop.',
      bytes: 74 * GB, itemCount: 3, risk: 'review', source: 'ai', action: 'delete', regenerable: false,
      samplePaths: ['D:\\Backups\\Macrium\\LAPTOP-OLD-00-00.mrimg'], defaultOn: false,
    },
    {
      id: 'd-dupes', categoryId: 'media', title: 'Duplicate camera imports',
      summary: '1,940 photos imported twice. The first copy of each is kept.',
      why: 'Byte-for-byte identical files.',
      bytes: 23.4 * GB, itemCount: 1_940, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['D:\\Archive\\Photos\\2021\\IMG_0412.JPG = D:\\Archive\\Photos\\Import 2\\IMG_0412.JPG'], defaultOn: false,
    },
    {
      id: 'd-iso', categoryId: 'other', title: 'Old Windows ISOs',
      summary: 'Install images for Windows versions you no longer run.',
      why: 'Microsoft offers current images for download.',
      bytes: 14.6 * GB, itemCount: 3, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['D:\\ISOs\\Win10_22H2_English_x64.iso', 'D:\\ISOs\\Win11_23H2_x64.iso'], defaultOn: true,
    },
    {
      id: 'd-starfield', categoryId: 'games', title: 'Starfield (not played in 13 months)',
      summary: 'Uninstall it from Steam; saves sync to Steam Cloud.',
      why: 'Vacancy won\'t delete games directly.',
      bytes: 132 * GB, itemCount: 1, risk: 'careful', source: 'rule', action: 'hint', regenerable: true,
      samplePaths: ['D:\\SteamLibrary\\steamapps\\common\\Starfield'], defaultOn: false,
    },
  ],
  reorgs: [],
};

/** PROTOTYPE stand-in data for a USB stick. */
const winE: Dataset = {
  os: 'win',
  machineName: 'DESKTOP-ALEX',
  userName: 'alex',
  disk: { name: 'SanDisk (E:)', totalBytes: 64 * GB, usedBytes: 41 * GB },
  scan: { files: 8_214, folders: 611, durationSec: 4, aiAgent: 'Claude Code' },
  categories: [
    {
      id: 'installers', name: 'Installers & disk images', bytes: 21 * GB, hue: 40,
      blurb: 'Install media and drivers.',
      items: [
        { name: 'Ubuntu & Windows ISOs', path: 'E:\\ISO', bytes: 14 * GB, lastUsedDays: 300 },
        { name: 'Driver installers', path: 'E:\\Drivers', bytes: 7 * GB, lastUsedDays: 120 },
      ],
    },
    {
      id: 'photos', name: 'Photos', bytes: 12 * GB, hue: 330,
      blurb: 'Phone photos copied over last year.',
      items: [{ name: 'Phone photos (copied 2025)', path: 'E:\\DCIM', bytes: 12 * GB, lastUsedDays: 240 }],
    },
    {
      id: 'docs', name: 'Documents', bytes: 6 * GB, hue: 205,
      blurb: 'School and work files.',
      items: [{ name: 'School & work files', path: 'E:\\Documents', bytes: 6 * GB, lastUsedDays: 18 }],
    },
    {
      id: 'other', name: 'Other', bytes: 2 * GB, hue: 0,
      blurb: 'Everything else on the stick.',
      items: [
        { name: 'macOS leftovers (._ files, .Trashes)', path: 'E:\\.Trashes', bytes: 1.4 * GB },
        { name: 'Miscellaneous', path: 'E:\\', bytes: 0.6 * GB },
      ],
    },
  ],
  proposals: [
    {
      id: 'e-trashes', categoryId: 'other', title: 'macOS leftovers on this stick',
      summary: 'A Mac\'s trash folder and ._ shadow files.',
      why: 'Macs create these on any drive they touch. Windows never uses them.',
      bytes: 1.4 * GB, itemCount: 3_120, risk: 'safe', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['E:\\.Trashes', 'E:\\._IMG_2201.JPG'], defaultOn: true,
    },
    {
      id: 'e-iso', categoryId: 'installers', title: 'Old ISO images',
      summary: 'Ubuntu 22.04 and Windows 10 install images.',
      why: 'Both have newer releases you can download.',
      bytes: 11.6 * GB, itemCount: 2, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['E:\\ISO\\ubuntu-22.04.4-desktop-amd64.iso', 'E:\\ISO\\Win10_22H2_x64.iso'], defaultOn: true,
    },
    {
      id: 'e-dupes', categoryId: 'photos', title: 'Photos already on your PC',
      summary: '2,214 photos that are byte-for-byte copies of files in C:\\Users\\alex\\Pictures.',
      why: 'The copies on C: stay. Only the stick\'s duplicates go.',
      bytes: 9.8 * GB, itemCount: 2_214, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['E:\\DCIM\\100APPLE\\IMG_0031.HEIC = C:\\Users\\alex\\Pictures\\2025\\IMG_0031.HEIC'], defaultOn: false,
    },
  ],
  reorgs: [],
};

// ---------- macOS ----------

/** PROTOTYPE stand-in data for an external SSD. */
const macT7: Dataset = {
  os: 'mac',
  machineName: 'Alex’s MacBook Air',
  userName: 'alex',
  disk: { name: 'Samsung T7', totalBytes: 1 * TB, usedBytes: 380 * GB },
  scan: { files: 96_440, folders: 7_102, durationSec: 9, aiAgent: 'Codex' },
  categories: [
    {
      id: 'video', name: 'Final Cut libraries', bytes: 214 * GB, hue: 330,
      blurb: 'Video projects, with their render and proxy media.',
      items: [
        { name: 'Iceland trip.fcpbundle', path: '/Volumes/Samsung T7/Final Cut/Iceland trip.fcpbundle', bytes: 128 * GB, lastUsedDays: 150 },
        { name: 'Wedding edit.fcpbundle', path: '/Volumes/Samsung T7/Final Cut/Wedding edit.fcpbundle', bytes: 86 * GB, lastUsedDays: 40 },
      ],
    },
    {
      id: 'photos', name: 'Photo exports', bytes: 72 * GB, hue: 205,
      blurb: 'Lightroom exports.',
      items: [{ name: 'Lightroom exports', path: '/Volumes/Samsung T7/Exports', bytes: 72 * GB }],
    },
    {
      id: 'backup', name: 'Old MacBook backup', bytes: 61 * GB, hue: 265,
      blurb: 'Folders copied off a 2019 MacBook Pro.',
      items: [{ name: 'MacBook Pro 2019 (copied folders)', path: '/Volumes/Samsung T7/Backup 2022', bytes: 61 * GB, lastUsedDays: 900 }],
    },
    {
      id: 'other', name: 'Other', bytes: 33 * GB, hue: 0,
      blurb: 'Everything else on the drive.',
      items: [
        { name: 'Installers (.dmg)', path: '/Volumes/Samsung T7/Installers', bytes: 21 * GB, lastUsedDays: 220 },
        { name: 'Miscellaneous', path: '/Volumes/Samsung T7', bytes: 12 * GB },
      ],
    },
  ],
  proposals: [
    {
      id: 't7-render', categoryId: 'video', title: 'Final Cut render files',
      summary: 'Render and proxy media Final Cut can rebuild.',
      why: 'Same as Final Cut\'s own Delete Generated Library Files. Your footage and edits stay.',
      bytes: 38.5 * GB, itemCount: 2, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['Iceland trip.fcpbundle/Render Files', 'Wedding edit.fcpbundle/Proxy Media'], defaultOn: true,
    },
    {
      id: 't7-dmg', categoryId: 'other', title: 'Old installers (.dmg)',
      summary: '17 disk images for apps you have installed.',
      why: 'The apps are already in Applications or can be downloaded again.',
      bytes: 21 * GB, itemCount: 17, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['Xcode_15.4.xip', 'Docker.dmg', 'Blender-4.1.dmg'], defaultOn: true,
    },
    {
      id: 't7-backup', categoryId: 'backup', title: 'Folders already in iCloud Drive',
      summary: 'Copies of Documents and Desktop that iCloud already keeps.',
      why: 'Vacancy asked Codex about this folder.',
      aiNote: 'Backup 2022 holds Documents and Desktop from an old MacBook Pro. 9,400 of its 9,812 files match files in your iCloud Drive by name, size and date. The 412 that do not match are kept.',
      bytes: 44 * GB, itemCount: 9_400, risk: 'review', source: 'ai', action: 'delete', regenerable: false,
      samplePaths: ['/Volumes/Samsung T7/Backup 2022/Documents', '/Volumes/Samsung T7/Backup 2022/Desktop'], defaultOn: false,
    },
  ],
  reorgs: [],
};

function drive(d: Omit<Drive, 'totalBytes' | 'usedBytes'>): Drive {
  return { ...d, totalBytes: d.data.disk.totalBytes, usedBytes: d.data.disk.usedBytes };
}

export const drives: Record<OS, Drive[]> = {
  win: [
    drive({ id: 'C', label: 'Windows-SSD (C:)', short: 'C:', mount: 'C:', kind: 'internal', media: 'internal SSD', system: true, removable: false, data: winC, standIn: false }),
    drive({ id: 'D', label: 'Data (D:)', short: 'D:', mount: 'D:', kind: 'internal', media: 'internal drive', system: false, removable: false, data: winD, standIn: true }),
    drive({ id: 'E', label: 'SanDisk (E:)', short: 'E:', mount: 'E:', kind: 'usb', media: 'USB stick', system: false, removable: true, data: winE, standIn: true }),
  ],
  mac: [
    drive({ id: 'hd', label: 'Macintosh HD', short: 'Macintosh HD', mount: '/', kind: 'internal', media: 'internal SSD', system: true, removable: false, data: datasets.mac, standIn: false }),
    drive({ id: 't7', label: 'Samsung T7', short: 'Samsung T7', mount: '/Volumes/Samsung T7', kind: 'external', media: 'external SSD', system: false, removable: true, data: macT7, standIn: true }),
  ],
};

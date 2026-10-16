// PROTOTYPE fake scan data for Vacancy. Shared by every variant so they show the same numbers.
// Nothing here touches a real disk.

export type OS = 'win' | 'mac';
export type Risk = 'safe' | 'review' | 'careful';
export type Source = 'rule' | 'ai';
export type AiAgent = 'Claude Code' | 'Codex';

export const GB = 1e9;

export interface CategoryItem {
  name: string;
  path: string;
  bytes: number;
  /** Days since anything in here was opened or changed. */
  lastUsedDays?: number;
}

export interface Category {
  id: string;
  name: string;
  bytes: number;
  /** Vacancy never proposes changes inside protected categories (OS, Photos library, cloud-sync roots). */
  protected?: boolean;
  /** Suggested hue (0-360) so a variant can colour categories consistently; variants may ignore it. */
  hue: number;
  blurb: string;
  items: CategoryItem[];
}

export interface Proposal {
  id: string;
  categoryId: string;
  title: string;
  /** One line, shown in lists. */
  summary: string;
  /** Why it is safe or what to watch for. Shown on expand. */
  why: string;
  bytes: number;
  itemCount: number;
  risk: Risk;
  source: Source;
  /** Present when source is 'ai': what the local CLI said. */
  aiNote?: string;
  /** 'delete' runs; 'hint' is advice Vacancy can't do itself (uninstall in Steam, etc.). */
  action: 'delete' | 'hint';
  /** Comes back on its own (npm install, cargo build, browser re-downloads). */
  regenerable: boolean;
  samplePaths: string[];
  /** Pre-selected in the plan. */
  defaultOn: boolean;
}

export interface TreeNode {
  name: string;
  /** Files directly or recursively inside, for display. */
  count?: number;
  bytes?: number;
  children?: TreeNode[];
}

export interface Move {
  from: string;
  to: string;
}

export interface ReorgProposal {
  id: string;
  folder: string;
  path: string;
  itemCount: number;
  moveCount: number;
  title: string;
  summary: string;
  aiNote: string;
  before: TreeNode;
  after: TreeNode;
  sampleMoves: Move[];
  defaultOn: boolean;
}

export interface Dataset {
  os: OS;
  machineName: string;
  userName: string;
  disk: { name: string; totalBytes: number; usedBytes: number };
  scan: { files: number; folders: number; durationSec: number; aiAgent: AiAgent | null };
  categories: Category[];
  proposals: Proposal[];
  reorgs: ReorgProposal[];
}

const win: Dataset = {
  os: 'win',
  machineName: 'DESKTOP-ALEX',
  userName: 'alex',
  disk: { name: 'Local Disk (C:)', totalBytes: 476 * GB, usedBytes: 431 * GB },
  scan: { files: 1_284_551, folders: 211_087, durationSec: 38, aiAgent: 'Claude Code' },
  categories: [
    {
      id: 'apps', name: 'Apps & games', bytes: 142 * GB, hue: 265,
      blurb: 'Installed programs and your Steam library.',
      items: [
        { name: 'Cyberpunk 2077 (Steam)', path: 'C:\\Program Files (x86)\\Steam\\steamapps\\common\\Cyberpunk 2077', bytes: 71 * GB, lastUsedDays: 430 },
        { name: "Baldur's Gate 3 (Steam)", path: "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Baldurs Gate 3", bytes: 25 * GB, lastUsedDays: 12 },
        { name: 'Program Files', path: 'C:\\Program Files', bytes: 31 * GB },
        { name: 'Adobe Creative Cloud', path: 'C:\\Program Files\\Adobe', bytes: 9 * GB, lastUsedDays: 41 },
        { name: 'Microsoft Office', path: 'C:\\Program Files\\Microsoft Office', bytes: 4 * GB, lastUsedDays: 1 },
        { name: 'Other apps', path: 'C:\\Program Files (x86)', bytes: 2 * GB },
      ],
    },
    {
      id: 'dev', name: 'Developer', bytes: 88 * GB, hue: 190,
      blurb: 'Project dependencies, build output, containers and SDKs.',
      items: [
        { name: 'node_modules (214 folders)', path: 'C:\\Users\\alex\\code\\**\\node_modules', bytes: 31 * GB, lastUsedDays: 96 },
        { name: 'Rust target/ folders (14)', path: 'C:\\Users\\alex\\code\\**\\target', bytes: 22 * GB, lastUsedDays: 120 },
        { name: 'Docker (WSL disk)', path: 'C:\\Users\\alex\\AppData\\Local\\Docker\\wsl\\disk\\docker_data.vhdx', bytes: 19 * GB, lastUsedDays: 33 },
        { name: 'Package caches (npm, pip, NuGet, cargo)', path: 'C:\\Users\\alex\\AppData\\Local\\npm-cache', bytes: 9 * GB },
        { name: 'Android SDK & emulators', path: 'C:\\Users\\alex\\AppData\\Local\\Android\\Sdk', bytes: 7 * GB, lastUsedDays: 240 },
      ],
    },
    {
      id: 'media', name: 'Photos, videos & documents', bytes: 74 * GB, hue: 330,
      blurb: 'Your personal files.',
      items: [
        { name: 'Videos', path: 'C:\\Users\\alex\\Videos', bytes: 41 * GB },
        { name: 'Pictures', path: 'C:\\Users\\alex\\Pictures', bytes: 21 * GB },
        { name: 'Documents', path: 'C:\\Users\\alex\\Documents', bytes: 12 * GB },
      ],
    },
    {
      id: 'system', name: 'Windows & system', bytes: 38 * GB, hue: 220, protected: true,
      blurb: 'Windows itself. Vacancy never touches this.',
      items: [
        { name: 'Windows', path: 'C:\\Windows', bytes: 24 * GB },
        { name: 'Page file', path: 'C:\\pagefile.sys', bytes: 8 * GB },
        { name: 'Hibernation file', path: 'C:\\hiberfil.sys', bytes: 6 * GB },
      ],
    },
    {
      id: 'downloads', name: 'Downloads', bytes: 36 * GB, hue: 40,
      blurb: '2,184 files, most of them older than a month.',
      items: [
        { name: 'Installers (.exe, .msi)', path: 'C:\\Users\\alex\\Downloads\\*.exe', bytes: 14 * GB, lastUsedDays: 210 },
        { name: 'Archives (.zip, .7z)', path: 'C:\\Users\\alex\\Downloads\\*.zip', bytes: 9 * GB, lastUsedDays: 150 },
        { name: 'Videos', path: 'C:\\Users\\alex\\Downloads\\*.mp4', bytes: 6 * GB },
        { name: 'PDFs & documents', path: 'C:\\Users\\alex\\Downloads\\*.pdf', bytes: 4 * GB },
        { name: 'Everything else', path: 'C:\\Users\\alex\\Downloads', bytes: 3 * GB },
      ],
    },
    {
      id: 'caches', name: 'Caches & temp', bytes: 29 * GB, hue: 150,
      blurb: 'Leftovers that Windows and your apps rebuild on their own.',
      items: [
        { name: 'Windows Update leftovers', path: 'C:\\Windows\\SoftwareDistribution\\Download', bytes: 8.2 * GB },
        { name: 'Browser caches', path: 'C:\\Users\\alex\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache', bytes: 6.3 * GB },
        { name: 'Temp folders', path: 'C:\\Users\\alex\\AppData\\Local\\Temp', bytes: 5.1 * GB },
        { name: 'Delivery Optimization', path: 'C:\\Windows\\ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization', bytes: 4.0 * GB },
        { name: 'Crash dumps & error reports', path: 'C:\\ProgramData\\Microsoft\\Windows\\WER', bytes: 3.1 * GB },
        { name: 'Teams, Discord & Spotify caches', path: 'C:\\Users\\alex\\AppData\\Roaming\\discord\\Cache', bytes: 2.3 * GB },
      ],
    },
    {
      id: 'cloud', name: 'OneDrive & Dropbox', bytes: 18 * GB, hue: 205, protected: true,
      blurb: 'Synced folders. Vacancy leaves sync roots alone so nothing disappears from your other devices.',
      items: [
        { name: 'OneDrive', path: 'C:\\Users\\alex\\OneDrive', bytes: 11 * GB },
        { name: 'Dropbox', path: 'C:\\Users\\alex\\Dropbox', bytes: 7 * GB },
      ],
    },
    {
      id: 'other', name: 'Other', bytes: 6 * GB, hue: 0,
      blurb: 'Everything else, including folders Vacancy asked Claude Code about.',
      items: [
        { name: 'Package Cache (installer copies)', path: 'C:\\ProgramData\\Package Cache', bytes: 4.1 * GB },
        { name: 'Miscellaneous', path: 'C:\\', bytes: 1.9 * GB },
      ],
    },
  ],
  proposals: [
    {
      id: 'w-update', categoryId: 'caches', title: 'Windows Update leftovers',
      summary: 'Old update packages Windows keeps after installing them.',
      why: 'Windows already installed these updates. Its own Disk Cleanup removes the same files.',
      bytes: 8.2 * GB, itemCount: 1_204, risk: 'safe', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['C:\\Windows\\SoftwareDistribution\\Download', 'C:\\Windows\\WinSxS (superseded components)'], defaultOn: true,
    },
    {
      id: 'w-browser', categoryId: 'caches', title: 'Browser caches',
      summary: 'Edge, Chrome and Firefox cached pages and images.',
      why: 'Browsers re-download these as you browse. You stay signed in; history and bookmarks are untouched.',
      bytes: 6.3 * GB, itemCount: 48_310, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['...\\Microsoft\\Edge\\User Data\\Default\\Cache', '...\\Google\\Chrome\\User Data\\Default\\Cache'], defaultOn: true,
    },
    {
      id: 'w-temp', categoryId: 'caches', title: 'Temp folders',
      summary: '18,402 temporary files left behind by installers and apps.',
      why: 'Files in Temp are meant to be thrown away. Anything currently in use is skipped.',
      bytes: 5.1 * GB, itemCount: 18_402, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['C:\\Users\\alex\\AppData\\Local\\Temp', 'C:\\Windows\\Temp'], defaultOn: true,
    },
    {
      id: 'w-delivery', categoryId: 'caches', title: 'Delivery Optimization cache',
      summary: 'Update pieces Windows shares with other PCs.',
      why: 'Purely a sharing cache. Windows refills it only if it needs to.',
      bytes: 4.0 * GB, itemCount: 212, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['...\\DeliveryOptimization\\Cache'], defaultOn: true,
    },
    {
      id: 'w-dumps', categoryId: 'caches', title: 'Crash dumps & error reports',
      summary: '46 crash dumps from apps that crashed months ago.',
      why: 'Only useful for sending to a developer right after a crash.',
      bytes: 3.1 * GB, itemCount: 46, risk: 'safe', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['C:\\ProgramData\\Microsoft\\Windows\\WER\\ReportArchive', 'C:\\Users\\alex\\AppData\\Local\\CrashDumps'], defaultOn: true,
    },
    {
      id: 'w-appcache', categoryId: 'caches', title: 'Teams, Discord & Spotify caches',
      summary: 'Chat media and streaming caches.',
      why: 'The apps rebuild these. Offline Spotify downloads are kept.',
      bytes: 2.3 * GB, itemCount: 9_871, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['...\\discord\\Cache', '...\\Microsoft\\Teams\\Cache', '...\\Spotify\\Data'], defaultOn: true,
    },
    {
      id: 'w-node', categoryId: 'dev', title: 'node_modules in 61 idle projects',
      summary: 'Dependencies in projects you haven\'t touched for 90+ days.',
      why: 'Run npm install in a project to bring them back. Projects you used recently are left alone.',
      bytes: 24.6 * GB, itemCount: 61, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['C:\\Users\\alex\\code\\portfolio-2024\\node_modules', 'C:\\Users\\alex\\code\\hackathon-bot\\node_modules'], defaultOn: true,
    },
    {
      id: 'w-rust', categoryId: 'dev', title: 'Rust build output in 12 projects',
      summary: 'target/ folders from old cargo builds.',
      why: 'cargo build recreates them. Source code is untouched.',
      bytes: 19.8 * GB, itemCount: 12, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['C:\\Users\\alex\\code\\raytracer\\target', 'C:\\Users\\alex\\code\\advent-2025\\target'], defaultOn: true,
    },
    {
      id: 'w-pkgcache', categoryId: 'dev', title: 'Package manager caches',
      summary: 'npm, pip, NuGet and cargo download caches.',
      why: 'Package managers re-download anything they need.',
      bytes: 8.7 * GB, itemCount: 4, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['...\\npm-cache', '...\\pip\\Cache', 'C:\\Users\\alex\\.nuget\\packages'], defaultOn: true,
    },
    {
      id: 'w-docker', categoryId: 'dev', title: 'Unused Docker images & build cache',
      summary: '23 images no container uses, plus old build layers.',
      why: 'Same as docker system prune. Images you still run are kept; others can be pulled again.',
      bytes: 11.4 * GB, itemCount: 23, risk: 'review', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['postgres:14 (2.1 GB)', 'node:18-bullseye (1.0 GB)', 'build cache (6.2 GB)'], defaultOn: false,
    },
    {
      id: 'w-installers', categoryId: 'downloads', title: 'Old installers in Downloads',
      summary: '47 .exe and .msi files older than 30 days.',
      why: 'These programs are already installed or can be downloaded again. Check the list if you keep offline installers.',
      bytes: 11.9 * GB, itemCount: 47, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['VSCodeUserSetup-x64-1.92.exe', 'SteamSetup.exe', 'Docker Desktop Installer.exe'], defaultOn: true,
    },
    {
      id: 'w-dupes', categoryId: 'media', title: 'Duplicate photos & videos',
      summary: '312 exact duplicates. The oldest copy of each is kept.',
      why: 'Byte-for-byte identical files. Vacancy keeps the copy in the most organized folder.',
      bytes: 6.4 * GB, itemCount: 312, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['Pictures\\Phone backup\\IMG_4402.JPG = Pictures\\2025\\Italy\\IMG_4402.JPG', 'Videos\\export (1).mp4 = Videos\\export.mp4'], defaultOn: false,
    },
    {
      id: 'w-android', categoryId: 'dev', title: 'Unused Android emulator images',
      summary: '2 emulators not started in 8 months.',
      why: 'Android Studio can recreate them from the SDK manager in a few minutes.',
      bytes: 5.2 * GB, itemCount: 2, risk: 'review', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['Pixel_6_API_33.avd', 'Pixel_4_API_30.avd'], defaultOn: false,
    },
    {
      id: 'w-pkgcache-ai', categoryId: 'other', title: 'Installer copies for removed programs',
      summary: 'Setup files kept for programs you have since uninstalled.',
      why: 'Vacancy asked Claude Code what this folder is. Copies for programs still installed are kept so they can repair and uninstall.',
      aiNote: 'C:\\ProgramData\\Package Cache holds copies of MSI/MSIX installers so Visual Studio, .NET SDKs and similar can repair or uninstall themselves. 31 of the 44 entries belong to products that are no longer installed (old .NET 6 and 7 SDKs, Visual Studio 2019 Build Tools). Removing those is safe; keep the other 13.',
      bytes: 3.2 * GB, itemCount: 31, risk: 'review', source: 'ai', action: 'delete', regenerable: false,
      samplePaths: ['{a1b2...}\\dotnet-sdk-6.0.428-win-x64.msi', '{c3d4...}\\vs_BuildTools2019.msi'], defaultOn: false,
    },
    {
      id: 'w-cyberpunk', categoryId: 'apps', title: 'Cyberpunk 2077 (not played in 14 months)',
      summary: 'Your biggest single item. Uninstall it from Steam; saves sync to Steam Cloud.',
      why: 'Vacancy won\'t delete games directly. It opens Steam\'s uninstall for you so Steam stays consistent.',
      bytes: 71 * GB, itemCount: 1, risk: 'careful', source: 'rule', action: 'hint', regenerable: true,
      samplePaths: ['C:\\Program Files (x86)\\Steam\\steamapps\\common\\Cyberpunk 2077'], defaultOn: false,
    },
  ],
  reorgs: [
    {
      id: 'w-reorg-downloads', folder: 'Downloads', path: 'C:\\Users\\alex\\Downloads', itemCount: 2_184, moveCount: 1_028,
      title: 'Sort Downloads by what each file is',
      summary: '1,028 loose files into 6 folders. Receipts grouped by sender, screenshots split from photos.',
      aiNote: 'I grouped by content rather than extension: invoices and receipts go under Documents/Receipts/<sender>, manuals under Documents/Manuals, phone photos under Images/Photos, and anything ambiguous goes to "_Sort later" instead of guessing.',
      before: {
        name: 'Downloads', count: 2_184, children: [
          { name: 'setup_v2.3.1.exe' }, { name: 'Invoice-0423.pdf' }, { name: 'IMG_4402.HEIC' }, { name: 'project-final-FINAL.zip' },
          { name: 'Screenshot 2026-09-12 104455.png' }, { name: 'receipt (3).pdf' }, { name: 'tax-return-2025.pdf' }, { name: 'Untitled.png' },
          { name: '...and 2,176 more', count: 2_176 },
        ],
      },
      after: {
        name: 'Downloads', count: 2_184, children: [
          { name: 'Documents', count: 318, children: [{ name: 'Receipts', count: 204 }, { name: 'Manuals', count: 22 }, { name: 'Taxes', count: 9 }, { name: 'Other', count: 83 }] },
          { name: 'Images', count: 508, children: [{ name: 'Screenshots', count: 412 }, { name: 'Photos', count: 96 }] },
          { name: 'Archives', count: 138 },
          { name: 'Video', count: 27 },
          { name: '_Sort later', count: 37 },
          { name: '(1,156 files left in place)', count: 1_156 },
        ],
      },
      sampleMoves: [
        { from: 'Downloads\\Invoice-0423.pdf', to: 'Downloads\\Documents\\Receipts\\Comcast\\Invoice-0423.pdf' },
        { from: 'Downloads\\Screenshot 2026-09-12 104455.png', to: 'Downloads\\Images\\Screenshots\\2026-09\\Screenshot 2026-09-12 104455.png' },
        { from: 'Downloads\\IMG_4402.HEIC', to: 'Downloads\\Images\\Photos\\IMG_4402.HEIC' },
        { from: 'Downloads\\tax-return-2025.pdf', to: 'Downloads\\Documents\\Taxes\\tax-return-2025.pdf' },
      ],
      defaultOn: true,
    },
    {
      id: 'w-reorg-desktop', folder: 'Desktop', path: 'C:\\Users\\alex\\Desktop', itemCount: 148, moveCount: 139,
      title: 'Clear the Desktop into project folders',
      summary: '139 items into Documents\\Projects, grouped by project. Shortcuts stay.',
      aiNote: 'Most Desktop files belong to four projects (wedding, kitchen remodel, resume, D&D campaign); I matched them by name and content. The 9 app shortcuts stay on the Desktop.',
      before: {
        name: 'Desktop', count: 148, children: [
          { name: 'wedding seating v4.xlsx' }, { name: 'cabinet quote.pdf' }, { name: 'Resume_2026_final2.docx' }, { name: 'campaign map.png' },
          { name: 'New folder (7)', count: 12 }, { name: '...and 143 more', count: 143 },
        ],
      },
      after: {
        name: 'Desktop', count: 9, children: [
          { name: '9 shortcuts (unchanged)', count: 9 },
          { name: 'Documents\\Projects', count: 139, children: [{ name: 'Wedding', count: 51 }, { name: 'Kitchen remodel', count: 38 }, { name: 'Resume', count: 14 }, { name: 'D&D campaign', count: 29 }, { name: 'Misc', count: 7 }] },
        ],
      },
      sampleMoves: [
        { from: 'Desktop\\wedding seating v4.xlsx', to: 'Documents\\Projects\\Wedding\\wedding seating v4.xlsx' },
        { from: 'Desktop\\cabinet quote.pdf', to: 'Documents\\Projects\\Kitchen remodel\\cabinet quote.pdf' },
      ],
      defaultOn: false,
    },
  ],
};

const mac: Dataset = {
  os: 'mac',
  machineName: 'Alex’s MacBook Air',
  userName: 'alex',
  disk: { name: 'Macintosh HD', totalBytes: 494 * GB, usedBytes: 460 * GB },
  scan: { files: 2_031_447, folders: 402_118, durationSec: 44, aiAgent: 'Codex' },
  categories: [
    {
      id: 'dev', name: 'Developer', bytes: 121 * GB, hue: 190,
      blurb: 'Xcode, simulators, project dependencies and containers.',
      items: [
        { name: 'iOS Simulators & runtimes', path: '~/Library/Developer/CoreSimulator', bytes: 34 * GB, lastUsedDays: 60 },
        { name: 'Xcode DerivedData', path: '~/Library/Developer/Xcode/DerivedData', bytes: 28 * GB, lastUsedDays: 3 },
        { name: 'node_modules (183 folders)', path: '~/code/**/node_modules', bytes: 26 * GB, lastUsedDays: 101 },
        { name: 'Docker', path: '~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw', bytes: 18 * GB, lastUsedDays: 20 },
        { name: 'Xcode Archives', path: '~/Library/Developer/Xcode/Archives', bytes: 9 * GB, lastUsedDays: 400 },
        { name: 'Homebrew & package caches', path: '~/Library/Caches/Homebrew', bytes: 6 * GB },
      ],
    },
    {
      id: 'photos', name: 'Photos Library', bytes: 96 * GB, hue: 330, protected: true,
      blurb: 'Your Photos library. Vacancy never edits it; use Optimize Mac Storage in Photos instead.',
      items: [{ name: 'Photos Library.photoslibrary', path: '~/Pictures/Photos Library.photoslibrary', bytes: 96 * GB }],
    },
    {
      id: 'apps', name: 'Applications', bytes: 64 * GB, hue: 265,
      blurb: 'Apps and the content they install.',
      items: [
        { name: 'Logic Pro + sound library', path: '/Library/Application Support/Logic', bytes: 31 * GB, lastUsedDays: 300 },
        { name: 'Xcode.app', path: '/Applications/Xcode.app', bytes: 14 * GB, lastUsedDays: 3 },
        { name: 'Adobe apps', path: '/Applications/Adobe Creative Cloud', bytes: 9 * GB, lastUsedDays: 55 },
        { name: 'Other apps', path: '/Applications', bytes: 10 * GB },
      ],
    },
    {
      id: 'system', name: 'macOS & System Data', bytes: 58 * GB, hue: 220, protected: true,
      blurb: 'macOS, local Time Machine snapshots and swap. Vacancy never touches this.',
      items: [
        { name: 'Time Machine local snapshots', path: '/ (APFS snapshots)', bytes: 21 * GB },
        { name: 'macOS', path: '/System', bytes: 14 * GB },
        { name: 'Other system files', path: '/private/var', bytes: 14 * GB },
        { name: 'Swap & sleep image', path: '/private/var/vm', bytes: 9 * GB },
      ],
    },
    {
      id: 'docs', name: 'Documents & Desktop', bytes: 41 * GB, hue: 300,
      blurb: 'Your personal files.',
      items: [
        { name: 'Documents', path: '~/Documents', bytes: 29 * GB },
        { name: 'Desktop', path: '~/Desktop', bytes: 12 * GB },
      ],
    },
    {
      id: 'downloads', name: 'Downloads', bytes: 33 * GB, hue: 40,
      blurb: '1,412 files, most of them older than a month.',
      items: [
        { name: 'Disk images & installers (.dmg, .pkg)', path: '~/Downloads/*.dmg', bytes: 12 * GB, lastUsedDays: 180 },
        { name: 'Archives (.zip)', path: '~/Downloads/*.zip', bytes: 8 * GB, lastUsedDays: 140 },
        { name: 'Videos', path: '~/Downloads/*.mov', bytes: 7 * GB },
        { name: 'PDFs & documents', path: '~/Downloads/*.pdf', bytes: 4 * GB },
        { name: 'Everything else', path: '~/Downloads', bytes: 2 * GB },
      ],
    },
    {
      id: 'caches', name: 'Caches, logs & Trash', bytes: 27 * GB, hue: 150,
      blurb: 'Leftovers that macOS and your apps rebuild on their own.',
      items: [
        { name: 'App caches', path: '~/Library/Caches', bytes: 14 * GB },
        { name: 'Slack, Spotify & Discord caches', path: '~/Library/Application Support/Slack/Cache', bytes: 6.4 * GB },
        { name: 'Logs & diagnostic reports', path: '~/Library/Logs', bytes: 3.1 * GB },
        { name: 'Mail attachment downloads', path: '~/Library/Containers/com.apple.mail/Data/Library/Mail Downloads', bytes: 2.2 * GB },
        { name: 'Trash', path: '~/.Trash', bytes: 1.3 * GB },
      ],
    },
    {
      id: 'cloud', name: 'iCloud Drive (downloaded)', bytes: 15 * GB, hue: 205, protected: true,
      blurb: 'Synced files. Vacancy leaves iCloud Drive alone so nothing disappears from your other devices.',
      items: [{ name: 'iCloud Drive', path: '~/Library/Mobile Documents', bytes: 15 * GB }],
    },
    {
      id: 'other', name: 'Other', bytes: 5 * GB, hue: 0,
      blurb: 'Everything else, including folders Vacancy asked Codex about.',
      items: [
        { name: 'VS Code workspace storage', path: '~/Library/Application Support/Code/User/workspaceStorage', bytes: 4.2 * GB },
        { name: 'Miscellaneous', path: '/', bytes: 0.8 * GB },
      ],
    },
  ],
  proposals: [
    {
      id: 'm-derived', categoryId: 'dev', title: 'Xcode DerivedData',
      summary: 'Build intermediates for every project you have opened in Xcode.',
      why: 'Xcode rebuilds this on your next build. Same as Product › Clean Build Folder for all projects.',
      bytes: 28 * GB, itemCount: 87, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['~/Library/Developer/Xcode/DerivedData/Weatherly-abcxyz', '~/Library/Developer/Xcode/DerivedData/ModuleCache.noindex'], defaultOn: true,
    },
    {
      id: 'm-caches', categoryId: 'caches', title: 'App caches',
      summary: 'Caches in ~/Library/Caches for apps that are not running.',
      why: 'Apps rebuild caches as needed. Caches belonging to running apps are skipped.',
      bytes: 11.8 * GB, itemCount: 61_204, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['~/Library/Caches/com.spotify.client', '~/Library/Caches/Google/Chrome'], defaultOn: true,
    },
    {
      id: 'm-appcache', categoryId: 'caches', title: 'Slack, Spotify & Discord caches',
      summary: 'Chat media and streaming caches.',
      why: 'The apps rebuild these. Offline Spotify downloads are kept.',
      bytes: 6.4 * GB, itemCount: 14_022, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['~/Library/Application Support/Slack/Cache', '~/Library/Application Support/discord/Cache'], defaultOn: true,
    },
    {
      id: 'm-logs', categoryId: 'caches', title: 'Logs & diagnostic reports',
      summary: 'Old app logs and crash reports.',
      why: 'Only useful right after a crash. macOS keeps writing new ones.',
      bytes: 3.1 * GB, itemCount: 3_310, risk: 'safe', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['~/Library/Logs/DiagnosticReports', '~/Library/Logs/CoreSimulator'], defaultOn: true,
    },
    {
      id: 'm-mail', categoryId: 'caches', title: 'Mail attachment downloads',
      summary: 'Copies of attachments you opened from Mail.',
      why: 'The originals stay in your mailbox. Mail downloads them again when you open them.',
      bytes: 2.2 * GB, itemCount: 912, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['~/Library/Containers/com.apple.mail/Data/Library/Mail Downloads'], defaultOn: true,
    },
    {
      id: 'm-trash', categoryId: 'caches', title: 'Empty the Trash',
      summary: '214 items already in the Trash.',
      why: 'You already threw these away.',
      bytes: 1.3 * GB, itemCount: 214, risk: 'safe', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['~/.Trash'], defaultOn: true,
    },
    {
      id: 'm-node', categoryId: 'dev', title: 'node_modules in 52 idle projects',
      summary: 'Dependencies in projects you haven\'t touched for 90+ days.',
      why: 'Run npm install in a project to bring them back. Projects you used recently are left alone.',
      bytes: 21.4 * GB, itemCount: 52, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['~/code/portfolio-2024/node_modules', '~/code/hackathon-bot/node_modules'], defaultOn: true,
    },
    {
      id: 'm-brew', categoryId: 'dev', title: 'Homebrew & package caches',
      summary: 'Downloaded bottles and package archives.',
      why: 'Same as brew cleanup. Installed packages keep working.',
      bytes: 5.6 * GB, itemCount: 418, risk: 'safe', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['~/Library/Caches/Homebrew/downloads', '~/.npm/_cacache'], defaultOn: true,
    },
    {
      id: 'm-sims', categoryId: 'dev', title: 'Old simulator runtimes & devices',
      summary: 'iOS 17 runtimes and 9 unavailable simulator devices.',
      why: 'Your projects target iOS 18+. Xcode can download a runtime again from Settings › Platforms.',
      bytes: 22.7 * GB, itemCount: 11, risk: 'review', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['iOS 17.5 Simulator Runtime (7.4 GB)', 'iPhone 12 mini (unavailable)'], defaultOn: true,
    },
    {
      id: 'm-archives', categoryId: 'dev', title: 'Xcode archives older than a year',
      summary: '31 archives of builds you already shipped.',
      why: 'Keep any archive you might need to symbolicate crash reports for an old release.',
      bytes: 9.0 * GB, itemCount: 31, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['~/Library/Developer/Xcode/Archives/2025-03-14/Weatherly 2.1.xcarchive'], defaultOn: false,
    },
    {
      id: 'm-docker', categoryId: 'dev', title: 'Unused Docker images & build cache',
      summary: '19 images no container uses, plus old build layers.',
      why: 'Same as docker system prune. Images you still run are kept; others can be pulled again.',
      bytes: 10.2 * GB, itemCount: 19, risk: 'review', source: 'rule', action: 'delete', regenerable: true,
      samplePaths: ['postgres:14 (2.1 GB)', 'build cache (5.8 GB)'], defaultOn: false,
    },
    {
      id: 'm-dmgs', categoryId: 'downloads', title: 'Old disk images in Downloads',
      summary: '38 .dmg and .pkg files older than 30 days.',
      why: 'These apps are installed or can be downloaded again.',
      bytes: 11.6 * GB, itemCount: 38, risk: 'review', source: 'rule', action: 'delete', regenerable: false,
      samplePaths: ['Docker.dmg', 'googlechrome.dmg', 'Figma-124.5.dmg'], defaultOn: true,
    },
    {
      id: 'm-vscode-ai', categoryId: 'other', title: 'VS Code state for deleted folders',
      summary: 'Workspace storage for 140 folders that no longer exist.',
      why: 'Vacancy asked Codex what this folder is. Entries for folders that still exist are kept.',
      aiNote: 'workspaceStorage holds per-folder VS Code state (open tabs, search history, extension caches) keyed by a hash of the folder path. 140 of the 171 entries point at folders that are gone, mostly old clones under ~/code/tmp. Deleting those is safe; VS Code recreates state when you reopen a folder.',
      bytes: 3.4 * GB, itemCount: 140, risk: 'review', source: 'ai', action: 'delete', regenerable: true,
      samplePaths: ['workspaceStorage/9f2c.../ (was ~/code/tmp/rust-playground)'], defaultOn: false,
    },
    {
      id: 'm-logic', categoryId: 'apps', title: 'Logic Pro sound library (not opened in 10 months)',
      summary: 'Remove installed sounds from inside Logic; it can download them again any time.',
      why: 'Vacancy won\'t delete app content directly. Use Logic Pro › Sound Library › Delete Installed Sounds.',
      bytes: 31 * GB, itemCount: 1, risk: 'careful', source: 'rule', action: 'hint', regenerable: true,
      samplePaths: ['/Library/Application Support/Logic'], defaultOn: false,
    },
  ],
  reorgs: [
    {
      id: 'm-reorg-downloads', folder: 'Downloads', path: '~/Downloads', itemCount: 1_412, moveCount: 702,
      title: 'Sort Downloads by what each file is',
      summary: '702 loose files into 5 folders. Receipts grouped by sender, screen recordings split from videos.',
      aiNote: 'Grouped by content, not extension: receipts and invoices under Documents/Receipts/<sender>, screen recordings separate from videos, and anything I was unsure about in "_Sort later".',
      before: {
        name: 'Downloads', count: 1_412, children: [
          { name: 'Invoice_Apple_0912.pdf' }, { name: 'Screen Recording 2026-08-02 at 14.10.mov' }, { name: 'IMG_0912.HEIC' },
          { name: 'final_v3 copy.key' }, { name: 'Archive.zip' }, { name: 'boarding-pass.pdf' }, { name: '...and 1,406 more', count: 1_406 },
        ],
      },
      after: {
        name: 'Downloads', count: 1_412, children: [
          { name: 'Documents', count: 241, children: [{ name: 'Receipts', count: 133 }, { name: 'Travel', count: 41 }, { name: 'Other', count: 67 }] },
          { name: 'Images', count: 289, children: [{ name: 'Screenshots', count: 201 }, { name: 'Photos', count: 88 }] },
          { name: 'Screen recordings', count: 64 },
          { name: 'Archives', count: 85 },
          { name: '_Sort later', count: 23 },
          { name: '(710 files left in place)', count: 710 },
        ],
      },
      sampleMoves: [
        { from: 'Downloads/Invoice_Apple_0912.pdf', to: 'Downloads/Documents/Receipts/Apple/Invoice_Apple_0912.pdf' },
        { from: 'Downloads/Screen Recording 2026-08-02 at 14.10.mov', to: 'Downloads/Screen recordings/2026-08/Screen Recording 2026-08-02 at 14.10.mov' },
        { from: 'Downloads/boarding-pass.pdf', to: 'Downloads/Documents/Travel/boarding-pass.pdf' },
      ],
      defaultOn: true,
    },
    {
      id: 'm-reorg-desktop', folder: 'Desktop', path: '~/Desktop', itemCount: 97, moveCount: 91,
      title: 'File Desktop screenshots by month',
      summary: '91 screenshots into ~/Pictures/Screenshots/<month>. 6 other files stay.',
      aiNote: 'Almost everything on the Desktop is a screenshot. I filed them by month and left the 6 documents you have open often.',
      before: {
        name: 'Desktop', count: 97, children: [
          { name: 'Screenshot 2026-10-01 at 09.14.22.png' }, { name: 'Screenshot 2026-09-28 at 17.40.03.png' }, { name: 'budget.numbers' },
          { name: '...and 94 more', count: 94 },
        ],
      },
      after: {
        name: 'Desktop', count: 6, children: [
          { name: '6 files (unchanged)', count: 6 },
          { name: '~/Pictures/Screenshots', count: 91, children: [{ name: '2026-10', count: 12 }, { name: '2026-09', count: 31 }, { name: '2026-08', count: 22 }, { name: 'Older', count: 26 }] },
        ],
      },
      sampleMoves: [
        { from: 'Desktop/Screenshot 2026-10-01 at 09.14.22.png', to: 'Pictures/Screenshots/2026-10/Screenshot 2026-10-01 at 09.14.22.png' },
      ],
      defaultOn: true,
    },
  ],
};

export const datasets: Record<OS, Dataset> = { win, mac };

/** Decimal units, the way Finder and most people count. */
export function formatBytes(bytes: number, digits = 1): string {
  const abs = Math.abs(bytes);
  if (abs >= 1e12) return `${(bytes / 1e12).toFixed(digits)} TB`;
  if (abs >= 1e9) return `${(bytes / 1e9).toFixed(digits)} GB`;
  if (abs >= 1e6) return `${(bytes / 1e6).toFixed(0)} MB`;
  if (abs >= 1e3) return `${(bytes / 1e3).toFixed(0)} KB`;
  return `${bytes} B`;
}

export function formatCount(n: number): string {
  return n.toLocaleString('en-US');
}

export function defaultSelection(d: Dataset): { proposals: Set<string>; reorgs: Set<string> } {
  return {
    proposals: new Set(d.proposals.filter((p) => p.defaultOn).map((p) => p.id)),
    reorgs: new Set(d.reorgs.filter((r) => r.defaultOn).map((r) => r.id)),
  };
}

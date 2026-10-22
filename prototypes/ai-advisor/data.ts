// THROWAWAY: the Windows fixture (src/bridge/fixtures.ts) plus everything Claude "says" in this prototype. Every reply
// is canned. Nothing here calls Claude, reads a disk or moves a file.
import { fixtures } from '../../src/bridge/fixtures';
import type { MapItem, Overview, Proposal, TidyMove, TidyProposal } from '../../src/bridge';

const GB = 1e9;
const fx = fixtures.windows;
const home = 'C:\\Users\\alex';
const local = `${home}\\AppData\\Local`;

export const VOLUME = fx.volumes[0]!;
export const RULE_PROPOSALS: Proposal[] = fx.proposals;
export const RULE_TIDY: TidyProposal[] = fx.tidy;
export const DELETES = RULE_PROPOSALS.filter((p) => p.action === 'delete');
/** The plan a fresh map starts with: what the rules turn on. */
export const DEFAULT_PLAN = RULE_PROPOSALS.filter((p) => p.defaultOn).map((p) => p.id);

// ---------- Claude's own items: proposals it adds, which pass the Guard ----------

function ai(p: Pick<Proposal, 'id' | 'category' | 'title' | 'summary' | 'aiNote' | 'samplePaths'> & { gb: number; items: number; places: number }): Proposal {
  return {
    id: p.id, category: p.category, title: p.title, summary: p.summary,
    why: p.aiNote ?? '', bytes: p.gb * GB, itemCount: p.items, placeCount: p.places,
    risk: 'review', source: 'ai', aiNote: p.aiNote, action: 'delete', regenerable: false,
    samplePaths: p.samplePaths, defaultOn: false,
  };
}

export const AI_ANDROID = ai({
  id: 'ai-android', category: 'dev', title: 'Android emulator images',
  summary: 'Emulator images for Android 9 to 11, unused for 240 days.',
  aiNote: 'These are the phone images Android Studio’s emulator boots. Nothing has started them in 240 days. Android Studio’s SDK Manager downloads one again if you need it.',
  gb: 4.6, items: 3_812, places: 3,
  samplePaths: [`${local}\\Android\\Sdk\\system-images\\android-30`, `${local}\\Android\\Sdk\\system-images\\android-29`, `${local}\\Android\\Sdk\\system-images\\android-28`],
});
export const AI_PUBLIC = ai({
  id: 'ai-public', category: 'other', title: 'Installers in Public Downloads',
  summary: '9 setup files from 2025 in the shared Public folder.',
  aiNote: 'Setup files someone saved to the shared Public Downloads folder last year: printer drivers and a Zoom installer. Every account on this PC can see them, so check nobody else still needs them.',
  gb: 1.4, items: 9, places: 9,
  samplePaths: ['C:\\Users\\Public\\Downloads\\HP_Smart_Setup.exe', 'C:\\Users\\Public\\Downloads\\ZoomInstallerFull.exe'],
});
export const AI_ITEMS: Record<string, Proposal> = { [AI_ANDROID.id]: AI_ANDROID, [AI_PUBLIC.id]: AI_PUBLIC };

/** Things Claude suggested that the Guard refused, with the Guard's reason. They never reach the plan. */
export interface Refused { title: string; path: string; bytes: number; reason: string }
export const REFUSED: Record<string, Refused> = {
  update: { title: 'Windows Update downloads', path: 'C:\\Windows\\SoftwareDistribution\\Download', bytes: 7.6 * GB, reason: 'It’s inside Windows’ own folder, which no proposal can touch.' },
  wer: { title: 'Old Windows error reports', path: 'C:\\ProgramData\\Microsoft\\Windows\\WER\\ReportArchive', bytes: 2.8 * GB, reason: 'It’s in ProgramData, programs’ shared data, which no proposal can touch.' },
};

// ---------- the map: the fixture's overview, with a tile carved out for each of Claude's items in the drive's proposals ----------

/** Where each of Claude's items is carved from: the folder tile it's part of. */
const CARVED_FROM: Record<string, string> = { 'ai-android': `${local}\\Android\\Sdk`, 'ai-public': 'C:\\Users\\Public' };

/** A browser profile tile, carved from the apps' small items, so the map has a folder that always stays hidden. */
export const EDGE_PROFILE = `${local}\\Microsoft\\Edge\\User Data`;

export function overviewWith(extra: readonly Proposal[]): Overview {
  const ids = new Set(extra.map((p) => p.id));
  return {
    ...fx.overview,
    categories: fx.overview.categories.map((c) => {
      const items: MapItem[] = [];
      for (const i of c.items) {
        if (i.kind === 'more' && c.id === 'apps') {
          const b = 3.4 * GB;
          items.push({ ...i, bytes: i.bytes - b, name: '211 smaller items', count: i.count - 1 });
          items.push({ id: 'fx-edge-profile', name: 'User Data', path: EDGE_PROFILE, bytes: b, files: 41_200, count: 0, lastUsed: Date.now() - 3_600_000, kind: 'folder', browsable: false, note: null, proposal: null });
          continue;
        }
        const carve = extra.find((p) => ids.has(p.id) && CARVED_FROM[p.id] === i.path);
        if (!carve) { items.push(i); continue; }
        items.push({ ...i, bytes: i.bytes - carve.bytes, files: Math.max(1, i.files - carve.itemCount) });
        items.push({ id: `p:${carve.id}`, name: carve.title, path: '', bytes: carve.bytes, files: carve.itemCount, count: 0, lastUsed: i.lastUsed, kind: 'proposal', browsable: false, note: null, proposal: carve.id });
      }
      return { ...c, items: items.sort((a, b) => b.bytes - a.bytes) };
    }),
  };
}

/** A map tile by its path in the fixture. */
export function tileByPath(path: string): MapItem | undefined {
  for (const c of overviewWith([]).categories) for (const i of c.items) if (i.path === path) return i;
  return undefined;
}

// ---------- job 2: Review my plan. Claude flags; it can't add. ----------

export interface Flag { id: string; reason: string }
export const REVIEW_FLAGS: Flag[] = [
  { id: 'w-installers', reason: 'Docker Desktop Installer.exe and node-v22.9.0-x64.msi look like versions you might keep for offline installs. Installers don’t come back on their own, so look through the 47 first.' },
  { id: 'w-node', reason: 'portfolio-2024 has no package-lock.json. npm install would fetch newer versions than it was built with, so it may not build the same way again.' },
];
/** A flag only reading files finds: privacy mode off. */
export const REVIEW_FLAGS_OPEN: Flag[] = [
  { id: 'w-rust', reason: 'I opened raytracer\\target: its renders folder holds 1.2 GB of .png images your program wrote. cargo build won’t make those again. Copy them out first.' },
];
/** What Claude says about the proposals it didn't flag. */
export const REVIEW_OK: Record<string, string> = {
  'w-rust': 'Comes back with cargo build.',
  'w-pkgcache': 'Package managers download it again.',
  'w-browser': 'Browsers refill it as you browse.',
  'w-temp': 'Meant to be thrown away.',
  'w-appcache': 'Apps rebuild it.',
  'w-dumps': 'Only useful right after a crash.',
};

// ---------- job 3: Let Claude pick. A whole new plan, as changes to the current one. ----------

export interface PickChange {
  id: string;
  /** untick or tick a rule's item, or add one of Claude's own (unticked). */
  change: 'untick' | 'tick' | 'add';
  why: string;
}
export const PICK_CHANGES: PickChange[] = [
  { id: 'w-installers', change: 'untick', why: 'Some look like offline installers you keep, and they don’t come back.' },
  { id: 'w-steam-advice', change: 'tick', why: 'A 71 GB game not played for 430 days. Steam uninstalls it; you get a reminder.' },
  { id: 'ai-android', change: 'add', why: 'Unused for 240 days, and Android Studio downloads them again.' },
  { id: 'ai-public', change: 'add', why: 'Old setup files, but every account can see this folder.' },
];
/** What Claude's pick refused itself to the Guard, by REFUSED key. */
export const PICK_REFUSED = ['update', 'wer'];

// ---------- job 5: Explain unknown big folders ----------

export interface Explanation {
  path: string;
  /** One line for the hover card or a list. */
  short: string;
  /** What Claude says it is, from names, sizes and dates (privacy mode on). */
  text: string;
  /** What it says once it could open files there (privacy mode off). */
  deep: string;
  /** Privacy mode off, and Claude met a folder that always stays hidden: what it says about that. */
  hidden?: string;
  /** One of Claude's items it suggests (passed the Guard), or a suggestion the Guard refused, or neither. */
  suggests?: string;
  refused?: keyof typeof REFUSED;
}
export const EXPLAIN: Explanation[] = [
  { path: `${local}\\Docker`, short: 'Docker Desktop’s virtual disk', text: 'Docker Desktop’s virtual disk (docker_data.vhdx): your images, containers and volumes. Deleting the folder breaks Docker and loses volumes. To shrink it, use Clean up in Docker Desktop, or docker system prune.', deep: 'I read Docker’s image list: 23 images, the biggest three old postgres versions (11, 12, 13) that no container uses. docker image prune -a in Docker would free about 11 GB. Deleting the folder itself breaks Docker.' },
  { path: 'C:\\Windows\\SoftwareDistribution\\Download', short: 'Windows Update’s downloads', text: 'Windows Update’s download cache. Windows clears it after updates install, and Disk Cleanup’s “Windows Update Cleanup” does it now.', deep: 'I opened the folder’s manifests: they’re for updates that installed in September. Disk Cleanup’s “Windows Update Cleanup” clears them.', refused: 'update' },
  { path: `${local}\\Android\\Sdk`, short: 'The Android SDK, with old emulator images', text: 'The Android SDK: build tools, platforms and emulator images. 4.6 GB of it is emulator images for Android 9 to 11 that haven’t been started in 240 days.', deep: 'I read each image’s source.properties: Google APIs images for Android 9, 10 and 11. I opened your emulator configs (.android\\avd): none of them uses these images.', suggests: 'ai-android' },
  { path: 'C:\\ProgramData\\Package Cache', short: 'Installers apps keep for repair', text: 'Installers that Visual Studio, .NET and other apps keep so they can repair or uninstall themselves. Deleting them can break uninstalling. Leave it.', deep: 'I read the installers’ manifests: Visual Studio 2022, the .NET 8 SDK and six Visual C++ runtimes. Each uninstaller needs its copy here. Leave it.' },
  { path: 'C:\\Windows\\ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization', short: 'Update pieces shared with other PCs', text: 'Pieces of updates Windows shares with other PCs. Windows manages and trims it; Settings › Delivery Optimization can limit it.', deep: 'I opened its cache index: pieces of the 24H2 feature update and Office updates. Windows trims it on its own.' },
  { path: 'C:\\ProgramData\\Microsoft\\Windows\\WER', short: 'Windows’ crash reports', text: 'Windows Error Reporting: crash reports queued or already sent. 2.8 GB are archives of reports Windows already sent.', deep: 'I opened the reports: 212 are from Discord and Steam crashes in 2025, already sent to Microsoft.', refused: 'wer' },
  { path: 'C:\\Users\\Public', short: 'The folder every account shares', text: 'The folder every account on this PC shares. Its Downloads holds 1.4 GB of setup files from 2025.', deep: 'I read the setup files’ version info: HP Smart 2025.2, Zoom 6.1 and seven printer drivers, all older than what’s installed.', suggests: 'ai-public' },
  { path: EDGE_PROFILE, short: 'Microsoft Edge’s profile', text: 'Microsoft Edge’s profile, from its folder names: cache, extensions, and the files where Edge keeps your sign-ins and history.', deep: 'I opened Edge’s cache and extension folders: 2.6 GB is Service Worker cache that Edge rebuilds, which the built-in Browser caches cleanup already covers.', hidden: 'I can’t look inside the rest: Login Data, Cookies, History and Web Data hold browser sign-ins and history, which always stay hidden.' },
];
export const explainOf = (path: string) => EXPLAIN.find((e) => e.path === path);

// ---------- job 4: Why is this risky? A chat about one item ----------

export interface FirstAnswer { what: string; lose: string; back: string; basis: string }
const FIRST: Record<string, Omit<FirstAnswer, 'basis'>> = {
  'w-node': { what: 'Downloaded dependencies of 61 JavaScript projects you haven’t opened in 90 days.', lose: 'Nothing you wrote. One project, portfolio-2024, has no lock file, so its exact versions are gone.', back: 'Yes. npm install in a project fetches them again, a minute or two each.' },
  'w-rust': { what: 'Compiled output of 12 Rust projects: the target folders cargo writes.', lose: 'Build time. The source is untouched.', back: 'Yes. cargo build makes it again.' },
  'w-installers': { what: '47 setup files (.exe and .msi) in Downloads, all older than 30 days.', lose: 'The installers themselves. If you keep any for offline installs, they’re gone for good.', back: 'Only if you download them again. Old versions may not be offered any more.' },
  'w-pkgcache': { what: 'npm, pip and NuGet’s download caches.', lose: 'Nothing but the next install’s speed.', back: 'Yes. They download what they need again.' },
  'w-browser': { what: 'Pages and images Edge and Chrome saved to load sites faster.', lose: 'A little speed on sites you visit. Sign-ins, history and passwords stay.', back: 'Yes, as you browse.' },
  'w-temp': { what: 'Temporary files apps and installers left in Temp.', lose: 'Nothing; files in use or changed today are left alone.', back: 'Apps make new ones as they need them.' },
  'w-appcache': { what: 'Caches Discord, Teams and Spotify rebuild.', lose: 'Offline songs in Spotify need downloading again.', back: 'Yes, as you use the apps.' },
  'w-dumps': { what: '46 crash dumps from apps that crashed.', lose: 'The chance to send one to a developer.', back: 'No, but new crashes make new ones.' },
  'ai-android': { what: 'Emulator images for Android 9 to 11, last started 240 days ago.', lose: 'Any emulator set up on those images, with its apps and data.', back: 'The images, yes: SDK Manager downloads them. An emulator’s own data, no.' },
  'ai-public': { what: '9 setup files in C:\\Users\\Public\\Downloads, from 2025.', lose: 'The installers, for every account on this PC.', back: 'Only if someone downloads them again.' },
};
/** What Claude adds once it can open files (privacy mode off), and which files it opened. */
const DEEP: Record<string, { opened: string; what?: string; lose?: string; hidden?: string }> = {
  'w-node': { opened: 'package.json and README.txt in each project', what: 'Dependencies of 61 projects: 52 Vite or Next.js sites and 9 Electron apps. portfolio-2024’s README.txt says “deployed, archived”.', lose: 'Nothing you wrote. portfolio-2024 has no lock file, but its README says it’s archived anyway.' },
  'w-rust': { opened: 'Cargo.toml in each project, and raytracer\\target', lose: 'Build time, and raytracer\\target\\renders: 1.2 GB of .png images your program wrote there. cargo build won’t make those again.' },
  'w-installers': { opened: 'the version info of the 47 setup files', what: '47 setup files. 31 are older versions of apps you have newer ones of; Docker Desktop 4.12 and Node 22.9 are the only copies of those versions.' },
  'w-browser': { opened: 'Edge and Chrome’s Cache_Data folders (caches stay readable)', hidden: 'I can’t look inside the rest of the profiles: they hold browser sign-ins, cookies and history, which always stay hidden.' },
  'ai-android': { opened: 'each image’s source.properties and your emulator configs', lose: 'Nothing in use: none of your three emulators boots these images.' },
  'fx-edge-profile': { opened: 'Edge’s cache and extension folders (caches stay readable)' },
};
const FOLLOW: Record<string, [string, string][]> = {
  'w-node': [
    ['Will my projects still build?', 'All but one should: the other 60 have lock files, so npm install brings back the same versions. portfolio-2024 would get the newest ones its package.json allows.'],
    ['Can I keep just one project?', 'Not from this plan item: it takes all 61 together. Untick it, or keep it and run npm install in that project afterwards.'],
  ],
  'w-installers': [
    ['Which ones should I keep?', 'From the names: Docker Desktop Installer.exe and node-v22.9.0-x64.msi are versions people keep. SteamSetup.exe and the VS Code setups download fresh.'],
    ['Are they all in the Recycle Bin after?', 'No. Vacancy deletes permanently and skips the Recycle Bin.'],
  ],
};
const GENERIC_FOLLOW: [string, string][] = [
  ['Can I get it back?', 'Not from Vacancy: deleting is permanent and skips the Recycle Bin. Whether it comes back on its own is in my first answer.'],
  ['What would you do?', 'From the names, sizes and dates I can see, I’d keep it ticked if the first answer’s “what you’d lose” doesn’t worry you. I can’t open the files to check.'],
];

export type Privacy = 'on' | 'off';
export const BASIS_ON = 'From names, sizes and dates only. I didn’t open any files.';

export function firstAnswer(subject: Subject, privacy: Privacy): FirstAnswer {
  const base = firstOf(subject, privacy);
  if (privacy === 'on') return { ...base, basis: BASIS_ON };
  const deep = DEEP[subject.id];
  const e = subject.kind === 'folder' ? explainOf(subject.path) : undefined;
  const hidden = deep?.hidden ?? e?.hidden;
  return {
    what: deep?.what ?? base.what, lose: deep?.lose ?? base.lose, back: base.back,
    basis: `${deep ? `I opened ${deep.opened}.` : 'I opened a few files in it; nothing there changes the answer.'}${hidden ? ` ${hidden}` : ''}`,
  };
}

function firstOf(subject: Subject, privacy: Privacy): Omit<FirstAnswer, 'basis'> {
  if (subject.kind === 'proposal') {
    const known = FIRST[subject.id];
    if (known) return known;
    const p = subject.proposal;
    return { what: p.summary, lose: p.regenerable ? 'Not much: it comes back on its own.' : 'What’s in it, for good.', back: p.regenerable ? 'Yes, on its own.' : 'No.' };
  }
  const e = explainOf(subject.path);
  return {
    what: (privacy === 'off' ? e?.deep : e?.text) ?? `A ${subject.name} folder I don’t recognise from its name alone.`,
    lose: e?.suggests ? 'Only what my suggestion takes, if you tick it. The rest stays.' : 'I’m not suggesting deleting any of it.',
    back: e?.refused ? 'Windows manages it. No proposal can touch it, so nothing here is deleted.' : e?.suggests ? 'See my suggestion: it says whether it comes back.' : 'Nothing goes, so nothing needs to come back.',
  };
}
export function followUps(subject: Subject): [string, string][] {
  return FOLLOW[subject.id] ?? GENERIC_FOLLOW;
}
/** Claude's answer to a typed question: canned, keyed on a few words. */
export function answerTo(subject: Subject, question: string, privacy: Privacy): string {
  const q = question.toLowerCase();
  const hit = followUps(subject).find(([k]) => q.includes(k.toLowerCase().split(' ').slice(-2).join(' ').replace('?', '')));
  if (hit) return hit[1];
  if (/back|undo|restore|recycle/.test(q)) return 'Deleting is permanent: it skips the Recycle Bin, and Vacancy can’t undo it. Only folder moves can be undone.';
  if (/content|read|open|see|inside|look/.test(q))
    return privacy === 'on'
      ? 'With privacy mode on I only see names, sizes, counts and dates from the scan. I never see what’s inside a file.'
      : `${firstAnswer(subject, privacy).basis} I can’t write, delete, run anything or go online.`;
  const lose = firstAnswer(subject, privacy).lose;
  return privacy === 'on'
    ? `From the names, sizes and dates I can see: ${lose}`
    : `From ${DEEP[subject.id]?.opened ?? 'the files I opened'}: ${lose}`;
}

/** What a chat is about: a proposal, or a folder on the map. */
export type Subject =
  | { kind: 'proposal'; id: string; name: string; proposal: Proposal }
  | { kind: 'folder'; id: string; name: string; path: string; bytes: number };

// ---------- job 6: Claude's Tidy up layouts ----------

/** Claude's folder for a file, from its name only: what it's for, not what kind it is. */
function claudeDest(m: TidyMove, folder: string): string {
  const n = m.name.toLowerCase();
  if (m.kind === 'later') return 'Sort later';
  if (m.kind === 'installers') return 'Installers';
  if (/statement|invoice|receipt|insurance|lease|warranty|tax|budget|car-loan|id-front|id-back/.test(n)) return 'Money & paperwork';
  if (/boarding|ticket|menu|packing/.test(n)) return 'Travel';
  if (/resume|cover-letter|notes-meeting|q3-|quote|minutes|todo/.test(n)) return 'Work';
  if (/screenshot|screen-recording/.test(n)) return 'Screenshots';
  if (/fonts|logo|floorplan|demo-cut|wallpaper|mods|project-old/.test(n)) return 'Projects & design';
  if (/img_|birthday|cat\.|photos-export|map\.png|scan0/.test(n)) return 'Photos';
  if (/voice-memo|podcast|ringtone/.test(n)) return 'Audio';
  if (/moving|recipes/.test(n)) return 'Home';
  if (m.kind === 'archives') return folder === 'Downloads' ? 'Archives' : 'Projects & design';
  if (m.kind === 'videos') return 'Photos';
  return 'Sort later';
}

/** Claude's layout of a folder, as a Tidy up proposal the drawer and the trees can show. Same files, other folders. */
export function claudeTidy(t: TidyProposal): TidyProposal {
  const moves = t.moves.map((m) => {
    const dest = claudeDest(m, t.name);
    // A reviewed name is pinned: only a file going where the built-in rule sent it keeps its numbered name.
    const same = dest === m.dest;
    return { ...m, dest, to: same ? m.to : m.name, renamed: same ? m.renamed : false };
  });
  return { ...t, moves, existing: t.existing.filter((e) => moves.some((m) => m.dest === e)) };
}

/** What Claude says about each folder's layout. */
export const TIDY_NOTES: Record<string, string> = {
  'tidy-downloads': 'Grouped by what the files are for, from their names: paperwork, travel, work, photos. Installers and archives keep your folders. Anything I can’t tell goes to Sort later.',
  'tidy-desktop': 'Screenshots get their own folder, since they’re most of the loose files. Shortcuts stay where you put them.',
  'tidy-documents': 'By what they’re about rather than by year: money and paperwork, work, home.',
};

export const fmtGb = (bytes: number) => `${(bytes / GB).toFixed(1)} GB`;

// ---------- privacy mode: what Claude never reads, even with privacy mode off ----------

export const HIDDEN: [string, string[]][] = [
  ['Keys and sign-ins', ['SSH and GPG keys', '.aws, .azure, gh, npm and Docker tokens', '.env files', 'Claude’s and Codex’s own sign-in files', 'Windows credential and DPAPI folders', 'the macOS Keychain']],
  ['Browser data', ['saved passwords', 'cookies', 'history', 'autofill', '(cache folders stay readable)']],
  ['Password managers and wallets', ['1Password', 'Bitwarden', 'KeePass .kdbx files', 'crypto wallets']],
];

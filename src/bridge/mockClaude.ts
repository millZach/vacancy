// The mock bridge's Claude (ADR 0008): canned answers for every job and every state, on each fixture, shaped like
// what the core sends (src-tauri's claude_* commands). Nothing here calls Claude, reads a disk or moves a file.
// Pick the state with `&claude=off|notfound|signedout|checktimeout|checkerror|on|slow|error|timeout` (`checktimeout`
// and `checkerror`: Claude is on, but asking Claude Code whether it's signed in timed out, or failed) and privacy mode with
// `&privacy=on|off`; `&claudeMs=` sets how long an answer takes (default 900 ms; `slow` takes 8 s, saying it's
// slow after 1.5 s). The answers follow the approved prototype's (prototype/ai-advisor, variant B).
import type {
  ClaudeAbout,
  ClaudeAnswer,
  ClaudeBasis,
  ClaudeChange,
  ClaudeDropped,
  ClaudeEvent,
  ClaudeFailure,
  ClaudeFolderNote,
  ClaudeHidden,
  ClaudeJob,
  ClaudeMessage,
  ClaudeRefreshed,
  ClaudeResult,
  ClaudeStatus,
  MapItem,
  Overview,
  Platform,
  Proposal,
  TidyLayout,
  TidyMove,
  TidyProposal,
} from './types';

export type MockClaudeState = 'off' | 'notfound' | 'signedout' | 'checktimeout' | 'checkerror' | 'on' | 'slow' | 'error' | 'timeout';
export const MOCK_CLAUDE_STATES: readonly MockClaudeState[] = [
  'off',
  'notfound',
  'signedout',
  'checktimeout',
  'checkerror',
  'on',
  'slow',
  'error',
  'timeout',
];

/** What the core says when asking Claude Code whether it's signed in didn't work (`claude::Status::message`). */
export const CHECK_TIMED_OUT = 'Claude Code didn’t answer within 30 seconds when Vacancy asked whether it’s signed in.';
export const CHECK_FAILED = 'Vacancy couldn’t start Claude Code: Access is denied. (os error 5)';

export interface MockClaudeOptions {
  /** Where Claude starts: `off` by default (Claude is off until the person turns it on). */
  claude?: MockClaudeState;
  /** Privacy mode at the start. Default: on. */
  privacy?: 'on' | 'off';
  /** How long an answer takes. Default 900 ms (`slow`: 8 s). */
  claudeMs?: number;
}

/** What the mock Claude reads and changes: a drive's scan, as the mock bridge keeps it. */
export interface MockClaudeScan {
  map: Overview;
  proposals: Proposal[];
  tidy: TidyProposal[];
  /** The layout of each sort not in use, once Claude has suggested one. */
  others: TidyProposal[];
}

const GB = 1e9;

/** The privacy screens' list, as the core gives it (`claude::hidden::list`). */
export const HIDDEN: ClaudeHidden[] = [
  {
    group: 'keys',
    title: 'Keys and sign-ins',
    items: [
      'SSH and GPG keys',
      '.aws, .azure, gh, npm and Docker tokens',
      '.env files',
      "Claude Code's and Codex's own folders, wherever they're set (CLAUDE_CONFIG_DIR, CODEX_HOME)",
      'Windows Credentials and DPAPI Protect folders',
      'the macOS Keychain',
    ],
  },
  {
    group: 'browser',
    title: 'Browser data',
    items: [
      'saved passwords and their keys',
      'cookies and site sign-ins',
      'history, sessions and bookmarks',
      'autofill and form history',
      'in Edge, Chrome, Brave, Vivaldi, Firefox and Safari (their caches stay readable)',
    ],
  },
  {
    group: 'vaults',
    title: 'Password managers and wallets',
    items: ['1Password', 'Bitwarden', 'KeePass .kdbx files', 'crypto wallets (Electrum, Exodus, the Ethereum keystore)'],
  },
];

const BASIS_ON = 'From names, sizes and dates only. Claude didn’t open any files.';
/** The core's refusal of a privacy-on chat that carries turns made with privacy mode off. */
export const PRIVACY_TURNS =
  'This chat began with privacy mode off. With it on, Vacancy won’t send what was said then. Start a new chat.';

/** One of Claude's own items, before the mock adds it: where its tile is carved from, and what it says. */
interface Item {
  id: string;
  from: string;
  category: Proposal['category'];
  title: string;
  why: string;
  gb: number;
  files: number;
  samplePaths: string[];
}

/** Something Claude suggested that the Guard refused. */
const refused = (name: string, path: string, reason: string): ClaudeDropped => ({ name, path, reason });

interface Canned {
  flags: { id: string; reason: string; risk: 'safe' | 'review' | 'careful' }[];
  /** A flag only reading files finds (privacy mode off). */
  openFlag: { id: string; reason: string; risk: 'safe' | 'review' | 'careful' } | null;
  fine: Record<string, string>;
  pick: { tick: { id: string; why: string }[]; untick: { id: string; why: string }[]; add: string[] };
  items: Item[];
  refused: ClaudeDropped[];
  explain: (Omit<ClaudeFolderNote, 'bytes' | 'files' | 'suggested'> & { deep: string; suggests?: string })[];
  first: Record<string, { what: string; lose: string; back: string }>;
  opened: Record<string, string>;
}

function windows(): Canned {
  const home = 'C:\\Users\\alex';
  const local = `${home}\\AppData\\Local`;
  return {
    flags: [
      { id: 'w-installers', risk: 'review', reason: 'Docker Desktop Installer.exe and node-v22.9.0-x64.msi look like versions you might keep for offline installs. Installers don’t come back on their own, so look through the 47 first.' },
      { id: 'w-node', risk: 'review', reason: 'portfolio-2024 has no package-lock.json. npm install would fetch newer versions than it was built with, so it may not build the same way again.' },
    ],
    openFlag: { id: 'w-rust', risk: 'careful', reason: 'I opened raytracer\\target: its renders folder holds 1.2 GB of .png images your program wrote. cargo build won’t make those again. Copy them out first.' },
    fine: {
      'w-rust': 'Comes back with cargo build.',
      'w-pkgcache': 'Package managers download it again.',
      'w-browser': 'Browsers refill it as you browse.',
      'w-temp': 'Meant to be thrown away.',
      'w-appcache': 'Apps rebuild it.',
      'w-dumps': 'Only useful right after a crash.',
    },
    pick: {
      untick: [{ id: 'w-installers', why: 'Some look like offline installers you keep, and they don’t come back.' }],
      tick: [{ id: 'w-steam-advice', why: 'A 71 GB game not played for 430 days. Steam uninstalls it; you get a reminder.' }],
      add: ['claude-android', 'claude-public'],
    },
    items: [
      {
        id: 'claude-android', from: `${local}\\Android\\Sdk`, category: 'dev', title: 'Android emulator images',
        why: 'These are the phone images Android Studio’s emulator boots. Nothing has started them in 240 days. Android Studio’s SDK Manager downloads one again if you need it.',
        gb: 4.6, files: 3812,
        samplePaths: [`${local}\\Android\\Sdk\\system-images\\android-30`, `${local}\\Android\\Sdk\\system-images\\android-29`],
      },
      {
        id: 'claude-public', from: 'C:\\Users\\Public', category: 'other', title: 'Installers in Public Downloads',
        why: 'Setup files someone saved to the shared Public Downloads folder last year. Every account on this PC can see them, so check nobody else still needs them.',
        gb: 1.4, files: 9,
        samplePaths: ['C:\\Users\\Public\\Downloads\\HP_Smart_Setup.exe', 'C:\\Users\\Public\\Downloads\\ZoomInstallerFull.exe'],
      },
    ],
    refused: [
      refused('Windows Update downloads', 'C:\\Windows\\SoftwareDistribution\\Download', 'The Guard protects it: it is, holds or is inside a place Vacancy never deletes.'),
      refused('Old Windows error reports', 'C:\\ProgramData\\Microsoft\\Windows\\WER\\ReportArchive', 'The Guard protects it: it is, holds or is inside a place Vacancy never deletes.'),
    ],
    explain: [
      { path: `${local}\\Docker`, name: 'Docker', short: 'Docker Desktop’s virtual disk', explanation: 'Docker Desktop’s virtual disk (docker_data.vhdx): your images, containers and volumes. Deleting the folder breaks Docker and loses volumes. To shrink it, use Clean up in Docker Desktop, or docker system prune.', deep: 'I read Docker’s image list: 23 images, the biggest three old postgres versions that no container uses. docker image prune -a in Docker would free about 11 GB. Deleting the folder itself breaks Docker.' },
      { path: `${local}\\Android\\Sdk`, name: 'Sdk', short: 'The Android SDK, with old emulator images', explanation: 'The Android SDK: build tools, platforms and emulator images. 4.6 GB of it is emulator images for Android 9 to 11 that haven’t been started in 240 days.', deep: 'I read each image’s source.properties: Google APIs images for Android 9, 10 and 11. None of your emulator configs uses them.', suggests: 'claude-android' },
      { path: 'C:\\ProgramData\\Package Cache', name: 'Package Cache', short: 'Installers apps keep for repair', explanation: 'Installers that Visual Studio, .NET and other apps keep so they can repair or uninstall themselves. Deleting them can break uninstalling. Leave it.', deep: 'I read the installers’ manifests: Visual Studio 2022, the .NET 8 SDK and six Visual C++ runtimes. Each uninstaller needs its copy here. Leave it.' },
      { path: 'C:\\Users\\Public', name: 'Public', short: 'The folder every account shares', explanation: 'The folder every account on this PC shares. Its Downloads holds 1.4 GB of setup files from 2025.', deep: 'I read the setup files’ version info: HP Smart 2025.2, Zoom 6.1 and seven printer drivers, all older than what’s installed.', suggests: 'claude-public' },
    ],
    first: {
      'w-node': { what: 'Downloaded dependencies of 61 JavaScript projects you haven’t opened in 90 days.', lose: 'Nothing you wrote. One project, portfolio-2024, has no lock file, so its exact versions are gone.', back: 'Yes. npm install in a project fetches them again, a minute or two each.' },
      'w-rust': { what: 'Compiled output of 12 Rust projects: the target folders cargo writes.', lose: 'Build time. The source is untouched.', back: 'Yes. cargo build makes it again.' },
      'w-installers': { what: '47 setup files (.exe and .msi) in Downloads, all older than 30 days.', lose: 'The installers themselves. If you keep any for offline installs, they’re gone for good.', back: 'Only if you download them again.' },
      'claude-android': { what: 'Emulator images for Android 9 to 11, last started 240 days ago.', lose: 'Any emulator set up on those images, with its apps and data.', back: 'The images, yes: SDK Manager downloads them. An emulator’s own data, no.' },
    },
    opened: {
      'w-node': 'package.json and README.txt in each project',
      'w-rust': 'Cargo.toml in each project, and raytracer\\target',
      'w-installers': 'the version info of the 47 setup files',
      'w-browser': 'Edge and Chrome’s Cache_Data folders (caches stay readable)',
    },
  };
}

function macos(): Canned {
  const home = '/Users/alex';
  const lib = `${home}/Library`;
  return {
    flags: [
      { id: 'm-archives', risk: 'careful', reason: 'Some of these archives are from 2021 and may be the only copy of what’s inside. They don’t come back on their own.' },
      { id: 'm-mail', risk: 'review', reason: 'Mail downloads attachments again from the server, but only for accounts that still have them. Check old POP accounts first.' },
    ],
    openFlag: { id: 'm-dmgs', risk: 'review', reason: 'I opened the disk images’ names and versions: Final Cut Pro 10.6 is the only copy of that version you have.' },
    fine: {
      'm-derived': 'Xcode rebuilds it on the next build.',
      'm-node': 'npm install brings it back.',
      'm-caches': 'Apps rebuild their caches.',
      'm-brew': 'Homebrew downloads them again if needed.',
      'm-logs': 'Only useful when something just went wrong.',
      'm-trash': 'You already threw these away.',
    },
    pick: {
      untick: [{ id: 'm-archives', why: 'Old archives may be the only copy of what’s inside.' }],
      tick: [],
      add: ['claude-shared'],
    },
    items: [
      {
        id: 'claude-shared', from: '/Users/Shared', category: 'other', title: 'Old installers in Shared',
        why: 'Installer packages from 2024 in the folder every account shares. Check nobody else on this Mac still needs them.',
        gb: 0.5, files: 6,
        samplePaths: ['/Users/Shared/Installers/Zoom.pkg', '/Users/Shared/Installers/HPEasyStart.pkg'],
      },
    ],
    refused: [
      refused('Time Machine local snapshots', '/System/Volumes/Data/.MobileBackups', 'The Guard protects it: it is, holds or is inside a place Vacancy never deletes.'),
    ],
    explain: [
      { path: `${lib}/Containers/com.docker.docker`, name: 'com.docker.docker', short: 'Docker Desktop’s virtual disk', explanation: 'Docker Desktop’s disk image: your images, containers and volumes. Deleting it breaks Docker. Use Clean up in Docker Desktop instead.', deep: 'I read Docker’s image list: old node and postgres images no container uses. docker image prune -a would free about 6 GB.' },
      { path: '/opt/homebrew', name: 'homebrew', short: 'Homebrew and what it installed', explanation: 'Homebrew itself and the command-line tools you installed with it. brew cleanup removes old versions safely.', deep: 'I read the Cellar: three old versions of python and node that brew cleanup would remove.' },
      { path: '/Users/Shared', name: 'Shared', short: 'The folder every account shares', explanation: 'The folder every account on this Mac shares. It holds installer packages from 2024.', deep: 'I read the packages’ names and versions: Zoom and HP printer installers, older than what’s installed.', suggests: 'claude-shared' },
    ],
    first: {
      'm-derived': { what: 'Xcode’s build output for your projects.', lose: 'Build time.', back: 'Yes, on the next build.' },
      'm-archives': { what: 'Old .zip and .dmg archives in Downloads.', lose: 'Whatever is only in them.', back: 'No.' },
      'claude-shared': { what: 'Installer packages in /Users/Shared from 2024.', lose: 'The installers, for every account.', back: 'Only if someone downloads them again.' },
    },
    opened: {
      'm-derived': 'the project names in DerivedData',
      'm-dmgs': 'the disk images’ names and version info',
    },
  };
}

/** Claude's folder for a loose file, from its name only: what it's for, not what kind it is. */
function claudeDest(m: TidyMove, folder: string): string {
  const n = m.name.toLowerCase();
  if (m.kind === 'later') return 'Sort later';
  if (m.kind === 'installers') return 'Installers';
  if (/statement|invoice|receipt|insurance|lease|warranty|tax|budget|loan|id-front|id-back/.test(n)) return 'Money & paperwork';
  if (/boarding|ticket|menu|packing/.test(n)) return 'Travel';
  if (/resume|cover-letter|notes-meeting|q3-|quote|minutes|todo/.test(n)) return 'Work';
  if (/screenshot|screen-recording/.test(n)) return 'Screenshots';
  if (/img_|birthday|cat\.|photos-export|scan0/.test(n)) return 'Photos';
  if (/voice-memo|podcast|ringtone/.test(n)) return 'Audio';
  if (m.kind === 'archives') return folder === 'Downloads' ? 'Archives' : 'Projects';
  if (m.kind === 'videos' || m.kind === 'images') return 'Photos';
  return 'Sort later';
}

const TIDY_NOTES: Record<string, string> = {
  'tidy-downloads': 'Grouped by what the files are for, from their names: paperwork, travel, work, photos. Installers and archives keep their folders. Anything I couldn’t tell goes to Sort later.',
  'tidy-desktop': 'Screenshots get their own folder, since they’re most of the loose files. Shortcuts stay where you put them.',
  'tidy-documents': 'By what they’re about rather than by year: money and paperwork, work, home.',
};

/** Claude's layout of a folder: the same files, other folders. Reviewed names stay pinned. */
export function claudeLayout(t: TidyProposal): TidyProposal {
  const moves = t.moves.map((m) => {
    const dest = claudeDest(m, t.name);
    const same = dest === m.dest;
    return { ...m, dest, to: same ? m.to : m.name, renamed: same ? m.renamed : false };
  });
  return {
    ...t,
    moves,
    existing: t.existing.filter((e) => moves.some((m) => m.dest === e)),
    layout: 'claude',
    note: TIDY_NOTES[t.id] ?? 'Grouped by what the files are for, from their names.',
  };
}

/** Claude's item as a proposal: unticked, Review, marked Claude's. */
function itemProposal(item: Item): Proposal {
  return {
    id: item.id,
    category: item.category,
    title: item.title,
    summary: 'Suggested by Claude · review before removing',
    why: item.why,
    bytes: item.gb * GB,
    itemCount: item.files,
    placeCount: 1,
    risk: 'review',
    source: 'claude',
    claudeNote: `Suggested by Claude: ${item.why}`,
    action: 'delete',
    regenerable: false,
    samplePaths: item.samplePaths,
    defaultOn: false,
  };
}

/** The map with a tile carved out for Claude's item, from the folder tile it's part of (the core carves it). */
function carve(map: Overview, item: Item): Overview {
  const out = structuredClone(map);
  for (const c of out.categories) {
    const at = c.items.findIndex((i) => i.path === item.from);
    if (at < 0) continue;
    const from = c.items[at]!;
    const bytes = Math.min(item.gb * GB, from.bytes * 0.9);
    from.bytes -= bytes;
    from.files = Math.max(1, from.files - item.files);
    const tile: MapItem = {
      id: `p:${item.id}`, name: item.title, path: '', bytes, files: item.files, count: 0, lastUsed: from.lastUsed,
      kind: 'proposal', browsable: false, note: null, proposal: item.id,
    };
    c.items.push(tile);
    c.items.sort((a, b) => b.bytes - a.bytes);
    return out;
  }
  return out;
}

/** The mock's Claude: settings, state and canned answers, over the mock bridge's scans. */
export function createMockClaude(
  platform: Platform,
  options: MockClaudeOptions,
  scanOf: (mountPoint: string) => MockClaudeScan | null,
) {
  const state = options.claude ?? 'off';
  let enabled = state !== 'off' && state !== 'notfound';
  let privacy = options.privacy !== 'off';
  const canned = platform === 'windows' ? windows() : macos();
  const requests = new Map<string, { timers: number[]; finish: (failure: ClaudeFailure) => void }>();
  let next = 0;

  const status = (): ClaudeStatus => ({
    state:
      state === 'notfound'
        ? 'notFound'
        : !enabled
          ? 'off'
          : state === 'signedout'
            ? 'signedOut'
            : state === 'checktimeout'
              ? 'checkTimedOut'
              : state === 'checkerror'
                ? 'checkFailed'
                : 'on',
    message: !enabled ? null : state === 'checktimeout' ? CHECK_TIMED_OUT : state === 'checkerror' ? CHECK_FAILED : null,
    privacy,
    fixture: false,
    hidden: structuredClone(HIDDEN),
  });

  const basis = (opened: string[]): ClaudeBasis =>
    privacy
      ? { privacy: true, text: BASIS_ON, opened: [], refused: [] }
      : {
          privacy: false,
          text: opened.length === 0 ? 'Claude could open files, but opened none for this answer.' : `Claude opened ${opened[0]}.`,
          opened,
          refused: [],
        };

  const refreshed = (scan: MockClaudeScan): ClaudeRefreshed => ({
    overview: structuredClone(scan.map),
    proposals: structuredClone(scan.proposals),
    tidy: structuredClone(scan.tidy),
    tidyOthers: structuredClone(scan.others),
  });

  /** Adds Claude's items `ids` to the scan, unticked, each carved out of its folder's tile. */
  const add = (scan: MockClaudeScan, ids: string[]): Proposal[] => {
    const added: Proposal[] = [];
    for (const id of ids) {
      const item = canned.items.find((i) => i.id === id);
      if (!item || scan.proposals.some((p) => p.id === id)) continue;
      const p = itemProposal(item);
      scan.proposals = [...scan.proposals, p].sort((a, b) => b.bytes - a.bytes);
      scan.map = carve(scan.map, item);
      added.push(p);
    }
    return added;
  };

  const title = (scan: MockClaudeScan, id: string) => scan.proposals.find((p) => p.id === id)?.title ?? id;

  /** The answer to `job`, applied to the scan like the core's. */
  const answer = (scan: MockClaudeScan, job: ClaudeJob): ClaudeAnswer | ClaudeFailure => {
    switch (job.job) {
      case 'reviewPlan': {
        if (job.ids.length === 0) return { kind: 'error', message: 'Tick something in the plan first: there’s nothing for Claude to review.' };
        const flags = [...canned.flags, ...(privacy || !canned.openFlag ? [] : [canned.openFlag])]
          .filter((f) => job.ids.includes(f.id))
          .map((f) => ({ ...f, title: title(scan, f.id) }));
        const fine = job.ids
          .filter((id) => !flags.some((f) => f.id === id) && scan.proposals.some((p) => p.id === id))
          .map((id) => ({ id, title: title(scan, id), note: canned.fine[id] ?? 'Fine to remove: it comes back or isn’t needed.' }));
        const result: ClaudeResult = {
          kind: 'review',
          summary: flags.length === 0 ? 'Your plan looks fine to me.' : `Your plan looks mostly fine. I’d look again at ${flags.length === 1 ? 'one item' : `${flags.length} items`} before you free it.`,
          flags,
          fine,
        };
        return { job: 'reviewPlan', basis: basis(privacy ? [] : Object.values(canned.opened).slice(0, 2)), result, dropped: [], refreshed: null };
      }
      case 'pickPlan': {
        const changes: ClaudeChange[] = [];
        let plan = [...job.ids];
        // Pick changes ticks only on the rules' proposals: Claude's own items stay as the person left them.
        const rules = (id: string) => scan.proposals.find((x) => x.id === id && x.source !== 'claude');
        for (const u of canned.pick.untick) {
          const p = rules(u.id);
          if (!p || !plan.includes(u.id)) continue;
          plan = plan.filter((id) => id !== u.id);
          changes.push({ id: u.id, title: p.title, change: 'untick', why: u.why, bytes: p.bytes });
        }
        for (const t of canned.pick.tick) {
          const p = rules(t.id);
          if (!p || plan.includes(t.id)) continue;
          plan.push(t.id);
          changes.push({ id: t.id, title: p.title, change: 'tick', why: t.why, bytes: p.bytes });
        }
        const added = add(scan, canned.pick.add);
        for (const p of added) changes.push({ id: p.id, title: p.title, change: 'add', why: p.why, bytes: p.bytes });
        const freed = (ids: string[]) => scan.proposals.filter((p) => p.action === 'delete' && ids.includes(p.id)).reduce((s, p) => s + p.bytes, 0);
        const result: ClaudeResult = {
          kind: 'pick',
          summary: 'I’d leave out what doesn’t come back, and add two things the rules don’t know.',
          changes,
          plan,
          bytesBefore: freed(job.ids),
          bytesAfter: freed(plan),
        };
        return { job: 'pickPlan', basis: basis([]), result, dropped: structuredClone(canned.refused), refreshed: added.length ? refreshed(scan) : null };
      }
      case 'ask':
        return ask(scan, job.about, job.messages);
      case 'explainFolders': {
        const tiles = scan.map.categories.flatMap((c) => c.items);
        const folders: ClaudeFolderNote[] = [];
        const suggestions: string[] = [];
        for (const e of canned.explain) {
          const tile = tiles.find((t) => t.path === e.path);
          if (!tile) continue;
          if (e.suggests) suggestions.push(e.suggests);
          folders.push({
            path: e.path,
            name: e.name,
            bytes: tile.bytes,
            files: tile.files,
            short: e.short,
            explanation: privacy ? e.explanation : e.deep,
            suggested: e.suggests ? [e.suggests] : [],
          });
        }
        add(scan, suggestions);
        const result: ClaudeResult = { kind: 'explain', folders };
        const opened = privacy ? [] : folders.slice(0, 2).map((f) => `${f.path}${platform === 'windows' ? '\\' : '/'}…`);
        return { job: 'explainFolders', basis: basis(opened), result, dropped: structuredClone(canned.refused), refreshed: refreshed(scan) };
      }
      case 'tidyLayout': {
        if (platform !== 'windows') return { kind: 'error', message: 'Tidy up is only on Windows.' };
        const chosen = scan.tidy.filter((t) => job.ids.length === 0 || job.ids.includes(t.id));
        if (chosen.length === 0) return { kind: 'error', message: 'There are no loose files here for Tidy up to sort.' };
        const layouts = chosen.map((t) => claudeLayout(t.layout === 'claude' ? (scan.others.find((o) => o.id === t.id) ?? t) : t));
        scan.others = [...scan.others.filter((o) => !layouts.some((l) => l.id === o.id)), ...layouts];
        const dropped: ClaudeDropped[] = [
          { name: 'desktop.ini', path: `${chosen[0]!.path}\\desktop.ini`, reason: 'Vacancy’s rules keep it where it is: Windows uses it to show this folder.' },
        ];
        return { job: 'tidyLayout', basis: basis([]), result: { kind: 'tidy', layouts }, dropped, refreshed: refreshed(scan) };
      }
    }
  };

  const ask = (scan: MockClaudeScan, about: ClaudeAbout, messages: ClaudeMessage[]): ClaudeAnswer | ClaudeFailure => {
    const last = messages.at(-1);
    if (!last || last.from !== 'person' || !last.text.trim()) return { kind: 'error', message: 'Type a question for Claude first.' };
    // As the core: with privacy mode on, nothing said with it off is sent again.
    if (privacy && messages.some((m) => !m.privacy)) return { kind: 'error', message: PRIVACY_TURNS };
    let reply: string;
    let opened: string[] = [];
    if (about.kind === 'proposal') {
      const p = scan.proposals.find((x) => x.id === about.id);
      if (!p) return { kind: 'error', message: 'That item isn’t in this scan any more. Scan again.' };
      const first = canned.first[p.id] ?? {
        what: p.summary,
        lose: p.regenerable ? 'Not much: it comes back on its own.' : 'What’s in it, for good.',
        back: p.regenerable ? 'Yes, on its own.' : 'No. Vacancy deletes permanently.',
      };
      if (!privacy && canned.opened[p.id]) opened = [canned.opened[p.id]!];
      reply =
        messages.length === 1
          ? `${first.what}\nWhat you’d lose: ${first.lose}\nDoes it come back? ${first.back}`
          : /back|undo|restore|recycle|trash/i.test(last.text)
            ? 'Deleting is permanent: it skips the Recycle Bin and the Trash, and Vacancy can’t undo it. Only Tidy up’s moves can be undone.'
            : `From ${privacy ? 'the names, sizes and dates I can see' : (opened[0] ?? 'the files I opened')}: ${first.lose}`;
    } else {
      const e = canned.explain.find((x) => x.path === about.path);
      const name = about.path.split(/[\\/]/).at(-1) ?? about.path;
      reply = e ? (privacy ? e.explanation : e.deep) : `A ${name} folder I don’t recognise from its name alone. Nothing in it is in your plan.`;
      if (!privacy && e) opened = [`${about.path}${platform === 'windows' ? '\\' : '/'}…`];
    }
    return {
      job: 'ask',
      basis: basis(opened),
      result: { kind: 'ask', reply, followUps: ['Can I get it back?', 'What would you do?'] },
      dropped: [],
      refreshed: null,
    };
  };

  /** Starts a request: Working now, then Slow (`slow`), then Done or Failed. Gives its id at once. */
  const start = (mountPoint: string, job: ClaudeJob, onEvent: (event: ClaudeEvent) => void): Promise<string> => {
    const requestId = `claude-${++next}`;
    const timers: number[] = [];
    let over = false;
    const finish = (failure: ClaudeFailure) => {
      if (over) return;
      over = true;
      for (const t of timers) clearTimeout(t);
      requests.delete(requestId);
      onEvent({ kind: 'failed', requestId, failure });
    };
    requests.set(requestId, { timers, finish });
    const at = (ms: number, f: () => void) => timers.push(window.setTimeout(f, ms));
    at(0, () => onEvent({ kind: 'working', requestId }));
    const ms = state === 'slow' ? 8000 : (options.claudeMs ?? 900);
    if (state === 'slow') at(1500, () => onEvent({ kind: 'slow', requestId }));
    at(state === 'timeout' ? 2500 : ms, () => {
      if (state === 'notfound') return finish({ kind: 'notFound' });
      if (!enabled) return finish({ kind: 'error', message: 'Claude is off. Turn it on first.' });
      if (state === 'signedout') return finish({ kind: 'signedOut' });
      if (state === 'checktimeout' || state === 'checkerror') return finish({ kind: 'error', message: status().message ?? '' });
      if (state === 'timeout') return finish({ kind: 'timedOut' });
      if (state === 'error')
        return finish({ kind: 'error', message: 'API Error: 429 rate_limit_error: This request would exceed your account’s rate limit. Please try again later.' });
      const scan = scanOf(mountPoint);
      if (!scan) return finish({ kind: 'error', message: 'Scan this drive first, then ask Claude.' });
      const result = answer(scan, job);
      if ('kind' in result) return finish(result);
      over = true;
      requests.delete(requestId);
      onEvent({ kind: 'done', requestId, answer: structuredClone(result) });
    });
    return Promise.resolve(requestId);
  };

  return {
    status: () => Promise.resolve(status()),
    setEnabled: (on: boolean) => {
      if (state !== 'notfound') enabled = on;
      if (!on) for (const r of [...requests.values()]) r.finish({ kind: 'cancelled' });
      // Turning on checks that Claude Code answers, as the core does: about as long as an answer (at most 1.2 s).
      const ms = on ? Math.min(options.claudeMs ?? 900, 1200) : 0;
      return new Promise<ClaudeStatus>((resolve) => window.setTimeout(() => resolve(status()), ms));
    },
    setPrivacy: (on: boolean) => {
      privacy = on;
      return Promise.resolve(status());
    },
    start,
    chat: (mountPoint: string, about: ClaudeAbout, messages: ClaudeMessage[], onEvent: (event: ClaudeEvent) => void) =>
      start(mountPoint, { job: 'ask', about, messages }, onEvent),
    cancel: (requestId: string) => {
      requests.get(requestId)?.finish({ kind: 'cancelled' });
      return Promise.resolve();
    },
    chooseLayout: (mountPoint: string, tidyId: string, layout: TidyLayout): Promise<TidyProposal[]> => {
      const scan = scanOf(mountPoint);
      if (!scan) return Promise.reject(new Error('Scan this drive again first.'));
      const current = scan.tidy.find((t) => t.id === tidyId);
      if (!current) return Promise.reject(new Error(`There is no Tidy up ${tidyId} in this scan.`));
      if (current.layout !== layout) {
        const other = scan.others.find((o) => o.id === tidyId);
        if (!other) return Promise.reject(new Error(`Claude hasn’t suggested a layout for ${current.name} yet.`));
        scan.tidy = scan.tidy.map((t) => (t.id === tidyId ? other : t));
        scan.others = [...scan.others.filter((o) => o.id !== tidyId), current];
      }
      return Promise.resolve(structuredClone(scan.tidy));
    },
    removeItems: (mountPoint: string, ids: string[]): Promise<ClaudeRefreshed> => {
      const scan = scanOf(mountPoint);
      if (!scan) return Promise.reject(new Error('Scan this drive again first.'));
      for (const id of ids) {
        const p = scan.proposals.find((x) => x.id === id);
        if (!p) return Promise.reject(new Error(`There is no item ${id} in this scan.`));
        if (p.source !== 'claude') return Promise.reject(new Error(`${p.title} isn’t Claude’s to take back.`));
      }
      scan.proposals = scan.proposals.filter((p) => !ids.includes(p.id));
      for (const c of scan.map.categories) {
        const gone = c.items.filter((i) => i.proposal && ids.includes(i.proposal));
        c.items = c.items.filter((i) => !(i.proposal && ids.includes(i.proposal)));
        // The carved bytes go back to the tile they came from.
        for (const g of gone) {
          const item = canned.items.find((x) => x.id === g.proposal);
          const from = item && c.items.find((i) => i.path === item.from);
          if (from) from.bytes += g.bytes;
        }
      }
      return Promise.resolve(refreshed(scan));
    },
  };
}

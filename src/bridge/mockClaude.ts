// The mock bridge's Claude (ADR 0008): canned answers for every job and every state, on each fixture, shaped like
// what the core sends (src-tauri's claude_* commands). Nothing here calls Claude, reads a disk or moves a file.
// Pick the state with `&claude=off|notfound|signedout|checktimeout|checkerror|on|slow|error|timeout` (`checktimeout`
// and `checkerror`: Claude is on, but asking Claude Code whether it's signed in timed out, or failed) and privacy mode with
// `&privacy=on|off` (`&reads=unavailable`: privacy mode off can't let Claude open files, so calls run as if it were
// on, saying why; `&reads=toobig`: the same for this scan only, because its rules are too many for Claude Code's
// settings, which the panel learns from the first answer); `&canary=ok|slow|fail` makes the check of Claude Code
// before privacy mode off lets Claude read due (it passes, passes after 6 s, or fails; unset, it passed before); `&claudeMs=` sets how long an answer takes (default 900 ms; `slow` takes 8 s, saying it's
// slow after 1.5 s). The model and reasoning picker is kept like the core's (in memory here), and each answer says
// which model made it (`MOCK_MODEL_NAMES`: "Your default" answers as Opus 5.5, as it did on Zach's PC). The answers
// follow the approved prototypes' (prototype/ai-advisor, variant B; prototype/claude-entry, variant A).
import type {
  ClaudeAbout,
  ClaudeAnswer,
  ClaudeBasis,
  ClaudeChange,
  ClaudeChatStarted,
  ClaudeDropped,
  ClaudeEffort,
  ClaudeEvent,
  ClaudeFailure,
  ClaudeFolderNote,
  ClaudeHidden,
  ClaudeJob,
  ClaudeMade,
  ClaudeModel,
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
  /**
   * `unavailable`: privacy mode off can't let Claude open files here (the core's `readsUnavailable`), so every call
   * runs as if it were on, and says why. `toobig`: the same for the scans only (the core's `too_big_on`), so the
   * status says nothing and each answer says why.
   */
  reads?: 'unavailable' | 'toobig';
  /**
   * The check of this Claude Code before privacy mode off lets Claude open files. Unset: it passed before. `ok`:
   * due, and passes (1.2 s); `slow`: due, and passes after 6 s; `fail`: due, and fails, so calls run as if privacy
   * mode were on and the privacy row offers Check again (which fails again).
   */
  canary?: 'ok' | 'slow' | 'fail';
}

/** One chat, as the core keeps it: what it's about, the privacy mode it began in, its turns, and whether it ended. */
export interface MockChat {
  about: ClaudeAbout;
  privacy: boolean;
  turns: { from: 'person' | 'claude'; text: string }[];
  /** Privacy mode changed, or Claude was turned off, since it began. */
  ended: boolean;
}

/** What the mock Claude reads and changes: a drive's scan, as the mock bridge keeps it. */
export interface MockClaudeScan {
  /** Which scan it is (`ScanOutcome`'s `scan`): Claude's commands name it, and are refused for another. */
  scan: number;
  map: Overview;
  proposals: Proposal[];
  tidy: TidyProposal[];
  /** The layout of each sort not in use, once Claude has suggested one. */
  others: TidyProposal[];
  /** The last `ClaudeRefreshed.revision` given out. */
  revision: number;
  /** The scan's chats, by id, as the core keeps them. */
  chats: Map<string, MockChat>;
}

/** What the core says when a Claude command names a scan the drive no longer has. */
export const SCAN_GONE = 'This drive’s map has changed since: scan it again, then ask Claude.';

/** The model each alias answers as in the mock, as the core names Claude Code's `modelUsage` id. */
export const MOCK_MODEL_NAMES: Record<ClaudeModel, string> = {
  default: 'Opus 5.5',
  opus: 'Opus 5.5',
  sonnet: 'Sonnet 5.5',
  haiku: 'Haiku 5.5',
  fable: 'Fable 5.5',
};

/** A checked answer before the mock stamps it with the model and reasoning it was asked with. */
type Answered = Omit<ClaudeAnswer, 'made'>;

const GB = 1e9;
const gb = (bytes: number) => `${(bytes / GB).toFixed(1)} GB`;

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
/** What the core says when no rule can keep a moved Claude Code folder hidden (`runner::reads_unavailable_in`). */
export const READS_UNAVAILABLE =
  'Claude can’t open files for now: Vacancy can’t name the folder \\\\nas\\home\\claude in a rule that keeps it hidden from Claude, so Claude gets names, sizes, counts and dates only.';
/** What the core says when this Claude Code didn't pass the check before privacy mode off (`canary::unverified`). */
export const CHECK_UNVERIFIED =
  'Claude can’t open files for now: Vacancy couldn’t verify this version of Claude Code keeps files hidden. Claude didn’t return the check’s readable file, so Vacancy couldn’t tell whether Claude Code took its settings. Claude gets names, sizes, counts and dates only.';
/** What the core says when a call comes before the check has run. */
export const CHECK_DUE =
  'Claude can’t open files for now: Vacancy couldn’t verify this version of Claude Code keeps files hidden. The check hasn’t run yet. Claude gets names, sizes, counts and dates only.';
/** What the core says when a scan's rules would be too big for Claude Code's settings (`runner::settings_too_big`). */
export const READS_TOO_BIG =
  'Claude can’t open files on this scan: the rules that keep this drive’s browser profiles and other hidden places from Claude would be 2148 KB, more than Claude Code takes (1536 KB), so Claude gets names, sizes, counts and dates only.';

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

/**
 * The mock's Claude: settings, state and canned answers, over the mock bridge's scans. `scanOf` gives the drive's
 * scan `scan` if it is still the drive's; `endChats` ends every chat on every scan (privacy changed, or Claude off).
 */
export function createMockClaude(
  platform: Platform,
  options: MockClaudeOptions,
  scanOf: (mountPoint: string, scan: number) => MockClaudeScan | null,
  endChats: () => void,
) {
  const state = options.claude ?? 'off';
  let enabled = state !== 'off' && state !== 'notfound';
  let setting = options.privacy !== 'off';
  /** The model and reasoning new questions ask for, as the core keeps them. */
  let model: ClaudeModel = 'default';
  let effort: ClaudeEffort = 'default';
  /** As the core, when no rule could keep Claude where it may read (`&reads=unavailable`): why. */
  const unavailable = options.reads === 'unavailable' ? READS_UNAVAILABLE : null;
  /** As the core, when the scan's rules wouldn't fit in Claude Code's settings (`&reads=toobig`): why, per answer. */
  const tooBig = options.reads === 'toobig' ? READS_TOO_BIG : null;
  /**
   * The check of this Claude Code before privacy mode off lets Claude open files (`&canary=`): unset, it passed
   * before (as kept from an earlier run); `ok`, `slow` and `fail` start due, and the check passes, passes after six
   * seconds, or fails. A call with privacy mode off while it is due checks first, as the core does.
   */
  let checked: 'unchecked' | 'passed' | 'failed' = options.canary ? 'unchecked' : 'passed';
  let checking = false;
  let pending: Promise<ClaudeStatus> | null = null;
  /** A call with privacy mode off while the check is due checks first, as the core's does. */
  const due = () => {
    if (enabled && !setting && checked === 'unchecked' && !checking) checked = options.canary === 'fail' ? 'failed' : 'passed';
  };
  /** Why Claude can't open files because of the check, or null once it passed. */
  const checkWhy = () => (checked === 'passed' ? null : checked === 'failed' ? CHECK_UNVERIFIED : CHECK_DUE);
  /** Privacy mode for a call: on, or off with reads unavailable, when the core runs it as if it were on. */
  const calls = () => setting || unavailable !== null || tooBig !== null || checkWhy() !== null;
  const canned = platform === 'windows' ? windows() : macos();
  const requests = new Map<string, { timers: number[]; finish: (failure: ClaudeFailure) => void }>();
  let next = 0;
  let chats = 0;

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
    privacy: setting,
    fixture: false,
    readsUnavailable: unavailable,
    hidden: structuredClone(HIDDEN),
    check:
      !enabled || setting || state === 'notfound' || state === 'signedout' || state === 'checktimeout' || state === 'checkerror'
        ? { kind: 'unchecked' }
        : checking
          ? { kind: 'checking' }
          : checked === 'passed'
            ? { kind: 'passed' }
            : checked === 'failed'
              ? { kind: 'failed', message: CHECK_UNVERIFIED }
              : { kind: 'unchecked' },
    model,
    effort,
  });

  const basis = (opened: string[]): ClaudeBasis => {
    // As the core: the check's reason goes in the answer's text only (the status says it while it lasts); a
    // lasting one (no rule for a folder, or this scan's rules too many) in its basis for the privacy row too.
    const why = setting ? null : (unavailable ?? checkWhy() ?? tooBig);
    const lasting = setting ? null : (unavailable ?? (checkWhy() === null ? tooBig : null));
    return calls()
      ? { privacy: true, text: why ?? BASIS_ON, opened: [], refused: [], unavailable: lasting }
      : {
          privacy: false,
          text:
            opened.length === 0
              ? 'Claude could open files on the drive you scanned, but opened none for this answer.'
              : opened.length === 1
                ? `Claude opened ${opened[0]}, from the drive you scanned.`
                : `Claude opened ${opened.length} files from the drive you scanned, like ${opened[0]}.`,
          opened,
          refused: [],
          unavailable: null,
        };
  };

  const refreshed = (scan: MockClaudeScan): ClaudeRefreshed => ({
    revision: ++scan.revision,
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
  const answer = (scan: MockClaudeScan, job: ClaudeJob): Answered | ClaudeFailure => {
    switch (job.job) {
      case 'reviewPlan': {
        if (job.ids.length === 0) return { kind: 'error', message: 'Tick something in the plan first: there’s nothing for Claude to review.' };
        const flags = [...canned.flags, ...(calls() || !canned.openFlag ? [] : [canned.openFlag])]
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
        return { job: 'reviewPlan', basis: basis(calls() ? [] : Object.values(canned.opened).slice(0, 2)), result, dropped: [], refreshed: null };
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
        return ask(scan, job.conversation, job.text);
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
            explanation: calls() ? e.explanation : e.deep,
            suggested: e.suggests ? [e.suggests] : [],
          });
        }
        add(scan, suggestions);
        const result: ClaudeResult = { kind: 'explain', folders };
        const opened = calls() ? [] : folders.slice(0, 2).map((f) => `${f.path}${platform === 'windows' ? '\\' : '/'}…`);
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

  /** A question about the whole scan: answered from its proposals and categories, names and sizes. It adds nothing. */
  const aboutScan = (scan: MockClaudeScan, first: boolean, text: string): string => {
    const top = [...scan.proposals].sort((a, b) => b.bytes - a.bytes).slice(0, 3);
    const back = scan.proposals.filter((p) => p.regenerable).length;
    const biggest = [...scan.map.categories].filter((c) => !c.protected).sort((a, b) => b.bytes - a.bytes)[0];
    if (/back|undo|restore|recycle|trash/i.test(text))
      return 'Deleting is permanent: it skips the Recycle Bin and the Trash, and Vacancy can’t undo it. Only Tidy up’s moves can be undone. Caches and build output come back on their own; your own files don’t.';
    if (first || /clean|free|delete|space|biggest/i.test(text))
      return `The biggest things on ${scan.map.volume.name} you could clean up are ${top.map((p) => `${p.title} (${gb(p.bytes)})`).join(', ')}. ${back} of the ${scan.proposals.length} proposals come back on their own. Tick what you want in the plan tray; nothing changes until you confirm.`;
    return biggest
      ? `From the scan: most of the space is ${biggest.name}, ${gb(biggest.bytes)}. Explain big folders tells you about the big folders the rules don’t know.`
      : 'From the scan, I can’t tell more than the names, sizes and dates show.';
  };

  const ask = (scan: MockClaudeScan, conversation: string, text: string): Answered | ClaudeFailure => {
    // As the core: only a chat begun in this privacy mode, and not ended since by a privacy change, is asked in.
    const chat = scan.chats.get(conversation);
    if (!chat || chat.ended || chat.privacy !== calls()) return { kind: 'chatEnded' };
    if (!text.trim()) return { kind: 'error', message: 'Type a question for Claude first.' };
    const about = chat.about;
    const last = { from: 'person' as const, text: text.trim() };
    const messages = [...chat.turns, last];
    let reply: string;
    let opened: string[] = [];
    let followUps = ['Can I get it back?', 'What would you do?'];
    if (about.kind === 'scan') {
      reply = aboutScan(scan, messages.length === 1, last.text);
      followUps = ['Which of these come back on their own?', 'Can I get them back after?'];
    } else if (about.kind === 'proposal') {
      const p = scan.proposals.find((x) => x.id === about.id);
      if (!p) return { kind: 'error', message: 'That item isn’t in this scan any more. Scan again.' };
      const first = canned.first[p.id] ?? {
        what: p.summary,
        lose: p.regenerable ? 'Not much: it comes back on its own.' : 'What’s in it, for good.',
        back: p.regenerable ? 'Yes, on its own.' : 'No. Vacancy deletes permanently.',
      };
      if (!calls() && canned.opened[p.id]) opened = [canned.opened[p.id]!];
      reply =
        messages.length === 1
          ? `${first.what}\nWhat you’d lose: ${first.lose}\nDoes it come back? ${first.back}`
          : /back|undo|restore|recycle|trash/i.test(last.text)
            ? 'Deleting is permanent: it skips the Recycle Bin and the Trash, and Vacancy can’t undo it. Only Tidy up’s moves can be undone.'
            : `From ${calls() ? 'the names, sizes and dates I can see' : (opened[0] ?? 'the files I opened')}: ${first.lose}`;
    } else {
      const e = canned.explain.find((x) => x.path === about.path);
      const name = about.path.split(/[\\/]/).at(-1) ?? about.path;
      reply = e ? (calls() ? e.explanation : e.deep) : `A ${name} folder I don’t recognise from its name alone. Nothing in it is in your plan.`;
      if (!calls() && e) opened = [`${about.path}${platform === 'windows' ? '\\' : '/'}…`];
    }
    chat.turns.push(last, { from: 'claude', text: reply });
    return {
      job: 'ask',
      basis: basis(opened),
      result: { kind: 'ask', reply, followUps },
      dropped: [],
      refreshed: null,
    };
  };

  /** Starts a request on the drive's scan `scan`: Working now, then Slow (`slow`), then Done or Failed. Gives its id at once. */
  const start = (mountPoint: string, scanId: number, job: ClaudeJob, onEvent: (event: ClaudeEvent) => void): Promise<string> => {
    const requestId = `claude-${++next}`;
    const timers: number[] = [];
    let over = false;
    // As the core: the model and reasoning are fixed when the request starts; a change applies to the next one.
    const made: ClaudeMade = { model, effort, name: MOCK_MODEL_NAMES[model] };
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
      due();
      const scan = scanOf(mountPoint, scanId);
      if (!scan) return finish({ kind: 'error', message: SCAN_GONE });
      const result = answer(scan, job);
      if ('kind' in result) return finish(result);
      over = true;
      requests.delete(requestId);
      onEvent({ kind: 'done', requestId, answer: { ...structuredClone(result), made } });
    });
    return Promise.resolve(requestId);
  };

  return {
    status: () => Promise.resolve(status()),
    setEnabled: (on: boolean) => {
      if (state !== 'notfound') enabled = on;
      if (!on) {
        endChats();
        for (const r of [...requests.values()]) r.finish({ kind: 'cancelled' });
      }
      // Turning on checks that Claude Code answers, as the core does: about as long as an answer (at most 1.2 s).
      const ms = on ? Math.min(options.claudeMs ?? 900, 1200) : 0;
      return new Promise<ClaudeStatus>((resolve) => window.setTimeout(() => resolve(status()), ms));
    },
    setPrivacy: (on: boolean) => {
      // As the core: the new mode and the end of every chat in one step.
      setting = on;
      endChats();
      return Promise.resolve(status());
    },
    /** As the core's `claude_set_model`: kept, and no chat ends. */
    setModel: (m: ClaudeModel, e: ClaudeEffort) => {
      model = m;
      effort = e;
      return Promise.resolve(status());
    },
    /** As the core's `claude_check`: checks this Claude Code when the check is due, or always with `again`. */
    check: (again: boolean): Promise<ClaudeStatus> => {
      if (!enabled || setting || state === 'notfound' || (!again && checked !== 'unchecked') || checking)
        return checking ? (pending ?? Promise.resolve(status())) : Promise.resolve(status());
      checking = true;
      const ms = options.canary === 'slow' ? 6000 : 1200;
      pending = new Promise<ClaudeStatus>((resolve) =>
        window.setTimeout(() => {
          checking = false;
          pending = null;
          checked = options.canary === 'fail' ? 'failed' : 'passed';
          resolve(status());
        }, ms),
      );
      return pending;
    },
    start,
    chat: (
      mountPoint: string,
      scanId: number,
      conversation: string | null,
      about: ClaudeAbout,
      text: string,
      onEvent: (event: ClaudeEvent) => void,
    ): Promise<ClaudeChatStarted> => {
      let id = conversation;
      if (id === null) {
        const scan = scanOf(mountPoint, scanId);
        if (!scan) return Promise.reject(new Error(SCAN_GONE));
        due();
        id = `chat-${++chats}`;
        scan.chats.set(id, { about, privacy: calls(), turns: [], ended: false });
      }
      const chat = id;
      return start(mountPoint, scanId, { job: 'ask', conversation: chat, text }, onEvent).then((requestId) => ({
        requestId,
        conversation: chat,
      }));
    },
    cancel: (requestId: string) => {
      requests.get(requestId)?.finish({ kind: 'cancelled' });
      return Promise.resolve();
    },
    chooseLayout: (mountPoint: string, scanId: number, tidyId: string, layout: TidyLayout): Promise<ClaudeRefreshed> => {
      const scan = scanOf(mountPoint, scanId);
      if (!scan) return Promise.reject(new Error(SCAN_GONE));
      const current = scan.tidy.find((t) => t.id === tidyId);
      if (!current) return Promise.reject(new Error(`There is no Tidy up ${tidyId} in this scan.`));
      if (current.layout !== layout) {
        const other = scan.others.find((o) => o.id === tidyId);
        if (!other) return Promise.reject(new Error(`Claude hasn’t suggested a layout for ${current.name} yet.`));
        scan.tidy = scan.tidy.map((t) => (t.id === tidyId ? other : t));
        scan.others = [...scan.others.filter((o) => o.id !== tidyId), current];
      }
      return Promise.resolve(refreshed(scan));
    },
    removeItems: (mountPoint: string, scanId: number, ids: string[]): Promise<ClaudeRefreshed> => {
      const scan = scanOf(mountPoint, scanId);
      if (!scan) return Promise.reject(new Error(SCAN_GONE));
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

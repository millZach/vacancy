// Plans and reports for tests, shaped like the core's (and the Windows fixture's): two steps, one of them partial.
import { fixtures, type PlanSummary, type Reorganized, type RunReport, type Skip, type StepReport } from '../bridge';

const GB = 1e9;

export const plan: PlanSummary = {
  id: 'plan-1',
  mountPoint: 'C:\\',
  bytes: 27.3 * GB,
  items: 460_428,
  places: 67,
  moves: 0,
  steps: [
    { id: 'w-node', title: 'node_modules in 61 idle projects', category: 'dev', bytes: 21 * GB, items: 412_118, places: 61, kind: 'delete', moves: 0, source: 'rule' },
    { id: 'w-browser', title: 'Browser caches', category: 'caches', bytes: 6.3 * GB, items: 48_310, places: 6, kind: 'delete', moves: 0, source: 'rule' },
  ],
  refused: [],
  movesRefused: [],
  leftForYou: [],
};

export const inUse: Skip = {
  path: 'C:\\Users\\alex\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data',
  reason: '212 files are open in Microsoft Edge',
  kind: 'inUse',
  bytes: 0.41 * GB,
  items: 212,
};

export const changed: Skip = {
  path: 'C:\\Users\\alex\\Downloads\\Installers\\Docker Desktop Installer.exe',
  reason: 'It changed since the scan',
  kind: 'changed',
  bytes: 0.62 * GB,
  items: 1,
};

/** A folder a run reorganized, as the core reports it, with no journal unless `over` gives one. */
export function reorganized(over: Partial<Reorganized> & Pick<Reorganized, 'id' | 'folder' | 'movedItems'>): Reorganized {
  return {
    plannedItems: over.movedItems,
    journal: '',
    path: `C:\\Users\\alex\\${over.folder}`,
    into: [],
    foldersMade: 0,
    undoable: false,
    interrupted: false,
    note: null,
    undo: null,
    ...over,
  };
}

export function stepReport(over: Partial<StepReport> & Pick<StepReport, 'id' | 'title' | 'category'>): StepReport {
  return {
    why: '',
    regenerable: true,
    plannedBytes: 0,
    freedBytes: 0,
    plannedItems: 0,
    deletedItems: 0,
    kind: 'delete',
    movedItems: 0,
    places: 1,
    source: 'rule',
    outcome: 'done',
    skipped: [],
    ...over,
  };
}

export const nodeDone = stepReport({
  id: 'w-node',
  title: 'node_modules in 61 idle projects',
  why: 'Run npm install in a project to bring them back. Projects you used in the last 90 days are left alone.',
  category: 'dev',
  plannedBytes: 21 * GB,
  freedBytes: 21 * GB,
  plannedItems: 412_118,
  deletedItems: 412_118,
  places: 61,
});

export const browserPartial = stepReport({
  id: 'w-browser',
  title: 'Browser caches',
  why: 'Browsers download these again as you browse.',
  category: 'caches',
  plannedBytes: 6.3 * GB,
  freedBytes: 5.89 * GB,
  plannedItems: 48_310,
  deletedItems: 48_098,
  places: 6,
  outcome: 'partial',
  skipped: [inUse],
});

/** The report of `plan`'s run on the Windows fixture's C: (476 GB, 431 GB used). */
export const report: RunReport = {
  id: 'run-1',
  planId: 'plan-1',
  mountPoint: 'C:\\',
  volumeName: 'Local Disk (C:)',
  startedAt: Date.UTC(2026, 9, 17, 9, 0, 0),
  durationMs: 72_400,
  cancelled: false,
  steps: [nodeDone, browserPartial],
  plannedBytes: 27.3 * GB,
  freedBytes: 26.89 * GB,
  deletedItems: 460_216,
  movedItems: 0,
  skippedItems: 212,
  freedByCategory: [
    { id: 'dev', bytes: 21 * GB },
    { id: 'caches', bytes: 5.89 * GB },
  ],
  totalBytes: 476 * GB,
  usedBefore: 431 * GB,
  usedAfter: 404.38 * GB,
  freeBefore: 45 * GB,
  freeAfter: 71.62 * GB,
  freeChange: 26.62 * GB,
  biggestWin: 'w-node',
  leftForYou: [],
  reorganized: [],
  map: fixtures.windows.overview,
};

/** A hard-link note: deleted here, its space still held by another name elsewhere. */
export const hardLinked: Skip = {
  path: 'C:\\Users\\alex\\code\\portfolio-2024\\node_modules\\.pnpm',
  reason: '38 files are also kept elsewhere under other names (hard links), so their space is still in use',
  kind: 'hardLinked',
  bytes: 0.09 * GB,
  items: 38,
};

/**
 * `report` with everything the finish can show: a hard-link note on the biggest win, advice left for the user, and
 * folders reorganized (as ticket 08's runs will report them).
 */
export const fullReport: RunReport = {
  ...report,
  id: 'run-full',
  steps: [{ ...nodeDone, skipped: [hardLinked] }, browserPartial],
  movedItems: 1_028,
  reorganized: [
    reorganized({ id: 'r-downloads', folder: 'Downloads', movedItems: 812 }),
    reorganized({ id: 'r-desktop', folder: 'Desktop', movedItems: 216 }),
  ],
  leftForYou: [
    {
      id: 'w-game',
      title: 'Cyberpunk 2077 (not played in 14 months)',
      summary: '71.0 GB in Steam.',
      why: 'Vacancy won’t delete games directly. Uninstall it from Steam so Steam stays consistent.',
      bytes: 71 * GB,
    },
  ],
};

/** A saved report with real moves and legacy advice, but no deletion steps. */
export const moveAdviceReport: RunReport = {
  ...fullReport,
  id: 'run-move-advice',
  steps: [], plannedBytes: 0, freedBytes: 0, deletedItems: 0, skippedItems: 0, freedByCategory: [], biggestWin: null,
  usedAfter: report.usedBefore, freeAfter: report.freeBefore, freeChange: 0,
  movedItems: 4,
  reorganized: [reorganized({ id: 'r-move-advice', folder: 'Downloads', movedItems: 4 })],
};

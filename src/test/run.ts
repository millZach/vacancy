// Plans and reports for tests, shaped like the core's (and the Windows fixture's): two steps, one of them partial.
import type { PlanSummary, RunReport, Skip, StepReport } from '../bridge';

const GB = 1e9;

export const plan: PlanSummary = {
  id: 'plan-1',
  mountPoint: 'C:\\',
  bytes: 27.3 * GB,
  items: 460_428,
  places: 67,
  moves: 0,
  steps: [
    { id: 'w-node', title: 'node_modules in 61 idle projects', category: 'dev', bytes: 21 * GB, items: 412_118, places: 61 },
    { id: 'w-browser', title: 'Browser caches', category: 'caches', bytes: 6.3 * GB, items: 48_310, places: 6 },
  ],
  refused: [],
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

export function stepReport(over: Partial<StepReport> & Pick<StepReport, 'id' | 'title' | 'category'>): StepReport {
  return {
    regenerable: true,
    plannedBytes: 0,
    freedBytes: 0,
    plannedItems: 0,
    deletedItems: 0,
    places: 1,
    outcome: 'done',
    skipped: [],
    ...over,
  };
}

export const nodeDone = stepReport({
  id: 'w-node',
  title: 'node_modules in 61 idle projects',
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
};

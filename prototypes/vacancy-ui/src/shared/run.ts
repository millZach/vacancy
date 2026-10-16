// PROTOTYPE run simulator. Turns a selection into the steps a run would take and the result it would report.
// Deterministic so every variant shows the same outcome.

import type { Dataset, Proposal, ReorgProposal } from './data';

export interface Skipped {
  path: string;
  reason: string;
  bytes: number;
}

export interface RunStep {
  id: string;
  kind: 'delete' | 'move';
  title: string;
  categoryId: string;
  /** Planned bytes (delete) or 0 (move). */
  plannedBytes: number;
  /** Bytes actually freed. Less than planned when files were in use. */
  freedBytes: number;
  /** Files deleted or moved. */
  items: number;
  /** Simulated duration for progress animations. */
  durationMs: number;
  outcome: 'done' | 'partial';
  skipped: Skipped[];
  /** Short log lines to stream while the step runs. */
  log: string[];
}

export interface RunResult {
  steps: RunStep[];
  freedBytes: number;
  plannedBytes: number;
  deletedItems: number;
  movedItems: number;
  skipped: Skipped[];
  durationSec: number;
  usedBefore: number;
  usedAfter: number;
  totalBytes: number;
  /** Freed bytes per category id. */
  freedByCategory: Record<string, number>;
  /** Advice items the user selected that Vacancy can't do itself (uninstall a game, etc.). */
  leftForYou: Proposal[];
  /** Moves are journaled; this is what Undo would restore. */
  undoable: { reorgId: string; folder: string; moves: number }[];
  biggestWin: RunStep | null;
}

// Files that were "in use" during the simulated run.
const SKIPS: Record<string, Skipped[]> = {
  'w-browser': [{ path: '...\\Microsoft\\Edge\\User Data\\Default\\Cache', reason: 'Edge was open', bytes: 0.4e9 }],
  'w-temp': [{ path: 'C:\\Users\\alex\\AppData\\Local\\Temp (112 files)', reason: 'In use by running apps', bytes: 0.2e9 }],
  'm-appcache': [{ path: '~/Library/Application Support/Slack/Cache', reason: 'Slack was running', bytes: 0.6e9 }],
  'm-caches': [{ path: '~/Library/Caches/com.apple.Safari', reason: 'Safari was running', bytes: 0.3e9 }],
};

function stepDuration(bytes: number, items: number): number {
  return Math.round(900 + Math.min(2600, bytes / 1e7 + items / 40));
}

export function buildRun(d: Dataset, proposalIds: Set<string>, reorgIds: Set<string>): RunResult {
  const steps: RunStep[] = [];
  const leftForYou: Proposal[] = [];

  for (const p of d.proposals) {
    if (!proposalIds.has(p.id)) continue;
    if (p.action === 'hint') {
      leftForYou.push(p);
      continue;
    }
    const skipped = SKIPS[p.id] ?? [];
    const skippedBytes = skipped.reduce((s, k) => s + k.bytes, 0);
    const freed = p.bytes - skippedBytes;
    steps.push({
      id: p.id,
      kind: 'delete',
      title: p.title,
      categoryId: p.categoryId,
      plannedBytes: p.bytes,
      freedBytes: freed,
      items: p.itemCount,
      durationMs: stepDuration(p.bytes, p.itemCount),
      outcome: skipped.length ? 'partial' : 'done',
      skipped,
      log: [
        ...p.samplePaths.map((s) => `deleted ${s}`),
        ...skipped.map((k) => `skipped ${k.path} (${k.reason.toLowerCase()})`),
      ],
    });
  }

  for (const r of d.reorgs) {
    if (!reorgIds.has(r.id)) continue;
    steps.push(reorgStep(r));
  }

  const freedBytes = steps.reduce((s, x) => s + x.freedBytes, 0);
  const plannedBytes = steps.reduce((s, x) => s + x.plannedBytes, 0);
  const freedByCategory: Record<string, number> = {};
  for (const s of steps) freedByCategory[s.categoryId] = (freedByCategory[s.categoryId] ?? 0) + s.freedBytes;
  const deleteSteps = steps.filter((s) => s.kind === 'delete');

  return {
    steps,
    freedBytes,
    plannedBytes,
    deletedItems: deleteSteps.reduce((s, x) => s + x.items, 0),
    movedItems: steps.filter((s) => s.kind === 'move').reduce((s, x) => s + x.items, 0),
    skipped: steps.flatMap((s) => s.skipped),
    durationSec: Math.round(steps.reduce((s, x) => s + x.durationMs, 0) / 1000 * 7.3),
    usedBefore: d.disk.usedBytes,
    usedAfter: d.disk.usedBytes - freedBytes,
    totalBytes: d.disk.totalBytes,
    freedByCategory,
    leftForYou,
    undoable: d.reorgs.filter((r) => reorgIds.has(r.id)).map((r) => ({ reorgId: r.id, folder: r.folder, moves: r.moveCount })),
    biggestWin: deleteSteps.reduce<RunStep | null>((best, s) => (!best || s.freedBytes > best.freedBytes ? s : best), null),
  };
}

function reorgStep(r: ReorgProposal): RunStep {
  return {
    id: r.id,
    kind: 'move',
    title: r.title,
    categoryId: r.folder.toLowerCase() === 'downloads' ? 'downloads' : 'docs',
    plannedBytes: 0,
    freedBytes: 0,
    items: r.moveCount,
    durationMs: 1400 + Math.min(2000, r.moveCount),
    outcome: 'done',
    skipped: [],
    log: r.sampleMoves.map((m) => `moved ${m.from} -> ${m.to}`),
  };
}

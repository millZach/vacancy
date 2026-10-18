// The mock bridge's planner and executor: plans from a drive's fixture proposals, and a run that plays them back on a
// clock, with the events and the report the core sends. Nothing is touched; the fixture's skips say what is left.
import type { RunSkip } from './fixtures';
import type { TidyScript } from './tidyFixture';
import type {
  CategoryFreed,
  LeftForYou,
  Overview,
  PlanSummary,
  Proposal,
  Reorganized,
  RunEvent,
  RunReport,
  Skip,
  StepReport,
  TidyMove,
  TidyProposal,
  Undone,
  VolumeInfo,
} from './types';
import { activeMoves, foldersMade, isTidy, kindFolders } from '../tidy/model';

/** One place a plan step deletes: a folder or file, with its share of the step's bytes and files. */
interface Place {
  path: string;
  bytes: number;
  items: number;
  /**
   * What the run finds there: something in use, which it leaves, or a note about something it deleted whose space
   * stays in use (a hard link kept elsewhere).
   */
  inUse: Skip | null;
}

interface MockStep {
  proposal: Proposal;
  places: Place[];
  /** Places planning left out, reported as skipped when the step runs. */
  refused: Skip[];
}

/** A Tidy up sort in a plan: the folder's proposal, the moves the plan makes, and what its run and Undo find. */
export interface MockSort {
  tidy: TidyProposal;
  moves: TidyMove[];
  script: TidyScript;
}

/** A plan the mock has made, until it runs or the drive is scanned again. */
export interface MockPlan {
  summary: PlanSummary;
  steps: MockStep[];
  /** Its sorts, after its deletes. */
  sorts: MockSort[];
  /** Advice in the selection, left for the user. */
  advice: LeftForYou[];
  /** The map of the scan it was made from, for the report. */
  map: Overview;
}

/**
 * The plan for proposals `ids` of a drive's scan: one step per delete proposal, biggest first, its places spread from
 * the proposal's sample paths. Places the fixture says changed since the scan are left out and listed in `refused`.
 * Hints are saved as reminders, never as deletion steps or freed space.
 */
export function makePlan(
  id: string,
  mountPoint: string,
  map: Overview,
  proposals: Proposal[],
  ids: string[],
  skips: RunSkip[],
  tidy: TidyProposal[] = [],
  scripts: Record<string, TidyScript> = {},
): MockPlan {
  const unknown = ids.find((i) => !proposals.some((p) => p.id === i) && !tidy.some((t) => t.id === i));
  if (unknown !== undefined) throw new Error(`there is no proposal ${unknown} in this scan`);
  const chosen = proposals.filter((p) => ids.includes(p.id));
  // Delete wins over sort: a file a delete step of this plan takes stays out of the sort.
  const deleting = chosen.filter((p) => p.action === 'delete').map((p) => p.id);
  const sorts = tidy
    .filter((t) => ids.includes(t.id))
    .map((t): MockSort => ({ tidy: t, moves: activeMoves(t, deleting), script: scripts[t.id] ?? { skips: [], stuck: [], kept: [] } }))
    .filter((s) => s.moves.length > 0);
  if (chosen.length === 0 && sorts.length === 0) throw new Error('the plan has no selected proposals');
  const steps = chosen
    .filter((p) => p.action === 'delete')
    .map((p): MockStep => {
      const mine = skips.filter((s) => s.proposal === p.id);
      const refused = mine.filter((s) => s.when === 'plan').map((s) => s.skip);
      const inUse = mine.filter((s) => s.when === 'run').map((s) => s.skip);
      const paths = placePaths(p).filter((path) => !refused.some((r) => r.path === path));
      const bytes = p.bytes - refused.reduce((sum, r) => sum + r.bytes, 0);
      const items = p.itemCount - refused.reduce((sum, r) => sum + r.items, 0);
      const places = spread(paths, bytes, items);
      for (const skip of inUse) {
        const at = places.find((pl) => pl.path === skip.path) ?? places[Math.floor(places.length * 0.6)];
        if (at) at.inUse = { ...skip, bytes: Math.min(skip.bytes, at.bytes), items: Math.min(skip.items, at.items) };
      }
      return { proposal: p, places, refused };
    });
  const planSteps: PlanSummary['steps'] = [
    ...steps.map((s) => ({
      id: s.proposal.id,
      title: s.proposal.title,
      category: s.proposal.category,
      bytes: sum(s.places, (pl) => pl.bytes),
      items: sum(s.places, (pl) => pl.items),
      places: s.places.length,
      kind: 'delete' as const,
      moves: 0,
    })),
    ...sorts.map((s) => ({
      id: s.tidy.id,
      title: `Sort ${s.tidy.name}`,
      category: s.tidy.folder === 'downloads' ? ('downloads' as const) : ('media' as const),
      bytes: 0,
      items: 0,
      places: 0,
      kind: 'move' as const,
      moves: s.moves.length,
    })),
  ];
  const advice = chosen
    .filter((p) => p.action === 'hint')
    .map((p): LeftForYou => ({ id: p.id, title: p.title, summary: p.summary, why: p.why, bytes: 0, hintAction: p.hintAction ?? null }));
  return {
    steps,
    sorts,
    advice,
    map,
    summary: {
      id,
      mountPoint,
      bytes: sum(planSteps, (s) => s.bytes),
      items: sum(planSteps, (s) => s.items),
      places: sum(planSteps, (s) => s.places),
      moves: sum(sorts, (s) => s.moves.length),
      movesRefused: [],
      steps: planSteps,
      refused: steps.flatMap((s) => s.refused),
      leftForYou: advice.map((a) => a.id),
    },
  };
}

/** What other apps wrote to the drive while a mock run went: the drive's free space grows by a little less. */
const OTHERS_WROTE = 0.27e9;

export interface Playback {
  /** Stops after the file being deleted. */
  cancel: () => void;
  done: Promise<RunReport>;
}

/**
 * Plays a plan back over `totalMs`: each step takes its share (bigger steps a little longer), with progress ten times
 * a second, each place deleted as it goes, and the fixture's skips as they happen. Cancelling stops after the file
 * being deleted: what is left is reported skipped, like the core does.
 */
export function play(
  run: { id: string; plan: MockPlan; volume: VolumeInfo; totalMs: number; moves?: Reorganized[] },
  onEvent: (event: RunEvent) => void,
): Playback {
  const { plan, volume } = run;
  const startedAt = Date.now();
  const biggest = Math.max(1, ...plan.steps.map((s) => s.proposal.bytes));
  // Sorts take a share like a small delete step's.
  const weights = [...plan.steps.map((s) => 0.6 + 0.4 * (s.proposal.bytes / biggest)), ...plan.sorts.map(() => 0.6)];
  const weightSum = sum(weights, (w) => w) || 1;
  const spans = weights.map((w) => (run.totalMs * w) / weightSum);
  const records: Reorganized[] = [];

  let cancelled = false;
  let index = 0;
  let stepStart = startedAt;
  let placesDone = 0;
  let stepFreed = 0;
  let stepDeleted = 0;
  let totalFreed = 0;
  let skipped: Skip[] = [];
  const reports: StepReport[] = [];
  let timer = 0;

  const skip = (s: Skip) => {
    skipped.push(s);
    onEvent({ kind: 'skipped', step: index, skip: s });
  };
  const progress = (currentPath: string, extraBytes = 0, extraItems = 0) =>
    onEvent({
      kind: 'progress',
      step: index,
      freedBytes: stepFreed + extraBytes,
      deletedItems: stepDeleted + extraItems,
      totalFreedBytes: totalFreed + stepFreed + extraBytes,
      currentPath,
    });
  const begin = () => {
    const step = plan.steps[index]!;
    stepStart = Date.now();
    placesDone = 0;
    stepFreed = 0;
    stepDeleted = 0;
    skipped = [];
    onEvent({ kind: 'stepStarted', step: index, id: step.proposal.id });
    for (const r of step.refused) skip(r);
  };
  const deletePlace = (place: Place) => {
    const freed = place.bytes - (place.inUse?.bytes ?? 0);
    // A name deleted while another keeps the file is deleted; only its space stays in use.
    const items = place.items - (place.inUse && !isNote(place.inUse) ? place.inUse.items : 0);
    stepFreed += freed;
    stepDeleted += items;
    onEvent({ kind: 'deleted', step: index, path: place.path, bytes: freed, items, whole: !place.inUse || isNote(place.inUse) });
    if (place.inUse) skip(place.inUse);
  };
  const finish = () => {
    const step = plan.steps[index]!;
    const p = step.proposal;
    const left = skipped.filter((s) => !isNote(s));
    const report: StepReport = {
      id: p.id,
      title: p.title,
      why: p.why,
      category: p.category,
      regenerable: p.regenerable,
      // As the core reports: what the confirm dialog showed, not the places refused while planning.
      plannedBytes: sum(step.places, (pl) => pl.bytes),
      freedBytes: stepFreed,
      plannedItems: sum(step.places, (pl) => pl.items),
      deletedItems: stepDeleted,
      kind: 'delete',
      movedItems: 0,
      places: step.places.length,
      outcome: left.length === 0 ? 'done' : stepFreed > 0 ? 'partial' : 'skipped',
      skipped,
    };
    totalFreed += stepFreed;
    reports.push(report);
    onEvent({ kind: 'stepFinished', step: index, report });
    index++;
  };
  /** The rest of the plan after Stop: the place being deleted keeps what went, and everything after is left. */
  const stopHere = (fraction: number) => {
    let step = plan.steps[index]!;
    const current = step.places[placesDone];
    if (current) {
      const items = Math.floor(current.items * fraction);
      const bytes = current.items > 0 ? Math.round((current.bytes * items) / current.items) : 0;
      stepFreed += bytes;
      stepDeleted += items;
      if (items > 0) onEvent({ kind: 'deleted', step: index, path: current.path, bytes, items, whole: false });
      const rest = current.items - items;
      skip({
        path: current.path,
        reason: rest === 1 ? 'It was left: you stopped the run' : `${rest.toLocaleString('en-US')} files were left: you stopped the run`,
        kind: 'cancelled',
        bytes: current.bytes - bytes,
        items: rest,
      });
      placesDone++;
    }
    for (;;) {
      for (const place of step.places.slice(placesDone)) skip(stopped(place));
      progress(step.places.at(-1)?.path ?? '');
      finish();
      if (index >= plan.steps.length) return;
      begin();
      step = plan.steps[index]!;
    }
  };

  // A sort: its moves go one after another; the script's skips are met on the way.
  let sortDone = 0;
  let sortSkips: Skip[] = [];
  const sortAt = () => plan.sorts[index - plan.steps.length]!;
  const beginSort = () => {
    const sort = sortAt();
    stepStart = Date.now();
    sortDone = 0;
    sortSkips = [];
    onEvent({ kind: 'stepStarted', step: index, id: sort.tidy.id });
  };
  const skipsOf = (sort: MockSort) =>
    sort.script.skips.flatMap((k) => {
      const at = sort.moves.findIndex((m) => m.name === k.name);
      const mv = sort.moves[at];
      return mv ? [{ at, skip: { path: `${sort.tidy.path}\\${mv.name}`, reason: k.reason, kind: k.kind, bytes: mv.bytes, items: 1 } }] : [];
    });
  const advanceSort = (to: number) => {
    const sort = sortAt();
    for (const { at, skip: k } of skipsOf(sort)) {
      if (at >= sortDone && at < to) {
        sortSkips.push(k);
        onEvent({ kind: 'skipped', step: index, skip: k });
      }
    }
    sortDone = to;
    onEvent({ kind: 'moveProgress', step: index, done: sortDone, total: sort.moves.length });
  };
  const finishSort = (stopped: boolean) => {
    const sort = sortAt();
    const left = sort.moves.length - sortDone;
    if (stopped && left > 0) {
      const k: Skip = {
        path: sort.tidy.path,
        reason: sortDone === 0 ? 'You stopped the run before it got here' : `${left.toLocaleString('en-US')} ${left === 1 ? 'file wasn’t' : 'files weren’t'} moved: you stopped the run`,
        kind: 'cancelled',
        bytes: 0,
        items: left,
      };
      sortSkips.push(k);
      onEvent({ kind: 'skipped', step: index, skip: k });
    }
    const moved = sortDone - sortSkips.filter((k) => k.kind !== 'cancelled').length;
    const moves = sort.moves.slice(0, sortDone).filter((m) => !sortSkips.some((k) => k.path.endsWith(`\\${m.name}`)));
    const report: StepReport = {
      id: sort.tidy.id,
      title: `Sort ${sort.tidy.name}`,
      why: `Loose files at the top of ${sort.tidy.name} sorted into folders by kind and year, by Vacancy’s built-in rules. Moved, not deleted.`,
      category: sort.tidy.folder === 'downloads' ? 'downloads' : 'media',
      regenerable: false,
      plannedBytes: 0,
      freedBytes: 0,
      plannedItems: sort.moves.length,
      deletedItems: 0,
      kind: 'move',
      movedItems: moved,
      places: 0,
      outcome: sortSkips.length === 0 ? 'done' : moved > 0 ? 'partial' : 'skipped',
      skipped: sortSkips,
    };
    madeFolders.set(`${run.id}:${sort.tidy.id}`, foldersMade(sort.tidy, moves));
    records.push({
      id: sort.tidy.id,
      folder: sort.tidy.name,
      movedItems: moved,
      plannedItems: sort.moves.length,
      journal: moved > 0 || sortDone > 0 ? `${run.id}-${sort.tidy.id}.journal` : '',
      path: sort.tidy.path,
      into: kindFolders(moves).map(([name]) => name),
      foldersMade: foldersMade(sort.tidy, moves).length,
      undoable: moved > 0,
      interrupted: false,
      note: null,
      undo: null,
    });
    reports.push(report);
    onEvent({ kind: 'stepFinished', step: index, report });
    index++;
  };

  const done = new Promise<RunReport>((resolve) => {
    const end = () => {
      clearInterval(timer);
      // Preview moves (`&moves=1`) stand in only for a plan with no sorts of its own.
      const moves = records.length > 0 ? records : (run.moves ?? []);
      resolve(runReport(run.id, plan, volume, reports, startedAt, Date.now() - startedAt, cancelled, moves));
    };
    if (plan.steps.length === 0 && plan.sorts.length === 0) {
      setTimeout(end, 0);
      return;
    }
    if (plan.steps.length > 0) begin();
    else beginSort();
    const sorting = () => {
      while (index < plan.steps.length + plan.sorts.length) {
        const sort = sortAt();
        const span = spans[index]!;
        const t = span <= 0 ? 1 : Math.min(1, (Date.now() - stepStart) / span);
        if (cancelled) {
          finishSort(true);
          while (index < plan.steps.length + plan.sorts.length) {
            beginSort();
            finishSort(true);
          }
          break;
        }
        advanceSort(Math.min(sort.moves.length, Math.floor(t * sort.moves.length + 1e-9)));
        if (t < 1) return false;
        finishSort(false);
        if (index < plan.steps.length + plan.sorts.length) beginSort();
        if (run.totalMs > 0) return false;
      }
      return true;
    };
    const tick = () => {
      if (index >= plan.steps.length) {
        if (sorting()) end();
        return;
      }
      while (index < plan.steps.length) {
        const step = plan.steps[index]!;
        const span = spans[index]!;
        const t = span <= 0 ? 1 : Math.min(1, (Date.now() - stepStart) / span);
        const reached = Math.min(step.places.length, Math.floor(t * step.places.length + 1e-9));
        while (placesDone < reached) deletePlace(step.places[placesDone++]!);
        // How far into the place being deleted now.
        const part = Math.max(0, Math.min(1, t * step.places.length - placesDone));
        if (cancelled) {
          stopHere(part);
          break;
        }
        if (t < 1) {
          const place = step.places[placesDone];
          progress(place?.path ?? '', place ? Math.round(place.bytes * part) : 0, place ? Math.floor(place.items * part) : 0);
          return;
        }
        progress(step.places.at(-1)?.path ?? '');
        finish();
        if (index < plan.steps.length) begin();
        else if (plan.sorts.length > 0) {
          beginSort();
          return;
        }
        if (run.totalMs > 0) return;
      }
      if (cancelled && plan.sorts.length > 0 && index < plan.steps.length + plan.sorts.length) {
        beginSort();
        if (sorting()) end();
        return;
      }
      end();
    };
    timer = window.setInterval(tick, run.totalMs <= 0 ? 0 : 100);
  });

  return {
    cancel: () => {
      cancelled = true;
    },
    done,
  };
}

function stopped(place: Place): Skip {
  return {
    path: place.path,
    reason: 'You stopped the run before it got here',
    kind: 'cancelled',
    bytes: place.bytes,
    items: place.items,
  };
}

/** A note about something deleted, not something left: its space stays in use (see the core's `SkipKind::is_note`). */
export function isNote(s: Skip): boolean {
  return s.kind === 'hardLinked' || s.kind === 'heldOpen';
}

/**
 * The report of a plan run to the end at `startedAt`, taking `durationMs`: every place deleted but what the fixture
 * finds in use there, for the history's past runs.
 */
export function pastReport(id: string, plan: MockPlan, volume: VolumeInfo, startedAt: number, durationMs: number): RunReport {
  const steps = plan.steps.map((step): StepReport => {
    const p = step.proposal;
    const found = step.places.flatMap((pl) => (pl.inUse ? [pl.inUse] : []));
    const left = found.filter((s) => !isNote(s));
    const freed = sum(step.places, (pl) => pl.bytes) - sum(found, (s) => s.bytes);
    return {
      id: p.id,
      title: p.title,
      why: p.why,
      category: p.category,
      regenerable: p.regenerable,
      plannedBytes: sum(step.places, (pl) => pl.bytes),
      freedBytes: freed,
      plannedItems: sum(step.places, (pl) => pl.items),
      deletedItems: sum(step.places, (pl) => pl.items) - sum(left, (s) => s.items),
      kind: 'delete',
      movedItems: 0,
      places: step.places.length,
      outcome: left.length === 0 && step.refused.length === 0 ? 'done' : freed > 0 ? 'partial' : 'skipped',
      skipped: [...step.refused, ...found],
    };
  });
  return runReport(id, plan, volume, steps, startedAt, durationMs, false, []);
}

function runReport(
  id: string,
  plan: MockPlan,
  volume: VolumeInfo,
  steps: StepReport[],
  startedAt: number,
  durationMs: number,
  cancelled: boolean,
  moves: Reorganized[],
): RunReport {
  const freedBytes = sum(steps, (s) => s.freedBytes);
  const byCategory: CategoryFreed[] = [];
  for (const s of steps.filter((x) => x.freedBytes > 0)) {
    const c = byCategory.find((x) => x.id === s.category);
    if (c) c.bytes += s.freedBytes;
    else byCategory.push({ id: s.category, bytes: s.freedBytes });
  }
  byCategory.sort((a, b) => b.bytes - a.bytes);
  const win = steps.filter((s) => s.freedBytes > 0).sort((a, b) => b.freedBytes - a.freedBytes)[0];
  // The drive's own figures: other apps wrote a little while it ran.
  const freeChange = freedBytes > OTHERS_WROTE * 4 ? freedBytes - OTHERS_WROTE : freedBytes;
  return {
    id,
    planId: plan.summary.id,
    mountPoint: volume.mountPoint,
    volumeName: volume.name,
    startedAt,
    durationMs,
    cancelled,
    steps,
    plannedBytes: sum(steps, (s) => s.plannedBytes),
    freedBytes,
    deletedItems: sum(steps, (s) => s.deletedItems),
    movedItems: sum(moves, (m) => m.movedItems),
    // Names deleted while other names keep the file were deleted, not skipped.
    skippedItems: sum(
      steps.flatMap((s) => s.skipped.filter((k) => !isNote(k))),
      (k) => k.items,
    ),
    freedByCategory: byCategory,
    totalBytes: volume.totalBytes,
    usedBefore: volume.usedBytes,
    usedAfter: volume.usedBytes - freeChange,
    freeBefore: volume.freeBytes,
    freeAfter: volume.freeBytes + freeChange,
    freeChange,
    biggestWin: win?.id ?? null,
    leftForYou: plan.advice,
    reorganized: moves,
    map: plan.map,
  };
}

/** Folders a proposal's places are named after their parent by: `…\portfolio-2024\node_modules`. */
const NAMED_BY_PARENT = /^(node_modules|target|Cache_Data|Cache|cache|_cacache|http|downloads|content-v2)$/;

/**
 * A proposal's places: its sample paths, then more like them (`portfolio-2024-2\node_modules`,
 * `SteamSetup (2).exe`) until there are as many as the proposal counts.
 */
function placePaths(p: Proposal): string[] {
  const samples = p.samplePaths.length ? p.samplePaths : [p.title];
  const out: string[] = [];
  for (let j = 0; j < Math.max(1, p.placeCount); j++) {
    const base = samples[j % samples.length]!;
    const round = Math.floor(j / samples.length);
    if (round === 0) {
      out.push(base);
      continue;
    }
    const sep = base.includes('\\') ? '\\' : '/';
    const parts = base.split(sep);
    const last = parts.length - 1;
    const name = parts[last]!;
    if (last >= 1 && NAMED_BY_PARENT.test(name)) parts[last - 1] = `${parts[last - 1]}-${round + 1}`;
    else {
      const dot = name.lastIndexOf('.');
      parts[last] = dot > 0 ? `${name.slice(0, dot)} (${round + 1})${name.slice(dot)}` : `${name} (${round + 1})`;
    }
    out.push(parts.join(sep));
  }
  return out;
}

/** Shares `bytes` and `items` out over `paths`, evenly, the remainder going to the last. */
function spread(paths: string[], bytes: number, items: number): Place[] {
  const n = paths.length;
  const eachBytes = Math.floor(bytes / n);
  const eachItems = Math.floor(items / n);
  const last = n - 1;
  return paths.map((path, i) => ({
    path,
    bytes: i === last ? bytes - eachBytes * last : eachBytes,
    items: i === last ? items - eachItems * last : eachItems,
    inUse: null,
  }));
}

function sum<T>(xs: readonly T[], f: (x: T) => number): number {
  return xs.reduce((s, x) => s + f(x), 0);
}

/**
 * What a mock Undo of the sort `id` of a kept run does: every moved file comes back but those the fixture's script
 * says can't (a file now has the old name), and the empty folders the run made go, but those still holding them.
 * The report is changed in place, as the core changes its kept copy.
 */
export function mockUndo(report: RunReport, id: string, scripts: Record<string, TidyScript>): void {
  const r = report.reorganized.find((x) => x.id === id);
  if (!r) throw new Error(`This run has no folder tidy-up ${id}.`);
  if (!r.undoable) throw new Error('Nothing of this tidy-up is left to undo.');
  const script = scripts[id] ?? { skips: [], stuck: [], kept: [] };
  const already = r.undo?.back ?? 0;
  const stuck = r.undo ? r.undo.stuck.filter((k) => k.kind !== 'changed') : script.stuck.map((k) => ({
    path: `${r.path}\\${k.dest}\\${k.name}`,
    reason: k.reason,
    kind: 'changed' as const,
    bytes: 0,
    items: 1,
  }));
  const kept = stuck.length > 0 ? script.kept : [];
  const made = madeFolders.get(`${report.id}:${id}`) ?? r.into.flatMap((top) => [top]);
  const undone: Undone = {
    at: Date.now(),
    back: Math.max(already, r.movedItems - stuck.length),
    stuck,
    removed: made.filter((d) => !kept.includes(d)),
    kept,
  };
  r.undo = undone;
  r.undoable = false;
  r.note = null;
}

/** The folders each mock sort made, by `run:sort`, for what a mock Undo removes. */
const madeFolders = new Map<string, string[]>();

/** Whether a plan step id names a sort. */
export const isSortStep = isTidy;

// The mock bridge's planner and executor: plans from a drive's fixture proposals, and a run that plays them back on a
// clock, with the events and the report the core sends. Nothing is touched; the fixture's skips say what is left.
import type { RunSkip } from './fixtures';
import type {
  CategoryFreed,
  PlanSummary,
  Proposal,
  RunEvent,
  RunReport,
  Skip,
  StepReport,
  VolumeInfo,
} from './types';

/** One place a plan step deletes: a folder or file, with its share of the step's bytes and files. */
interface Place {
  path: string;
  bytes: number;
  items: number;
  /** What the run finds in use there, and leaves. */
  inUse: Skip | null;
}

interface MockStep {
  proposal: Proposal;
  places: Place[];
  /** Places planning left out, reported as skipped when the step runs. */
  refused: Skip[];
}

/** A plan the mock has made, until it runs or the drive is scanned again. */
export interface MockPlan {
  summary: PlanSummary;
  steps: MockStep[];
}

/**
 * The plan for proposals `ids` of a drive's scan: one step per delete proposal, biggest first, its places spread from
 * the proposal's sample paths. Places the fixture says changed since the scan are left out and listed in `refused`.
 * Advice and tools can't be carried out, so they are left for the user.
 */
export function makePlan(id: string, mountPoint: string, proposals: Proposal[], ids: string[], skips: RunSkip[]): MockPlan {
  const unknown = ids.find((i) => !proposals.some((p) => p.id === i));
  if (unknown !== undefined) throw new Error(`there is no proposal ${unknown} in this scan`);
  const chosen = proposals.filter((p) => ids.includes(p.id));
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
  const planSteps = steps.map((s) => ({
    id: s.proposal.id,
    title: s.proposal.title,
    category: s.proposal.category,
    bytes: sum(s.places, (pl) => pl.bytes),
    items: sum(s.places, (pl) => pl.items),
    places: s.places.length,
  }));
  return {
    steps,
    summary: {
      id,
      mountPoint,
      bytes: sum(planSteps, (s) => s.bytes),
      items: sum(planSteps, (s) => s.items),
      places: sum(planSteps, (s) => s.places),
      moves: 0,
      steps: planSteps,
      refused: steps.flatMap((s) => s.refused),
      leftForYou: chosen.filter((p) => p.action !== 'delete').map((p) => p.id),
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
  run: { id: string; plan: MockPlan; volume: VolumeInfo; totalMs: number },
  onEvent: (event: RunEvent) => void,
): Playback {
  const { plan, volume } = run;
  const startedAt = Date.now();
  const biggest = Math.max(1, ...plan.steps.map((s) => s.proposal.bytes));
  const weights = plan.steps.map((s) => 0.6 + 0.4 * (s.proposal.bytes / biggest));
  const weightSum = sum(weights, (w) => w) || 1;
  const spans = weights.map((w) => (run.totalMs * w) / weightSum);

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
    const items = place.items - (place.inUse?.items ?? 0);
    stepFreed += freed;
    stepDeleted += items;
    onEvent({ kind: 'deleted', step: index, path: place.path, bytes: freed, items });
    if (place.inUse) skip(place.inUse);
  };
  const finish = () => {
    const step = plan.steps[index]!;
    const p = step.proposal;
    const left = skipped.filter((s) => s.kind !== 'hardLinked');
    const report: StepReport = {
      id: p.id,
      title: p.title,
      category: p.category,
      regenerable: p.regenerable,
      plannedBytes: sum(step.places, (pl) => pl.bytes) + sum(step.refused, (r) => r.bytes),
      freedBytes: stepFreed,
      plannedItems: sum(step.places, (pl) => pl.items) + sum(step.refused, (r) => r.items),
      deletedItems: stepDeleted,
      places: step.places.length + step.refused.length,
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
      if (items > 0) onEvent({ kind: 'deleted', step: index, path: current.path, bytes, items });
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

  const done = new Promise<RunReport>((resolve) => {
    const end = () => {
      clearInterval(timer);
      resolve(runReport(run.id, plan, volume, reports, startedAt, cancelled));
    };
    if (plan.steps.length === 0) {
      setTimeout(end, 0);
      return;
    }
    begin();
    const tick = () => {
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
        if (run.totalMs > 0) return;
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

function runReport(
  id: string,
  plan: MockPlan,
  volume: VolumeInfo,
  steps: StepReport[],
  startedAt: number,
  cancelled: boolean,
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
    durationMs: Date.now() - startedAt,
    cancelled,
    steps,
    plannedBytes: sum(steps, (s) => s.plannedBytes),
    freedBytes,
    deletedItems: sum(steps, (s) => s.deletedItems),
    skippedItems: sum(
      steps.flatMap((s) => s.skipped),
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
    leftForYou: plan.summary.leftForYou,
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

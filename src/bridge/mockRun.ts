// The mock bridge's planner and executor: plans from a drive's fixture proposals, and a run that plays them back on a
// clock, with the events and the report the core sends. Nothing is touched; the fixture's skips say what is left.
import type { RunSkip } from './fixtures';
import { DUPLICATES, type MockDuplicates } from './mockDuplicates';
import type {
  CategoryFreed,
  DuplicatePair,
  DuplicatesDone,
  FileKind,
  PlannedDuplicates,
  LeftForYou,
  Overview,
  PlanSummary,
  Proposal,
  Reorganized,
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
  /**
   * What the run finds there: something in use, which it leaves, or a note about something it deleted whose space
   * stays in use (a hard link kept elsewhere).
   */
  inUse: Skip | null;
  /** Left whole when the run gets there (an extra copy whose kept copy changed since the scan). */
  whole?: Skip;
  /** For an extra copy of a duplicate group: its group and its map tile. */
  dup?: { group: string; kind: FileKind; name: string; kept: string; tile: string };
}

interface MockStep {
  proposal: Proposal;
  places: Place[];
  /** Places planning left out, reported as skipped when the step runs. */
  refused: Skip[];
  /** For the duplicates step: the groups the user unticked. */
  skippedGroups?: number;
}

/** A plan the mock has made, until it runs or the drive is scanned again. */
export interface MockPlan {
  summary: PlanSummary;
  steps: MockStep[];
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
  duplicates: { found: MockDuplicates | null; skipped: string[] } = { found: null, skipped: [] },
): MockPlan {
  const unknown = ids.find((i) => !proposals.some((p) => p.id === i));
  if (unknown !== undefined) throw new Error(`there is no proposal ${unknown} in this scan`);
  const chosen = proposals.filter((p) => ids.includes(p.id));
  if (chosen.length === 0) throw new Error('the plan has no selected proposals');
  const steps = chosen
    .filter((p) => p.action === 'delete')
    .map((p): MockStep => {
      if (p.id === DUPLICATES && duplicates.found) return duplicateStep(p, duplicates.found, duplicates.skipped);
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
  // Duplicates with every group unticked delete nothing: no step.
  const kept = steps.filter((s) => s.proposal.id !== DUPLICATES || s.places.length > 0);
  steps.splice(0, steps.length, ...kept);
  const planSteps = steps.map((s) => ({
    id: s.proposal.id,
    title: s.proposal.title,
    category: s.proposal.category,
    bytes: sum(s.places, (pl) => pl.bytes),
    items: sum(s.places, (pl) => pl.items),
    places: s.places.length,
  }));
  const advice = chosen
    .filter((p) => p.action === 'hint')
    .map((p): LeftForYou => ({ id: p.id, title: p.title, summary: p.summary, why: p.why, bytes: 0, hintAction: p.hintAction ?? null }));
  return {
    steps,
    advice,
    map,
    summary: {
      id,
      mountPoint,
      bytes: sum(planSteps, (s) => s.bytes),
      items: sum(planSteps, (s) => s.items),
      places: sum(planSteps, (s) => s.places),
      moves: 0,
      steps: planSteps,
      refused: steps.flatMap((s) => s.refused),
      leftForYou: advice.map((a) => a.id),
      duplicates: plannedDuplicates(steps),
    },
  };
}

/**
 * The duplicates step: one place per extra copy of each group the user didn't untick. The fixture's changed group
 * has its kept copy changed since the scan, so the run leaves its extra copies, as the core does.
 */
function duplicateStep(p: Proposal, found: MockDuplicates, skipped: string[]): MockStep {
  const places: Place[] = [];
  for (const g of found.duplicates.groups) {
    if (skipped.includes(g.id)) continue;
    for (const e of g.extras) {
      const changed = g.id === found.changedGroup;
      places.push({
        path: e.path,
        bytes: e.bytes,
        items: 1,
        inUse: null,
        ...(changed
          ? {
              whole: {
                path: e.path,
                reason: `The copy Vacancy keeps of it (${g.keep.path}) changed or is gone since the scan, so this copy stays`,
                kind: 'changed' as const,
                bytes: e.bytes,
                items: 1,
              },
            }
          : {}),
        dup: { group: g.id, kind: g.kind, name: g.name, kept: g.keep.path, tile: e.tile },
      });
    }
  }
  const skippedGroups = found.duplicates.groups.filter((g) => skipped.includes(g.id)).length;
  return { proposal: p, places, refused: [], skippedGroups };
}

/** What the confirm dialog says of a plan's duplicates. */
function plannedDuplicates(steps: MockStep[]): PlannedDuplicates | null {
  const step = steps.find((s) => s.proposal.id === DUPLICATES);
  if (!step) return null;
  const tiles = new Map<string, number>();
  for (const pl of step.places) if (pl.dup) tiles.set(pl.dup.tile, (tiles.get(pl.dup.tile) ?? 0) + pl.bytes);
  return {
    groups: new Set(step.places.map((pl) => pl.dup?.group)).size,
    copies: step.places.length,
    bytes: sum(step.places, (pl) => pl.bytes),
    skippedGroups: step.skippedGroups ?? 0,
    tiles: [...tiles].map(([id, bytes]) => ({ id, bytes })),
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
  /** What the duplicates step freed per tile, and the extra copies each group lost. */
  const dupFreed = new Map<string, number>();
  const dupCleared = new Map<string, number>();
  const dupKeptWhole = new Set<string>();

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
    if (place.whole) {
      if (place.dup) dupKeptWhole.add(place.dup.group);
      skip(place.whole);
      return;
    }
    if (place.dup) {
      dupFreed.set(place.dup.tile, (dupFreed.get(place.dup.tile) ?? 0) + place.bytes);
      dupCleared.set(place.dup.group, (dupCleared.get(place.dup.group) ?? 0) + 1);
    }
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

  const done = new Promise<RunReport>((resolve) => {
    const end = () => {
      clearInterval(timer);
      const report = runReport(run.id, plan, volume, reports, startedAt, Date.now() - startedAt, cancelled, run.moves ?? []);
      resolve({ ...report, duplicates: duplicatesDone(plan, reports, dupFreed, dupCleared, dupKeptWhole) });
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

/** What the duplicates step came to, as the core reports it. */
function duplicatesDone(
  plan: MockPlan,
  reports: StepReport[],
  freed: Map<string, number>,
  cleared: Map<string, number>,
  keptWhole: Set<string>,
): DuplicatesDone | null {
  const step = plan.steps.find((s) => s.proposal.id === DUPLICATES);
  const report = reports.find((r) => r.id === DUPLICATES);
  if (!step || !report) return null;
  const planned = new Map<string, number>();
  for (const pl of step.places) if (pl.dup) planned.set(pl.dup.tile, (planned.get(pl.dup.tile) ?? 0) + pl.bytes);
  const groups = [...new Map(step.places.flatMap((pl) => (pl.dup ? [[pl.dup.group, pl] as const] : []))).values()];
  const done = groups.filter((pl) => cleared.has(pl.dup!.group));
  const pick = (want: (k: FileKind) => boolean) => done.find((pl) => want(pl.dup!.kind));
  const samples = [
    pick((k) => k === 'photo'),
    pick((k) => k === 'video'),
    pick((k) => k !== 'photo' && k !== 'video'),
  ].filter((pl): pl is Place => !!pl);
  return {
    groups: groups.length,
    clearedGroups: done.length,
    keptWhole: keptWhole.size,
    skippedGroups: step.skippedGroups ?? 0,
    copiesDeleted: report.deletedItems,
    freedBytes: report.freedBytes,
    samples: samples.map((pl): DuplicatePair => ({
      kind: pl.dup!.kind,
      name: pl.dup!.name,
      kept: pl.dup!.kept,
      removed: pl.path,
      copies: cleared.get(pl.dup!.group) ?? 0,
    })),
    tiles: [...planned].map(([id, bytes]) => ({ id, plannedBytes: bytes, freedBytes: freed.get(id) ?? 0 })),
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

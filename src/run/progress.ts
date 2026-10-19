// A running plan as the run panel and the map show it: each step's state and figures, the skips as they arrive, and
// the log. This is the pure part: the view a plan starts with, and how the run's events (and, at the end, its report)
// change it. The session keeps one per drive.
import type { CategoryId, Outcome, PlanSummary, RunEvent, RunReport, Skip, StepKind, StepReport } from '../bridge';
import { formatBytes, formatCount } from '../format';

export type StepStatus = 'pending' | 'running' | 'done';

/** One step of the plan, as far as the run has got with it. */
export interface StepView {
  /** The proposal's id; its map tile is `p:<id>`. */
  id: string;
  title: string;
  category: CategoryId;
  /** Allocated bytes the step means to free: the plan's figure, without the places it refused. */
  plannedBytes: number;
  /** Freed so far. */
  freedBytes: number;
  /** Files deleted so far. */
  deletedItems: number;
  /** Deletes, or moves (a Tidy up sort, which frees nothing). */
  kind: StepKind;
  /** Files a move step means to move. */
  plannedMoves: number;
  /** Moves a move step has handled so far (moved, or skipped with a reason). */
  handled: number;
  /** Files a move step moved, once it has finished. */
  movedItems: number;
  status: StepStatus;
  /** How it ended, once it has. */
  outcome: Outcome | null;
  /** What it left in place so far, and why, in the order they happened. */
  skips: Skip[];
}

/** One line of the run's log. */
export interface LogLine {
  /** Unique within the run, for keys. */
  id: number;
  kind: 'del' | 'skip' | 'sum';
  text: string;
}

export interface RunView {
  steps: StepView[];
  /** The step running now, or the last one that started. */
  current: number;
  /** Freed by the whole run so far. */
  freedBytes: number;
  /** Things left in place so far, across steps. */
  skips: number;
  /** The newest lines, at most `LOG_KEEP`. */
  log: LogLine[];
  /** Lines written so far, kept or not. */
  written: number;
}

/** How many log lines a run keeps. The panel shows the newest of them. */
export const LOG_KEEP = 200;

/** The view of a plan before its run has done anything. */
export function startRun(plan: PlanSummary): RunView {
  return {
    steps: plan.steps.map((s) => ({
      id: s.id,
      title: s.title,
      category: s.category,
      plannedBytes: s.bytes,
      freedBytes: 0,
      deletedItems: 0,
      kind: s.kind,
      plannedMoves: s.moves,
      handled: 0,
      movedItems: 0,
      status: 'pending',
      outcome: null,
      skips: [],
    })),
    current: 0,
    freedBytes: 0,
    skips: 0,
    log: [],
    written: 0,
  };
}

/** The view after `events`, in the order they happened. Events for a step the plan doesn't have are ignored. */
export function foldEvents(view: RunView, events: readonly RunEvent[]): RunView {
  if (events.length === 0) return view;
  const steps = [...view.steps];
  let { current, skips, written } = view;
  const lines: LogLine[] = [];
  const write = (kind: LogLine['kind'], text: string) => lines.push({ id: written++, kind, text });
  const touch = (i: number): StepView | null => {
    const s = steps[i];
    if (!s) return null;
    const copy = { ...s };
    steps[i] = copy;
    return copy;
  };

  for (const e of events) {
    const s = touch(e.step);
    if (!s) continue;
    switch (e.kind) {
      case 'stepStarted':
        s.status = 'running';
        current = e.step;
        break;
      case 'progress':
        if (s.status === 'pending') s.status = 'running';
        s.freedBytes = e.freedBytes;
        s.deletedItems = e.deletedItems;
        current = e.step;
        break;
      case 'deleted':
        write('del', e.whole ? `deleted ${e.path}` : `deleted ${e.items} ${e.items === 1 ? 'file' : 'files'} in ${e.path}`);
        break;
      case 'skipped':
        s.skips = [...s.skips, e.skip];
        skips++;
        write('skip', `skipped ${e.skip.path} (${inSentence(e.skip.reason)})`);
        break;
      case 'stepFinished':
        settleStep(s, e.report);
        write('sum', stepLine(e.report));
        break;
      case 'moveProgress':
        if (s.status === 'pending') s.status = 'running';
        s.handled = e.done;
        s.plannedMoves = e.total;
        current = e.step;
        break;
    }
  }

  const log = lines.length ? [...view.log, ...lines].slice(-LOG_KEEP) : view.log;
  return { steps, current, freedBytes: total(steps), skips, log, written };
}

/**
 * The view once the run has ended, from its report: every step's own figures, outcome and skips. Steps the run
 * never reached (it was stopped) end as the report says.
 */
export function settleRun(view: RunView, report: RunReport): RunView {
  const steps = view.steps.map((s) => {
    const r = report.steps.find((x) => x.id === s.id);
    if (!r) return s;
    const copy = { ...s };
    settleStep(copy, r);
    return copy;
  });
  return {
    ...view,
    steps,
    freedBytes: report.freedBytes,
    skips: steps.reduce((n, s) => n + s.skips.length, 0),
  };
}

function settleStep(s: StepView, r: StepReport) {
  s.status = 'done';
  s.outcome = r.outcome;
  s.freedBytes = r.freedBytes;
  s.deletedItems = r.deletedItems;
  if (r.kind === 'move') {
    s.movedItems = r.movedItems;
    s.handled = r.plannedItems;
  }
  // The report's list is the step's own: at most a dozen of each kind, the rest summed into one.
  s.skips = r.skipped;
}

const total = (steps: readonly StepView[]) => steps.reduce((sum, s) => sum + s.freedBytes, 0);

/** A finished step the run never got to: it was stopped first, so it deleted (or moved) nothing. */
export function neverReached(done: number, skips: readonly Skip[]): boolean {
  return done === 0 && skips.some((k) => k.kind === 'cancelled');
}

/** What a finished step did, in files: deleted, or for a move step moved. */
export const doneOf = (r: Pick<StepReport, 'deletedItems'> & { movedItems?: number }) => r.deletedItems + (r.movedItems ?? 0);

/**
 * The line a finished step adds to the log: "Browser caches: 5.9 GB freed, 212 files left", or for a sort "Sort
 * Downloads: 184 files moved, nothing deleted, every move recorded".
 */
export function stepLine(r: StepReport): string {
  const left = r.skipped.reduce((n, k) => n + k.items, 0);
  if (r.kind === 'move') {
    const moved = `${formatCount(r.movedItems)} ${r.movedItems === 1 ? 'file' : 'files'} moved`;
    return `${r.title}: ${moved}, nothing deleted${r.movedItems > 0 ? ', every move recorded' : ''}${left > 0 ? `, ${formatCount(left)} left where ${left === 1 ? 'it was' : 'they were'}` : ''}`;
  }
  const freed = r.outcome === 'skipped' ? 'nothing freed' : `${formatBytes(r.freedBytes)} freed`;
  return left > 0 ? `${r.title}: ${freed}, ${formatCount(left)} ${left === 1 ? 'file' : 'files'} left` : `${r.title}: ${freed}`;
}

/** A reason ("It changed since the scan") as it reads inside a sentence: "it changed since the scan". */
export function inSentence(reason: string): string {
  return /^(It|You) /.test(reason) ? reason[0]!.toLowerCase() + reason.slice(1) : reason;
}

/** A reason as a sentence of its own: "It changed since the scan." */
export function asSentence(reason: string): string {
  const s = reason.charAt(0).toUpperCase() + reason.slice(1);
  return /[.!?]$/.test(s) ? s : `${s}.`;
}

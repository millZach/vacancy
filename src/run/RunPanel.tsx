import { Check, LoaderCircle, Minus, TriangleAlert } from 'lucide-react';
import { useEffect, useRef } from 'react';
import type { PlanRun } from '../drives/session';
import { formatBytes, formatCount } from '../format';
import { tileOf } from '../plan/totals';
import { doneOf, neverReached, type StepView } from './progress';
import { count, remindersOnly } from '../wrapped/deck';
import './run.css';

/** A run on the run panel: going, ended, or failed. */
export type ShownRun = Extract<PlanRun, { phase: 'running' | 'ended' | 'failed' }>;

interface Props {
  run: ShownRun;
  /** The map tile under the pointer, so a step's row and its tile light together. */
  hot: string | null;
  onHot: (tile: string | null) => void;
  onStop: () => void;
  onSeeChanges: () => void;
  /** After a failed run: back to the plan, or scan again. */
  onBack: () => void;
  onScanAgain: () => void;
}

/** The newest log lines shown. */
const LOG_SHOWN = 80;
/** Skips said in a step's row; the rest are counted. */
const ROW_SKIPS = 2;

/**
 * Variant D's run panel, where the plan tray was: the step running now, a live freed counter with a meter, one row
 * per step (waiting, running with a bar, done with a tick, or a warning with what it left and why), and a log of
 * each place deleted or skipped as it happens. Stop ends the run after the file being deleted; then "See what
 * changed" opens the summary.
 */
export function RunPanel({ run, hot, onHot, onStop, onSeeChanges, onBack, onScanAgain }: Props) {
  const { view, plan } = run;
  const n = view.steps.length;
  const log = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLOListElement>(null);
  // The log follows its newest line (also when the footer grows at the end and the log gets shorter), and the step
  // list the step running now.
  const written = view.written;
  const phase = run.phase;
  useEffect(() => {
    const el = log.current;
    if (el && written > 0) el.scrollTop = el.scrollHeight;
  }, [written, phase]);
  const at = view.current;
  useEffect(() => {
    const row = list.current?.children[at] as HTMLElement | undefined;
    row?.scrollIntoView?.({ block: 'nearest' });
  }, [at]);

  const share = plan.bytes > 0 ? Math.min(1, view.freedBytes / plan.bytes) : run.phase === 'running' ? 0 : 1;
  const current = view.steps[view.current];
  const reminders = plan.leftForYou.length;
  const report = run.phase === 'ended' ? run.report : null;
  /** Steps that delete: a plan of Tidy up sorts alone moves files and frees nothing. */
  const deletes = view.steps.filter((s) => s.kind !== 'move').length;
  const movesOnly = deletes === 0 && plan.moves > 0;
  const adviceOnly = report ? remindersOnly(report) : n === 0 && plan.moves === 0 && reminders > 0;
  const ran = report ? report.steps.filter((s) => !neverReached(doneOf(s), s.skipped)).length : 0;
  const somethingRan = view.steps.some((s) => s.status !== 'pending');

  return (
    <aside className="tray run" aria-label="Run" data-no-ripple>
      <div className="tray-head">
        <div>
          <h2>
            {run.phase === 'running'
              ? adviceOnly ? 'Saving reminders' : movesOnly ? 'Moving files' : 'Freeing space'
              : run.phase === 'failed'
                ? 'The run stopped'
                : report?.cancelled
                  ? 'Stopped'
                  : adviceOnly ? 'Reminders saved' : 'Done'}
          </h2>
          {run.phase === 'failed' ? (
            <p className="run-error" role="alert">
              {run.message}
            </p>
          ) : (
            <p aria-live="polite">
              {run.phase === 'running'
                ? run.stopping
                  ? current?.kind === 'move' ? 'Stopping after the current move' : 'Stopping after the file being deleted'
                  : current
                    ? `Step ${view.current + 1} of ${n}: ${current.title}`
                    : adviceOnly ? 'Saving advice to this run’s history. No files will be deleted.' : plan.moves > 0 ? `Moving ${count(plan.moves, 'file')}. Every move is recorded.` : 'Starting'
                : report?.cancelled
                  ? `You stopped the run. ${ran} of ${n} ${n === 1 ? 'step' : 'steps'} ran.`
                  : adviceOnly ? 'No files were deleted or moved.' : n === 0 ? report && report.movedItems > 0 ? `${count(report.movedItems, 'file')} moved.` : 'The run finished.' : n === 1
                    ? 'The step finished.'
                    : `All ${n} steps finished.`}
            </p>
          )}
        </div>
      </div>

      <div className="run-counter">
        <span className="run-counter-num">{formatBytes(view.freedBytes)}</span>
        <span className="run-counter-of">freed of {formatBytes(plan.bytes)} planned</span>
        <span
          className="run-meter"
          role="progressbar"
          aria-label="Freed"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(share * 100)}
        >
          <i style={{ width: `${share * 100}%` }} />
        </span>
      </div>

      <ol className="run-steps" ref={list} aria-label="Steps">
        {view.steps.map((s) => (
          <StepRow key={s.id} step={s} hot={hot === tileOf(s.id)} onHot={onHot} />
        ))}
      </ol>

      <div className="run-log" ref={log} role="log" aria-label="Log" aria-live="off">
        {view.log.length === 0 && run.phase === 'running' && (
          <div className="run-log-line is-sum">{adviceOnly ? 'Saving the selected reminders…' : movesOnly ? 'Checking every file again before it moves…' : 'Checking every place again before it goes…'}</div>
        )}
        {view.log.slice(-LOG_SHOWN).map((l) => (
          <div key={l.id} className={`run-log-line is-${l.kind}`}>
            {l.text}
          </div>
        ))}
      </div>

      <div className="run-foot">
        {run.phase === 'running' && (
          <div className="run-going">
            <p>
              {adviceOnly ? 'No apps are opened and no files are changed.' : `${view.skips > 0 ? `${formatCount(view.skips)} skipped so far. ` : ''}${current?.kind === 'move' ? 'Every move is recorded. ' : ''}Keep using your computer; anything open is skipped.`}
            </p>
            <button type="button" className="btn" onClick={onStop} disabled={run.stopping}>
              {run.stopping ? 'Stopping' : 'Stop'}
            </button>
          </div>
        )}
        {report && (
          <>
            <p>
              {report.cancelled
                ? `${formatBytes(report.freedBytes)} freed, ${formatCount(report.skippedItems)} ${report.skippedItems === 1 ? 'file' : 'files'} left in place.`
                : `${formatBytes(report.freedBytes)} freed${
                    report.skippedItems > 0
                      ? `, ${formatCount(report.skippedItems)} ${report.skippedItems === 1 ? 'file' : 'files'} skipped`
                      : ''
                  }.`}
            </p>
            {report.movedItems > 0 && (
              <p>
                {count(report.movedItems, 'file')} moved. Moves free no space, and you can undo them from the recap or Past runs.
              </p>
            )}
            {reminders > 0 && <p>{reminders} {reminders === 1 ? 'reminder' : 'reminders'} left for you, not counted as freed space.</p>}
            <button type="button" className="cta run-cta" onClick={onSeeChanges}>
              See what changed
            </button>
          </>
        )}
        {run.phase === 'failed' && (
          <div className="run-failed">
            {somethingRan ? (
              <>
                <p>Some of the plan may have run. Scan again to see the drive as it is now.</p>
                <button type="button" className="cta run-cta" onClick={onScanAgain}>
                  Scan again
                </button>
              </>
            ) : (
              <>
                <p>Nothing was deleted.</p>
                <button type="button" className="cta run-cta" onClick={onBack}>
                  Back to the plan
                </button>
              </>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}

/**
 * One step: its state, what it freed (or plans to), a bar while it runs, and what it left and why. Places a Stop
 * left are said once, summed; a step a Stop never reached says so.
 */
function StepRow({ step: s, hot, onHot }: { step: StepView; hot: boolean; onHot: (tile: string | null) => void }) {
  const move = s.kind === 'move';
  const notRun = s.status === 'done' && neverReached(move ? s.movedItems : s.deletedItems, s.skips);
  const others = s.skips.filter((k) => k.kind !== 'cancelled');
  const stopped = s.skips.filter((k) => k.kind === 'cancelled');
  const warn = !notRun && s.skips.some((k) => k.kind !== 'hardLinked');
  const cls = [
    'run-step',
    `is-${s.status}`,
    s.status === 'done' && warn && 'has-skip',
    notRun && 'is-notrun',
    hot && 'is-hot',
  ]
    .filter(Boolean)
    .join(' ');
  const said = [
    ...(notRun ? ['Not run: you stopped the run first'] : []),
    // A move left the file where it was: say which file, not bytes "kept".
    ...others.slice(0, ROW_SKIPS).map((k) => (move ? `${k.reason}: ${nameOf(k.path)}` : `${k.reason}, ${formatBytes(k.bytes)} kept`)),
    ...(others.length > ROW_SKIPS ? [`${formatCount(others.length - ROW_SKIPS)} more`] : []),
    ...(!notRun && stopped.length > 0
      ? [
          move
            ? `you stopped the run, ${count(stopped.reduce((sum, k) => sum + k.items, 0), 'file')} not moved`
            : `you stopped the run, ${formatBytes(stopped.reduce((sum, k) => sum + k.bytes, 0))} kept`,
        ]
      : []),
  ];
  return (
    <li className={cls} onMouseEnter={() => onHot(tileOf(s.id))} onMouseLeave={() => onHot(null)}>
      <span className="run-step-icon">
        {s.status === 'running' ? (
          <LoaderCircle size={13} className="run-spin" role="img" aria-label="Running" />
        ) : s.status === 'pending' ? (
          <i role="img" aria-label="Waiting" />
        ) : notRun ? (
          <Minus size={13} role="img" aria-label="Not run" />
        ) : warn ? (
          <TriangleAlert size={13} role="img" aria-label="Done, some left" />
        ) : (
          <Check size={13} strokeWidth={2.6} role="img" aria-label="Done" />
        )}
      </span>
      <span className="run-step-title">{s.title}</span>
      <span className="run-step-num">
        {move
          ? s.status === 'pending'
            ? count(s.plannedMoves, 'file')
            : s.status === 'running'
              ? `${formatCount(s.handled)} of ${formatCount(s.plannedMoves)}`
              : `${formatCount(s.movedItems)} moved`
          : formatBytes(s.status === 'pending' ? s.plannedBytes : s.freedBytes)}
      </span>
      {s.status === 'running' && (
        <span className="run-step-bar" aria-hidden>
          <i
            style={{
              width: `${
                move
                  ? s.plannedMoves > 0 ? Math.min(1, s.handled / s.plannedMoves) * 100 : 0
                  : s.plannedBytes > 0 ? Math.min(1, s.freedBytes / s.plannedBytes) * 100 : 0
              }%`,
            }}
          />
        </span>
      )}
      {said.length > 0 && (
        <span className={notRun && others.length === 0 ? 'run-step-skip is-quiet' : 'run-step-skip'}>
          {said.join('; ')}
        </span>
      )}
    </li>
  );
}

/** A path's last name: the file a skip is about. */
function nameOf(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

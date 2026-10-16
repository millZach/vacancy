// PROTOTYPE run step: each buildRun step is a live row with a progress line, streamed log lines and skips as they happen.
import { useEffect, useRef } from 'react';
import { ArrowRight, Check, CircleAlert, Trash2 } from 'lucide-react';
import { formatBytes, formatCount, type Dataset } from '../../shared/data';
import type { RunResult, RunStep } from '../../shared/run';
import { Say, You } from './Feed';
import { formatDuration, joinWords, plural } from './util';

export interface RunClock {
  starts: number[];
  durations: number[];
  total: number;
  elapsed: number;
}

export function stepProgress(clock: RunClock, i: number): number {
  const e = clock.elapsed - clock.starts[i];
  return Math.max(0, Math.min(1, e / clock.durations[i]));
}

export function freedSoFar(result: RunResult, clock: RunClock): number {
  return result.steps.reduce((s, st, i) => s + st.freedBytes * stepProgress(clock, i), 0);
}

export function approvalText(result: RunResult, data: Dataset): string {
  const del = result.steps.filter((s) => s.kind === 'delete');
  const planned = del.reduce((s, x) => s + x.plannedBytes, 0);
  const folders = data.reorgs.filter((r) => result.undoable.some((u) => u.reorgId === r.id)).map((r) => r.folder);
  const parts: string[] = [];
  if (del.length) parts.push(`delete ${formatBytes(planned)}`);
  if (folders.length) parts.push(`tidy ${joinWords(folders)}`);
  const s = parts.join(' and ') || 'go ahead';
  return `Go ahead: ${s}.`;
}

export function RunSection({ data, result, clock, onSeeReceipt, ensureVisible }: {
  data: Dataset;
  result: RunResult;
  clock: RunClock;
  onSeeReceipt: () => void;
  ensureVisible: (el: HTMLElement) => void;
}) {
  const done = clock.elapsed >= clock.total;
  const active = result.steps.findIndex((_, i) => stepProgress(clock, i) < 1);
  const rowRefs = useRef<(HTMLLIElement | null)[]>([]);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = active >= 0 ? rowRefs.current[active] : endRef.current;
    if (el) ensureVisible(el);
  }, [active, done]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <section className="vb-section" data-vb-anchor="run">
      <You>{approvalText(result, data)}</You>
      <Say avatar>
        <p className="vb-voice">On it. I'll call out anything I have to skip.</p>
      </Say>
      <ol className="vb-card vb-runlist">
        {result.steps.map((s, i) => (
          <RunRow key={s.id} step={s} p={stepProgress(clock, i)} live={i === active} rowRef={(el) => { rowRefs.current[i] = el; }} />
        ))}
      </ol>
      <div ref={endRef}>
        {done && (
          <Say avatar animate>
            <p className="vb-voice">
              All done. I freed <b>{formatBytes(result.freedBytes)}</b>
              {result.skipped.length > 0 ? ` and skipped ${plural(result.skipped.length, 'thing')} that ${result.skipped.length === 1 ? 'was' : 'were'} in use` : ''}.
              Printing your receipt…
            </p>
            <button type="button" className="vb-secondary" onClick={onSeeReceipt}>See your receipt</button>
          </Say>
        )}
      </div>
    </section>
  );
}

function RunRow({ step, p, live, rowRef }: { step: RunStep; p: number; live: boolean; rowRef: (el: HTMLLIElement | null) => void }) {
  const state = p >= 1 ? (step.outcome === 'partial' ? 'partial' : 'done') : live ? 'live' : 'wait';
  // Skips get their own callout below, so the stream shows only what was done.
  const lines = step.log.filter((l) => !l.startsWith('skipped '));
  const shown = lines.filter((_, i) => p >= (i + 1) / (lines.length + 1));
  const skipsShown = p >= 0.72 ? step.skipped : [];
  const isMove = step.kind === 'move';
  return (
    <li ref={rowRef} className={`vb-run vb-run-${state}`}>
      <span className="vb-run-mark" aria-hidden>
        {state === 'done' && <Check size={13} strokeWidth={2.6} />}
        {state === 'partial' && <Check size={13} strokeWidth={2.6} />}
        {state === 'live' && <i className="vb-spin" />}
      </span>
      <div className="vb-run-main">
        <div className="vb-run-top">
          <span className="vb-run-title">{step.title}</span>
          <span className="vb-run-num">
            {isMove
              ? `${formatCount(Math.round(step.items * p))} moved`
              : state === 'wait' ? formatBytes(step.plannedBytes) : formatBytes(step.freedBytes * p)}
          </span>
        </div>
        <div className="vb-run-line" aria-hidden><i style={{ transform: `scaleX(${p})` }} /></div>
        {live && shown.length > 0 && (
          <ul className="vb-log">
            {shown.slice(-3).map((l) => <LogLine key={l} line={l} />)}
          </ul>
        )}
        {skipsShown.map((k) => (
          <div key={k.path} className="vb-skip">
            <CircleAlert size={13} strokeWidth={2} aria-hidden />
            <span>
              Skipped <span className="vb-mono">{k.path}</span>: {k.reason}. {formatBytes(k.bytes)} left in place.
            </span>
          </div>
        ))}
        {state !== 'live' && state !== 'wait' && (
          <div className="vb-run-sub">
            {isMove ? `${plural(step.items, 'file')} moved, journaled for undo` : `${plural(step.items, 'item')} deleted`}
          </div>
        )}
      </div>
    </li>
  );
}

function LogLine({ line }: { line: string }) {
  const m = /^(deleted|moved|skipped) (.*)$/.exec(line);
  const verb = m?.[1] ?? '';
  const rest = m?.[2] ?? line;
  if (verb === 'moved') {
    const [from, to] = rest.split(' -> ');
    return (
      <li className="vb-log-move">
        <ArrowRight size={11} strokeWidth={2} aria-hidden />
        <span className="vb-mono">{from}</span>
        <span className="vb-log-to vb-mono">{to}</span>
      </li>
    );
  }
  return (
    <li className={verb === 'skipped' ? 'vb-log-skip' : ''}>
      {verb === 'skipped' ? <CircleAlert size={11} strokeWidth={2} aria-hidden /> : <Trash2 size={11} strokeWidth={2} aria-hidden />}
      <span className="vb-mono">{rest}</span>
    </li>
  );
}

export function RunRecap({ result }: { result: RunResult }) {
  return (
    <div className="vb-recap">
      <Check size={14} strokeWidth={2.2} aria-hidden />
      <span>
        Ran {plural(result.steps.length, 'step')} in {formatDuration(result.durationSec)}
        {result.skipped.length ? `, ${result.skipped.length} skipped` : ''}.
      </span>
    </div>
  );
}

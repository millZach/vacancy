// PROTOTYPE variant A "Atlas": plays buildRun() steps back on a clock and shows them as a progress list with a live log.
import { Check, LoaderCircle, TriangleAlert } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { formatCount } from '../../shared/data';
import type { RunResult, Skipped } from '../../shared/run';
import { gb } from './model';

export interface StepState {
  status: 'pending' | 'running' | 'done';
  p: number;
}

export interface LogLine {
  key: string;
  text: string;
  kind: 'del' | 'move' | 'skip' | 'sum';
}

export interface Playback {
  done: boolean;
  states: StepState[];
  current: number;
  freed: number;
  lines: LogLine[];
  skipped: Skipped[];
}

/** Compresses the simulated durations so a run takes about ten seconds. */
const SPEED = 0.45;
const LEAD = 650;

export function usePlayback(result: RunResult, active: boolean): Playback {
  const [t, setT] = useState(0);
  const timeline = useMemo(() => {
    let at = LEAD;
    return result.steps.map((s) => {
      const d = s.durationMs * SPEED;
      const span = { start: at, dur: d };
      at += d;
      return span;
    });
  }, [result]);
  const total = timeline.length ? timeline[timeline.length - 1].start + timeline[timeline.length - 1].dur : LEAD;

  useEffect(() => {
    if (!active) return;
    setT(0);
    let raf = 0;
    const start = performance.now();
    const tick = (now: number) => {
      const e = now - start;
      setT(e);
      if (e < total + 100) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [active, total]);

  return useMemo(() => {
    const states: StepState[] = timeline.map(({ start, dur }) => {
      if (t < start) return { status: 'pending', p: 0 };
      if (t >= start + dur) return { status: 'done', p: 1 };
      return { status: 'running', p: (t - start) / dur };
    });
    let current = states.findIndex((s) => s.status !== 'done');
    if (current < 0) current = states.length - 1;
    const freed = result.steps.reduce((s, x, i) => s + x.freedBytes * states[i].p, 0);
    const lines: LogLine[] = [];
    const skipped: Skipped[] = [];
    result.steps.forEach((s, i) => {
      const st = states[i];
      if (st.status === 'pending') return;
      const n = s.log.length;
      s.log.forEach((text, j) => {
        if (st.p < (j + 1) / (n + 1)) return;
        const kind: LogLine['kind'] = text.startsWith('skipped') ? 'skip' : text.startsWith('moved') ? 'move' : 'del';
        lines.push({ key: `${s.id}:${j}`, text, kind });
        if (kind === 'skip') {
          const k = s.skipped[j - (n - s.skipped.length)];
          if (k) skipped.push(k);
        }
      });
      if (st.status === 'done')
        lines.push({
          key: `${s.id}:sum`,
          text: s.kind === 'move' ? `${s.title}: ${formatCount(s.items)} files moved` : `${s.title}: ${gb(s.freedBytes)} freed`,
          kind: 'sum',
        });
    });
    return { done: t >= total && states.length > 0, states, current, freed, lines, skipped };
  }, [t, timeline, total, result]);
}

export function RunPanel({ result, pb, onDone }: { result: RunResult; pb: Playback; onDone: () => void }) {
  const logRef = useRef<HTMLDivElement>(null);
  const stepsRef = useRef<HTMLOListElement>(null);
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [pb.lines.length]);
  useEffect(() => {
    const row = stepsRef.current?.children[pb.current] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest' });
  }, [pb.current]);

  const cur = result.steps[pb.current];
  const pct = result.plannedBytes ? Math.min(1, pb.freed / result.plannedBytes) : pb.done ? 1 : 0;
  const n = result.steps.length;

  return (
    <aside className="va-side va-run" aria-live="polite">
      <header className="va-side-head">
        <div>
          <h2>{pb.done ? 'Done' : 'Freeing space'}</h2>
          <p>{pb.done ? `All ${n} steps finished.` : cur ? `Step ${pb.current + 1} of ${n}: ${cur.title}` : 'Starting'}</p>
        </div>
      </header>

      <div className="va-counter">
        <span className="va-counter-num">{gb(pb.freed)}</span>
        <span className="va-counter-of">freed of {gb(result.plannedBytes)} planned</span>
        <div className="va-meter">
          <i style={{ width: `${pct * 100}%` }} />
        </div>
      </div>

      <ol className="va-steps" ref={stepsRef}>
        {result.steps.map((s, i) => {
          const st = pb.states[i];
          const skippedNow = st.status === 'done' && s.skipped.length > 0;
          return (
            <li key={s.id} className={`va-step is-${st.status} ${skippedNow ? 'has-skip' : ''}`}>
              <span className="va-step-icon">
                {st.status === 'done' ? (
                  skippedNow ? (
                    <TriangleAlert size={13} />
                  ) : (
                    <Check size={13} strokeWidth={2.6} />
                  )
                ) : st.status === 'running' ? (
                  <LoaderCircle size={13} className="va-spin" />
                ) : (
                  <i />
                )}
              </span>
              <span className="va-step-title">{s.title}</span>
              <span className="va-step-num">
                {s.kind === 'move'
                  ? `${formatCount(Math.round(s.items * st.p))} moved`
                  : st.status === 'pending'
                    ? gb(s.plannedBytes)
                    : gb(s.freedBytes * st.p)}
              </span>
              {st.status === 'running' && (
                <span className="va-step-bar">
                  <i style={{ width: `${st.p * 100}%` }} />
                </span>
              )}
              {skippedNow && <span className="va-step-skip">{s.skipped.map((k) => `${k.reason}, ${gb(k.bytes)} kept`).join('; ')}</span>}
            </li>
          );
        })}
      </ol>

      <div className="va-log" ref={logRef} aria-label="Log">
        {pb.lines.length === 0 && <div className="va-log-line is-sum">Checking that nothing in the plan is open...</div>}
        {pb.lines.slice(-80).map((l) => (
          <div key={l.key} className={`va-log-line is-${l.kind}`}>
            {l.text}
          </div>
        ))}
      </div>

      <footer className="va-run-foot">
        {pb.done ? (
          <>
            <p>
              {gb(result.freedBytes)} freed{result.skipped.length ? `, ${result.skipped.length} ${result.skipped.length === 1 ? 'thing' : 'things'} skipped` : ''}.
            </p>
            <button type="button" className="va-cta" onClick={onDone}>
              See what changed
            </button>
          </>
        ) : (
          <p className="va-run-wait">
            {pb.skipped.length ? `${pb.skipped.length} skipped so far. ` : ''}Keep using your computer; anything open is skipped.
          </p>
        )}
      </footer>
    </aside>
  );
}

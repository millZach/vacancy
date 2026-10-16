// PROTOTYPE variant C: run step as a control room. Odometer, per-step progress, streaming log, skips as they happen.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { CircleCheck, CircleDashed, FolderInput, LoaderCircle, TriangleAlert } from 'lucide-react';
import { buildRun } from '../../shared/run';
import type { VariantProps } from '../../shared/types';
import { CapacityBar, Header, Sign, segsFor } from './chrome';
import { buildTimeline, catColor, clock, gb, n, progressAt } from './model';

function Wheel({ pos }: { pos: number }) {
  return (
    <span className="vc-wheel" aria-hidden>
      <span style={{ transform: `translateY(${(-pos * 100) / 11}%)` } as CSSProperties}>
        {[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map((d, i) => (
          <i key={i}>{d}</i>
        ))}
      </span>
    </span>
  );
}

/** Mechanical odometer: each wheel only turns while the one to its right rolls from 9 to 0. Shows tenths. */
export function Odometer({ value, intDigits }: { value: number; intDigits: number }) {
  const tenths = Math.max(0, value * 10);
  const places = intDigits + 1;
  const wheels: number[] = [];
  for (let k = places - 1; k >= 0; k--) {
    const unit = Math.pow(10, k);
    const d = Math.floor(tenths / unit) % 10;
    const rem = tenths % unit;
    const roll = k === 0 ? tenths % 1 : rem > unit - 1 ? rem - (unit - 1) : 0;
    wheels.push(d + roll);
  }
  return (
    <span className="vc-odo" role="img" aria-label={`${value.toFixed(1)} GB`}>
      {wheels.map((w, i) => (
        <span key={i} className="vc-odo-slot">
          {i === places - 1 && <b className="vc-odo-dot">.</b>}
          <Wheel pos={w} />
        </span>
      ))}
    </span>
  );
}

export function RunRoom({ data, onStep, proposals, reorgs }: VariantProps) {
  const run = useMemo(() => buildRun(data, proposals, reorgs), [data, proposals, reorgs]);
  const tl = useMemo(() => buildTimeline(run), [run]);
  const [t, setT] = useState(0);
  const logRef = useRef<HTMLOListElement>(null);

  useEffect(() => {
    let raf = 0;
    const t0 = performance.now();
    let last = 0;
    const tick = (now: number) => {
      const x = now - t0;
      if (x - last > 30 || x >= tl.total) {
        last = x;
        setT(Math.min(x, tl.total));
      }
      if (x < tl.total) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [tl]);

  const done = t >= tl.total;
  useEffect(() => {
    if (!done) return;
    const id = window.setTimeout(() => onStep('summary'), 2600);
    return () => clearTimeout(id);
  }, [done]);

  const lines = tl.lines.filter((l) => l.t <= t);
  useLayoutEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines.length]);

  // Live numbers.
  let freed = 0;
  let deleted = 0;
  let moved = 0;
  let pending = 0;
  const minus: Record<string, number> = {};
  for (const s of tl.slots) {
    const p = progressAt(s, t);
    const st = s.step;
    if (st.kind === 'delete') {
      freed += st.freedBytes * p;
      deleted += st.items * p;
      pending += st.plannedBytes * (1 - p);
      // Planned bytes leave the category; skipped bytes come back to it as they're found.
      minus[st.categoryId] = (minus[st.categoryId] ?? 0) + st.plannedBytes - (st.plannedBytes - st.freedBytes) * p;
    } else moved += st.items * p;
  }
  const { totalBytes: total, usedBytes: used } = data.disk;
  const free = total - used + freed;
  const frac = tl.total ? t / tl.total : 1;
  const skippedNow = tl.lines.filter((l) => l.kind === 'skip' && l.t <= t).length;
  const skips = run.skipped.slice(0, skippedNow);
  const started = tl.slots.filter((s) => t >= s.start).length;
  const intDigits = run.plannedBytes >= 100e9 ? 3 : 2;

  return (
    <>
      <Header
        data={data}
        sign={<Sign lit={7} glow={1.25} />}
        stats={[
          { label: 'Used', value: gb(used - freed) },
          { label: 'Free', value: gb(free), tone: 'pink' },
          { label: 'Elapsed', value: clock(frac * run.durationSec) },
        ]}
        bar={<CapacityBar segs={segsFor(data, minus)} ghost={pending} total={total} free={free} live />}
      />
      <main className={`vc-run ${done ? 'done' : ''}`}>
        <section className="vc-run-odo">
          <h2>{done ? 'Freed' : 'Freeing'}</h2>
          <div className="vc-odo-wrap">
            <Odometer value={freed / 1e9} intDigits={intDigits} />
            <span className="vc-odo-unit">GB</span>
          </div>
          <div className="vc-run-of">
            of {gb(run.plannedBytes)} planned
            <span className="vc-run-meter">
              <i style={{ width: `${(run.plannedBytes ? freed / run.plannedBytes : frac) * 100}%` }} />
            </span>
          </div>
          <dl className="vc-run-stats">
            <div>
              <dt>Deleted</dt>
              <dd className="num">{n(Math.round(deleted))}</dd>
            </div>
            <div>
              <dt>Moved</dt>
              <dd className="num">{n(Math.round(moved))}</dd>
            </div>
            <div>
              <dt>Skipped</dt>
              <dd className={`num ${skips.length ? 'amber' : ''}`}>{skips.length}</dd>
            </div>
          </dl>
          <div className="vc-run-skips">
            <h3>Skipped</h3>
            {skips.length ? (
              <ul>
                {skips.map((s) => (
                  <li key={s.path}>
                    <TriangleAlert size={13} strokeWidth={2.2} />
                    <div>
                      <span title={s.path}>{s.path}</span>
                      <small>
                        {s.reason}. {gb(s.bytes)} left in place.
                      </small>
                    </div>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="vc-mute">Nothing skipped so far. Files in use are left alone.</p>
            )}
          </div>
        </section>

        <section className="vc-run-steps" aria-label="Steps">
          <h2>
            {done ? 'All steps finished' : `Step ${Math.max(1, started)} of ${tl.slots.length}`}
            <em>{tl.slots.filter((s) => progressAt(s, t) >= 1).length} done</em>
          </h2>
          <ol>
            {tl.slots.map((s, i) => {
              const p = progressAt(s, t);
              const st = s.step;
              const cat = data.categories.find((c) => c.id === st.categoryId);
              const state = p >= 1 ? (st.outcome === 'partial' ? 'partial' : 'done') : p > 0 ? 'running' : 'queued';
              return (
                <li key={st.id} className={`vc-step ${state}`} style={{ '--c': cat ? catColor(cat) : '#888' } as CSSProperties}>
                  <span className="vc-step-ic">
                    {state === 'done' ? (
                      <CircleCheck size={15} />
                    ) : state === 'partial' ? (
                      <TriangleAlert size={15} />
                    ) : state === 'running' ? (
                      <LoaderCircle size={15} className="vc-spin" />
                    ) : st.kind === 'move' ? (
                      <FolderInput size={15} />
                    ) : (
                      <CircleDashed size={15} />
                    )}
                  </span>
                  <span className="vc-step-title">{st.title}</span>
                  <span className="vc-step-bar">
                    <i style={{ width: `${p * 100}%` }} />
                  </span>
                  <span className="vc-step-val num">
                    {st.kind === 'delete' ? gb(state === 'queued' ? st.plannedBytes : st.freedBytes * p) : `${n(Math.round(st.items * (p || 1)))} moves`}
                  </span>
                </li>
              );
            })}
          </ol>
        </section>

        <section className="vc-run-log" aria-label="Log">
          <h2>Log</h2>
          <ol ref={logRef}>
            {lines.map((l, i) => (
              <li key={i} className={`vc-log-${l.kind}`}>
                <time className="num">{clock((l.t / tl.total) * run.durationSec)}</time>
                <span>{l.text}</span>
              </li>
            ))}
            {!done && <li className="vc-log-cursor"><time /><i className="vc-caret-blink" /></li>}
          </ol>
        </section>
      </main>
      <footer className="vc-foot vc-run-foot">
        <p className="vc-foot-say">
          {done ? (
            <>
              Done. <b className="pink">{gb(run.freedBytes)}</b> freed{run.skipped.length ? `, ${run.skipped.length} items skipped because they were in use` : ''}.
            </>
          ) : (
            <>Working. You can keep using your computer; anything in use gets skipped, not forced.</>
          )}
        </p>
        <button className="vc-btn vc-btn-neon" disabled={!done} onClick={() => onStep('summary')}>
          See the breakdown
        </button>
      </footer>
    </>
  );
}

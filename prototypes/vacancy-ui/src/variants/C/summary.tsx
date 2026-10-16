// PROTOTYPE variant C: summary as "Wrapped"-style story slides. Auto-advance, arrows, dots, final recap with Scan again.
import { useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ChevronLeft, ChevronRight, CornerDownRight, Pause, Play, RotateCcw, TriangleAlert, Undo2 } from 'lucide-react';
import { buildRun, type RunResult } from '../../shared/run';
import type { Dataset } from '../../shared/data';
import type { VariantProps } from '../../shared/types';
import { Sign, segsFor } from './chrome';
import { catColor, formatDuration, gb, n, sourceLabel } from './model';

const DUR = 6500;

export function Wrapped({ data, onStep, proposals, reorgs }: VariantProps) {
  const run = useMemo(() => buildRun(data, proposals, reorgs), [data, proposals, reorgs]);
  const [i, setI] = useState(0);
  const [dir, setDir] = useState(1);
  const [paused, setPaused] = useState(false);
  const [hover, setHover] = useState(false);
  const [undo, setUndo] = useState<Record<string, 'idle' | 'busy' | 'done'>>({});

  const slides: { id: string; label: string; node: ReactNode }[] = [
    { id: 'headline', label: 'Freed', node: <Headline data={data} run={run} /> },
    { id: 'disk', label: 'Disk', node: <DiskSlide data={data} run={run} /> },
    { id: 'win', label: 'Biggest win', node: <WinSlide data={data} run={run} /> },
    { id: 'cats', label: 'By category', node: <CatSlide data={data} run={run} /> },
    {
      id: 'moved',
      label: 'Moved',
      node: (
        <MovedSlide
          data={data}
          run={run}
          undo={undo}
          onUndo={(id) => {
            setPaused(true);
            setUndo((u) => ({ ...u, [id]: 'busy' }));
            window.setTimeout(() => setUndo((u) => ({ ...u, [id]: 'done' })), 1300);
          }}
        />
      ),
    },
    { id: 'kept', label: 'Skipped and left for you', node: <KeptSlide data={data} run={run} proposals={proposals} /> },
    { id: 'recap', label: 'Recap', node: <Recap data={data} run={run} onScan={() => onStep('scan')} onReplay={() => go(0)} /> },
  ];
  const last = slides.length - 1;
  const go = (k: number) => {
    const next = Math.max(0, Math.min(last, k));
    setDir(next >= i ? 1 : -1);
    setI(next);
  };
  const running = !paused && !hover && i < last;

  return (
    <div className={`vc-wrap vc-wrap-${slides[i].id}`}>
      <div className="vc-wrap-haze" aria-hidden />
      <div className={`vc-wrap-mini ${i === 0 ? 'hide' : ''}`}>
        <Sign lit={7} glow={0.8} />
      </div>
      <div className="vc-wrap-stage" onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
        <AnimatePresence mode="wait" custom={dir} initial={false}>
          <motion.section
            key={slides[i].id}
            className={`vc-slide vc-slide-${slides[i].id}`}
            custom={dir}
            initial={{ opacity: 0, x: 48 * dir }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -48 * dir }}
            transition={{ duration: 0.32, ease: [0.2, 0.7, 0.2, 1] }}
            aria-roledescription="slide"
            aria-label={`${i + 1} of ${slides.length}: ${slides[i].label}`}
          >
            {slides[i].node}
          </motion.section>
        </AnimatePresence>
      </div>
      <nav className="vc-wrap-nav" aria-label="Breakdown slides">
        <button className="vc-round" onClick={() => go(i - 1)} disabled={i === 0} aria-label="Previous slide">
          <ChevronLeft size={18} />
        </button>
        <div className="vc-dots">
          {slides.map((s, k) => (
            <button key={s.id} className={`vc-dotbtn ${k === i ? 'on' : ''} ${k < i ? 'past' : ''}`} onClick={() => go(k)} aria-label={`${k + 1}: ${s.label}`} aria-current={k === i}>
              {k === i && i < last && (
                <i
                  key={`${i}-fill`}
                  style={{ animationDuration: `${DUR}ms`, animationPlayState: running ? 'running' : 'paused' } as CSSProperties}
                  onAnimationEnd={() => go(i + 1)}
                />
              )}
            </button>
          ))}
        </div>
        <button className="vc-round" onClick={() => go(i + 1)} disabled={i === last} aria-label="Next slide">
          <ChevronRight size={18} />
        </button>
        <button className="vc-round ghost" onClick={() => setPaused((p) => !p)} disabled={i === last} aria-label={paused ? 'Play' : 'Pause'}>
          {paused || i === last ? <Play size={15} /> : <Pause size={15} />}
        </button>
      </nav>
      {i < last && (
        <button className="vc-link vc-wrap-skip" onClick={() => go(last)}>
          Skip to recap
        </button>
      )}
    </div>
  );
}

type S = { data: Dataset; run: RunResult };

function Tiles({ text, delay = 0 }: { text: string; delay?: number }) {
  return (
    <span className="vc-tiles" aria-hidden>
      {text.split('').map((ch, k) =>
        ch === ' ' ? (
          <span key={k} className="vc-tile-gap" />
        ) : (
          <motion.span
            key={k}
            className="vc-tile"
            initial={{ rotateX: -95, opacity: 0 }}
            animate={{ rotateX: 0, opacity: 1 }}
            transition={{ delay: delay + k * 0.06, type: 'spring', stiffness: 260, damping: 18 }}
          >
            {ch}
          </motion.span>
        ),
      )}
    </span>
  );
}

function Headline({ data, run }: S) {
  const dur = formatDuration(run.durationSec);
  return (
    <div className="vc-hl">
      <Sign lit={7} glow={1.3} size="hero" />
      <div className="vc-board" role="img" aria-label={`${gb(run.freedBytes)} freed`}>
        <Tiles text={gb(run.freedBytes).toUpperCase()} delay={0.5} />
        <Tiles text="FREED" delay={1.0} />
      </div>
      <p className="vc-hl-sub">
        {data.disk.name} has room again. Vacancy deleted {n(run.deletedItems)} items
        {run.movedItems ? <> and sorted {n(run.movedItems)} files</> : null} in {dur}.
      </p>
    </div>
  );
}

function DiskBar({ data, minus, free, label, after, gain }: { data: Dataset; minus?: Record<string, number>; free: number; label: string; after?: boolean; gain?: number }) {
  const total = data.disk.totalBytes;
  const segs = segsFor(data, minus);
  return (
    <div className={`vc-dbar ${after ? 'after' : ''}`}>
      <span className="vc-dbar-label">{label}</span>
      <div className="vc-dbar-track">
        {segs.map((s, k) => (
          <motion.i
            key={s.id}
            className={s.locked ? 'locked' : ''}
            style={{ background: s.color } as CSSProperties}
            initial={after ? { width: `${((s.bytes + (minus?.[s.id] ?? 0)) / total) * 100}%` } : false}
            animate={{ width: `${(s.bytes / total) * 100}%` }}
            transition={{ delay: 0.5 + k * 0.05, duration: 0.9, ease: [0.3, 0.7, 0.2, 1] }}
            title={`${s.name}: ${gb(s.bytes)}`}
          />
        ))}
        <span className="vc-dbar-free">
          {gain ? (
            <motion.em initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.9, duration: 0.4 }}>
              +{gb(gain)}
            </motion.em>
          ) : null}
        </span>
      </div>
      <span className="vc-dbar-val num">
        <b className={after ? 'pink' : ''}>{gb(free)}</b> free
      </span>
    </div>
  );
}

function DiskSlide({ data, run }: S) {
  const free0 = run.totalBytes - run.usedBefore;
  const free1 = run.totalBytes - run.usedAfter;
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">
        Free space went from {gb(free0)} to {gb(free1)}.
      </h1>
      <div className="vc-dbars">
        <DiskBar data={data} free={free0} label="Before" />
        <DiskBar data={data} minus={run.freedByCategory} free={free1} label="After" after gain={run.freedBytes} />
      </div>
      <p className="vc-sl-sub">
        That's {(free1 / Math.max(1, free0)).toFixed(1)} times the room you had. {data.disk.name} is {Math.round((run.usedAfter / run.totalBytes) * 100)}% full now, down from{' '}
        {Math.round((run.usedBefore / run.totalBytes) * 100)}%.
      </p>
    </div>
  );
}

function WinSlide({ data, run }: S) {
  const w = run.biggestWin;
  if (!w) {
    return (
      <div className="vc-sl">
        <h1 className="vc-sl-title">No deletions this time.</h1>
        <p className="vc-sl-sub">This plan only sorted folders. Your biggest change is on the next slides.</p>
      </div>
    );
  }
  const p = data.proposals.find((x) => x.id === w.id);
  const cat = data.categories.find((c) => c.id === w.categoryId);
  const share = Math.round((w.freedBytes / Math.max(1, run.freedBytes)) * 100);
  return (
    <div className="vc-sl vc-win">
      <motion.div className="vc-fob-wrap" initial={{ rotate: -22, y: -40, opacity: 0 }} animate={{ rotate: -9, y: 0, opacity: 1 }} transition={{ type: 'spring', stiffness: 120, damping: 11, delay: 0.15 }}>
        <span className="vc-fob-ring" aria-hidden />
        <div className="vc-fob">
          <span className="vc-fob-hole" aria-hidden />
          <small>Room 1</small>
          <b>{gb(w.freedBytes).replace(' GB', '')}</b>
          <em>GB</em>
          <span className="vc-fob-foot">{cat?.name}</span>
        </div>
      </motion.div>
      <div className="vc-win-text">
        <h1 className="vc-sl-title">Your biggest win was {w.title.charAt(0).toLowerCase() + w.title.slice(1)}.</h1>
        <p className="vc-sl-sub">
          {gb(w.freedBytes)} from {n(w.items)} item{w.items === 1 ? '' : 's'}, {share}% of everything Vacancy freed.
        </p>
        {p && <p className="vc-win-why">{p.why}</p>}
      </div>
    </div>
  );
}

function CatSlide({ data, run }: S) {
  const rows = Object.entries(run.freedByCategory)
    .filter(([, b]) => b > 0)
    .sort((a, b) => b[1] - a[1])
    .map(([id, b]) => ({ c: data.categories.find((x) => x.id === id)!, b }))
    .filter((r) => r.c);
  const max = Math.max(1, ...rows.map((r) => r.b));
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">Where the {gb(run.freedBytes)} came from.</h1>
      <div className="vc-cchart" role="table" aria-label="Freed space by category">
        {rows.map((r, k) => (
          <div key={r.c.id} className="vc-cchart-row" role="row" title={`${r.c.name}: ${gb(r.b)} (${Math.round((r.b / run.freedBytes) * 100)}%)`}>
            <span className="vc-cchart-name" role="cell">
              <i className="vc-swatch" style={{ background: catColor(r.c, 'dot') }} />
              {r.c.name}
            </span>
            <span className="vc-cchart-track" role="cell">
              <motion.i
                style={{ background: catColor(r.c) }}
                initial={{ width: 0 }}
                animate={{ width: `${(r.b / max) * 100}%` }}
                transition={{ delay: 0.25 + k * 0.08, duration: 0.8, ease: [0.3, 0.7, 0.2, 1] }}
              />
            </span>
            <span className="vc-cchart-val num" role="cell">
              {gb(r.b)}
            </span>
            <span className="vc-cchart-pct num" role="cell">
              {Math.round((r.b / run.freedBytes) * 100)}%
            </span>
          </div>
        ))}
        {!rows.length && <p className="vc-sl-sub">Nothing was deleted.</p>}
      </div>
    </div>
  );
}

function MovedSlide({ data, run, undo, onUndo }: S & { undo: Record<string, 'idle' | 'busy' | 'done'>; onUndo: (id: string) => void }) {
  if (!run.undoable.length) {
    return (
      <div className="vc-sl">
        <h1 className="vc-sl-title">No folders were moved this time.</h1>
        <p className="vc-sl-sub">Reorganizations you skip stay in the plan for the next scan.</p>
      </div>
    );
  }
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">{n(run.movedItems)} files found a home.</h1>
      <p className="vc-sl-sub">Every move was journaled. Undo puts each file back exactly where it was.</p>
      <div className="vc-moved">
        {run.undoable.map((u) => {
          const r = data.reorgs.find((x) => x.id === u.reorgId)!;
          const st = undo[u.reorgId] ?? 'idle';
          return (
            <div key={u.reorgId} className={`vc-moved-card ${st}`}>
              <div className="vc-moved-head">
                <div>
                  <b>{u.folder}</b>
                  <span>
                    {n(u.moves)} files, suggested by {sourceLabel(data, { source: 'ai' })}
                  </span>
                </div>
                <button className="vc-btn vc-btn-ghost" disabled={st !== 'idle'} onClick={() => onUndo(u.reorgId)}>
                  {st === 'busy' ? <RotateCcw size={14} className="vc-spin-rev" /> : <Undo2 size={14} />}
                  {st === 'idle' ? `Undo ${u.folder}` : st === 'busy' ? 'Moving back' : 'Moved back'}
                </button>
              </div>
              <ul className="vc-moved-folders">
                {(r.after.children ?? [])
                  .filter((c) => !c.name.startsWith('(') && !c.name.includes('unchanged'))
                  .map((c) => (
                    <li key={c.name}>
                      {c.name}
                      {c.count != null && <em className="num">{n(c.count)}</em>}
                    </li>
                  ))}
              </ul>
              <ul className="vc-moved-sample">
                {r.sampleMoves.slice(0, 2).map((m) => (
                  <li key={m.from}>
                    <CornerDownRight size={12} />
                    <span title={m.to}>{m.to}</span>
                  </li>
                ))}
              </ul>
              {st === 'done' && <p className="vc-moved-done">{n(u.moves)} files are back where they were.</p>}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function KeptSlide({ data, run, proposals }: S & { proposals: Set<string> }) {
  const extra = data.proposals.filter((p) => p.action === 'hint' && !proposals.has(p.id));
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">{run.skipped.length || run.leftForYou.length ? 'A few things stayed put.' : 'Nothing was skipped.'}</h1>
      <div className="vc-kept">
        <div>
          <h2>Skipped because they were in use</h2>
          {run.skipped.length ? (
            <ul>
              {run.skipped.map((s) => (
                <li key={s.path}>
                  <TriangleAlert size={14} />
                  <div>
                    <b>{s.reason}</b>
                    <span title={s.path}>{s.path}</span>
                  </div>
                  <em className="num">{gb(s.bytes)}</em>
                </li>
              ))}
            </ul>
          ) : (
            <p className="vc-mute">Every file in the plan was free to go.</p>
          )}
          {run.skipped.length > 0 && <p className="vc-kept-note">They'll be picked up on your next scan.</p>}
        </div>
        <div>
          <h2>Left for you</h2>
          {run.leftForYou.length ? (
            <ul>
              {run.leftForYou.map((p) => (
                <li key={p.id} className="hint">
                  <span className="vc-kept-check" aria-hidden />
                  <div>
                    <b>{p.title}</b>
                    <span>{p.why}</span>
                  </div>
                  <em className="num">{gb(p.bytes)}</em>
                </li>
              ))}
            </ul>
          ) : (
            <>
              <p className="vc-mute">You didn't add any tips to your list.</p>
              {extra.length > 0 && (
                <ul className="vc-kept-extra">
                  {extra.map((p) => (
                    <li key={p.id} className="hint">
                      <span className="vc-kept-check" aria-hidden />
                      <div>
                        <b>Also worth a look: {p.title}</b>
                        <span>{p.why}</span>
                      </div>
                      <em className="num">{gb(p.bytes)}</em>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
          <p className="vc-kept-note">Tips aren't counted in freed space. Vacancy can't do them for you.</p>
        </div>
      </div>
    </div>
  );
}

function Recap({ data, run, onScan, onReplay }: S & { onScan: () => void; onReplay: () => void }) {
  const free1 = run.totalBytes - run.usedAfter;
  const cells: { k: string; v: string; s?: string; hot?: boolean }[] = [
    { k: 'Freed', v: gb(run.freedBytes), s: `of ${gb(run.plannedBytes)} planned`, hot: true },
    { k: 'Free now', v: gb(free1), s: `was ${gb(run.totalBytes - run.usedBefore)}` },
    { k: 'Deleted', v: n(run.deletedItems), s: 'items, permanently' },
    { k: 'Moved', v: n(run.movedItems), s: run.undoable.length ? `files in ${run.undoable.map((u) => u.folder).join(' and ')}` : 'files' },
    { k: 'Took', v: formatDuration(run.durationSec), s: `${run.steps.length} steps` },
    { k: 'Biggest win', v: run.biggestWin ? gb(run.biggestWin.freedBytes) : 'None', s: run.biggestWin?.title },
    { k: 'Skipped', v: String(run.skipped.length), s: run.skipped.length ? `${gb(run.skipped.reduce((s, x) => s + x.bytes, 0))} in use` : 'nothing in use' },
    { k: 'Left for you', v: String(run.leftForYou.length), s: run.leftForYou.length ? run.leftForYou.map((p) => p.title).join(', ') : 'no tips added' },
  ];
  return (
    <div className="vc-sl vc-recap">
      <h1 className="vc-sl-title">That's checkout for {data.disk.name}.</h1>
      <div className="vc-recap-grid">
        {cells.map((c) => (
          <div key={c.k} className={c.hot ? 'hot' : ''}>
            <span>{c.k}</span>
            <b className="num">{c.v}</b>
            {c.s && <small title={c.s}>{c.s}</small>}
          </div>
        ))}
      </div>
      <div className="vc-recap-actions">
        <button className="vc-btn vc-btn-neon" onClick={onScan}>
          Scan again
        </button>
        <button className="vc-btn vc-btn-ghost" onClick={onReplay}>
          <RotateCcw size={14} /> Watch again
        </button>
      </div>
    </div>
  );
}

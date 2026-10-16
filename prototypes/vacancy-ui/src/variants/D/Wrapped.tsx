// PROTOTYPE variant D: the summary's story slides, adapted from variant C's "Wrapped" and restyled in Atlas's look
// (Overpass, A's neon pink, A's dark surfaces and dot ground). The last slide is a checkout receipt whose numbers fly
// into Atlas's summary panel as the before/after map fades up underneath. Class names keep C's vc- prefix; their
// D styling lives in vd.css, scoped under .vd-root.
import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ChevronLeft, ChevronRight, CornerDownRight, Map as MapIcon, Pause, Play, RotateCcw, TriangleAlert, Undo2 } from 'lucide-react';
import type { RunResult } from '../../shared/run';
import { formatCount, type Dataset } from '../../shared/data';
import { duration, gb, tint, type AtlasModel, type Region } from './model';

const DUR = 6500;
const DUR_LAST = 3400;

export type UndoState = Record<string, 'working' | 'done'>;
export type HandoffKind = 'fly' | 'skip';

interface Props {
  data: Dataset;
  model: AtlasModel;
  result: RunResult;
  proposals: Set<string>;
  undo: UndoState;
  onUndo: (reorgId: string) => void;
  /** Set while the slides make way for the map summary. */
  leaving: HandoffKind | null;
  /** Fading in over the map summary (Replay highlights). */
  entering: boolean;
  onHandoff: (how: HandoffKind) => void;
}

export function Wrapped({ data, model, result: run, proposals, undo, onUndo, leaving, entering, onHandoff }: Props) {
  const [i, setI] = useState(0);
  const [dir, setDir] = useState(1);
  const [paused, setPaused] = useState(false);
  const [hover, setHover] = useState(false);

  const slides: { id: string; label: string; node: ReactNode }[] = [
    { id: 'headline', label: 'Freed', node: <Headline data={data} run={run} /> },
    { id: 'disk', label: 'Disk', node: <DiskSlide data={data} model={model} run={run} /> },
    { id: 'win', label: 'Biggest win', node: <WinSlide data={data} model={model} run={run} /> },
    { id: 'cats', label: 'By category', node: <CatSlide model={model} run={run} /> },
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
            onUndo(id);
          }}
        />
      ),
    },
    { id: 'kept', label: 'Skipped and left for you', node: <KeptSlide data={data} run={run} proposals={proposals} /> },
    { id: 'checkout', label: 'Checkout', node: <Checkout data={data} run={run} onMap={() => onHandoff('fly')} /> },
  ];
  const last = slides.length - 1;
  const go = (k: number) => {
    if (leaving) return;
    if (k > last) return onHandoff('fly');
    const next = Math.max(0, k);
    setDir(next >= i ? 1 : -1);
    setI(next);
  };
  const running = !paused && !hover && !leaving && !entering;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (leaving) return;
      if (e.key === 'ArrowRight') go(i + 1);
      if (e.key === 'ArrowLeft') go(i - 1);
      if (e.key === 'Escape') onHandoff('skip');
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  return (
    <div
      className={`vd-wrap vc-wrap vc-wrap-${slides[i].id} ${leaving ? `is-leaving is-${leaving}` : ''} ${entering ? 'is-entering' : ''}`}
      role="region"
      aria-label="Highlights of this run"
    >
      <div className="vd-wrap-bg" aria-hidden />
      <div className="vc-wrap-haze" aria-hidden />
      <div className={`vd-wrap-top ${i === 0 ? 'hide' : ''}`} aria-hidden>
        <span className="va-wordmark is-lit">Vacancy</span>
      </div>
      <div className="vc-wrap-stage" onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
        <AnimatePresence mode="wait" custom={dir}>
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
      <nav className="vc-wrap-nav vd-fade" aria-label="Highlight slides">
        <button type="button" className="vc-round" onClick={() => go(i - 1)} disabled={i === 0} aria-label="Previous slide">
          <ChevronLeft size={18} />
        </button>
        <div className="vc-dots">
          {slides.map((s, k) => (
            <button
              type="button"
              key={s.id}
              className={`vc-dotbtn ${k === i ? 'on' : ''} ${k < i ? 'past' : ''}`}
              onClick={() => go(k)}
              aria-label={`${k + 1}: ${s.label}`}
              aria-current={k === i}
            >
              {k === i && (
                <i
                  key={`${i}-fill`}
                  style={{ animationDuration: `${i === last ? DUR_LAST : DUR}ms`, animationPlayState: running ? 'running' : 'paused' } as CSSProperties}
                  onAnimationEnd={() => go(i + 1)}
                />
              )}
            </button>
          ))}
        </div>
        <button type="button" className="vc-round" onClick={() => go(i + 1)} aria-label={i === last ? 'Show the map summary' : 'Next slide'}>
          <ChevronRight size={18} />
        </button>
        <button type="button" className="vc-round ghost" onClick={() => setPaused((p) => !p)} aria-label={paused ? 'Play' : 'Pause'}>
          {paused ? <Play size={15} /> : <Pause size={15} />}
        </button>
      </nav>
      {i < last && (
        <button type="button" className="vc-link vc-wrap-skip vd-fade" onClick={() => onHandoff('skip')}>
          Skip to recap
        </button>
      )}
    </div>
  );
}

type S = { data: Dataset; run: RunResult };

/** The motel sign: A's neon script, each letter catching in turn. */
function NeonSign() {
  const word = 'Vacancy';
  return (
    <div className="vd-sign" role="img" aria-label="Vacancy">
      <span className="vd-sign-off" aria-hidden>
        {word}
      </span>
      <span className="vd-sign-lit" aria-hidden>
        {word.split('').map((ch, k) => (
          <span key={k} style={{ animationDelay: `${160 + k * 95}ms` }}>
            {ch}
          </span>
        ))}
      </span>
    </div>
  );
}

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
  return (
    <div className="vc-hl">
      <NeonSign />
      <div className="vc-board" role="img" aria-label={`${gb(run.freedBytes)} freed`}>
        <Tiles text={gb(run.freedBytes).toUpperCase()} delay={0.9} />
        <Tiles text="FREED" delay={1.35} />
      </div>
      <p className="vc-hl-sub">
        {data.disk.name} has room again. Vacancy deleted {formatCount(run.deletedItems)} items
        {run.movedItems ? <> and sorted {formatCount(run.movedItems)} files</> : null} in {duration(run.durationSec)}.
      </p>
    </div>
  );
}

const usedRegions = (model: AtlasModel) =>
  model.regions.filter((r) => !r.free).sort((a, b) => Number(a.protected) - Number(b.protected) || b.bytes - a.bytes);
const segColor = (r: Region) => (r.protected ? undefined : tint(r.hue, 0.47, 0.075));

function DiskBar({ model, total, minus, label, after, gain, free }: { model: AtlasModel; total: number; minus?: Record<string, number>; label: string; after?: boolean; gain?: number; free: number }) {
  const regions = usedRegions(model);
  const freeBefore = model.freeBytes;
  return (
    <div className={`vc-dbar ${after ? 'after' : ''}`}>
      <span className="vc-dbar-label">{label}</span>
      <div className="vc-dbar-track">
        {regions.map((r, k) => {
          const now = Math.max(0, r.bytes - (minus?.[r.id] ?? 0));
          return (
            <motion.i
              key={r.id}
              className={r.protected ? 'locked' : ''}
              style={{ background: segColor(r) } as CSSProperties}
              initial={after ? { width: `${(r.bytes / total) * 100}%` } : false}
              animate={{ width: `${(now / total) * 100}%` }}
              transition={{ delay: 0.55 + k * 0.05, duration: 0.9, ease: [0.3, 0.7, 0.2, 1] }}
              title={`${r.name}: ${gb(now)}${r.protected ? ' (protected)' : ''}`}
            />
          );
        })}
        {after && (
          <span className="vd-dbar-vacant">
            {gain ? (
              <motion.em initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 1.1, duration: 0.4 }}>
                +{gb(gain)}
              </motion.em>
            ) : null}
          </span>
        )}
        <span className="vc-dbar-free" style={after ? { flex: 'none', width: `${(freeBefore / total) * 100}%` } : undefined} />
      </div>
      <span className="vc-dbar-val">
        <b className={after ? 'pink' : ''}>{gb(free)}</b> free
      </span>
    </div>
  );
}

function DiskSlide({ data, model, run }: S & { model: AtlasModel }) {
  const free0 = run.totalBytes - run.usedBefore;
  const free1 = run.totalBytes - run.usedAfter;
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">
        Free space went from {gb(free0)} to {gb(free1)}.
      </h1>
      <div className="vc-dbars">
        <DiskBar model={model} total={run.totalBytes} free={free0} label="Before" />
        <DiskBar model={model} total={run.totalBytes} minus={run.freedByCategory} free={free1} label="After" after gain={run.freedBytes} />
      </div>
      <p className="vc-sl-sub">
        That's {(free1 / Math.max(1, free0)).toFixed(1)} times the room you had. {data.disk.name} is {Math.round((run.usedAfter / run.totalBytes) * 100)}% full now, down from{' '}
        {Math.round((run.usedBefore / run.totalBytes) * 100)}%.
      </p>
    </div>
  );
}

function WinSlide({ data, model, run }: S & { model: AtlasModel }) {
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
  const region = model.regionById.get(w.categoryId);
  const share = Math.round((w.freedBytes / Math.max(1, run.freedBytes)) * 100);
  const [num, unit] = gb(w.freedBytes).split(/\s/);
  return (
    <div className="vc-sl vc-win">
      <motion.div className="vc-fob-wrap" initial={{ rotate: -22, y: -40, opacity: 0 }} animate={{ rotate: -9, y: 0, opacity: 1 }} transition={{ type: 'spring', stiffness: 120, damping: 11, delay: 0.15 }}>
        <span className="vc-fob-ring" aria-hidden />
        <div className="vc-fob">
          <span className="vc-fob-hole" aria-hidden />
          <small>Room 1</small>
          <b>{num}</b>
          <em>{unit}</em>
          <span className="vc-fob-foot">{region?.name}</span>
        </div>
      </motion.div>
      <div className="vc-win-text">
        <h1 className="vc-sl-title">Your biggest win was {w.title.charAt(0).toLowerCase() + w.title.slice(1)}.</h1>
        <p className="vc-sl-sub">
          {gb(w.freedBytes)} from {formatCount(w.items)} item{w.items === 1 ? '' : 's'}, {share}% of everything Vacancy freed.
          {p?.regenerable ? ' It comes back on its own if you ever need it.' : ''}
        </p>
        {p && <p className="vc-win-why">{p.why}</p>}
      </div>
    </div>
  );
}

function CatSlide({ model, run }: { model: AtlasModel; run: RunResult }) {
  const rows = Object.entries(run.freedByCategory)
    .filter(([, b]) => b > 0)
    .sort((a, b) => b[1] - a[1])
    .map(([id, b]) => ({ r: model.regionById.get(id)!, b }))
    .filter((x) => x.r);
  const max = Math.max(1, ...rows.map((x) => x.b));
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">Where the {gb(run.freedBytes)} came from.</h1>
      <div className="vc-cchart" role="table" aria-label="Freed space by category">
        {rows.map(({ r, b }, k) => (
          <div key={r.id} className="vc-cchart-row" role="row" title={`${r.name}: ${gb(b)} of ${gb(r.bytes)} (${Math.round((b / run.freedBytes) * 100)}% of the total)`}>
            <span className="vc-cchart-name" role="cell">
              <i className="vc-swatch" style={{ background: tint(r.hue, 0.62, 0.1) }} />
              {r.name}
            </span>
            <span className="vc-cchart-track" role="cell">
              <motion.i
                initial={{ width: 0 }}
                animate={{ width: `${(b / max) * 100}%` }}
                transition={{ delay: 0.25 + k * 0.08, duration: 0.8, ease: [0.3, 0.7, 0.2, 1] }}
              />
            </span>
            <span className="vc-cchart-val" role="cell">
              {gb(b)}
            </span>
            <span className="vc-cchart-pct" role="cell">
              {Math.round((b / run.freedBytes) * 100)}%
            </span>
          </div>
        ))}
        {!rows.length && <p className="vc-sl-sub">Nothing was deleted.</p>}
      </div>
    </div>
  );
}

function MovedSlide({ data, run, undo, onUndo }: S & { undo: UndoState; onUndo: (id: string) => void }) {
  if (!run.undoable.length) {
    return (
      <div className="vc-sl">
        <h1 className="vc-sl-title">No folders were moved this time.</h1>
        <p className="vc-sl-sub">Reorganizations you skip stay in the plan for the next scan.</p>
      </div>
    );
  }
  const agent = data.scan.aiAgent ?? 'your AI agent';
  return (
    <div className="vc-sl">
      <h1 className="vc-sl-title">{formatCount(run.movedItems)} files found a home.</h1>
      <p className="vc-sl-sub">Every move was recorded. Undo puts each file back exactly where it was.</p>
      <div className="vc-moved">
        {run.undoable.map((u) => {
          const r = data.reorgs.find((x) => x.id === u.reorgId)!;
          const st = undo[u.reorgId];
          return (
            <div key={u.reorgId} className={`vc-moved-card ${st ?? 'idle'}`}>
              <div className="vc-moved-head">
                <div>
                  <b>{u.folder}</b>
                  <span>
                    {formatCount(u.moves)} files, suggested by {agent}
                  </span>
                </div>
                {st === 'done' ? (
                  <span className="va-undone">Moved back</span>
                ) : (
                  <button type="button" className="va-btn va-btn--sm" disabled={st === 'working'} onClick={() => onUndo(u.reorgId)}>
                    {st === 'working' ? <RotateCcw size={13} className="vd-spin-rev" /> : <Undo2 size={13} />}
                    {st === 'working' ? 'Moving back' : `Undo ${u.folder}`}
                  </button>
                )}
              </div>
              <ul className="vc-moved-folders">
                {(r.after.children ?? [])
                  .filter((c) => !c.name.startsWith('(') && !c.name.includes('unchanged'))
                  .map((c) => (
                    <li key={c.name}>
                      {c.name}
                      {c.count != null && <em>{formatCount(c.count)}</em>}
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
              {st === 'done' && <p className="vc-moved-done">{formatCount(u.moves)} files are back where they were.</p>}
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
                  <em>{gb(s.bytes)}</em>
                </li>
              ))}
            </ul>
          ) : (
            <p className="vc-mute">Every file in the plan was free to go.</p>
          )}
          {run.skipped.length > 0 && <p className="vc-kept-note">They go on your next run if the apps are closed.</p>}
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
                  <em>{gb(p.bytes)}</em>
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
                      <em>{gb(p.bytes)}</em>
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

/** The receipt. Its numbers use the summary panel's own markup so they can fly into place there. */
function Checkout({ data, run, onMap }: S & { onMap: () => void }) {
  const free0 = run.totalBytes - run.usedBefore;
  const free1 = run.totalBytes - run.usedAfter;
  const [num, unit] = gb(run.freedBytes).split(/\s/);
  const [whole, frac] = num.split('.');
  return (
    <div className="vc-sl vd-co">
      <h1 className="vc-sl-title vd-fade">That's checkout for {data.disk.name}.</h1>
      <div className="vd-co-hero">
        <span className="va-hero-num" data-fly="freed" aria-label={`${gb(run.freedBytes)} freed`}>
          {whole}
          {frac !== undefined && <span className="va-hero-frac">.{frac}</span>}
          <span className="va-hero-unit">{unit}</span>
        </span>
        <p className="vd-co-sub vd-fade">
          freed. Free space went from {gb(free0)} to {gb(free1)}.
        </p>
      </div>
      <dl className="vd-co-stats">
        <div>
          <dt className="vd-fade">items deleted</dt>
          <dd data-fly="deleted">{formatCount(run.deletedItems)}</dd>
        </div>
        <div>
          <dt className="vd-fade">files moved</dt>
          <dd data-fly="moved">{formatCount(run.movedItems)}</dd>
        </div>
        <div>
          <dt className="vd-fade">start to finish</dt>
          <dd data-fly="took">{duration(run.durationSec)}</dd>
        </div>
      </dl>
      <div className="vd-co-actions vd-fade">
        <button type="button" className="va-cta" onClick={onMap}>
          <MapIcon size={15} /> See it on the map
        </button>
      </div>
    </div>
  );
}

/**
 * Flies each `[data-fly]` number in the slides onto its `[data-fly-target]` twin in the summary panel. Both use the
 * same markup, so a translate plus a uniform scale (the font-size ratio) lands each glyph on its twin.
 */
export function flyNumbers(root: HTMLElement): Promise<void> {
  const flights: Promise<unknown>[] = [];
  const sources = [...root.querySelectorAll<HTMLElement>('[data-fly]')];
  // The hero leaves first; the stats leave right to left so the ones ahead never overtake the ones behind.
  sources.forEach((src, idx) => {
    const k = idx === 0 ? 0 : sources.length - idx;
    const dst = root.querySelector<HTMLElement>(`[data-fly-target="${src.dataset.fly}"]`);
    if (!dst) return;
    const a = src.getBoundingClientRect();
    const b = dst.getBoundingClientRect();
    const s = parseFloat(getComputedStyle(dst).fontSize) / parseFloat(getComputedStyle(src).fontSize);
    const dx = b.left - a.left;
    const dy = b.top - a.top;
    const anim = src.animate(
      [
        { transform: 'translate(0, 0) scale(1)' },
        { transform: `translate(${dx}px, ${dy}px) scale(${s})` },
      ],
      { duration: k === 0 ? 980 : 860, delay: k === 0 ? 0 : 90 + k * 55, easing: 'cubic-bezier(0.65, 0, 0.25, 1)', fill: 'forwards' },
    );
    flights.push(anim.finished.catch(() => undefined));
  });
  return Promise.all(flights).then(() => undefined);
}

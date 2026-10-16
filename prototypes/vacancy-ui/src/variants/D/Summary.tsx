// PROTOTYPE variant D (copied from A "Atlas"): the payoff's resting screen, reached after the highlight slides. A before/after swipe map where cleared parcels light up as vacant lots,
// and a ledger of what the run did.
import { animate, motion, useMotionValue, useMotionValueEvent, useReducedMotion, useTransform } from 'motion/react';
import { Check, Play, RotateCcw, Signpost, TriangleAlert, Undo2 } from 'lucide-react';
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { formatCount, type Dataset } from '../../shared/data';
import type { RunResult } from '../../shared/run';
import { Legend } from './Legend';
import { MapView, useSize, type TileLook } from './MapView';
import { duration, gb, layoutAtlas, leafIdFor, tint, type AtlasModel, type Leaf, type Rect } from './model';

interface MapProps {
  model: AtlasModel;
  result: RunResult;
  hot: string | null;
  onHot: (id: string | null) => void;
  tipStatus: (l: Leaf) => ReactNode;
}

export function SummaryMap({ model, result, hot, onHot, tipStatus }: MapProps) {
  const [ref, size] = useSize<HTMLDivElement>();
  const reduce = useReducedMotion();
  const split = useMotionValue(0);
  const clip = useTransform(split, (v) => `inset(0 ${(1 - v) * 100}% 0 0)`);
  const left = useTransform(split, (v) => `${v * 100}%`);
  const knobLeft = useTransform(split, (v) => `clamp(18px, ${v * 100}%, calc(100% - 18px))`);
  const [side, setSide] = useState<'before' | 'after'>('before');
  const [lit, setLit] = useState('');

  const rects = useMemo(() => layoutAtlas(model, size.w, size.h, null), [model, size.w, size.h]);
  const freed = useMemo(() => {
    const m = new Map<string, { freed: number; planned: number }>();
    for (const s of result.steps) if (s.kind === 'delete') m.set(leafIdFor(s.id), { freed: s.freedBytes, planned: s.plannedBytes });
    return m;
  }, [result]);

  const relight = (v: number) => {
    if (!size.w) return;
    const ids = [...freed.keys()].filter((id) => {
      const r = rects.get(id);
      return r && (r.x + r.w * 0.5) / size.w <= v;
    });
    const key = ids.join(',');
    setLit((prev) => (prev === key ? prev : key));
    setSide(v < 0.5 ? 'before' : 'after');
  };
  useMotionValueEvent(split, 'change', relight);
  useEffect(() => relight(split.get()), [rects]);

  const ready = size.w > 0;
  useEffect(() => {
    if (!ready) return;
    split.set(0);
    const c = animate(split, 1, { duration: reduce ? 0 : 1.25, delay: reduce ? 0 : 0.2, ease: [0.5, 0, 0.3, 1] });
    return () => c.stop();
  }, [ready]);

  const go = (to: number) => animate(split, to, { duration: reduce ? 0 : 0.7, ease: [0.4, 0, 0.2, 1] });
  const litSet = new Set(lit ? lit.split(',') : []);

  const afterLook = (l: Leaf, rect: Rect): TileLook => {
    const f = freed.get(l.id);
    if (!f) return {};
    const on = litSet.has(l.id);
    const big = rect.w >= 92 && rect.h >= 58;
    const fits = rect.w >= 58 && rect.h >= 30;
    return {
      cls: `is-vacant ${on ? 'is-lit' : ''}`,
      fill: f.planned ? 1 - f.freed / f.planned : 0,
      label: fits ? (
        <div className="va-vacant">
          {big && <span className="va-vacant-word">Vacant</span>}
          <span className="va-vacant-num">{gb(f.freed)}</span>
          {big && <span className="va-vacant-was">{l.name}</span>}
        </div>
      ) : undefined,
    };
  };

  return (
    <>
      <div className="va-toolbar">
        <div className="va-seg" role="group" aria-label="Compare">
          <button type="button" className={side === 'before' ? 'is-on' : ''} onClick={() => go(0)}>
            Before
          </button>
          <button type="button" className={side === 'after' ? 'is-on' : ''} onClick={() => go(1)}>
            After
          </button>
        </div>
        <span className="va-toolbar-hint">Drag the line to compare.</span>
        <Legend mode="summary" bytesInView={model.totalBytes} w={size.w} h={size.h} />
      </div>
      <div className="va-stage" ref={ref}>
        <MapView model={model} width={size.w} height={size.h} focus={null} look={() => ({})} hot={hot} onHot={onHot} tipStatus={tipStatus} interactive clickable={() => false} />
        <motion.div className="va-after" style={{ clipPath: clip }}>
          <MapView
            model={model}
            width={size.w}
            height={size.h}
            focus={null}
            look={afterLook}
            hot={hot}
            onHot={onHot}
            tipStatus={(l) => {
              const f = freed.get(l.id);
              return f ? `Vacant now. ${gb(f.freed)} freed${f.freed < f.planned ? `, ${gb(f.planned - f.freed)} skipped` : ''}.` : tipStatus(l);
            }}
            freeLabel={`${gb(model.freeBytes)} + ${gb(result.freedBytes)} vacant`}
            interactive
            clickable={() => false}
          />
        </motion.div>
        <motion.div className="va-swipe" style={{ left }}>
          <span className="va-swipe-tag va-swipe-tag--l">After</span>
          <span className="va-swipe-tag va-swipe-tag--r">Before</span>
        </motion.div>
        <motion.button
          type="button"
          className="va-swipe-knob"
          style={{ left: knobLeft }}
          role="slider"
          aria-label="Before and after"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(split.get() * 100)}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            split.stop();
          }}
          onPointerMove={(e) => {
            const stage = e.currentTarget.closest('.va-stage') as HTMLElement;
            const box = stage.getBoundingClientRect();
            if (e.currentTarget.hasPointerCapture(e.pointerId)) split.set(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)));
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowLeft') split.set(Math.max(0, split.get() - 0.05));
            if (e.key === 'ArrowRight') split.set(Math.min(1, split.get() + 0.05));
          }}
        >
          <span />
        </motion.button>
      </div>
    </>
  );
}

interface PanelProps {
  data: Dataset;
  model: AtlasModel;
  result: RunResult;
  hot: string | null;
  onHot: (id: string | null) => void;
  onScanAgain: () => void;
  /** D: undo state is shared with the highlight slides, which offer the same Undo. */
  undo: Record<string, 'working' | 'done'>;
  onUndo: (reorgId: string) => void;
  onReplay: () => void;
}

export function SummaryPanel({ data, model, result, onHot, onScanAgain, undo, onUndo: runUndo, onReplay }: PanelProps) {
  const freeBefore = result.totalBytes - result.usedBefore;
  const freeAfter = result.totalBytes - result.usedAfter;
  const cats = Object.entries(result.freedByCategory)
    .filter(([, b]) => b > 0)
    .map(([id, b]) => ({ r: model.regionById.get(id), b }))
    .filter((x): x is { r: NonNullable<typeof x.r>; b: number } => !!x.r)
    .sort((a, b) => b.b - a.b);
  const maxCat = Math.max(1, ...cats.map((c) => c.r.bytes));
  const win = result.biggestWin;
  const winRegion = win ? model.regionById.get(win.categoryId) : undefined;
  const winProposal = win ? data.proposals.find((p) => p.id === win.id) : undefined;
  const hints = result.leftForYou.length ? result.leftForYou : data.proposals.filter((p) => p.action === 'hint');
  const [num, unit] = gb(result.freedBytes).split(/\s/);
  const [whole, frac] = num.split('.');

  return (
    <aside className="va-side va-summary" aria-label="What Vacancy did">
      <div className="va-side-scroll">
        <section className="va-hero">
          <h1 className="va-hero-num" data-fly-target="freed" aria-label={`${gb(result.freedBytes)} freed`}>
            {whole}
            {frac !== undefined && <span className="va-hero-frac">.{frac}</span>}
            <span className="va-hero-unit">{unit}</span>
          </h1>
          <p className="va-hero-sub">
            back on {data.disk.name}. Free space went from {gb(freeBefore)} to {gb(freeAfter)}
            {freeAfter / freeBefore >= 1.5 ? `, ${(freeAfter / freeBefore).toFixed(1)} times the room you started with.` : '.'}
          </p>
        </section>

        <section className="va-diskba" aria-label="Disk before and after">
          <DiskBar label="Before" total={result.totalBytes} used={result.usedBefore} freed={0} />
          <DiskBar label="After" total={result.totalBytes} used={result.usedAfter} freed={result.freedBytes} />
        </section>

        <dl className="va-stats">
          <div>
            <dt>items deleted</dt>
            <dd data-fly-target="deleted">{formatCount(result.deletedItems)}</dd>
          </div>
          <div>
            <dt>files moved</dt>
            <dd data-fly-target="moved">{formatCount(result.movedItems)}</dd>
          </div>
          <div>
            <dt>start to finish</dt>
            <dd data-fly-target="took">{duration(result.durationSec)}</dd>
          </div>
        </dl>

        {win && (
          <section className="va-sec">
            <h3>Biggest win</h3>
            <button type="button" className="va-win" onMouseEnter={() => onHot(leafIdFor(win.id))} onMouseLeave={() => onHot(null)} onFocus={() => onHot(leafIdFor(win.id))} onBlur={() => onHot(null)}>
              <span className="va-win-num">{gb(win.freedBytes)}</span>
              <span className="va-win-text">
                <b>{win.title}</b>
                <span>
                  {winRegion?.name}
                  {winProposal?.regenerable ? '. Comes back on its own if you ever need it.' : '.'}
                </span>
              </span>
            </button>
          </section>
        )}

        {cats.length > 0 && (
          <section className="va-sec">
            <h3>Freed by category</h3>
            <ul className="va-cats">
              {cats.map(({ r, b }) => (
                <li key={r.id}>
                  <span className="va-cat-name">
                    <i style={{ background: tint(r.hue, 0.62, 0.1) }} />
                    {r.name}
                  </span>
                  <span className="va-cat-num">{gb(b)}</span>
                  <span className="va-cat-bar" style={{ width: `${(r.bytes / maxCat) * 100}%` }}>
                    <i className="va-cat-kept" style={{ width: `${((r.bytes - b) / r.bytes) * 100}%`, background: tint(r.hue, 0.42, 0.06) }} />
                    <i className="va-cat-freed" style={{ width: `${(b / r.bytes) * 100}%` }} />
                  </span>
                  <span className="va-cat-ba">
                    {gb(r.bytes, 0)} to {gb(r.bytes - b)}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        )}

        {result.undoable.length > 0 && (
          <section className="va-sec">
            <h3>Reorganized</h3>
            <ul className="va-undo">
              {result.undoable.map((u) => {
                const r = data.reorgs.find((x) => x.id === u.reorgId);
                const st = undo[u.reorgId];
                return (
                  <li key={u.reorgId}>
                    <span className="va-undo-text">
                      <b>{r?.title ?? u.folder}</b>
                      <span>
                        {st === 'done' ? `All ${formatCount(u.moves)} files are back where they were.` : `${formatCount(u.moves)} files moved in ${u.folder}. Every move is recorded.`}
                      </span>
                    </span>
                    {st === 'done' ? (
                      <span className="va-undone">
                        <Check size={13} /> Undone
                      </span>
                    ) : (
                      <button type="button" className="va-btn va-btn--sm" disabled={st === 'working'} onClick={() => runUndo(u.reorgId)}>
                        <Undo2 size={13} /> {st === 'working' ? 'Moving back' : 'Undo'}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        )}

        {result.skipped.length > 0 && (
          <section className="va-sec">
            <h3>Skipped</h3>
            <ul className="va-skips">
              {result.skipped.map((k) => (
                <li key={k.path}>
                  <TriangleAlert size={13} />
                  <span>
                    <b>{k.reason}.</b> {gb(k.bytes)} stayed. It goes next time if the app is closed.
                    <code>{k.path}</code>
                  </span>
                </li>
              ))}
            </ul>
          </section>
        )}

        {hints.length > 0 && (
          <section className="va-sec">
            <h3>Left for you</h3>
            {!result.leftForYou.length && <p className="va-sec-note">Not part of this run, but it's the biggest thing still on the map.</p>}
            <ul className="va-hints">
              {hints.map((h) => (
                <li key={h.id} onMouseEnter={() => onHot(leafIdFor(h.id))} onMouseLeave={() => onHot(null)}>
                  <Signpost size={14} />
                  <span>
                    <b>{h.title}</b>
                    {h.summary}
                  </span>
                  <span className="va-hint-num">{gb(h.bytes, 0)}</span>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
      <footer className="va-summary-foot vd-summary-foot">
        <button type="button" className="vd-replay" onClick={onReplay}>
          <Play size={12} strokeWidth={2.4} /> Replay highlights
        </button>
        <button type="button" className="va-btn" onClick={onScanAgain}>
          <RotateCcw size={14} /> Scan again
        </button>
      </footer>
    </aside>
  );
}

function DiskBar({ label, total, used, freed }: { label: string; total: number; used: number; freed: number }) {
  const pct = (b: number) => `${(b / total) * 100}%`;
  return (
    <div className="va-diskbar">
      <span className="va-diskbar-label">{label}</span>
      <span className="va-diskbar-track">
        <i className="va-diskbar-used" style={{ width: pct(used) }} />
        {freed > 0 && <i className="va-diskbar-freed" style={{ width: pct(freed) }} />}
      </span>
      <span className="va-diskbar-num">{gb(total - used)} free</span>
    </div>
  );
}

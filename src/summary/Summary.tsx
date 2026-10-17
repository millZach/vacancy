import { FolderInput, Info, Play, RotateCcw, Signpost, TriangleAlert, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import type { RunReport, Skip, StepReport } from '../bridge';
import { formatBytes, formatCount, formatDuration } from '../format';
import { Legend } from '../map/Legend';
import { MapView, type TileLook } from '../map/MapView';
import { layoutAtlas, type AtlasModel, type Leaf, type Rect } from '../map/model';
import { useSize } from '../map/useSize';
import { tileOf } from '../plan/totals';
import { HintActionButton } from '../plan/HintActionButton';
import { asSentence, neverReached } from '../run/progress';
import { biggestWin, categoryOf, count, keptOf, remindersOnly } from '../wrapped/deck';
import { tokenMs } from '../wrapped/motion';
import { HeroNumber } from './HeroNumber';
import { formatWhen } from './when';
import './summary.css';

interface MapProps {
  /** The map the run started from. */
  model: AtlasModel;
  report: RunReport;
  hot: string | null;
  onHot: (id: string | null) => void;
}

/** Skips listed one by one; the rest are counted. */
const SKIPS_SHOWN = 12;

const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
const reducedMotion = () => window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

/**
 * Variant D's resting summary map: the map before the run with the map after it laid over it, split by a line that
 * sweeps from Before to After when it opens and can be dragged back. On the After side each cleared tile is vacant
 * ground with a neon "Vacant" sign and what it freed; the signs catch as the line passes them.
 */
export function SummaryMap({ model, report, hot, onHot }: MapProps) {
  const [stageRef, size] = useSize<HTMLDivElement>();
  const knob = useRef<HTMLButtonElement>(null);
  const split = useRef(0);
  const raf = useRef(0);
  const [lit, setLit] = useState('');
  const [side, setSide] = useState<'before' | 'after'>('before');

  /** What each cleared tile freed, by tile id. */
  const freed = useMemo(() => {
    const m = new Map<string, StepReport>();
    for (const s of report.steps) if (s.freedBytes > 0) m.set(tileOf(s.id), s);
    return m;
  }, [report]);
  const rects = useMemo(() => layoutAtlas(model, size.w, size.h, null), [model, size.w, size.h]);

  const apply = useCallback(
    (v: number) => {
      split.current = v;
      stageRef.current?.style.setProperty('--split', String(v));
      knob.current?.setAttribute('aria-valuenow', String(Math.round(v * 100)));
      if (!size.w) return;
      const on = [...freed.keys()].filter((id) => {
        const r = rects.get(id);
        return r && (r.x + r.w / 2) / size.w <= v;
      });
      const key = on.join(',');
      setLit((was) => (was === key ? was : key));
      setSide(v < 0.5 ? 'before' : 'after');
    },
    [freed, rects, size.w, stageRef],
  );
  const stop = () => cancelAnimationFrame(raf.current);
  const glide = useCallback(
    (to: number, ms: number, delay = 0) => {
      cancelAnimationFrame(raf.current);
      if (reducedMotion()) return apply(to);
      const from = split.current;
      const start = performance.now() + delay;
      // Timed by the clock it started from, not the frame's timestamp, which runs on another clock (devtools can
      // slow it down).
      const step = () => {
        const t = Math.max(0, Math.min(1, (performance.now() - start) / ms));
        apply(from + (to - from) * ease(t));
        if (t < 1) raf.current = requestAnimationFrame(step);
      };
      raf.current = requestAnimationFrame(step);
    },
    [apply],
  );

  // Once the map has a size, sweep to After. A new size keeps the line where it is and relights.
  const ready = size.w > 0;
  const swept = useRef(false);
  useEffect(() => {
    if (!ready) return;
    if (swept.current) {
      apply(split.current);
      return;
    }
    swept.current = true;
    glide(1, tokenMs('--summary-sweep-ms'), tokenMs('--summary-sweep-delay-ms'));
  }, [ready, apply, glide]);
  useEffect(() => () => cancelAnimationFrame(raf.current), []);

  const litSet = useMemo(() => new Set(lit ? lit.split(',') : []), [lit]);
  const afterLook = (l: Leaf, rect: Rect): TileLook => {
    const s = freed.get(l.id);
    if (!s) return {};
    const big = rect.w >= 92 && rect.h >= 58;
    const fits = rect.w >= 58 && rect.h >= 30;
    return {
      cls: litSet.has(l.id) ? 'is-vacant is-lit' : 'is-vacant',
      fill: l.bytes > 0 ? 1 - s.freedBytes / l.bytes : 0,
      label: fits ? (
        <div className="vacant">
          {big && <span className="vacant-word">Vacant</span>}
          <span className="vacant-num">{formatBytes(s.freedBytes)}</span>
          {big && <span className="vacant-was">{l.name}</span>}
        </div>
      ) : undefined,
    };
  };
  const afterStatus = (l: Leaf) => {
    const s = freed.get(l.id);
    if (!s) return null;
    const stayed = Math.max(0, l.bytes - s.freedBytes);
    return `Vacant now. ${formatBytes(s.freedBytes)} freed${stayed >= 1e6 ? `, ${formatBytes(stayed)} stayed` : ''}.`;
  };

  const drag = (e: PointerEvent<HTMLButtonElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId) || !stageRef.current) return;
    const box = stageRef.current.getBoundingClientRect();
    if (box.width > 0) apply(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)));
  };
  const key = (e: KeyboardEvent) => {
    const to =
      e.key === 'ArrowLeft'
        ? split.current - 0.05
        : e.key === 'ArrowRight'
          ? split.current + 0.05
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? 1
              : null;
    if (to === null) return;
    e.preventDefault();
    stop();
    apply(Math.min(1, Math.max(0, to)));
  };
  return (
    <>
      <div className="atlas-toolbar">
        <div className="seg" role="group" aria-label="Compare">
          <button type="button" className={side === 'before' ? 'is-on' : undefined} onClick={() => glide(0, tokenMs('--summary-glide-ms'))}>
            Before
          </button>
          <button type="button" className={side === 'after' ? 'is-on' : undefined} onClick={() => glide(1, tokenMs('--summary-glide-ms'))}>
            After
          </button>
        </div>
        <span className="toolbar-hint">Drag the line to compare.</span>
        <Legend bytesInView={model.totalBytes} w={size.w} h={size.h} vacant />
      </div>
      <div className="atlas-stage summary-stage" ref={stageRef}>
        <div className="summary-before">
          <MapView model={model} width={size.w} height={size.h} focus={null} interactive hot={hot} onHot={onHot} />
        </div>
        <div className="summary-after">
          <MapView
            model={model}
            width={size.w}
            height={size.h}
            focus={null}
            interactive
            hot={hot}
            onHot={onHot}
            look={afterLook}
            leafStatus={afterStatus}
            freeLabel={`${formatBytes(model.freeBytes)} + ${formatBytes(report.freedBytes)} vacant`}
          />
        </div>
        <div className="swipe" aria-hidden>
          <span className="swipe-tag swipe-tag--l">After</span>
          <span className="swipe-tag swipe-tag--r">Before</span>
        </div>
        <button
          ref={knob}
          type="button"
          className="swipe-knob"
          role="slider"
          aria-label="Before and after"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={0}
          aria-valuetext={side === 'after' ? 'After' : 'Before'}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            stop();
          }}
          onPointerMove={drag}
          onKeyDown={key}
        >
          <span />
        </button>
      </div>
    </>
  );
}

interface PanelProps {
  /** The run, as it was kept. Every figure here comes from it, the categories from the map it started from. */
  report: RunReport;
  /** "Windows" or "macOS", for the free-space cross-check. */
  os: string;
  onHot: (id: string | null) => void;
  /** Plays the highlight slides again. */
  onReplay: () => void;
  /** The drive's own summary: scan it again. */
  onScanAgain?: () => void;
  /** A run reopened from the history: back to where the user was. Its date shows at the top. */
  onClose?: () => void;
}

/**
 * Variant D's summary panel: what the run freed and on which drive, free space before and after (and, when it
 * differs, what the drive itself reports), the counts, the biggest win, what each category got back, what was
 * moved, what was left in place and why, what was deleted but still holds its space, and the advice left for the
 * user. The freed figure and the counts are where the checkout slide's numbers land. Replay highlights plays the
 * slides again.
 */
export function SummaryPanel({ report: r, os, onHot, onReplay, onScanAgain, onClose }: PanelProps) {
  const [allSkips, setAllSkips] = useState(false);
  const [allNotes, setAllNotes] = useState(false);
  const known = r.totalBytes > 0;
  const growth = r.freeBefore > 0 ? r.freeAfter / r.freeBefore : 0;
  const differs = known && formatBytes(r.freeChange) !== formatBytes(r.freedBytes);
  const ran = r.steps.filter((s) => !neverReached(s.deletedItems, s.skipped));
  const name = (id: StepReport['category']) => categoryOf(r, id)?.name ?? id;

  const win = biggestWin(r);
  const cats = r.freedByCategory
    .filter((c) => c.bytes > 0)
    .map((c) => ({ ...c, before: categoryOf(r, c.id)?.bytes ?? c.bytes }));
  const widest = Math.max(1, ...cats.map((c) => c.before));

  const { left, stopped, notes } = keptOf(r);
  const stoppedSteps = [...new Set(r.steps.filter((s) => s.skipped.some((k) => k.kind === 'cancelled')).map((s) => s.title))];
  const stoppedBytes = stopped.reduce((s, k) => s + k.bytes, 0);

  return (
    <aside className="tray summary" aria-label="What Vacancy did" data-no-ripple>
      <div className="summary-scroll">
        <section className="summary-hero">
          {onClose && <p className="summary-when">Run of {formatWhen(r.startedAt)}</p>}
          <h1
            className="summary-num"
            aria-label={`${formatBytes(r.freedBytes)} freed`}
            data-fly-target="freed"
            tabIndex={-1}
          >
            <HeroNumber bytes={r.freedBytes} />
          </h1>
          <p className="summary-sub">
            {remindersOnly(r)
              ? `${r.leftForYou.length} ${r.leftForYou.length === 1 ? 'reminder' : 'reminders'} saved for you. No files were deleted or moved.`
              : `back on ${r.volumeName}.`}
            {known &&
              ` Free space went from ${formatBytes(r.freeBefore)} to ${formatBytes(r.freeAfter)}${
                growth >= 1.5 ? `, ${growth.toFixed(1)} times the room you started with.` : '.'
              }`}
          </p>
          {r.cancelled && (
            <p className="summary-note is-stopped">
              You stopped the run. {ran.length} of {r.steps.length} {r.steps.length === 1 ? 'step' : 'steps'} ran, and
              what it hadn’t reached is still on the drive.
            </p>
          )}
          {differs && (
            <p className="summary-note">
              {r.freeChange >= 0
                ? `${os} reports ${formatBytes(r.freeChange)} more free space. Other apps write to the drive too, so the two rarely match.`
                : `${os} reports ${formatBytes(-r.freeChange)} less free space than before: other apps wrote more than this run freed.`}
            </p>
          )}
        </section>

        {known && (
          <section className="diskba" aria-label="Disk before and after">
            <DiskBar label="Before" total={r.totalBytes} used={r.usedBefore} freed={0} free={r.freeBefore} />
            <DiskBar label="After" total={r.totalBytes} used={r.usedAfter} freed={r.freedBytes} free={r.freeAfter} />
          </section>
        )}

        <dl className="stats">
          <div>
            <dt>items deleted</dt>
            <dd data-fly-target="deleted">{formatCount(r.deletedItems)}</dd>
          </div>
          <div>
            <dt>files moved</dt>
            <dd data-fly-target="moved">{formatCount(r.movedItems)}</dd>
          </div>
          <div>
            <dt>start to finish</dt>
            <dd data-fly-target="took">{formatDuration(r.durationMs)}</dd>
          </div>
        </dl>

        {win && (
          <section className="summary-sec">
            <h3>Biggest win</h3>
            <div
              className="win"
              onMouseEnter={() => onHot(tileOf(win.id))}
              onMouseLeave={() => onHot(null)}
            >
              <span className="win-num">{formatBytes(win.freedBytes)}</span>
              <span className="win-text">
                <b>{win.title}</b>
                <span>
                  {name(win.category)}.{win.regenerable && ' Comes back on its own if you ever need it.'}
                </span>
              </span>
            </div>
          </section>
        )}

        {cats.length > 0 && (
          <section className="summary-sec">
            <h3>Freed by category</h3>
            <ul className="cats">
              {cats.map((c) => (
                <li key={c.id} data-cat={c.id}>
                  <span className="cat-name">
                    <i aria-hidden />
                    {name(c.id)}
                  </span>
                  <span className="cat-num">{formatBytes(c.bytes)}</span>
                  <span className="cat-bar" style={{ width: `${(c.before / widest) * 100}%` }} aria-hidden>
                    <i className="cat-kept" style={{ width: `${Math.max(0, (c.before - c.bytes) / c.before) * 100}%` }} />
                    <i className="cat-freed" style={{ width: `${Math.min(1, c.bytes / c.before) * 100}%` }} />
                  </span>
                  <span className="cat-ba">
                    {formatBytes(c.before, 0)} to {formatBytes(Math.max(0, c.before - c.bytes))}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        )}

        {r.reorganized.length > 0 && (
          <section className="summary-sec">
            <h3>Reorganized</h3>
            <ul className="moves">
              {r.reorganized.map((m) => (
                <li key={m.id}>
                  <FolderInput size={13} aria-hidden />
                  <span>
                    <b>{m.folder}.</b> {count(m.movedItems, 'file')} moved. Every move is recorded.
                  </span>
                </li>
              ))}
            </ul>
          </section>
        )}

        {(left.length > 0 || stopped.length > 0) && (
          <section className="summary-sec">
            <h3>Skipped</h3>
            <ul className="skips">
              {(allSkips ? left : left.slice(0, SKIPS_SHOWN)).map((k, i) => (
                <li key={`${k.path}:${i}`}>
                  <TriangleAlert size={13} aria-hidden />
                  <span>
                    <b>{asSentence(k.reason)}</b> {stayed(k)}
                    {hint(k)}
                    <code>{k.path}</code>
                  </span>
                </li>
              ))}
              {left.length > SKIPS_SHOWN && (
                <li className="skips-more">
                  <span />
                  <button type="button" className="replay" aria-expanded={allSkips} onClick={() => setAllSkips(!allSkips)}>
                    {allSkips ? 'Show fewer skipped places' : `Show all ${formatCount(left.length)} skipped places`}
                  </button>
                </li>
              )}
              {stopped.length > 0 && (
                <li>
                  <TriangleAlert size={13} aria-hidden />
                  <span>
                    <b>You stopped the run.</b> {formatBytes(stoppedBytes)} stayed, in {list(stoppedSteps)}.
                  </span>
                </li>
              )}
            </ul>
          </section>
        )}

        {notes.length > 0 && (
          <section className="summary-sec">
            <h3>Deleted, space still in use</h3>
            <ul className="skips is-notes">
              {(allNotes ? notes : notes.slice(0, SKIPS_SHOWN)).map((k, i) => (
                <li key={`${k.path}:${i}`}>
                  <Info size={13} aria-hidden />
                  <span>
                    {asSentence(k.reason)}
                    <code>{k.path}</code>
                  </span>
                </li>
              ))}
              {notes.length > SKIPS_SHOWN && (
                <li className="skips-more">
                  <span />
                  <button type="button" className="replay" aria-expanded={allNotes} onClick={() => setAllNotes(!allNotes)}>
                    {allNotes ? 'Show fewer space notes' : `Show all ${formatCount(notes.length)} space notes`}
                  </button>
                </li>
              )}
            </ul>
          </section>
        )}

        {r.leftForYou.length > 0 && (
          <section className="summary-sec">
            <h3>Left for you</h3>
            <ul className="advice">
              {r.leftForYou.map((a) => (
                <li key={a.id} onMouseEnter={() => onHot(tileOf(a.id))} onMouseLeave={() => onHot(null)}>
                  <Signpost size={14} aria-hidden />
                  <span>
                    <b>{a.title}</b>
                    {a.summary}
                    <span className="advice-why">{a.why}</span>
                    {a.hintAction?.kind === 'steamUninstall' && <HintActionButton mountPoint={r.mountPoint} proposalId={a.id} />}
                  </span>
                  {a.bytes > 0 && <span className="advice-num">{formatBytes(a.bytes, 0)}</span>}
                </li>
              ))}
            </ul>
            <p className="summary-note">Tips aren’t counted in freed space. Vacancy can’t do them for you.</p>
          </section>
        )}
      </div>
      <div className="summary-foot">
        <button type="button" className="replay" onClick={onReplay}>
          <Play size={12} strokeWidth={2.4} aria-hidden /> Replay highlights
        </button>
        {onClose ? (
          <button type="button" className="btn" onClick={onClose}>
            <X size={14} aria-hidden /> Close
          </button>
        ) : (
          onScanAgain && (
            <button type="button" className="btn" onClick={onScanAgain}>
              <RotateCcw size={14} aria-hidden /> Scan again
            </button>
          )
        )}
      </div>
    </aside>
  );
}

/** What stayed of a skip: "410 MB stayed." */
function stayed(k: Skip): string {
  return k.bytes > 0 ? `${formatBytes(k.bytes)} stayed.` : 'It stayed.';
}

/** What to do about a skip, when there is something. */
function hint(k: Skip): string {
  if (k.kind === 'inUse') return ' It can go next time, once nothing has it open.';
  if (k.kind === 'changed') return ' Scan again to see it as it is now.';
  return '';
}

/** "A", "A and B", "A, B and 3 more". */
function list(names: string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  if (names.length === 2) return `${names[0]} and ${names[1]}`;
  if (names.length === 3) return `${names[0]}, ${names[1]} and ${names[2]}`;
  return `${names[0]}, ${names[1]} and ${names.length - 2} more`;
}

function DiskBar({ label, total, used, freed, free }: { label: string; total: number; used: number; freed: number; free: number }) {
  const pct = (b: number) => `${Math.max(0, Math.min(1, b / total)) * 100}%`;
  return (
    <div className="diskbar">
      <span className="diskbar-label">{label}</span>
      <span className="diskbar-track" aria-hidden>
        <i className="diskbar-used" style={{ width: pct(used) }} />
        {freed > 0 && <i className="diskbar-freed" style={{ width: pct(Math.min(freed, total - used)) }} />}
      </span>
      <span className="diskbar-num">{formatBytes(free)} free</span>
    </div>
  );
}

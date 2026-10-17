// The seven highlight slides. Each reads only the run's report: its figures, its steps, and the map the run started
// from (category names and sizes). The checkout's numbers use the summary panel's own markup, so they can fly there.
import { Info, Map as MapIcon, Signpost, TriangleAlert } from 'lucide-react';
import type { CSSProperties } from 'react';
import type { MapCategory, RunReport } from '../bridge';
import { formatBytes, formatCount, formatDuration, NBSP } from '../format';
import { asSentence } from '../run/progress';
import { HeroNumber } from '../summary/HeroNumber';
import { KeyFob, Letterboard, NeonSign } from './bits';
import {
  biggestWin,
  boardText,
  categoryOf,
  count,
  diskSub,
  headlineSub,
  keptOf,
  share,
} from './deck';
import '../map/map.css';

/** Skips a slide lists one by one; the summary lists them all. */
const SKIPS_SHOWN = 4;
const NOTES_SHOWN = 2;

interface SlideProps {
  report: RunReport;
}

/** 1. The neon sign, with what was freed on the letterboard under it. */
export function FreedSlide({ report: r }: SlideProps) {
  return (
    <div className="sl-freed">
      <h1 className="sr-only">
        {formatBytes(r.freedBytes)} freed on {r.volumeName}
      </h1>
      <NeonSign />
      <Letterboard lines={[boardText(r.freedBytes), 'FREED']} label={`${formatBytes(r.freedBytes)} freed`} />
      <p className="sl-lead">{headlineSub(r)}</p>
    </div>
  );
}

/** The used categories for the disk bars: the ones Vacancy may clean, biggest first, then the locked ones. */
function segments(r: RunReport): MapCategory[] {
  return [...r.map.categories].sort((a, b) => Number(a.protected) - Number(b.protected) || b.bytes - a.bytes);
}

/** 2. Free space before and after, as the drive's bars by category, the freed space a vacant lot. */
export function DiskSlide({ report: r }: SlideProps) {
  const pct = (b: number) => `${(Math.max(0, b) / r.totalBytes) * 100}%`;
  const freed = new Map(r.freedByCategory.map((c) => [c.id, c.bytes]));
  const cats = segments(r);
  const bar = (after: boolean) => (
    <div className={after ? 'dbar is-after' : 'dbar'}>
      <span className="dbar-label">{after ? 'After' : 'Before'}</span>
      <span className="dbar-track">
        {cats.map((c, k) => {
          const now = after ? c.bytes - (freed.get(c.id) ?? 0) : c.bytes;
          return (
            <i
              key={c.id}
              data-cat={c.id}
              className={c.protected ? 'is-locked' : undefined}
              style={{ '--from': pct(c.bytes), width: pct(now), '--k': k } as CSSProperties}
              title={`${c.name}: ${formatBytes(Math.max(0, now))}${c.protected ? ' (protected)' : ''}`}
            />
          );
        })}
        {after && (
          <span className="dbar-vacant">
            {r.freedBytes > 0 && <em>+{formatBytes(r.freedBytes)}</em>}
          </span>
        )}
        <span className="dbar-free" style={after ? { flex: 'none', width: pct(r.freeBefore) } : undefined} />
      </span>
      <span className="dbar-val">
        <b>{formatBytes(after ? r.freeAfter : r.freeBefore)}</b> free
      </span>
    </div>
  );
  return (
    <div className="sl">
      <h1 className="sl-title">
        Free space went from {formatBytes(r.freeBefore)} to {formatBytes(r.freeAfter)}.
      </h1>
      <div
        className="dbars"
        role="img"
        aria-label={`Before the run: ${formatBytes(r.freeBefore)} free. After it: ${formatBytes(r.freeAfter)} free.`}
      >
        {bar(false)}
        {bar(true)}
      </div>
      <p className="sl-sub">{diskSub(r)}</p>
    </div>
  );
}

/** 3. The biggest win, on a motel key fob. */
export function WinSlide({ report: r }: SlideProps) {
  const w = biggestWin(r);
  if (!w) {
    return (
      <div className="sl">
        <h1 className="sl-title">{r.deletedItems > 0 ? 'Items were deleted without freeing space.' : 'Nothing was deleted this time.'}</h1>
        <p className="sl-sub">
          {r.deletedItems > 0
            ? `${count(r.deletedItems, 'item')} deleted. Their space may still be in use, or the items held no allocated space.`
            : r.movedItems > 0
              ? 'Folders were reorganized. The next slides show what moved.'
            : r.steps.length === 0 && r.leftForYou.length > 0
              ? 'Your reminders are saved under Left for you. Vacancy hasn’t touched those items.'
            : 'Everything in the plan stayed where it was. The next slides say why.'}
        </p>
      </div>
    );
  }
  const [num = '', unit = ''] = formatBytes(w.freedBytes).split(NBSP);
  return (
    <div className="sl sl-win">
      <KeyFob num={num} unit={unit} foot={categoryOf(r, w.category)?.name ?? ''} />
      <div className="win-words">
        <h1 className="sl-title">Your biggest win: {w.title}.</h1>
        <p className="sl-sub">
          {formatBytes(w.freedBytes)} from {count(w.deletedItems, 'item')}, {share(w.freedBytes, r.freedBytes)}% of
          everything Vacancy freed.{w.regenerable ? ' It comes back on its own if you ever need it.' : ''}
        </p>
        {w.why && <p className="win-why">{w.why}</p>}
      </div>
    </div>
  );
}

/** 4. What each category got back. */
export function CategoriesSlide({ report: r }: SlideProps) {
  const rows = r.freedByCategory.filter((c) => c.bytes > 0);
  const most = Math.max(1, ...rows.map((c) => c.bytes));
  return (
    <div className="sl">
      <h1 className="sl-title">Where the {formatBytes(r.freedBytes)} came from.</h1>
      {rows.length > 0 ? (
        <ul className="chart" aria-label="Freed space by category">
          {rows.map((c, k) => (
            <li key={c.id} data-cat={c.id} style={{ '--k': k } as CSSProperties}>
              <span className="chart-name">
                <i aria-hidden />
                {categoryOf(r, c.id)?.name ?? c.id}
              </span>
              <span className="chart-track" aria-hidden>
                <i style={{ width: `${(c.bytes / most) * 100}%` }} />
              </span>
              <span className="chart-val">{formatBytes(c.bytes)}</span>
              <span className="chart-pct">{share(c.bytes, r.freedBytes)}%</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="sl-sub">No category freed any disk space.</p>
      )}
    </div>
  );
}

/** 5. Folders the run reorganized. Shown only when it moved something. */
export function MovedSlide({ report: r }: SlideProps) {
  return (
    <div className="sl">
      <h1 className="sl-title">{count(r.movedItems, 'file')} found a home.</h1>
      <p className="sl-sub">Every move was recorded, so each one can be undone.</p>
      <ul className="moved">
        {r.reorganized.map((m) => (
          <li key={m.id} className="moved-card">
            <b>{m.folder}</b>
            <span>{count(m.movedItems, 'file')} moved</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 6. What stayed, and why; what was deleted but still holds its space; and the advice left for the user. */
export function KeptSlide({ report: r }: SlideProps) {
  const { left, stopped, notes } = keptOf(r);
  const skipped = left.length > 0 || stopped.length > 0;
  const stoppedBytes = stopped.reduce((s, k) => s + k.bytes, 0);
  const stoppedItems = stopped.reduce((s, k) => s + k.items, 0);
  const inUse = left.some((k) => k.kind === 'inUse');
  const changed = left.some((k) => k.kind === 'changed');
  return (
    <div className="sl">
      <h1 className="sl-title">
        {skipped || r.leftForYou.length > 0 ? 'A few things stayed put.' : 'Nothing was skipped.'}
      </h1>
      <div className="kept">
        <section aria-label="Skipped">
          <h2>Skipped, and why</h2>
          {skipped ? (
            <ul>
              {left.slice(0, SKIPS_SHOWN).map((k, i) => (
                <li key={`${k.path}:${i}`}>
                  <TriangleAlert size={14} aria-hidden />
                  <div>
                    <b>{asSentence(k.reason)}</b>
                    <code title={k.path}>{k.path}</code>
                  </div>
                  <em>{formatBytes(k.bytes)}</em>
                </li>
              ))}
              {left.length > SKIPS_SHOWN && (
                <li className="kept-more">And {formatCount(left.length - SKIPS_SHOWN)} more, listed in the summary.</li>
              )}
              {stopped.length > 0 && (
                <li>
                  <TriangleAlert size={14} aria-hidden />
                  <div>
                    <b>You stopped the run.</b>
                    <span>{count(stoppedItems, 'file')} weren’t reached, and are still on the drive.</span>
                  </div>
                  <em>{formatBytes(stoppedBytes)}</em>
                </li>
              )}
            </ul>
          ) : (
            <p className="kept-none">{r.steps.length === 0 ? 'No files were selected for deletion.' : 'Every file in the plan was free to go.'}</p>
          )}
          {(inUse || changed) && (
            <p className="kept-hint">
              {inUse && 'Files in use can go next time, once nothing has them open.'}
              {inUse && changed && ' '}
              {changed && 'Scan again to see the changed files as they are now.'}
            </p>
          )}
          {notes.length > 0 && (
            <ul className="kept-notes" aria-label="Deleted, space still in use">
              {notes.slice(0, NOTES_SHOWN).map((k, i) => (
                <li key={`${k.path}:${i}`}>
                  <Info size={13} aria-hidden />
                  <span>
                    Deleted. {asSentence(k.reason)}
                    <code title={k.path}>{k.path}</code>
                  </span>
                </li>
              ))}
              {notes.length > NOTES_SHOWN && (
                <li className="kept-more">And {formatCount(notes.length - NOTES_SHOWN)} more, in the summary.</li>
              )}
            </ul>
          )}
        </section>
        <section aria-label="Left for you">
          <h2>Left for you</h2>
          {r.leftForYou.length > 0 ? (
            <>
              <ul>
                {r.leftForYou.map((a) => (
                  <li key={a.id} className="is-advice">
                    <Signpost size={14} aria-hidden />
                    <div>
                      <b>{a.title}</b>
                      <span>{a.why}</span>
                    </div>
                    {a.bytes > 0 && <em>{formatBytes(a.bytes)}</em>}
                  </li>
                ))}
              </ul>
              <p className="kept-hint">Tips aren’t counted in freed space. Vacancy can’t do them for you.</p>
            </>
          ) : (
            <p className="kept-none">Nothing in the plan was left for you to do.</p>
          )}
        </section>
      </div>
    </div>
  );
}

/** 7. The receipt. Its numbers fly into the summary panel's when it hands off to the map. */
export function CheckoutSlide({ report: r, onMap }: SlideProps & { onMap: () => void }) {
  return (
    <div className="sl sl-checkout">
      <h1 className="sl-title wrap-fade">That’s checkout for {r.volumeName}.</h1>
      <div className="co-hero">
        <span className="summary-num co-num" data-fly="freed" aria-label={`${formatBytes(r.freedBytes)} freed`}>
          <HeroNumber bytes={r.freedBytes} />
        </span>
        <p className="co-sub wrap-fade">
          freed.
          {r.totalBytes > 0 &&
            ` Free space went from ${formatBytes(r.freeBefore)} to ${formatBytes(r.freeAfter)}.`}
        </p>
      </div>
      <dl className="co-stats">
        <div>
          <dt className="wrap-fade">items deleted</dt>
          <dd data-fly="deleted">{formatCount(r.deletedItems)}</dd>
        </div>
        <div>
          <dt className="wrap-fade">files moved</dt>
          <dd data-fly="moved">{formatCount(r.movedItems)}</dd>
        </div>
        <div>
          <dt className="wrap-fade">start to finish</dt>
          <dd data-fly="took">{formatDuration(r.durationMs)}</dd>
        </div>
      </dl>
      <div className="co-actions wrap-fade">
        <button type="button" className="cta cta--lg" onClick={onMap}>
          <MapIcon size={15} aria-hidden /> See it on the map
        </button>
      </div>
    </div>
  );
}

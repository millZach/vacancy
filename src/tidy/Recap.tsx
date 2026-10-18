import { FolderInput, Info, TriangleAlert } from 'lucide-react';
import type { Reorganized, RunReport, Skip } from '../bridge';
import { formatCount } from '../format';
import { asSentence } from '../run/progress';
import { movedLine, sortSkips, stayedOf } from './model';
import { UndoButton, UndoResult, type OnUndo } from './Undo';
import './tidy.css';

/**
 * The summary's "Reorganized" section: each folder the run sorted, what moved and what stayed (with why), its Undo,
 * and once Undo has run, what came back. Moves free nothing, and the section says so.
 */
export function ReorganizedSection({ report: r, onUndo }: { report: RunReport; onUndo?: OnUndo | undefined }) {
  if (r.reorganized.length === 0) return null;
  const undoable = r.reorganized.some((m) => m.journal !== '');
  return (
    <section className="summary-sec reorg-recap" aria-label="Reorganized">
      <h3>
        Reorganized <span className="sec-aside">moved, not deleted · 0 B freed</span>
      </h3>
      {r.reorganized.map((m) => (
        <div key={m.id} className="recap-reorg">
          <div className="rr-head">
            <FolderInput size={14} aria-hidden />
            <span className="rr-text">
              <b>{m.folder}.</b> {movedLine(m)}
            </span>
            {onUndo && <UndoButton record={m} onUndo={onUndo} />}
          </div>
          {m.note && !m.undo && (
            <p className="rr-note">
              <Info size={13} aria-hidden /> {m.note}
            </p>
          )}
          <SortSkips skips={sortSkips(r, m.id)} />
          <UndoResult record={m} />
        </div>
      ))}
      {undoable && <p className="summary-note">Every move was recorded. Undo also lives in Past runs.</p>}
    </section>
  );
}

/** What stayed during a folder's sort, each with why and where. */
function SortSkips({ skips }: { skips: Skip[] }) {
  if (skips.length === 0) return null;
  return (
    <ul className="skips run-skips">
      {skips.map((k, i) => (
        <li key={`${k.path}:${i}`}>
          <TriangleAlert size={13} aria-hidden />
          <span>
            <b>{asSentence(k.reason)}</b> {k.kind === 'cancelled' ? 'They stayed where they were.' : 'It stayed where it was.'}
            <code>{k.path}</code>
          </span>
        </li>
      ))}
    </ul>
  );
}

/** One folder's card on the Moved slide: where its files went, how many stayed, its Undo and what Undo did. */
export function MovedCard({ report, record: m, onUndo }: { report: RunReport; record: Reorganized; onUndo?: OnUndo | undefined }) {
  const stayed = stayedOf(sortSkips(report, m.id));
  return (
    <li className="moved-card">
      <b>{m.folder}</b>
      <span>
        {formatCount(m.movedItems)} {m.movedItems === 1 ? 'file' : 'files'}
        {m.into.length > 0 ? ` into ${formatCount(m.into.length)} ${m.into.length === 1 ? 'folder' : 'folders'}` : ' moved'}
        {stayed > 0 && `, ${formatCount(stayed)} stayed`}
      </span>
      {m.into.length > 0 && <span className="moved-dests">{m.into.join(' · ')}</span>}
      {onUndo && (
        <div className="moved-undo">
          <UndoButton record={m} onUndo={onUndo} />
        </div>
      )}
      <UndoResult record={m} />
    </li>
  );
}

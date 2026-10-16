// PROTOTYPE variant C: the single confirmation. A short typed-out summary and a plain permanent-deletion warning.
import { useEffect, useRef, useState } from 'react';
import { TriangleAlert } from 'lucide-react';
import type { Dataset } from '../../shared/data';
import { binName, gb, n, type PlanTotals } from './model';

export function Confirm({ data, totals, onCancel, onRun }: { data: Dataset; totals: PlanTotals; onCancel: () => void; onRun: () => void }) {
  const locked = data.categories.filter((c) => c.protected).map((c) => c.name);
  const lines = [
    totals.delCount ? `Permanently delete ${gb(totals.delBytes)}: ${n(totals.delItems)} items in ${totals.delCount} places.` : 'Delete nothing.',
    totals.reorgCount ? `Move ${n(totals.moveCount)} files in ${totals.folders.join(' and ')}. Each move can be undone.` : null,
    totals.hints.length ? `Add ${totals.hints.length} tip${totals.hints.length === 1 ? '' : 's'} to your list: ${totals.hints.map((h) => h.title).join(', ')}.` : null,
    `Skip anything in use. Never touch ${locked.join(', ').replace(/, ([^,]*)$/, ' or $1')}.`,
  ].filter(Boolean) as string[];
  const full = lines.join('\n');
  const [shown, setShown] = useState(0);
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
    let i = 0;
    const id = window.setInterval(() => {
      i += 3;
      setShown(Math.min(full.length, i));
      if (i >= full.length) clearInterval(id);
    }, 16);
    return () => clearInterval(id);
  }, [full]);

  let budget = shown;
  return (
    <div
      className="vc-modal-back"
      onKeyDown={(e) => {
        if (e.key === 'Escape') onCancel();
      }}
    >
      <div className="vc-modal" role="alertdialog" aria-modal="true" aria-labelledby="vc-confirm-title" aria-describedby="vc-confirm-warn">
        <h2 id="vc-confirm-title">Run this plan on {data.disk.name}?</h2>
        <ol className="vc-typed" aria-label={full}>
          {lines.map((l, i) => {
            const s = l.slice(0, Math.max(0, budget));
            budget -= l.length + 1;
            const typing = s.length < l.length && s.length > 0;
            return (
              <li key={i} className={s.length ? '' : 'wait'}>
                {s}
                {typing && <i className="vc-caret-blink" />}
                <span className="rest">{l.slice(s.length)}</span>
              </li>
            );
          })}
        </ol>
        {totals.delCount > 0 ? (
          <div className="vc-warn" id="vc-confirm-warn">
            <TriangleAlert size={18} strokeWidth={2.2} />
            <p>
              <b>
                {gb(totals.delBytes)} in {n(totals.delItems)} items will be deleted permanently.
              </b>{' '}
              They skip the {binName(data)} and can't be recovered. Folder moves are journaled, so you can undo them from the summary.
            </p>
          </div>
        ) : (
          <div className="vc-warn calm" id="vc-confirm-warn">
            <p>Nothing will be deleted. Folder moves are journaled, so you can undo them from the summary.</p>
          </div>
        )}
        <div className="vc-modal-actions">
          <button className="vc-btn vc-btn-ghost" ref={cancelRef} onClick={onCancel}>
            Go back
          </button>
          <button className="vc-btn vc-btn-danger" onClick={onRun}>
            {totals.delCount ? `Delete ${gb(totals.delBytes)} and run` : 'Run plan'}
          </button>
        </div>
      </div>
    </div>
  );
}

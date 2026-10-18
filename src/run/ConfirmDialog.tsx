import { useEffect, useId, useRef, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { PlanSummary, Platform, Skip } from '../bridge';
import { formatBytes, formatCount } from '../format';
import './run.css';

interface Props {
  /** The plan as the core made it: every number here comes from it. */
  plan: PlanSummary;
  /** Which bin deleted files skip: the Recycle Bin or the Trash. */
  platform: Platform;
  onCancel: () => void;
  onGo: () => void;
}

/** Skips that mean the place is not what the scan saw. */
const CHANGED: ReadonlySet<Skip['kind']> = new Set(['changed', 'gone', 'link', 'otherVolume']);

/**
 * Variant D's one confirmation before a plan runs: what will be deleted for good, from how many places, and that it
 * skips the bin. Tidy up's moves (which free nothing and can be undone from the recap or Past runs) and places
 * that changed since the scan are said quietly. "Go back" has
 * the focus; Escape and the scrim go back too.
 */
export function ConfirmDialog({ plan, platform, onCancel, onGo }: Props) {
  const title = useId();
  const box = useRef<HTMLDivElement>(null);
  const bin = platform === 'macos' ? 'Trash' : 'Recycle Bin';
  const deletes = plan.bytes > 0 || plan.items > 0;
  const size = formatBytes(plan.bytes);
  const reminders = plan.leftForYou.length;
  const n = plan.refused.length;
  const allChanged = plan.refused.every((r) => CHANGED.has(r.kind));

  // Escape goes back, before anything else hears it (the map would zoom out).
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      onCancel();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onCancel]);

  // Focus goes back where it was when the dialog closes.
  useEffect(() => {
    const before = document.activeElement;
    return () => {
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  }, []);

  // Tab stays inside the dialog.
  const trap = (e: KeyboardEvent) => {
    if (e.key !== 'Tab' || !box.current) return;
    const buttons = [...box.current.querySelectorAll<HTMLButtonElement>('button')];
    const first = buttons[0];
    const last = buttons.at(-1);
    if (!first || !last) return;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  return createPortal(
    <div className="scrim" onClick={onCancel} data-no-ripple>
      <div
        ref={box}
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={title}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={trap}
      >
        <h2 id={title}>
          {deletes
            ? `Permanently delete ${size}?`
            : plan.moves > 0
              ? `Move ${formatCount(plan.moves)} ${plan.moves === 1 ? 'file' : 'files'}?`
              : reminders > 0 ? `Save ${reminders} ${reminders === 1 ? 'reminder' : 'reminders'}?` : 'Nothing left to delete'}
        </h2>
        {deletes && (
          <p className="dialog-lead">
            {formatCount(plan.items)} {plan.items === 1 ? 'item' : 'items'} in {formatCount(plan.places)}{' '}
            {plan.places === 1 ? 'place' : 'places'} will be deleted for good. They skip the {bin}, so they can’t be
            brought back.
          </p>
        )}
        {plan.moves > 0 && (
          <p className={deletes ? 'dialog-quiet' : undefined}>
            {formatCount(plan.moves)} {plan.moves === 1 ? 'file' : 'files'} will be moved into folders, which frees no
            space. Every move is recorded, so you can undo it from the recap or Past runs.
          </p>
        )}
        {reminders > 0 && <p className="dialog-quiet">{reminders} {reminders === 1 ? 'reminder will' : 'reminders will'} be saved under Left for you. Vacancy won’t touch these items or count them as freed space.</p>}
        {!deletes && plan.moves === 0 && reminders === 0 && (
          <p>Everything in the plan changed since the scan. Scan again to see the drive as it is now.</p>
        )}
        {n > 0 && (deletes || plan.moves > 0 || reminders > 0) && (
          <p className="dialog-quiet" title={plan.refused.map((r) => r.path).join('\n')}>
            {n === 1 ? '1 place' : `${formatCount(n)} places`}{' '}
            {allChanged
              ? `changed since the scan and will be left alone.`
              : `changed since the scan or can’t be touched, and will be left alone.`}
          </p>
        )}
        <div className="dialog-actions">
          {/* Focus starts on the safe answer. */}
          <button type="button" className="btn" onClick={onCancel} autoFocus>
            Go back
          </button>
          {(deletes || plan.moves > 0 || reminders > 0) && (
            <button type="button" className={deletes ? 'cta cta--danger' : 'cta'} onClick={onGo}>
              {deletes ? `Delete ${size} permanently` : plan.moves > 0 ? `Move ${formatCount(plan.moves)} files` : 'Save reminders'}
            </button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

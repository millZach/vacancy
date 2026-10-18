import { LoaderCircle, RotateCcw, TriangleAlert, Undo2 } from 'lucide-react';
import { useState } from 'react';
import type { Reorganized, Skip } from '../bridge';
import { errorMessage } from '../errors';
import { formatCount } from '../format';
import { canUndo, listOf } from './model';
import './tidy.css';

/** Undoes one folder's Tidy up of a kept run. Resolves once the run's report shows what Undo did. */
export type OnUndo = (tidyId: string) => Promise<unknown>;

interface ButtonProps {
  record: Reorganized;
  onUndo: OnUndo;
  /** The small button of the recap, the slides and Past runs. */
  small?: boolean;
  /** Inside the Past runs menu: a menu item. */
  menuItem?: boolean;
}

/**
 * Undo for one folder, and what became of it: "Undo Downloads" while it can run, a spinner while it does, then
 * "Moved back" (or "Moved back · 2 stayed"). Nothing at all for a folder with nothing to undo and no Undo yet.
 */
export function UndoButton({ record: r, onUndo, small = true, menuItem = false }: ButtonProps) {
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stuck = r.undo?.stuck.length ?? 0;
  if (r.undo && !canUndo(r)) {
    return (
      <span className={stuck ? 'undone is-partial' : 'undone'}>
        {stuck ? `Moved back · ${formatCount(stuck)} stayed` : 'Moved back'}
      </span>
    );
  }
  if (!canUndo(r)) return null;
  const go = () => {
    setWorking(true);
    setError(null);
    onUndo(r.id).then(
      () => setWorking(false),
      (e: unknown) => {
        setWorking(false);
        setError(errorMessage(e));
      },
    );
  };
  return (
    <span className="undo-control">
      <button
        type="button"
        className={small ? 'btn btn--sm' : 'btn'}
        disabled={working}
        onClick={go}
        aria-label={`Undo ${r.folder}`}
        {...(menuItem ? { role: 'menuitem', tabIndex: -1 } : {})}
      >
        {working ? <LoaderCircle size={13} className="tray-spin" aria-hidden /> : <Undo2 size={13} aria-hidden />}
        {working ? 'Moving back' : r.undo ? 'Try again' : `Undo ${r.folder}`}
      </button>
      {error && (
        <span className="undo-error" role="alert">
          {error}
        </span>
      )}
    </span>
  );
}

/** Files that couldn't go back, grouped by why and by the folder they are still in. */
function stuckGroups(stuck: readonly Skip[], base: string) {
  const groups: { reason: string; where: string; names: string[] }[] = [];
  for (const k of stuck) {
    const rel = k.path.startsWith(`${base}\\`) ? k.path.slice(base.length + 1) : k.path;
    const at = rel.lastIndexOf('\\');
    const where = at > 0 ? rel.slice(0, at) : '';
    const name = at >= 0 ? rel.slice(at + 1) : rel;
    const g = groups.find((x) => x.reason === k.reason && x.where === where);
    if (g) g.names.push(name);
    else groups.push({ reason: k.reason, where, names: [name] });
  }
  return groups;
}

/** A reason as it reads after "couldn't go back:": "a file now has its old name". */
const lower = (reason: string) => reason.charAt(0).toLowerCase() + reason.slice(1).replace(/\.$/, '');

/** What Undo did for a folder: what came back, what couldn't and why, and the empty folders it removed. */
export function UndoResult({ record: r }: { record: Reorganized }) {
  const u = r.undo;
  if (!u) return null;
  const tried = u.back + u.stuck.length;
  const groups = stuckGroups(u.stuck, r.path);
  return (
    <div className="undo-result" role="status">
      <p className="undo-head">
        <RotateCcw size={13} aria-hidden />
        {u.stuck.length === 0
          ? `${u.back === 1 ? 'The file is' : `All ${formatCount(u.back)} files are`} back in ${r.folder}.`
          : `${formatCount(u.back)} of ${formatCount(tried)} files are back in ${r.folder}.`}
      </p>
      {groups.map((g) => (
        <p className="undo-warn" key={`${g.reason}:${g.where}`}>
          <TriangleAlert size={13} aria-hidden />
          <span>
            <b>
              {g.names.length === 1 ? '1 file' : `${formatCount(g.names.length)} files`} couldn’t go back: {lower(g.reason)}.
            </b>{' '}
            {g.names.length === 1 ? 'It’s' : 'They’re'} still in {g.where ? `${r.folder}\\${g.where}` : r.folder}: {g.names.join(', ')}.
          </span>
        </p>
      ))}
      {(u.removed.length > 0 || u.kept.length > 0) && (
        <p className="undo-quiet">
          {u.removed.length > 0 &&
            `Removed the ${u.removed.length === 1 ? 'empty folder' : `${formatCount(u.removed.length)} empty folders`} Vacancy made.`}
          {u.kept.length > 0 &&
            ` Kept ${listOf(u.kept)}, which still ${u.kept.length === 1 ? 'holds' : 'hold'} ${u.stuck.length === 1 ? 'that file' : 'those files'}.`}
        </p>
      )}
      {r.note && <p className="undo-quiet">{r.note}</p>}
    </div>
  );
}

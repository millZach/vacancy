import { ArrowRight, ChevronRight, Undo2 } from 'lucide-react';
import type { TidyProposal } from '../bridge';
import { formatCount } from '../format';
import { Checkbox, SourceTag } from '../plan/bits';
import { activeMoves, goingInPlan, kindFolders } from './model';
import './tidy.css';

interface Props {
  /** The drive's Tidy up proposals (none on macOS, and none on a drive without the known folders). */
  tidy: TidyProposal[];
  /** The ids in the drive's plan. */
  plan: string[];
  /** The proposal whose drawer is open, if one is. */
  open: string | null;
  onToggle: (id: string) => void;
  /** Opens (or closes) a proposal's drawer, with its before and after trees. */
  onOpen: (id: string | null) => void;
  /** The folder's map tile, if the map has one, so a row and its tile light together. */
  tileOf: (t: TidyProposal) => string | null;
  hot: string | null;
  onHot: (tile: string | null) => void;
}

/**
 * The plan tray's "Tidy up" group, below Delete: one row per folder the built-in rules can sort, with a tick box
 * (they start out of the plan), how many of its loose files move, a small before and after, and "Undo any time".
 * The title or the before and after opens the drawer. Moves free nothing, and the group says so.
 */
export function TidyGroup({ tidy, plan, open, onToggle, onOpen, tileOf, hot, onHot }: Props) {
  if (tidy.length === 0) return null;
  const on = tidy.filter((t) => plan.includes(t.id)).length;
  return (
    <section className="tray-group" aria-label="Tidy up">
      <h3>
        Tidy up{' '}
        <span>
          {on} of {tidy.length}
        </span>
      </h3>
      <p className="tray-group-note">Sorts loose files into folders. Moves only: nothing is deleted and no space is freed.</p>
      {tidy.map((t) => {
        const tile = tileOf(t);
        return (
          <TidyRow
            key={t.id}
            tidy={t}
            plan={plan}
            on={plan.includes(t.id)}
            open={open === t.id}
            hot={!!tile && hot === tile}
            onToggle={() => onToggle(t.id)}
            onOpen={() => onOpen(open === t.id ? null : t.id)}
            onHot={(h) => onHot(h ? tile : null)}
          />
        );
      })}
    </section>
  );
}

interface RowProps {
  tidy: TidyProposal;
  plan: string[];
  on: boolean;
  open: boolean;
  hot: boolean;
  onToggle: () => void;
  onOpen: () => void;
  onHot: (hot: boolean) => void;
}

function TidyRow({ tidy: t, plan, on, open, hot, onToggle, onOpen, onHot }: RowProps) {
  const moves = activeMoves(t, plan).length;
  const going = goingInPlan(t, plan).length;
  const cls = ['tray-row', 'tidy-row', on && 'is-on', (hot || open) && 'is-hot'].filter(Boolean).join(' ');
  return (
    <div className={cls} onMouseEnter={() => onHot(true)} onMouseLeave={() => onHot(false)} onFocus={() => onHot(true)} onBlur={() => onHot(false)}>
      <Checkbox on={on} onChange={onToggle} label={`Include Sort ${t.name}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={onOpen} aria-expanded={open} aria-haspopup="dialog">
          <span>Sort {t.name}</span>
          <ChevronRight size={13} aria-hidden />
        </button>
        <p className="tray-row-sum">
          {formatCount(moves)} of {formatCount(t.loose)} loose {t.loose === 1 ? 'file' : 'files'}, sorted by kind and year.
          {going > 0 && ` ${formatCount(going)} ${going === 1 ? 'goes' : 'go'} in this plan’s deletes instead.`}
          {t.capped && (t.later > 0 ? ` ${formatCount(t.later)} more wait for a later run.` : ' Any more wait for a later run.')}
        </p>
        <MiniBeforeAfter tidy={t} plan={plan} onOpen={onOpen} />
        <div className="tray-row-meta">
          <SourceTag source="rule" agent={null} />
          <span className="source">
            <Undo2 size={11} aria-hidden /> Undo any time
          </span>
        </div>
      </div>
      <span className="tray-row-size is-files">
        {formatCount(moves)} {moves === 1 ? 'file' : 'files'}
      </span>
    </div>
  );
}

/** The row's small before and after: a pile of loose files, then bars for the folders they go into. */
function MiniBeforeAfter({ tidy: t, plan, onOpen }: { tidy: TidyProposal; plan: string[]; onOpen: () => void }) {
  const tops = kindFolders(activeMoves(t, plan));
  const shown = tops.slice(0, 4);
  const most = Math.max(1, ...shown.map(([, n]) => n));
  const dots = Math.min(28, Math.max(3, Math.round(t.loose / 7)));
  return (
    <button type="button" className="mini-ba" onClick={onOpen} aria-label={`Before and after: ${t.name}`}>
      <span className="mini-before">
        <span className="mini-pile" aria-hidden>
          {Array.from({ length: dots }, (_, i) => (
            <i key={i} style={{ left: `${(i * 37) % 86}%`, top: `${(i * 53) % 78}%` }} />
          ))}
        </span>
        <span className="mini-cap">{formatCount(t.loose)} loose</span>
      </span>
      <ArrowRight size={13} className="mini-arrow" aria-hidden />
      <span className="mini-after">
        {shown.map(([name, n]) => (
          <span key={name} className="mini-folder">
            <span className="mini-name">{name}</span>
            <span className="mini-bar">
              <i style={{ width: `${(n / most) * 100}%` }} />
            </span>
            <span className="mini-n">{formatCount(n)}</span>
          </span>
        ))}
        {tops.length > shown.length && (
          <span className="mini-more">
            +{tops.length - shown.length} more {tops.length - shown.length === 1 ? 'folder' : 'folders'}
          </span>
        )}
      </span>
    </button>
  );
}

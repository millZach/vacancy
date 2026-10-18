import { Copy, LoaderCircle } from 'lucide-react';
import { useMemo } from 'react';
import type { DuplicateGroup } from '../bridge';
import type { DuplicateSearch } from '../drives/session';
import { formatBytes, formatCount } from '../format';
import { layoutAtlas, type AtlasModel, type Rect } from './model';
import './duplicates.css';

interface PillProps {
  search: DuplicateSearch | null;
  groups: readonly DuplicateGroup[];
  /** The duplicates panel is open. */
  open: boolean;
  onToggle: () => void;
}

/**
 * The duplicate search on the map's toolbar (variant B): a quiet pill with its progress while it reads in the
 * background (the map and the plan stay usable), then a button with what it found, which opens the duplicates
 * panel. Says so quietly when it found nothing, and when it couldn't finish.
 */
export function DuplicatesPill({ search, groups, open, onToggle }: PillProps) {
  if (!search || search.phase === 'stopped') return null;
  if (search.phase === 'checking') {
    const pct = Math.max(0, Math.min(100, search.progress?.percent ?? 0));
    return (
      <span
        className="dup-pill"
        role="progressbar"
        aria-label="Finding duplicates"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
      >
        <LoaderCircle size={12} className="dup-pill-spin" aria-hidden />
        Finding duplicates <b>{pct}%</b>
        <span className="dup-pill-bar" aria-hidden>
          <i style={{ width: `${pct}%` }} />
        </span>
      </span>
    );
  }
  if (search.phase === 'failed')
    return (
      <span className="dup-pill is-quiet" title={search.message}>
        Couldn’t check for duplicates
      </span>
    );
  if (groups.length === 0) return <span className="dup-pill is-quiet">No duplicate files</span>;
  const bytes = groups.reduce((s, g) => s + g.freed, 0);
  return (
    <button
      type="button"
      className={open ? 'dup-pill is-ready is-open' : 'dup-pill is-ready'}
      onClick={onToggle}
      aria-pressed={open}
    >
      <Copy size={12} aria-hidden />
      {formatCount(groups.length)} duplicate {groups.length === 1 ? 'group' : 'groups'} · <b>{formatBytes(bytes)}</b>
    </button>
  );
}

interface LinksProps {
  /** The map as it is drawn, at the whole disk. */
  model: AtlasModel;
  width: number;
  height: number;
  /** The group whose links to draw. */
  group: DuplicateGroup;
}

/**
 * A group's links on the map: from the tile of the folder where the kept copy lives to the duplicates tile of each
 * folder holding an extra copy, as dashed arcs. Laid out as the map lays out its tiles.
 */
export function DuplicateLinks({ model, width, height, group }: LinksProps) {
  const rects = useMemo(() => layoutAtlas(model, width, height, null), [model, width, height]);
  const from = rects.get(group.keep.tile);
  if (!from || width <= 0) return null;
  const c = centre(from);
  const to = [...new Set(group.extras.map((e) => e.tile))]
    .map((id) => ({ id, rect: rects.get(id) }))
    .filter((t): t is { id: string; rect: Rect } => !!t.rect);
  return (
    <svg className="dup-links" width={width} height={height} aria-hidden data-group={group.id}>
      {to.map(({ id, rect }) => {
        const t = centre(rect);
        const mx = (c.x + t.x) / 2;
        const my = Math.min(c.y, t.y) - Math.min(60, Math.abs(c.x - t.x) / 3 + 24);
        return (
          <g key={id} data-to={id}>
            <path d={`M${c.x},${c.y} Q${mx},${my} ${t.x},${t.y}`} />
            <circle cx={t.x} cy={t.y} r={4} />
          </g>
        );
      })}
      <circle className="is-keep" cx={c.x} cy={c.y} r={5} />
    </svg>
  );
}

const centre = (r: Rect) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 });

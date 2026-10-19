import { Lock } from 'lucide-react';
import {
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from 'react';
import { ago, formatBytes, percentOf } from '../format';
import { layoutAtlas, regionKey, type AtlasModel, type Leaf, type Rect, type Region } from './model';
import './map.css';

export interface MapViewProps {
  model: AtlasModel;
  /**
   * The map `model` was made from, holding the same tiles at other sizes (the map a run started from). Laid out
   * against it, every tile keeps its place there and only grows or shrinks, gliding at the pace of the run's events.
   */
  base?: AtlasModel;
  width: number;
  height: number;
  /** The region filling the map, or null for the whole disk. */
  focus: string | null;
  /** Hover cards, clicks and keyboard. Off while scanning. */
  interactive: boolean;
  /** The tile or region header under the pointer (or focused), by data-tile id. */
  hot: string | null;
  onHot: (id: string | null) => void;
  onLeaf?: (leaf: Leaf) => void;
  onRegion?: (region: Region) => void;
  /** Whether a tile reacts to clicks. */
  clickable?: (leaf: Leaf) => boolean;
  /** The last line of a tile's hover card ("Click to open."), if any. */
  leafAction?: (leaf: Leaf) => string | null;
  /** The last line of a region header's hover card, if any. */
  regionAction?: (region: Region) => string | null;
  /** How a tile looks beyond its kind: extra classes (reclaimable, selected, draining) and a badge in its corner. */
  look?: (leaf: Leaf, rect: Rect) => TileLook;
  /** What a tile's hover card says under its numbers, in place of `leafAction` (a proposal's risk and summary). */
  leafStatus?: (leaf: Leaf) => ReactNode;
  /** The plan tray is open: reclaimable tiles are being picked. */
  planning?: boolean;
  /** What the free-space tile says in place of its size (while a run frees space, the free space it will have). */
  freeLabel?: ReactNode;
}

/** A tile's extra look, and what assistive tech hears about it. */
export interface TileLook {
  cls?: string;
  /**
   * How full the tile is, 0..1. Below 1 its colour drains from the top and shows the vacant ground under it.
   * By default, the share of the tile its vacant ground leaves.
   */
  fill?: number;
  /** In place of the tile's name and size (a "Vacant" sign). */
  label?: ReactNode;
  /** In place of the tile's size under its name (what a cleared tile freed). */
  size?: ReactNode;
  badge?: ReactNode;
  /** Said after the tile's name and size ("reclaimable, in the plan"). */
  state?: string;
  /** For tiles that go in or out of something on click: whether they are in. */
  pressed?: boolean;
}

const LOCKED = 'Protected. Vacancy never changes anything here.';

/**
 * The space map: regions (categories, or an opened folder) with a header band, tiles inside them, and free space.
 * Tiles glide to their new places when the layout changes (zooming, filling in during a scan), and while a run frees
 * space they shrink where they stand as the free space grows (`base`); colours come from the category hues in the
 * design tokens.
 */
export function MapView(p: MapViewProps) {
  const rects = useMemo(
    () => layoutAtlas(p.model, p.width, p.height, p.focus, p.base),
    [p.model, p.width, p.height, p.focus, p.base],
  );
  const [tip, setTip] = useState<{ id: string; x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  const onMove = (e: PointerEvent) => {
    if (!p.interactive || !ref.current) return;
    const el = (e.target as HTMLElement).closest<HTMLElement>('[data-tile]');
    const id = el?.dataset.tile ?? null;
    if (!id) {
      if (tip) setTip(null);
      if (p.hot) p.onHot(null);
      return;
    }
    const box = ref.current.getBoundingClientRect();
    setTip({ id, x: e.clientX - box.left, y: e.clientY - box.top });
    if (id !== p.hot) p.onHot(id);
  };
  const onLeave = () => {
    setTip(null);
    p.onHot(null);
  };
  const press = (e: KeyboardEvent, go: () => void) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      go();
    }
  };

  return (
    <div
      ref={ref}
      className={['map', p.planning && 'is-planning', p.base && 'is-freeing'].filter(Boolean).join(' ')}
      onPointerMove={onMove}
      onPointerLeave={onLeave}
    >
      {p.model.regions.map((r) => {
        const rect = rects.get(regionKey(r.id));
        if (!rect || r.pending) return null;
        const shown = !p.focus || p.focus === r.id;
        const cls = ['map-region', r.free && 'is-free', r.protected && 'is-locked'].filter(Boolean).join(' ');
        return (
          <div key={r.id} className={cls} data-cat={r.category ?? undefined} data-region={r.id} style={place(rect, shown)}>
            {!r.free && rect.w > 40 && (
              <button
                type="button"
                className="map-region-head"
                data-tile={regionKey(r.id)}
                tabIndex={p.interactive && shown && p.onRegion ? 0 : -1}
                disabled={!p.interactive}
                onClick={() => p.onRegion?.(r)}
                onFocus={() => p.onHot(regionKey(r.id))}
              >
                {r.protected && <Lock size={11} strokeWidth={2.2} aria-hidden />}
                <span className="map-region-name">{r.name}</span>
                {rect.w > 150 && <span className="map-region-size">{formatBytes(r.bytes, 0)}</span>}
              </button>
            )}
          </div>
        );
      })}

      {p.model.leaves.map((l) => {
        const rect = rects.get(l.id);
        const r = p.model.regionById.get(l.regionId);
        if (!rect || !r || r.pending) return null;
        const shown = !p.focus || p.focus === l.regionId;
        const canClick = p.interactive && shown && (p.clickable?.(l) ?? false);
        const look = p.look?.(l, rect) ?? {};
        const fill = look.fill ?? (l.vacant > 0 ? l.bytes / (l.bytes + l.vacant) : undefined);
        // A badge needs room; a narrow tile with one has room for its size only, its name is in the hover card.
        const badge = rect.w >= 40 && rect.h >= 26 ? look.badge : undefined;
        const compact = !!badge && rect.w < 92;
        const showName = rect.w >= 50 && rect.h >= 22 && l.kind !== 'scanned';
        const showSize = (compact ? rect.w >= 78 : rect.w >= 50 && rect.h >= 40) && l.kind !== 'scanned';
        const oneLine = rect.w < 96 || rect.h < 58;
        const cls = [
          'map-tile',
          `map-tile--${l.kind}`,
          r.protected && 'is-locked',
          p.hot === l.id && 'is-hot',
          canClick && 'is-clickable',
          badge && 'has-badge',
          look.cls,
        ]
          .filter(Boolean)
          .join(' ');
        return (
          <div
            key={l.id}
            data-tile={l.id}
            data-cat={r.category ?? undefined}
            className={cls}
            role={canClick ? 'button' : undefined}
            tabIndex={canClick ? 0 : -1}
            aria-label={
              canClick ? [l.name, formatBytes(l.bytes), look.state].filter(Boolean).join(', ') : undefined
            }
            aria-pressed={canClick ? look.pressed : undefined}
            onClick={canClick ? () => p.onLeaf?.(l) : undefined}
            onKeyDown={canClick ? (e) => press(e, () => p.onLeaf?.(l)) : undefined}
            onFocus={canClick ? () => p.onHot(l.id) : undefined}
            style={{ ...place(rect, shown), '--shade': l.shade } as CSSProperties}
          >
            {l.kind !== 'free' && (
              <div
                className="map-fill"
                style={fill === undefined ? undefined : { height: `${Math.max(0, Math.min(1, fill)) * 100}%` }}
              />
            )}
            {look.label ??
              (showName && (
                <div className={oneLine ? 'map-label is-1line' : 'map-label'}>
                  {!compact && <span className="map-name">{l.name}</span>}
                  {showSize && (
                    <span className="map-size">
                      {look.size ?? (l.kind === 'free' && p.freeLabel ? p.freeLabel : formatBytes(l.bytes))}
                    </span>
                  )}
                </div>
              ))}
            {badge}
          </div>
        );
      })}

      {tip && p.interactive && (
        <Tip
          model={p.model}
          id={tip.id}
          x={tip.x}
          y={tip.y}
          w={p.width}
          h={p.height}
          leafAction={p.leafAction}
          regionAction={p.regionAction}
          leafStatus={p.leafStatus}
        />
      )}
    </div>
  );
}

function place(rect: Rect, shown: boolean): CSSProperties {
  return { left: rect.x, top: rect.y, width: rect.w, height: rect.h, opacity: shown ? 1 : 0 };
}

interface TipProps {
  model: AtlasModel;
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  leafAction?: ((leaf: Leaf) => string | null) | undefined;
  regionAction?: ((region: Region) => string | null) | undefined;
  leafStatus?: ((leaf: Leaf) => ReactNode) | undefined;
}

const TIP_W = 300;
const TIP_H = 170;

/** The hover card: what a tile or region is, how big, when it was last used, and what clicking does. */
function Tip({ model, id, x, y, w, h, leafAction, regionAction, leafStatus }: TipProps) {
  const left = x + 18 + TIP_W > w ? Math.max(4, x - TIP_W - 14) : x + 18;
  const top = y + 18 + TIP_H > h ? Math.max(4, y - TIP_H) : y + 18;
  const style = { left, top, width: TIP_W };
  const pct = (bytes: number) => `${percentOf(bytes, model.totalBytes)}% of the disk`;

  if (id.startsWith('r:')) {
    const r = model.regionById.get(id.slice(2));
    if (!r) return null;
    const action = regionAction?.(r);
    return (
      <div className="map-tip" style={style} role="tooltip">
        <div className="map-tip-name">{r.name}</div>
        <div className="map-tip-row">
          <b>{formatBytes(r.bytes)}</b>
          <span>{pct(r.bytes)}</span>
        </div>
        {r.blurb && <p className="map-tip-blurb">{r.blurb}</p>}
        {(r.protected || action) && (
          <div className="map-tip-status">
            {r.protected ? <Locked /> : action}
          </div>
        )}
      </div>
    );
  }

  const l = model.leafById.get(id);
  const r = l && model.regionById.get(l.regionId);
  if (!l || !r) return null;
  const custom = !r.protected ? leafStatus?.(l) : null;
  const action = leafAction?.(l);
  const status =
    l.kind === 'free' ? 'Space you already have.' : r.protected || custom ? null : (l.note ?? action);
  return (
    <div className="map-tip" style={style} role="tooltip">
      <div className="map-tip-name">{l.name}</div>
      {l.path && <div className="map-tip-path">{l.path}</div>}
      <div className="map-tip-row">
        <b>{formatBytes(l.bytes)}</b>
        <span>{pct(l.bytes)}</span>
        {l.lastUsed !== null && <span>Last used {ago(l.lastUsed)}</span>}
      </div>
      {r.protected && l.kind !== 'free' && (
        <div className="map-tip-status">
          {l.note && <p className="map-tip-note">{l.note}</p>}
          <Locked />
        </div>
      )}
      {status && <div className="map-tip-status">{status}</div>}
      {custom && <div className="map-tip-status">{custom}</div>}
      {!r.free && (
        <div className="map-tip-region" data-cat={r.category ?? undefined}>
          <i aria-hidden />
          {r.categoryName}
        </div>
      )}
    </div>
  );
}

function Locked() {
  return (
    <span className="map-tip-lock">
      <Lock size={12} aria-hidden /> {LOCKED}
    </span>
  );
}

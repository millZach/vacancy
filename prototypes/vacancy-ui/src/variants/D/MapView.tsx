// PROTOTYPE variant D (copied from A "Atlas"): the space map. Regions are categories, parcels are items or proposals.
import { motion, useReducedMotion } from 'motion/react';
import { Lock } from 'lucide-react';
import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { ago, gb, layoutAtlas, regionKey, tint, type AtlasModel, type Leaf, type Rect, type Region } from './model';

export interface TileLook {
  cls?: string;
  /** Remaining fill, 0..1. Below 1 the parcel drains and shows the vacant ground under it. */
  fill?: number;
  /** Replaces the default name and size label. */
  label?: ReactNode;
  badge?: ReactNode;
}

export interface MapViewProps {
  model: AtlasModel;
  width: number;
  height: number;
  focus: string | null;
  look: (leaf: Leaf, rect: Rect) => TileLook;
  regionCls?: (r: Region) => string | undefined;
  /** Scan reveal: regions not yet discovered stay hidden. */
  visible?: (r: Region) => boolean;
  hot?: string | null;
  onHot?: (id: string | null) => void;
  onLeaf?: (leaf: Leaf) => void;
  onRegion?: (r: Region) => void;
  tipStatus?: (leaf: Leaf) => ReactNode;
  clickable?: (leaf: Leaf) => boolean;
  freeLabel?: ReactNode;
  interactive?: boolean;
  className?: string;
  style?: CSSProperties;
}

export function useSize<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      const w = Math.round(e.contentRect.width);
      const h = Math.round(e.contentRect.height);
      setSize((s) => (s.w === w && s.h === h ? s : { w, h }));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, size] as const;
}

export function MapView(p: MapViewProps) {
  const reduce = useReducedMotion();
  const rects = useMemo(() => layoutAtlas(p.model, p.width, p.height, p.focus), [p.model, p.width, p.height, p.focus]);
  const [tip, setTip] = useState<{ id: string; x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const move = reduce ? { duration: 0 } : ({ type: 'spring', stiffness: 150, damping: 24, mass: 0.9 } as const);

  const onMove = (e: React.PointerEvent) => {
    if (!p.interactive || !ref.current) return;
    const el = (e.target as HTMLElement).closest<HTMLElement>('[data-tile]');
    const id = el?.dataset.tile ?? null;
    if (!id) {
      if (tip) setTip(null);
      if (p.hot) p.onHot?.(null);
      return;
    }
    const box = ref.current.getBoundingClientRect();
    setTip({ id, x: e.clientX - box.left, y: e.clientY - box.top });
    if (id !== p.hot) p.onHot?.(id);
  };
  const onLeave = () => {
    setTip(null);
    p.onHot?.(null);
  };

  const leafIndex = new Map<string, number>();
  for (const r of p.model.regions) r.leaves.forEach((l, i) => leafIndex.set(l.id, i));

  return (
    <div
      ref={ref}
      className={`va-map ${p.className ?? ''}`}
      style={p.style}
      onPointerMove={onMove}
      onPointerLeave={onLeave}
    >
      {p.model.regions.map((r) => {
        const rect = rects.get(regionKey(r.id));
        if (!rect) return null;
        const shown = (p.visible ? p.visible(r) : true) && (!p.focus || p.focus === r.id);
        const cls = ['va-region', r.free && 'va-region--free', r.protected && 'is-protected', p.focus === r.id && 'is-focus', p.regionCls?.(r)]
          .filter(Boolean)
          .join(' ');
        return (
          <motion.div
            key={r.id}
            className={cls}
            style={{ background: r.free ? undefined : tint(r.hue, 0.215, 0.022), ['--va-h' as string]: r.hue } as CSSProperties}
            initial={false}
            animate={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h, opacity: shown ? 1 : 0 }}
            transition={{ default: move, opacity: { duration: reduce ? 0 : 0.35 } }}
          >
            {!r.free && rect.w > 40 && (
              <button
                type="button"
                className="va-region-head"
                data-tile={regionKey(r.id)}
                tabIndex={p.interactive && shown ? 0 : -1}
                onClick={() => p.onRegion?.(r)}
              >
                {r.protected && <Lock size={11} strokeWidth={2.2} />}
                <span className="va-region-name">{r.name}</span>
                {rect.w > 150 && <span className="va-region-size">{gb(r.bytes, 0)}</span>}
              </button>
            )}
          </motion.div>
        );
      })}

      {p.model.leaves.map((l) => {
        const rect = rects.get(l.id);
        if (!rect) return null;
        const r = p.model.regionById.get(l.regionId)!;
        const shown = (p.visible ? p.visible(r) : true) && (!p.focus || p.focus === l.regionId);
        const look = p.look(l, rect);
        const fill = look.fill ?? 1;
        const showName = rect.w >= 50 && rect.h >= 22;
        const showSize = rect.w >= 50 && rect.h >= 40;
        const oneLine = rect.w < 96 || rect.h < 58;
        // Narrow parcels with a pin or badge only have room for the size; the name is in the tooltip.
        const compact = !!look.badge && rect.w < 92;
        const canClick = p.interactive && shown && (p.clickable ? p.clickable(l) : true);
        const cls = [
          'va-tile',
          `va-tile--${l.kind}`,
          r.protected && 'is-protected',
          p.hot === l.id && 'is-hot',
          canClick && 'is-clickable',
          look.badge && 'has-badge',
          look.cls,
        ]
          .filter(Boolean)
          .join(' ');
        const idx = leafIndex.get(l.id) ?? 0;
        return (
          <motion.div
            key={l.id}
            data-tile={l.id}
            className={cls}
            role={canClick ? 'button' : undefined}
            tabIndex={canClick ? 0 : -1}
            aria-label={canClick ? `${l.name}, ${gb(l.bytes)}` : undefined}
            onClick={canClick ? () => p.onLeaf?.(l) : undefined}
            onKeyDown={canClick ? (e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), p.onLeaf?.(l)) : undefined}
            onFocus={canClick ? () => p.onHot?.(l.id) : undefined}
            initial={false}
            animate={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h, opacity: shown ? 1 : 0 }}
            transition={{ default: move, opacity: { duration: reduce ? 0 : 0.4, delay: shown && p.visible ? 0.12 + idx * 0.05 : 0 } }}
          >
            {l.kind !== 'free' && (
              <div
                className="va-fill"
                style={{
                  background: tint(r.hue, 0.3 + l.shade * 0.028, r.protected ? 0.02 : 0.05),
                  height: `${fill * 100}%`,
                }}
              />
            )}
            {look.label ??
              (showName && (
                <div className={`va-label ${oneLine ? 'is-1line' : ''}`}>
                  {!compact && <span className="va-name">{l.kind === 'free' ? 'Free space' : l.name}</span>}
                  {(compact ? rect.w >= 78 : showSize) && <span className="va-size">{l.kind === 'free' && p.freeLabel ? p.freeLabel : gb(l.bytes)}</span>}
                </div>
              ))}
            {look.badge}
          </motion.div>
        );
      })}

      {tip && p.interactive && <Tip model={p.model} id={tip.id} x={tip.x} y={tip.y} w={p.width} h={p.height} status={p.tipStatus} />}
    </div>
  );
}

function Tip({ model, id, x, y, w, h, status }: { model: AtlasModel; id: string; x: number; y: number; w: number; h: number; status?: (l: Leaf) => ReactNode }) {
  const TW = 300;
  const left = x + 18 + TW > w ? Math.max(4, x - TW - 14) : x + 18;
  const top = y + 18 + 170 > h ? Math.max(4, y - 170) : y + 18;
  const pct = (b: number) => {
    const v = (b / model.totalBytes) * 100;
    return v < 1 ? '<1' : v.toFixed(0);
  };

  if (id.startsWith('r:')) {
    const r = model.regionById.get(id.slice(2));
    if (!r) return null;
    return (
      <div className="va-tip" style={{ left, top, width: TW }}>
        <div className="va-tip-name">{r.name}</div>
        <div className="va-tip-row">
          <b>{gb(r.bytes)}</b>
          <span>{pct(r.bytes)}% of the disk</span>
        </div>
        <p className="va-tip-blurb">{r.blurb}</p>
        <div className="va-tip-status">
          {r.protected ? (
            <span className="va-tip-lock">
              <Lock size={12} /> Protected. Vacancy never changes anything here.
            </span>
          ) : (
            'Click to zoom in or out.'
          )}
        </div>
      </div>
    );
  }

  const l = model.leafById.get(id);
  if (!l) return null;
  const r = model.regionById.get(l.regionId)!;
  return (
    <div className="va-tip" style={{ left, top, width: TW }}>
      <div className="va-tip-name">{l.name}</div>
      {l.path && <div className="va-tip-path">{l.path}</div>}
      <div className="va-tip-row">
        <b>{gb(l.bytes)}</b>
        <span>{pct(l.bytes)}% of the disk</span>
        {l.lastUsedDays !== undefined && <span>Last used {ago(l.lastUsedDays)}</span>}
      </div>
      {status && <div className="va-tip-status">{status(l)}</div>}
      {!r.free && (
        <div className="va-tip-region">
          <i style={{ background: tint(r.hue, 0.62, 0.1) }} />
          {r.name}
        </div>
      )}
    </div>
  );
}

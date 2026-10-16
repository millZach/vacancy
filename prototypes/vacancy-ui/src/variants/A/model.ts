// PROTOTYPE variant A "Atlas": turns a dataset into map regions (categories) and parcels (items and proposals),
// and lays them out as a squarified treemap.
import { hierarchy, treemap, treemapSquarify } from 'd3-hierarchy';
import { formatBytes, type Dataset, type Proposal } from '../../shared/data';

export type LeafKind = 'reclaim' | 'hint' | 'item' | 'free';

export interface Leaf {
  id: string;
  regionId: string;
  kind: LeafKind;
  name: string;
  path: string;
  bytes: number;
  lastUsedDays?: number;
  proposal?: Proposal;
  /** What is left of an item after a proposal was carved out of it. */
  rest?: boolean;
  /** 0..2, varies lightness between neighbouring parcels. */
  shade: number;
}

export interface Region {
  id: string;
  name: string;
  bytes: number;
  hue: number;
  protected: boolean;
  blurb: string;
  free: boolean;
  leaves: Leaf[];
}

export interface AtlasModel {
  regions: Region[];
  leaves: Leaf[];
  leafById: Map<string, Leaf>;
  regionById: Map<string, Region>;
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
  reclaimableBytes: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const HEADER = 24;
export const FREE_ID = 'free';
export const leafIdFor = (proposalId: string) => `p:${proposalId}`;
export const regionKey = (regionId: string) => `r:${regionId}`;

const STOP = new Set(['the', 'and', 'for', 'old', 'in', 'of', 'not', 'unused', 'idle', 'than', 'with', 'from', 'your', 'empty']);

function tokens(s: string): string[] {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9_]+/g, ' ')
    .split(' ')
    .filter((t) => t.length >= 2 && !STOP.has(t) && !/^\d+$/.test(t));
}

function overlap(a: string[], b: string[]): number {
  let n = 0;
  for (const x of a) {
    if (b.some((y) => y === x || (x.length >= 4 && y.length >= 4 && (x.startsWith(y) || y.startsWith(x))))) n++;
  }
  return n;
}

export function buildModel(d: Dataset): AtlasModel {
  const regions: Region[] = [];

  for (const cat of d.categories) {
    const remaining = cat.items.map((i) => i.bytes);
    const touched = new Set<number>();
    const leaves: Leaf[] = [];

    for (const p of d.proposals.filter((x) => x.categoryId === cat.id)) {
      const pt = tokens(p.title);
      let best = -1;
      let bestScore = 0;
      cat.items.forEach((it, i) => {
        let s = overlap(pt, tokens(it.name));
        if (s > 0 && remaining[i] >= p.bytes * 0.98) s += 0.5;
        if (s > bestScore) {
          bestScore = s;
          best = i;
        }
      });
      if (best < 0 && cat.items.length) best = remaining.indexOf(Math.max(...remaining));
      const it = best >= 0 ? cat.items[best] : undefined;
      if (best >= 0) {
        remaining[best] = Math.max(0, remaining[best] - p.bytes);
        touched.add(best);
      }
      leaves.push({
        id: leafIdFor(p.id),
        regionId: cat.id,
        kind: p.action === 'hint' ? 'hint' : 'reclaim',
        name: p.title,
        path: it?.path ?? p.samplePaths[0] ?? '',
        bytes: p.bytes,
        lastUsedDays: it?.lastUsedDays,
        proposal: p,
        shade: 0,
      });
    }

    cat.items.forEach((it, i) => {
      if (remaining[i] < 0.05e9) return;
      leaves.push({
        id: `i:${cat.id}:${i}`,
        regionId: cat.id,
        kind: 'item',
        name: touched.has(i) && remaining[i] < it.bytes * 0.5 ? `Rest of ${it.name}` : it.name,
        path: it.path,
        bytes: remaining[i],
        lastUsedDays: it.lastUsedDays,
        rest: touched.has(i),
        shade: 0,
      });
    });

    leaves.sort((a, b) => b.bytes - a.bytes);
    leaves.forEach((l, i) => (l.shade = i % 3));

    regions.push({
      id: cat.id,
      name: cat.name,
      bytes: cat.bytes,
      hue: cat.hue,
      protected: !!cat.protected,
      blurb: cat.blurb,
      free: false,
      leaves,
    });
  }

  regions.sort((a, b) => b.bytes - a.bytes);
  const freeBytes = d.disk.totalBytes - d.disk.usedBytes;
  regions.push({
    id: FREE_ID,
    name: 'Free space',
    bytes: freeBytes,
    hue: 0,
    protected: false,
    blurb: 'Unused space on this disk.',
    free: true,
    leaves: [{ id: 'free:space', regionId: FREE_ID, kind: 'free', name: 'Free space', path: '', bytes: freeBytes, shade: 0 }],
  });

  const leaves = regions.flatMap((r) => r.leaves);
  return {
    regions,
    leaves,
    leafById: new Map(leaves.map((l) => [l.id, l])),
    regionById: new Map(regions.map((r) => [r.id, r])),
    totalBytes: d.disk.totalBytes,
    usedBytes: d.disk.usedBytes,
    freeBytes,
    reclaimableBytes: d.proposals.filter((p) => p.action === 'delete').reduce((s, p) => s + p.bytes, 0),
  };
}

interface Datum {
  id: string;
  bytes: number;
  free?: boolean;
  children?: Datum[];
}

/** Squarified layout. Region rects are keyed `r:<id>`, parcels by leaf id. */
export function layoutRegions(regions: Region[], w: number, h: number): Map<string, Rect> {
  const out = new Map<string, Rect>();
  if (w < 20 || h < 20 || !regions.length) return out;
  const data: Datum = {
    id: '__root',
    bytes: 0,
    children: regions.map((r) => ({
      id: regionKey(r.id),
      bytes: 0,
      free: r.free,
      children: r.leaves.map((l) => ({ id: l.id, bytes: l.bytes })),
    })),
  };
  const root = hierarchy<Datum>(data)
    .sum((d) => (d.children ? 0 : d.bytes))
    .sort((a, b) => (a.data.free ? 1 : 0) - (b.data.free ? 1 : 0) || (b.value ?? 0) - (a.value ?? 0));
  const laid = treemap<Datum>()
    .size([w, h])
    .tile(treemapSquarify.ratio(1.1))
    .round(true)
    .paddingInner((n) => (n.depth === 0 ? 4 : 2))
    .paddingOuter((n) => (n.depth === 1 && !n.data.free ? 2 : 0))
    .paddingTop((n) => (n.depth === 1 && !n.data.free ? HEADER : 0))(root);
  laid.each((n) => {
    if (n.depth === 0) return;
    out.set(n.data.id, { x: n.x0, y: n.y0, w: Math.max(0, n.x1 - n.x0), h: Math.max(0, n.y1 - n.y0) });
  });
  return out;
}

/** Top-level layout, or the focused region filling the map with everything else pushed out of view. */
export function layoutAtlas(model: AtlasModel, w: number, h: number, focus: string | null): Map<string, Rect> {
  const top = layoutRegions(model.regions, w, h);
  if (!focus) return top;
  const region = model.regionById.get(focus);
  const fr = top.get(regionKey(focus));
  if (!region || !fr || fr.w < 1 || fr.h < 1) return top;
  const zoomed = layoutRegions([region], w, h);
  const sx = w / fr.w;
  const sy = h / fr.h;
  const out = new Map<string, Rect>();
  for (const [id, r] of top) {
    out.set(id, zoomed.get(id) ?? { x: (r.x - fr.x) * sx, y: (r.y - fr.y) * sy, w: r.w * sx, h: r.h * sy });
  }
  return out;
}

export const tint = (hue: number, l: number, c: number, a = 1) => `oklch(${l} ${c} ${hue}${a < 1 ? ` / ${a}` : ''})`;

export const gb = (bytes: number, digits = 1) => formatBytes(bytes, digits).replace(' ', ' ');

export function ago(days?: number): string | undefined {
  if (days === undefined) return undefined;
  if (days <= 0) return 'today';
  if (days === 1) return 'yesterday';
  if (days < 45) return `${days} days ago`;
  if (days < 548) return `${Math.round(days / 30.4)} months ago`;
  return `${(days / 365).toFixed(1).replace(/\.0$/, '')} years ago`;
}

export function duration(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return m ? `${m} min ${s} s` : `${s} s`;
}

export function toggled<T>(set: Set<T>, v: T): Set<T> {
  const n = new Set(set);
  if (n.has(v)) n.delete(v);
  else n.add(v);
  return n;
}

export const sum = <T,>(xs: T[], f: (x: T) => number) => xs.reduce((s, x) => s + f(x), 0);

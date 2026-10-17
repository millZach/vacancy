// The map's model: regions (categories, or an opened folder) holding leaves (tiles), plus the free-space region,
// and the squarified treemap layout that places them. The layout maths (squarify ratio, padding, header band, the
// zoom that pushes other regions out of view) is the prototype's, kept as-is because it is what makes the look.
import { hierarchy, treemap, treemapSquarify } from 'd3-hierarchy';
import type {
  CategoryId,
  CategoryTotal,
  ItemKind,
  MapFolder,
  MapItem,
  Overview,
  VolumeInfo,
} from '../bridge';

/** What a tile is: a scanned item, the free space, or (while scanning) a category's bytes so far. */
export type LeafKind = ItemKind | 'free' | 'scanned';

export interface Leaf {
  id: string;
  regionId: string;
  kind: LeafKind;
  name: string;
  path: string;
  bytes: number;
  lastUsed: number | null;
  /** A folder that can be opened. */
  browsable: boolean;
  note: string | null;
  /** 0..2: varies lightness between neighbouring tiles. */
  shade: number;
}

export interface Region {
  id: string;
  /** Which category's colour it takes; null for free space and the not-yet-scanned area. */
  category: CategoryId | null;
  /** The category's name, for the hover card's footer (an opened folder's region is named after the folder). */
  categoryName: string;
  name: string;
  bytes: number;
  protected: boolean;
  blurb: string;
  free: boolean;
  /** While scanning: the used space not reached yet. Laid out, never drawn, so the dot field shows through. */
  pending: boolean;
  leaves: Leaf[];
}

export interface AtlasModel {
  regions: Region[];
  leaves: Leaf[];
  leafById: Map<string, Leaf>;
  regionById: Map<string, Region>;
  /** The volume's size, for "% of the disk". */
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The height of a region's header band, px. */
export const HEADER = 24;
export const FREE_ID = 'free';
export const PENDING_ID = 'pending';
export const regionKey = (regionId: string) => `r:${regionId}`;

function leaf(item: MapItem, regionId: string): Leaf {
  return {
    id: item.id,
    regionId,
    kind: item.kind,
    name: item.name,
    path: item.path,
    bytes: item.bytes,
    lastUsed: item.lastUsed,
    browsable: item.browsable,
    note: item.note,
    shade: 0,
  };
}

function shaded(leaves: Leaf[]): Leaf[] {
  return leaves
    .filter((l) => l.bytes > 0)
    .sort((a, b) => b.bytes - a.bytes)
    .map((l, i) => ({ ...l, shade: i % 3 }));
}

function freeRegion(bytes: number): Region {
  return {
    id: FREE_ID,
    category: null,
    categoryName: 'Free space',
    name: 'Free space',
    bytes,
    protected: false,
    blurb: 'Unused space on this disk.',
    free: true,
    pending: false,
    leaves: [
      {
        id: 'free:space',
        regionId: FREE_ID,
        kind: 'free',
        name: 'Free space',
        path: '',
        bytes,
        lastUsed: null,
        browsable: false,
        note: null,
        shade: 0,
      },
    ],
  };
}

function finish(regions: Region[], volume: VolumeInfo): AtlasModel {
  const leaves = regions.flatMap((r) => r.leaves);
  return {
    regions,
    leaves,
    leafById: new Map(leaves.map((l) => [l.id, l])),
    regionById: new Map(regions.map((r) => [r.id, r])),
    totalBytes: volume.totalBytes,
    usedBytes: volume.usedBytes,
    freeBytes: Math.max(0, volume.totalBytes - volume.usedBytes),
  };
}

/** The overview: one region per category, biggest first, then free space. */
export function overviewModel(overview: Overview): AtlasModel {
  const regions: Region[] = overview.categories
    .filter((c) => c.bytes > 0)
    .map((c) => ({
      id: c.id,
      category: c.id,
      categoryName: c.name,
      name: c.name,
      bytes: c.bytes,
      protected: c.protected,
      blurb: c.blurb,
      free: false,
      pending: false,
      leaves: shaded(c.items.map((i) => leaf(i, c.id))),
    }))
    .sort((a, b) => b.bytes - a.bytes);
  regions.push(freeRegion(overview.freeBytes));
  return finish(regions, overview.volume);
}

/**
 * The map while scanning: each category found so far as one tile of its bytes so far, the used space not reached
 * yet (invisible, so the dot field shows through), and free space.
 */
export function scanModel(volume: VolumeInfo, preview: CategoryTotal[], scannedBytes: number): AtlasModel {
  const regions: Region[] = preview
    .filter((c) => c.bytes > 0)
    .map((c) => ({
      id: c.id,
      category: c.id,
      categoryName: c.name,
      name: c.name,
      bytes: c.bytes,
      protected: c.protected,
      blurb: '',
      free: false,
      pending: false,
      leaves: [
        {
          id: `scan:${c.id}`,
          regionId: c.id,
          kind: 'scanned' as const,
          name: c.name,
          path: '',
          bytes: c.bytes,
          lastUsed: null,
          browsable: false,
          note: null,
          shade: 0,
        },
      ],
    }))
    .sort((a, b) => b.bytes - a.bytes);
  const left = Math.max(0, volume.usedBytes - scannedBytes);
  if (left > 0) {
    regions.push({
      id: PENDING_ID,
      category: null,
      categoryName: '',
      name: 'Not scanned yet',
      bytes: left,
      protected: false,
      blurb: '',
      free: false,
      pending: true,
      leaves: [
        {
          id: 'pending:space',
          regionId: PENDING_ID,
          kind: 'scanned',
          name: 'Not scanned yet',
          path: '',
          bytes: left,
          lastUsed: null,
          browsable: false,
          note: null,
          shade: 0,
        },
      ],
    });
  }
  regions.push(freeRegion(Math.max(0, volume.totalBytes - volume.usedBytes)));
  return finish(regions, volume);
}

/** An opened folder: one region, the folder, filling the map with what it holds. */
export function folderModel(
  folder: MapFolder,
  category: { name: string; protected: boolean },
  volume: VolumeInfo,
): AtlasModel {
  const id = `folder:${folder.item.id}`;
  const region: Region = {
    id,
    category: folder.category,
    categoryName: category.name,
    name: folder.item.name,
    bytes: folder.item.bytes,
    protected: category.protected,
    blurb: folder.item.path,
    free: false,
    pending: false,
    leaves: shaded(folder.children.map((c) => leaf(c, id))),
  };
  return finish([region], volume);
}

interface Datum {
  id: string;
  bytes: number;
  rank: number;
  children?: Datum[];
}

/** Free space goes last; the not-yet-scanned area just before it. */
const rank = (r: Region) => (r.free ? 2 : r.pending ? 1 : 0);

/** Squarified layout. Region rects are keyed `r:<id>`, tiles by leaf id. */
export function layoutRegions(regions: Region[], w: number, h: number): Map<string, Rect> {
  const out = new Map<string, Rect>();
  if (w < 20 || h < 20 || !regions.length) return out;
  const data: Datum = {
    id: '__root',
    bytes: 0,
    rank: 0,
    children: regions.map((r) => ({
      id: regionKey(r.id),
      bytes: 0,
      rank: rank(r),
      children: r.leaves.map((l) => ({ id: l.id, bytes: l.bytes, rank: 0 })),
    })),
  };
  const root = hierarchy<Datum>(data)
    .sum((d) => (d.children ? 0 : d.bytes))
    .sort((a, b) => a.data.rank - b.data.rank || (b.value ?? 0) - (a.value ?? 0));
  const headed = (n: { depth: number; data: Datum }) => n.depth === 1 && n.data.rank === 0;
  const laid = treemap<Datum>()
    .size([w, h])
    .tile(treemapSquarify.ratio(1.1))
    .round(true)
    .paddingInner((n) => (n.depth === 0 ? 4 : 2))
    .paddingOuter((n) => (headed(n) ? 2 : 0))
    .paddingTop((n) => (headed(n) ? HEADER : 0))(root);
  laid.each((n) => {
    if (n.depth === 0) return;
    out.set(n.data.id, { x: n.x0, y: n.y0, w: Math.max(0, n.x1 - n.x0), h: Math.max(0, n.y1 - n.y0) });
  });
  return out;
}

/** The top-level layout, or the focused region filling the map with everything else pushed out of view. */
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

// The map's model: regions (categories, or an opened folder) holding leaves (tiles), plus the free-space region,
// and the squarified treemap layout that places them. The layout maths (squarify ratio, padding, header band, the
// zoom that pushes other regions out of view) is the prototype's, kept as-is because it is what makes the look.
// While a run frees space the map is laid out against the map it started from, so tiles keep their places and only
// change size as their space moves to the free region.
import { hierarchy, treemap, treemapResquarify } from 'd3-hierarchy';
import type {
  CategoryId,
  CategoryTotal,
  ItemKind,
  LockedGroup,
  LockedKind,
  MapFolder,
  MapItem,
  Overview,
  VolumeInfo,
} from '../bridge';
import { formatCount } from '../format';

/**
 * What a tile is: a scanned item, the free space, (while scanning) a category's bytes so far, or (after a fast scan)
 * the total of folders this account can't open.
 */
export type LeafKind = ItemKind | 'free' | 'scanned' | 'locked';

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
  /** The cleanup proposal the tile is, if it is one. */
  proposal: string | null;
  /** 0..2: varies lightness between neighbouring tiles. */
  shade: number;
  /**
   * Bytes of vacant ground the tile shows above what it holds: a share of what a run freed from it, so a cleared
   * tile still marks where the space came from. Laid out with `bytes`; 0 except on a run's tiles.
   */
  vacant: number;
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
/** The Locked folders block of a fast scan's map. */
export const LOCKED_ID = 'locked';
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
    proposal: item.proposal,
    shade: 0,
    vacant: 0,
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
    leaves: [soleLeaf('free:space', FREE_ID, 'free', 'Free space', bytes)],
  };
}

function finish(regions: Region[], volume: VolumeInfo): AtlasModel {
  return assemble(regions, volume.totalBytes, volume.usedBytes);
}

function assemble(regions: Region[], totalBytes: number, usedBytes: number): AtlasModel {
  const leaves = regions.flatMap((r) => r.leaves);
  return {
    regions,
    leaves,
    leafById: new Map(leaves.map((l) => [l.id, l])),
    regionById: new Map(regions.map((r) => [r.id, r])),
    totalBytes,
    usedBytes,
    freeBytes: Math.max(0, totalBytes - usedBytes),
  };
}

/** A category's region, holding `leaves`. */
function categoryRegion(c: CategoryTotal, blurb: string, leaves: Leaf[]): Region {
  return {
    id: c.id,
    category: c.id,
    categoryName: c.name,
    name: c.name,
    bytes: c.bytes,
    protected: c.protected,
    blurb,
    free: false,
    pending: false,
    leaves,
  };
}

/** A region's only tile: free space, or (while scanning) a category's bytes so far or the space not reached. */
function soleLeaf(id: string, regionId: string, kind: 'free' | 'scanned', name: string, bytes: number): Leaf {
  return {
    id,
    regionId,
    kind,
    name,
    path: '',
    bytes,
    lastUsed: null,
    browsable: false,
    note: null,
    proposal: null,
    shade: 0,
    vacant: 0,
  };
}

/**
 * The overview: one region per category, biggest first, then (after a fast scan) the Locked folders block, then
 * free space. Locked folders are in no category, so categories, locked folders and free space add up to the drive.
 */
export function overviewModel(overview: Overview): AtlasModel {
  const regions: Region[] = overview.categories
    .filter((c) => c.bytes > 0)
    .map((c) => categoryRegion(c, c.blurb, shaded(c.items.map((i) => leaf(i, c.id)))))
    .sort((a, b) => b.bytes - a.bytes);
  const locked = lockedRegion(overview.scan.locked);
  if (locked) regions.push(locked);
  regions.push(freeRegion(overview.freeBytes));
  return finish(regions, overview.volume);
}

/** What each kind of locked folder is called on the map, and why it is locked. */
const LOCKED_KINDS: Record<LockedKind, { name: string; why: string }> = {
  otherAccounts: {
    name: 'Other accounts on this PC',
    why: 'Files that belong to another account. Only that account, or Windows, can open them.',
  },
  windowsOnly: {
    name: 'Folders only Windows opens',
    why: 'Restore points, app packages and other folders Windows keeps to itself.',
  },
};

const folders = (n: number) => `${formatCount(n)} ${n === 1 ? 'folder' : 'folders'}`;

/** A fast scan's locked folders, together: their bytes and how many folders. */
export function lockedTotals(groups: readonly LockedGroup[]): { bytes: number; folders: number } {
  const shown = groups.filter((g) => g.bytes > 0);
  return { bytes: shown.reduce((n, g) => n + g.bytes, 0), folders: shown.reduce((n, g) => n + g.folders, 0) };
}

/**
 * The Locked folders block: folders this account can't open, which a fast scan counted from the file index. One
 * tile per reason, with its total only: no names, no paths, never opened, never a proposal. Null when there are none.
 */
export function lockedRegion(groups: readonly LockedGroup[]): Region | null {
  const shown = groups.filter((g) => g.bytes > 0);
  if (shown.length === 0) return null;
  const totals = lockedTotals(shown);
  const leaves = shown
    .map((g): Leaf => ({
      id: `locked:${g.kind}`,
      regionId: LOCKED_ID,
      kind: 'locked',
      name: LOCKED_KINDS[g.kind].name,
      path: '',
      bytes: g.bytes,
      lastUsed: null,
      browsable: false,
      note: `${LOCKED_KINDS[g.kind].why} Fast scan counted ${folders(g.folders)} here and shows their total only.`,
      proposal: null,
      shade: 0,
      vacant: 0,
    }))
    .sort((a, b) => b.bytes - a.bytes)
    .map((l, i) => ({ ...l, shade: i % 3 }));
  return {
    id: LOCKED_ID,
    // System's hue, drawn colourless (map.css): the block belongs to no category.
    category: 'system',
    categoryName: 'Locked folders',
    name: 'Locked folders',
    bytes: totals.bytes,
    protected: true,
    blurb: `Totals only: ${folders(totals.folders)} your account can’t open. Vacancy shows no names from inside them, and can’t propose or delete anything there.`,
    free: false,
    pending: false,
    leaves,
  };
}

/**
 * The map while scanning: each category found so far as one tile of its bytes so far, the used space not reached
 * yet (invisible, so the dot field shows through), and free space.
 */
export function scanModel(volume: VolumeInfo, preview: CategoryTotal[], scannedBytes: number): AtlasModel {
  const regions: Region[] = preview
    .filter((c) => c.bytes > 0)
    .map((c) => categoryRegion(c, '', [soleLeaf(`scan:${c.id}`, c.id, 'scanned', c.name, c.bytes)]))
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
      leaves: [soleLeaf('pending:space', PENDING_ID, 'scanned', 'Not scanned yet', left)],
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

/** What a run has done to one tile so far. */
export interface Freed {
  /** Bytes freed from it. */
  bytes: number;
  /** Bytes it is known to have left in place. */
  kept: number;
}

/** The share of what a tile freed that it keeps as vacant ground: a sliver, once it is cleared. */
export const VACANT_SHARE = 0.15;

/**
 * The map as a run has left it so far. Each tile in `freed` holds what is still there (its bytes less what was
 * freed, never below what was left in place) under a sliver of vacant ground, its region is smaller by as much, and
 * free space has grown by `total`, everything the run freed. Laid out against `model` (`layoutAtlas`'s `base`),
 * every tile keeps its place and only changes size.
 */
export function freedModel(model: AtlasModel, freed: ReadonlyMap<string, Freed>, total: number): AtlasModel {
  const gone = Math.max(0, total);
  const regions = model.regions.map((r): Region => {
    if (r.free) return { ...r, bytes: r.bytes + gone, leaves: r.leaves.map((l) => ({ ...l, bytes: l.bytes + gone })) };
    let less = 0;
    const leaves = r.leaves.map((l) => {
      const f = freed.get(l.id);
      if (!f || f.bytes <= 0) return l;
      const bytes = Math.max(0, l.bytes - f.bytes, Math.min(l.bytes, f.kept));
      less += l.bytes - bytes;
      return { ...l, bytes, vacant: f.bytes * VACANT_SHARE };
    });
    return leaves.some((l, i) => l !== r.leaves[i]) ? { ...r, bytes: Math.max(0, r.bytes - less), leaves } : r;
  });
  return assemble(regions, model.totalBytes, Math.max(0, model.usedBytes - gone));
}

interface Datum {
  id: string;
  bytes: number;
  rank: number;
  children?: Datum[];
}

/** Free space goes last; the not-yet-scanned area just before it. */
const rank = (r: Region) => (r.free ? 2 : r.pending ? 1 : 0);

/** What a tile takes up on the map: what it holds, and its vacant ground. */
const weight = (l: Leaf) => l.bytes + l.vacant;

/** Whether two maps hold the same tiles in the same regions, whatever their sizes. */
function sameTiles(a: Region[], b: Region[]): boolean {
  const ids = (rs: Region[]) => new Set(rs.flatMap((r) => r.leaves.map((l) => `${r.id}/${l.id}`)));
  const x = ids(a);
  const y = ids(b);
  return x.size === y.size && [...x].every((id) => y.has(id));
}

/**
 * Squarified layout. Region rects are keyed `r:<id>`, tiles by leaf id. With `base` (the same regions and tiles at
 * other sizes) the arrangement is `base`'s: it is laid out first, then each tile is resized to its size in `regions`
 * where it stands, rows and order kept (d3's resquarify), so nothing reshuffles as sizes change.
 */
export function layoutRegions(regions: Region[], w: number, h: number, base?: Region[]): Map<string, Rect> {
  const out = new Map<string, Rect>();
  if (w < 20 || h < 20 || !regions.length) return out;
  const stable = base && base !== regions && sameTiles(base, regions) ? base : null;
  const data: Datum = {
    id: '__root',
    bytes: 0,
    rank: 0,
    children: (stable ?? regions).map((r) => ({
      id: regionKey(r.id),
      bytes: 0,
      rank: rank(r),
      children: r.leaves.map((l) => ({ id: l.id, bytes: weight(l), rank: 0 })),
    })),
  };
  const root = hierarchy<Datum>(data)
    .sum((d) => (d.children ? 0 : d.bytes))
    .sort((a, b) => a.data.rank - b.data.rank || (b.value ?? 0) - (a.value ?? 0));
  const headed = (n: { depth: number; data: Datum }) => n.depth === 1 && n.data.rank === 0;
  // Resquarify squarifies a node the first time it is laid out, and keeps its rows every time after.
  const lay = treemap<Datum>()
    .size([w, h])
    .tile(treemapResquarify.ratio(1.1))
    .round(true)
    .paddingInner((n) => (n.depth === 0 ? 4 : 2))
    .paddingOuter((n) => (headed(n) ? 2 : 0))
    .paddingTop((n) => (headed(n) ? HEADER : 0));
  let laid = lay(root);
  if (stable) {
    const now = new Map(regions.flatMap((r) => r.leaves.map((l) => [l.id, weight(l)] as const)));
    laid = lay(root.sum((d) => (d.children ? 0 : (now.get(d.id) ?? 0))));
  }
  laid.each((n) => {
    if (n.depth === 0) return;
    out.set(n.data.id, { x: n.x0, y: n.y0, w: Math.max(0, n.x1 - n.x0), h: Math.max(0, n.y1 - n.y0) });
  });
  return out;
}

/**
 * The top-level layout, or the focused region filling the map with everything else pushed out of view. With
 * `base`, the map `model` was made from (the same tiles, at their sizes before a run), tiles keep `base`'s places.
 */
export function layoutAtlas(
  model: AtlasModel,
  w: number,
  h: number,
  focus: string | null,
  base?: AtlasModel,
): Map<string, Rect> {
  const top = layoutRegions(model.regions, w, h, base?.regions);
  if (!focus) return top;
  const region = model.regionById.get(focus);
  const fr = top.get(regionKey(focus));
  if (!region || !fr || fr.w < 1 || fr.h < 1) return top;
  const was = base?.regionById.get(focus);
  const zoomed = layoutRegions([region], w, h, was && [was]);
  const sx = w / fr.w;
  const sy = h / fr.h;
  const out = new Map<string, Rect>();
  for (const [id, r] of top) {
    out.set(id, zoomed.get(id) ?? { x: (r.x - fr.x) * sx, y: (r.y - fr.y) * sy, w: r.w * sx, h: r.h * sy });
  }
  return out;
}

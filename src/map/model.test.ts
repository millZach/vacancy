import { describe, expect, it } from 'vitest';
import { fixtures } from '../bridge';
import { fastOverview } from '../bridge/mockFast';
import {
  FREE_ID,
  LOCKED_ID,
  PENDING_ID,
  VACANT_SHARE,
  folderModel,
  freedModel,
  layoutAtlas,
  layoutRegions,
  lockedRegion,
  lockedTotals,
  overviewModel,
  regionKey,
  scanModel,
  type AtlasModel,
  type Rect,
} from './model';
import { scaleSquare } from './scale';

const W = 1200;
const H = 700;
const area = (r: Rect) => r.w * r.h;
const inside = (inner: Rect, outer: Rect) =>
  inner.x >= outer.x - 0.5 &&
  inner.y >= outer.y - 0.5 &&
  inner.x + inner.w <= outer.x + outer.w + 0.5 &&
  inner.y + inner.h <= outer.y + outer.h + 0.5;

function everyTileInItsRegion(model: AtlasModel, rects: Map<string, Rect>) {
  for (const leaf of model.leaves) {
    const tile = rects.get(leaf.id);
    const region = rects.get(regionKey(leaf.regionId));
    expect(tile, leaf.id).toBeDefined();
    expect(inside(tile!, region!), `${leaf.id} sits in its region`).toBe(true);
  }
}

describe('the overview model', () => {
  const overview = fixtures.windows.overview;
  const model = overviewModel(overview);

  it('has a region per category, biggest first, and free space last', () => {
    expect(model.regions.map((r) => r.id)).toEqual([
      'apps',
      'dev',
      'media',
      'system',
      'downloads',
      'caches',
      'cloud',
      'other',
      FREE_ID,
    ]);
    expect(model.regionById.get(FREE_ID)!.bytes).toBe(overview.freeBytes);
    expect(model.regionById.get('system')!.protected).toBe(true);
    expect(model.totalBytes).toBe(overview.volume.totalBytes);
  });

  it('shades neighbouring tiles differently, biggest first', () => {
    const apps = model.regionById.get('apps')!;
    expect(apps.leaves.map((l) => l.shade).slice(0, 4)).toEqual([0, 1, 2, 0]);
    expect(apps.leaves[0]!.name).toBe('Cyberpunk 2077');
  });

  it('lays tiles out by size, inside their regions, in the space given', () => {
    const rects = layoutRegions(model.regions, W, H);
    everyTileInItsRegion(model, rects);
    for (const r of rects.values()) {
      expect(r.x).toBeGreaterThanOrEqual(0);
      expect(r.x + r.w).toBeLessThanOrEqual(W);
      expect(r.y + r.h).toBeLessThanOrEqual(H);
    }
    const free = rects.get('free:space')!;
    const cyberpunk = rects.get(model.leaves.find((l) => l.name === 'Cyberpunk 2077')!.id)!;
    const bg3 = rects.get(model.leaves.find((l) => l.name === "Baldur's Gate 3")!.id)!;
    expect(area(cyberpunk)).toBeGreaterThan(area(bg3) * 2);
    expect(area(free)).toBeGreaterThan(0);
    // Regions leave a header band above their tiles.
    const head = rects.get(regionKey('apps'))!;
    const top = Math.min(...model.regionById.get('apps')!.leaves.map((l) => rects.get(l.id)!.y));
    expect(top - head.y).toBeGreaterThanOrEqual(24);
  });

  it('zooms into a region: it fills the map and the others are pushed out of view', () => {
    const rects = layoutAtlas(model, W, H, 'dev');
    const dev = rects.get(regionKey('dev'))!;
    expect(dev).toEqual({ x: 0, y: 0, w: W, h: H });
    everyTileInItsRegion({ ...model, leaves: model.regionById.get('dev')!.leaves }, rects);
    const apps = rects.get(regionKey('apps'))!;
    const inView = apps.x < W - 1 && apps.x + apps.w > 1 && apps.y < H - 1 && apps.y + apps.h > 1;
    expect(inView, 'another region is pushed off the map').toBe(false);
  });

  it('lays out nothing in a space too small to read', () => {
    expect(layoutRegions(model.regions, 10, 500).size).toBe(0);
  });
});

describe('the scanning model', () => {
  const volume = fixtures.windows.overview.volume;
  it('shows categories found so far, the rest of the used space unscanned, and free space', () => {
    const model = scanModel(
      volume,
      [
        { id: 'apps', name: 'Apps & games', protected: false, bytes: 100e9 },
        { id: 'system', name: 'Windows & system', protected: true, bytes: 20e9 },
        { id: 'dev', name: 'Developer', protected: false, bytes: 0 },
      ],
      120e9,
    );
    expect(model.regions.map((r) => r.id)).toEqual(['apps', 'system', PENDING_ID, FREE_ID]);
    expect(model.regionById.get(PENDING_ID)!.bytes).toBe(volume.usedBytes - 120e9);
    expect(model.regionById.get(PENDING_ID)!.pending).toBe(true);
    const rects = layoutRegions(model.regions, W, H);
    expect(rects.get(regionKey('apps'))).toBeDefined();
  });

  it('has nothing left to scan once the scan has counted the used space', () => {
    const model = scanModel(volume, [], volume.usedBytes + 1);
    expect(model.regions.map((r) => r.id)).toEqual([FREE_ID]);
  });
});

describe('the folder model', () => {
  it('is one region, the folder, holding its contents', () => {
    const folder = fixtures.windows.folders['fx-apps-2']!;
    const model = folderModel(folder, { name: 'Apps & games', protected: false }, fixtures.windows.overview.volume);
    expect(model.regions).toHaveLength(1);
    const region = model.regions[0]!;
    expect(region.name).toBe('Adobe');
    expect(region.category).toBe('apps');
    expect(region.categoryName).toBe('Apps & games');
    expect(region.leaves.map((l) => l.name)).toEqual([
      'Adobe Photoshop 2026',
      'Adobe Premiere Pro 2026',
      'Adobe Creative Cloud',
    ]);
    everyTileInItsRegion(model, layoutRegions(model.regions, W, H));
  });
});

describe('the map as a run frees space', () => {
  const model = overviewModel(fixtures.windows.overview);
  const NODE = 'p:w-node';
  const BROWSER = 'p:w-browser';
  const bytesOf = (id: string) => model.leafById.get(id)!.bytes;
  // Mid-run: node_modules has freed two thirds of itself; the browser caches freed 5 GB and kept 2 GB that were in
  // use, more than the 1.3 GB its figures alone would leave.
  const freed = new Map([
    [NODE, { bytes: 14e9, kept: 0 }],
    [BROWSER, { bytes: 5e9, kept: 2e9 }],
  ]);
  const total = 19e9;
  const grown = freedModel(model, freed, total);

  /** How many neighbouring tiles (side by side, or one above the other) in `a` have swapped sides in `b`. */
  function swapped(a: Map<string, Rect>, b: Map<string, Rect>) {
    const ids = model.leaves.map((l) => l.id);
    const overlap = (p0: number, p1: number, q0: number, q1: number) => Math.min(p1, q1) - Math.max(p0, q0) > 1;
    let n = 0;
    for (const x of ids)
      for (const y of ids) {
        const [ax, ay, bx, by] = [a.get(x)!, a.get(y)!, b.get(x)!, b.get(y)!];
        const leftOf = ax.x + ax.w <= ay.x && overlap(ax.y, ax.y + ax.h, ay.y, ay.y + ay.h);
        const above = ax.y + ax.h <= ay.y && overlap(ax.x, ax.x + ax.w, ay.x, ay.x + ay.w);
        if ((leftOf && bx.x + bx.w > by.x + 1) || (above && bx.y + bx.h > by.y + 1)) n++;
      }
    return n;
  }

  it('shrinks a tile by what its step freed, never below what it left in place, and grows free space by it all', () => {
    expect(grown.leafById.get(NODE)!.bytes).toBe(bytesOf(NODE) - 14e9);
    expect(grown.leafById.get(NODE)!.vacant).toBe(14e9 * VACANT_SHARE);
    expect(grown.leafById.get(BROWSER)!.bytes).toBe(2e9);
    expect(grown.regionById.get('dev')!.bytes).toBe(model.regionById.get('dev')!.bytes - 14e9);
    expect(grown.regionById.get(FREE_ID)!.bytes).toBe(model.regionById.get(FREE_ID)!.bytes + total);
    expect(grown.leafById.get('free:space')!.bytes).toBe(model.leafById.get('free:space')!.bytes + total);
    expect(grown.freeBytes).toBe(model.freeBytes + total);
    // Tiles the run didn't touch are the same tiles; a region with none of its tiles touched is the same region.
    const apps = model.regionById.get('apps')!;
    expect(grown.regionById.get('apps')).toBe(apps);
    expect(grown.leafById.get(apps.leaves[0]!.id)).toBe(apps.leaves[0]);
    expect(freedModel(model, new Map(), 0).regions.filter((r) => !r.free)).toEqual(model.regions.filter((r) => !r.free));
  });

  it('grows the free region by about the share freed and shrinks the draining tiles, reshuffling nothing', () => {
    const before = layoutAtlas(model, W, H, null);
    const after = layoutAtlas(grown, W, H, null, model);
    const share = (rects: Map<string, Rect>, id: string) => area(rects.get(id)!) / (W * H);

    const grew = share(after, 'free:space') - share(before, 'free:space');
    expect(grew).toBeGreaterThan((total / model.totalBytes) * 0.8);
    expect(grew).toBeLessThan((total / model.totalBytes) * 1.2);
    expect(area(after.get(regionKey(FREE_ID))!)).toBeGreaterThan(area(before.get(regionKey(FREE_ID))!));
    // node_modules keeps a third of itself and a sliver of vacant ground: under half its area.
    expect(area(after.get(NODE)!)).toBeLessThan(area(before.get(NODE)!) * 0.5);
    expect(area(after.get(BROWSER)!)).toBeLessThan(area(before.get(BROWSER)!) * 0.6);
    everyTileInItsRegion(grown, after);

    // Nothing reshuffles: every tile keeps its neighbours on the same sides, and the ones the run didn't touch stay
    // about as big, a little way from where they were (the space freed before them has gone).
    expect(swapped(before, after)).toBe(0);
    for (const l of model.leaves) {
      if (freed.has(l.id) || l.kind === 'free') continue;
      const [a, b] = [before.get(l.id)!, after.get(l.id)!];
      if (area(a) < 2000) continue;
      expect(Math.abs(area(b) / area(a) - 1), `${l.name} keeps its size`).toBeLessThan(0.1);
      expect(Math.hypot(b.x - a.x, b.y - a.y), `${l.name} stays near`).toBeLessThan(W * 0.08);
    }
    // Laid out afresh instead, the same sizes would shuffle tiles around.
    expect(swapped(before, layoutAtlas(grown, W, H, null))).toBeGreaterThan(5);
  });

  it('settles back to the map it started from when nothing was freed', () => {
    expect(layoutAtlas(freedModel(model, new Map(), 0), W, H, null, model)).toEqual(layoutAtlas(model, W, H, null));
  });

  it('lays out afresh against a map with other tiles', () => {
    const scanning = scanModel(fixtures.windows.overview.volume, [], 100e9);
    expect(layoutRegions(scanning.regions, W, H, model.regions)).toEqual(layoutRegions(scanning.regions, W, H));
  });
});

describe('the scale square', () => {
  it('picks the round amount whose square is 9 to 20 px wide', () => {
    const s = scaleSquare(476e9, W * H)!;
    expect(s.side).toBeGreaterThanOrEqual(9);
    expect(s.side).toBeLessThanOrEqual(20);
    expect(s.gb).toBe(0.2);
    expect(scaleSquare(9e9, W * H)!.gb).toBeLessThan(0.01);
    expect(scaleSquare(0, W * H)).toBeNull();
  });
});

describe('a fast scan’s map', () => {
  const overview = fastOverview(structuredClone(fixtures.windows.overview), 1);
  const model = overviewModel(overview);
  const locked = model.regionById.get(LOCKED_ID)!;

  it('has a Locked folders block before free space, one totals-only tile per reason', () => {
    expect(model.regions.at(-2)!.id).toBe(LOCKED_ID);
    expect(model.regions.at(-1)!.id).toBe(FREE_ID);
    expect(locked.name).toBe('Locked folders');
    expect(locked.protected).toBe(true);
    expect(locked.bytes).toBeCloseTo(25.7e9);
    expect(locked.blurb).toBe(
      'Totals only: 1,287 folders your account can’t open. Vacancy shows no names from inside them, and can’t propose or delete anything there.',
    );
    expect(locked.leaves.map((l) => [l.name, l.kind])).toEqual([
      ['Other accounts on this PC', 'locked'],
      ['Folders only Windows opens', 'locked'],
    ]);
    expect(locked.leaves[0]!.note).toBe(
      'Files that belong to another account. Only that account, or Windows, can open them. Fast scan counted 1,204 folders here and shows their total only.',
    );
    expect(locked.leaves[1]!.note).toBe(
      'Restore points, app packages and other folders Windows keeps to itself. Fast scan counted 83 folders here and shows their total only.',
    );
  });

  it('names nothing inside them, opens nothing and proposes nothing', () => {
    for (const l of locked.leaves) {
      expect(l.path).toBe('');
      expect(l.browsable).toBe(false);
      expect(l.proposal).toBeNull();
      expect(l.lastUsed).toBeNull();
    }
  });

  it('still adds up to the drive: categories, locked folders and free space', () => {
    const sum = model.regions.reduce((n, r) => n + r.bytes, 0);
    expect(sum).toBeCloseTo(overview.volume.totalBytes, -3);
    const categories = overview.categories.reduce((n, c) => n + c.bytes, 0);
    expect(categories + lockedTotals(overview.scan.locked).bytes).toBeCloseTo(overview.volume.usedBytes, -3);
  });

  it('counts the file index’s records in System, with a note, where the unseen space was', () => {
    const system = model.regionById.get('system')!;
    const records = system.leaves.find((l) => l.kind === 'records')!;
    expect(records.name).toBe('File system records');
    expect(records.note).toContain('Fast scan reads them');
    expect(records.browsable).toBe(false);
    expect(system.leaves.some((l) => l.kind === 'hidden')).toBe(false);
  });

  it('lays the block out like any region, its tiles inside it', () => {
    const rects = layoutRegions(model.regions, W, H);
    everyTileInItsRegion(model, rects);
    expect(area(rects.get(regionKey(LOCKED_ID))!)).toBeGreaterThan(0);
  });

  it('has no block after a normal scan, nor for empty groups', () => {
    expect(overviewModel(fixtures.windows.overview).regionById.has(LOCKED_ID)).toBe(false);
    expect(lockedRegion([{ kind: 'windowsOnly', bytes: 0, files: 0, folders: 0 }])).toBeNull();
    expect(lockedRegion([{ kind: 'windowsOnly', bytes: 1e9, files: 3, folders: 1 }])!.blurb).toContain('1 folder your');
  });
});

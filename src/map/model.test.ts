import { describe, expect, it } from 'vitest';
import { fixtures } from '../bridge';
import {
  FREE_ID,
  PENDING_ID,
  folderModel,
  layoutAtlas,
  layoutRegions,
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

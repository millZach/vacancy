import { describe, expect, it } from 'vitest';
import { createMockBridge } from './mock';
import { fixtures } from './fixtures';
import type { DuplicatesOutcome } from './types';

async function found(name: keyof typeof fixtures) {
  const bridge = createMockBridge(fixtures[name], { scanMs: 0, dupsMs: 0, runMs: 0 });
  const mount = fixtures[name].volumes[0]!.mountPoint;
  await bridge.scan(mount, () => {});
  const outcome = await bridge.findDuplicates(mount, () => {});
  if (outcome.status !== 'done') throw new Error('the mock search finishes');
  return { bridge, mount, outcome };
}

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

describe('the mock duplicate finder', () => {
  for (const name of ['windows', 'macos'] as const) {
    it(`finds groups on the ${name} fixture's system drive, with a tile per folder and every byte once`, async () => {
      const { outcome } = await found(name);
      const groups = outcome.duplicates.groups;
      expect(groups.length).toBeGreaterThan(10);
      const tiles = outcome.overview.categories.flatMap((c) => c.items).filter((i) => i.proposal === 'duplicates');
      const folders = new Set(groups.flatMap((g) => g.extras.map((e) => e.folder)));
      expect(tiles).toHaveLength(folders.size);
      const p = outcome.proposals.find((x) => x.id === 'duplicates')!;
      expect(p.defaultOn).toBe(false);
      expect(p.risk).toBe('review');
      expect(p.bytes).toBe(sum(groups.map((g) => g.freed)));
      expect(sum(tiles.map((t) => t.bytes))).toBe(p.bytes);
      const before = sum(fixtures[name].overview.categories.map((c) => c.bytes));
      expect(sum(outcome.overview.categories.map((c) => c.bytes))).toBe(before);
      const ids = new Set(outcome.overview.categories.flatMap((c) => c.items.map((i) => i.id)));
      for (const g of groups) {
        expect(ids.has(g.keep.tile)).toBe(true);
        for (const e of g.extras) expect(ids.has(e.tile)).toBe(true);
      }
    });
  }

  it('reports progress and finds nothing on another drive', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0, dupsMs: 0 });
    await bridge.scan('D:\\', () => {});
    const heard: number[] = [];
    const outcome = await bridge.findDuplicates('D:\\', (p) => heard.push(p.percent));
    expect(heard.at(-1)).toBe(100);
    expect(outcome.status === 'done' && outcome.duplicates.groups).toEqual([]);
  });

  it('is stopped by a new scan', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0, dupsMs: 60_000 });
    await bridge.scan('C:\\', () => {});
    const searching = bridge.findDuplicates('C:\\', () => {});
    void bridge.scan('C:\\', () => {});
    await expect(searching).resolves.toEqual<DuplicatesOutcome>({ status: 'cancelled' });
  });

  it('plans the ticked groups only, and the run reports one copy of each group kept', async () => {
    const { bridge, mount, outcome } = await found('windows');
    const groups = outcome.duplicates.groups;
    const skip = groups[0]!.id;
    const plan = await bridge.buildPlan(mount, ['duplicates'], [skip]);
    expect(plan.duplicates?.skippedGroups).toBe(1);
    expect(plan.duplicates?.groups).toBe(groups.length - 1);
    expect(plan.duplicates?.copies).toBe(sum(groups.slice(1).map((g) => g.extras.length)));
    const report = await bridge.runPlan(mount, plan.id, () => {});
    const d = report.duplicates!;
    expect(d.groups).toBe(groups.length - 1);
    // The fixture's changed group keeps every copy.
    expect(d.keptWhole).toBe(1);
    expect(d.clearedGroups).toBe(d.groups - 1);
    expect(sum(d.tiles.map((t) => t.freedBytes))).toBe(d.freedBytes);
    expect(d.samples.length).toBeGreaterThan(0);
  });
});

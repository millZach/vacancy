import { afterEach, describe, expect, it, vi } from 'vitest';
import { createMockBridge, fixtures, type RunEvent, type ScanUpdate } from '.';

afterEach(() => {
  vi.useRealTimers();
});

describe('the mock bridge', () => {
  it('replays a scan: counts climb, categories are found one by one, then the map', async () => {
    vi.useFakeTimers();
    const bridge = createMockBridge(fixtures.windows, { scanMs: 1000 });
    const updates: ScanUpdate[] = [];
    const done = bridge.scan('C:\\', (u) => updates.push(u));
    await vi.advanceTimersByTimeAsync(1100);
    const outcome = await done;

    expect(outcome.status).toBe('done');
    expect(updates.length).toBeGreaterThan(5);
    expect(updates.every((u, i) => i === 0 || u.files >= updates[i - 1]!.files)).toBe(true);
    const found = updates.map((u) => u.categories?.length ?? 0);
    expect(found[0]).toBeLessThan(found.at(-1)!);
    expect(updates.at(-1)!.files).toBe(fixtures.windows.overview.scan.files);
    expect(updates.some((u) => u.currentPath.startsWith('C:\\'))).toBe(true);
  });

  it('scans another drive at that drive’s size', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
    const outcome = await bridge.scan('D:\\', () => {});
    if (outcome.status !== 'done') throw new Error('expected a map');
    expect(outcome.overview.volume.name).toBe('Data (D:)');
    expect(outcome.overview.categories[0]!.items[0]!.path.startsWith('D:\\')).toBe(true);
    const total = outcome.overview.categories.reduce((s, c) => s + c.bytes, 0);
    expect(total).toBeCloseTo(outcome.overview.volume.usedBytes, -3);
  });

  it('answers with each drive’s own proposals, the way the core carves them into the map', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
    const c = await bridge.scan('C:\\', () => {});
    const d = await bridge.scan('D:\\', () => {});
    if (c.status !== 'done' || d.status !== 'done') throw new Error('expected maps');
    expect(c.proposals.map((p) => p.id)).toEqual(fixtures.windows.proposals.map((p) => p.id));
    const tiles = c.overview.categories.flatMap((cat) => cat.items.filter((i) => i.kind === 'proposal'));
    expect(tiles.map((t) => t.proposal).sort()).toEqual(c.proposals.map((p) => p.id).sort());
    for (const t of tiles) expect(t.bytes).toBe(c.proposals.find((p) => p.id === t.proposal)!.bytes);

    const node = d.proposals.find((p) => p.id === 'w-node')!;
    expect(node.bytes).toBeCloseTo((21e9 * 700) / 431, -3);
    expect(node.samplePaths.every((s) => s.startsWith('D:\\'))).toBe(true);
  });

  it('stops when cancelled', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 60_000 });
    const outcome = bridge.scan('C:\\', () => {});
    await bridge.cancelScan();
    expect(await outcome).toEqual({ status: 'cancelled' });
  });

  it('opens folders of a scanned drive and refuses unknown ones', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
    await expect(bridge.browse('C:\\', 'fx-apps-2')).rejects.toThrow('there is no finished scan of C:\\ to explore');
    await bridge.scan('C:\\', () => {});
    const adobe = await bridge.browse('C:\\', 'fx-apps-2');
    expect(adobe.item.name).toBe('Adobe');
    await expect(bridge.browse('C:\\', 'nope')).rejects.toThrow('there is no folder nope in this scan');
  });

  it('keeps a map per drive, with paths on that drive, and forgets a drive that goes', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
    await bridge.scan('C:\\', () => {});
    await bridge.scan('D:\\', () => {});
    expect((await bridge.browse('C:\\', 'fx-apps-2')).item.path).toBe('C:\\Program Files\\Adobe');
    expect((await bridge.browse('D:\\', 'fx-apps-2')).item.path).toBe('D:\\Program Files\\Adobe');

    bridge.unplug('D:\\');
    expect((await bridge.listVolumes()).map((v) => v.mountPoint)).toEqual(['C:\\', 'E:\\']);
    await bridge.forgetDrive('D:\\');
    await expect(bridge.browse('D:\\', 'fx-apps-2')).rejects.toThrow('there is no finished scan of D:\\');
    await expect(bridge.scan('D:\\', () => {})).rejects.toThrow('not connected');
    expect((await bridge.browse('C:\\', 'fx-apps-2')).item.name).toBe('Adobe');
  });

  it('stops the scan of a drive it forgets', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 60_000 });
    const outcome = bridge.scan('E:\\', () => {});
    await bridge.forgetDrive('E:\\');
    expect(await outcome).toEqual({ status: 'cancelled' });
  });

  describe('plans and runs', () => {
    const ids = fixtures.windows.proposals.map((p) => p.id);
    const scanned = async (options = {}) => {
      const bridge = createMockBridge(fixtures.windows, { scanMs: 0, runMs: 0, ...options });
      await bridge.scan('C:\\', () => {});
      return bridge;
    };

    it('plans from the scan, leaving out a place that changed since it', async () => {
      const bridge = await scanned();
      const plan = await bridge.buildPlan('C:\\', ['w-installers', 'w-node']);
      expect(plan.steps.map((s) => s.id)).toEqual(['w-node', 'w-installers']);
      expect(plan.refused).toEqual([expect.objectContaining({ kind: 'changed', reason: 'It changed since the scan' })]);
      const installers = fixtures.windows.proposals.find((p) => p.id === 'w-installers')!;
      expect(plan.steps[1]).toMatchObject({ bytes: installers.bytes - 0.62e9, items: 46, places: 46 });
      expect(plan.bytes).toBe(plan.steps[0]!.bytes + plan.steps[1]!.bytes);
      expect(plan.moves).toBe(0);
    });

    it('refuses to plan without a scan, or from proposals the scan doesn’t have', async () => {
      const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
      await expect(bridge.buildPlan('C:\\', ['w-node'])).rejects.toThrow('there is no finished scan of C:\\ to plan from');
      await bridge.scan('C:\\', () => {});
      await expect(bridge.buildPlan('C:\\', ['m-trash'])).rejects.toThrow('there is no proposal m-trash in this scan');
    });

    it('runs a plan step by step, with every place and skip as it happens, and reports it', async () => {
      const bridge = await scanned();
      const plan = await bridge.buildPlan('C:\\', ids);
      const events: RunEvent[] = [];
      const report = await bridge.runPlan('C:\\', plan.id, (e) => events.push(e));

      const started = events.filter((e) => e.kind === 'stepStarted').map((e) => e.id);
      expect(started).toEqual(plan.steps.map((s) => s.id));
      expect(events.filter((e) => e.kind === 'deleted')).toHaveLength(plan.places);
      const skips = events.flatMap((e) => (e.kind === 'skipped' ? [e.skip] : []));
      // Left in place: one changed since the scan, one in use. Deleted, its space still in use: hard links.
      expect(skips.map((k) => k.kind).sort()).toEqual(['changed', 'hardLinked', 'inUse']);

      expect(report.cancelled).toBe(false);
      expect(report.steps.map((s) => s.outcome)).toContain('partial');
      // The hard-linked names were deleted; only their space stays in use.
      expect(report.deletedItems).toBe(plan.items - 212);
      expect(report.freedBytes).toBeCloseTo(plan.bytes - 0.41e9 - 0.09e9, -3);
      // What the confirm dialog showed: the place refused while planning is a skip, not part of the plan.
      expect(report.plannedBytes).toBeCloseTo(plan.bytes, -3);
      expect(report.skippedItems).toBe(213);
      expect(report.biggestWin).toBe('w-node');
      expect(report.freedByCategory.map((c) => c.id)).toEqual(['dev', 'caches', 'downloads']);
      // The drive's own figures: a little less than what was freed, since other apps write too.
      expect(report.freeChange).toBeLessThan(report.freedBytes);
      expect(report.freeAfter - report.freeBefore).toBe(report.freeChange);
      // Kept in the history, newest first, with everything the finish shows.
      const history = await bridge.listRuns();
      expect(history.unreadable).toBe(0);
      expect(history.runs[0]).toEqual({
        id: report.id,
        mountPoint: 'C:\\',
        volumeName: 'Local Disk (C:)',
        volumeId: report.map.volume.id,
        startedAt: report.startedAt,
        freedBytes: report.freedBytes,
        deletedItems: report.deletedItems,
        movedItems: 0,
        cancelled: false,
      });
      expect(await bridge.openRun(report.id)).toEqual(report);
      expect(report.map.volume.mountPoint).toBe('C:\\');
      expect(report.steps[0]!.why).toBe(fixtures.windows.proposals.find((p) => p.id === 'w-node')!.why);
      const c = (await bridge.listVolumes())[0]!;
      expect(c.freeBytes).toBe(report.freeAfter);
    });

    it('runs each scan’s plan once', async () => {
      const bridge = await scanned();
      const plan = await bridge.buildPlan('C:\\', ['w-temp']);
      await bridge.runPlan('C:\\', plan.id, () => {});
      await expect(bridge.runPlan('C:\\', plan.id, () => {})).rejects.toThrow('there is no plan');
      await expect(bridge.buildPlan('C:\\', ['w-temp'])).rejects.toThrow('already ran a plan: scan again first');
      await bridge.scan('C:\\', () => {});
      await expect(bridge.buildPlan('C:\\', ['w-temp'])).resolves.toMatchObject({ steps: [{ id: 'w-temp' }] });
    });

    it('stops after the file being deleted, and reports what was left', async () => {
      vi.useFakeTimers();
      const bridge = createMockBridge(fixtures.windows, { scanMs: 0, runMs: 8000 });
      const scan = bridge.scan('C:\\', () => {});
      await vi.advanceTimersByTimeAsync(10);
      await scan;
      const plan = await (async () => {
        const p = bridge.buildPlan('C:\\', ids);
        await vi.advanceTimersByTimeAsync(10);
        return p;
      })();
      const events: RunEvent[] = [];
      const done = bridge.runPlan('C:\\', plan.id, (e) => events.push(e));
      await vi.advanceTimersByTimeAsync(1500);
      const stopping = bridge.cancelRun();
      await vi.advanceTimersByTimeAsync(200);
      await stopping;
      const report = await done;

      expect(report.cancelled).toBe(true);
      expect(report.freedBytes).toBeGreaterThan(0);
      expect(report.freedBytes).toBeLessThan(plan.bytes);
      expect(report.steps).toHaveLength(plan.steps.length);
      expect(report.steps.at(-1)!.outcome).toBe('skipped');
      expect(report.steps.at(-1)!.skipped.every((k) => k.kind === 'cancelled')).toBe(true);
      // Every file is either deleted or left.
      const left = report.steps.flatMap((s) => s.skipped).reduce((n, k) => n + k.items, 0);
      expect(report.deletedItems + left).toBe(plan.items + 1);
      expect(events.at(-1)!.kind).toBe('stepFinished');
    });

    it('fails every plan and run with the fail option', async () => {
      const bridge = createMockBridge(fixtures.windows, { fail: 'Access is denied.' });
      await expect(bridge.buildPlan('C:\\', [])).rejects.toThrow('Access is denied.');
      await expect(bridge.runPlan('C:\\', 'plan-1', () => {})).rejects.toThrow('Access is denied.');
    });
  });
});

import { afterEach, describe, expect, it, vi } from 'vitest';
import { createMockBridge, fixtures, type ScanUpdate } from '.';

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
});

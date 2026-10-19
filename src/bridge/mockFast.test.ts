import { afterEach, describe, expect, it, vi } from 'vitest';
import { createMockBridge, defaultBridge, fixtures, type MockOptions, type ScanUpdate } from '.';

afterEach(() => {
  vi.useRealTimers();
});

/** Scans a drive of the Windows fixture with the mock, fast or not, and keeps every update. */
async function scan(options: MockOptions, mountPoint = 'C:\\', fast = true) {
  vi.useFakeTimers();
  const bridge = createMockBridge(fixtures.windows, { scanMs: 1000, uacMs: 500, ...options });
  const updates: ScanUpdate[] = [];
  const done = bridge.scan(mountPoint, (u) => updates.push(u), { fast });
  await vi.advanceTimersByTimeAsync(2000);
  const outcome = await done;
  if (outcome.status !== 'done') throw new Error('the scan did not finish');
  return { updates, overview: outcome.overview };
}

describe('the mock bridge’s fast scan', () => {
  it('asks for admin, reads the index, sorts it, and ends with the locked folders and the records', async () => {
    const { updates, overview } = await scan({});
    expect([...new Set(updates.map((u) => u.stage))]).toEqual(['asking', 'reading', 'sorting']);
    expect(updates.every((u) => u.method === 'fast' && u.fallback === null)).toBe(true);
    // Nothing is mapped before the sort: the map fills in once it starts.
    expect(updates.filter((u) => u.stage !== 'sorting').every((u) => u.categories === null && u.bytes === 0)).toBe(true);
    const reading = updates.filter((u) => u.stage === 'reading');
    expect(reading.length).toBeGreaterThan(1);
    expect(reading.every((u) => u.index !== null && u.index.done <= u.index.total)).toBe(true);
    expect(updates.at(-1)!.categories!.length).toBeGreaterThan(3);

    expect(overview.scan).toMatchObject({ method: 'fast', fallback: null, unreadable: 0, unreadableSamples: [], hiddenBytes: 0 });
    expect(overview.scan.locked.map((g) => g.kind)).toEqual(['otherAccounts', 'windowsOnly']);
    const items = overview.categories.flatMap((c) => c.items);
    expect(items.some((i) => i.kind === 'hidden')).toBe(false);
    expect(items.find((i) => i.kind === 'records')?.name).toBe('File system records');
  });

  it('scales the locked folders to another NTFS drive', async () => {
    const { overview } = await scan({}, 'D:\\');
    const [c, d] = [fixtures.windows.volumes[0]!, fixtures.windows.volumes[1]!];
    expect(overview.scan.method).toBe('fast');
    expect(overview.scan.locked[0]!.bytes).toBeCloseTo((21.4e9 * d.usedBytes) / c.usedBytes, -3);
  });

  it.each([
    ['declined', 'declined'],
    ['failed', 'helperFailed'],
    ['unreadable', 'indexUnreadable'],
  ] as const)('runs the normal scan after the prompt when it is %s, saying why', async (fast, fallback) => {
    const { updates, overview } = await scan({ fast });
    expect(updates[0]!.stage).toBe('asking');
    const normal = updates.filter((u) => u.stage !== 'asking');
    expect(normal.length).toBeGreaterThan(3);
    expect(normal.every((u) => u.method === 'normal' && u.fallback === fallback && u.stage === null)).toBe(true);
    expect(overview.scan).toMatchObject({ method: 'normal', fallback, locked: [] });
  });

  it('runs the normal scan of a drive that isn’t NTFS straight away, with no prompt', async () => {
    const { updates, overview } = await scan({}, 'E:\\');
    expect(updates.some((u) => u.stage === 'asking')).toBe(false);
    expect(updates.every((u) => u.method === 'normal' && u.fallback === 'notNtfs')).toBe(true);
    expect(overview.scan.fallback).toBe('notNtfs');
  });

  it('leaves a normal scan as it was', async () => {
    const { updates, overview } = await scan({ fast: 'declined' }, 'C:\\', false);
    expect(updates.every((u) => u.method === 'normal' && u.fallback === null && u.stage === null)).toBe(true);
    expect(overview.scan).toMatchObject({ method: 'normal', fallback: null, locked: [], recordsBytes: 0 });
  });

  it('takes its answers from the page’s address', async () => {
    vi.useFakeTimers();
    const bridge = defaultBridge('?fixture=windows&scan=1000&fast=declined&uac=300');
    const updates: ScanUpdate[] = [];
    const done = bridge.scan('C:\\', (u) => updates.push(u), { fast: true });
    await vi.advanceTimersByTimeAsync(250);
    expect(updates.at(-1)!.stage).toBe('asking');
    await vi.advanceTimersByTimeAsync(1500);
    await done;
    expect(updates.at(-1)!.fallback).toBe('declined');
  });
});

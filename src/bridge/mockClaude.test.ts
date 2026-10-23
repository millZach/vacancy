import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createMockBridge,
  defaultBridge,
  fixtures,
  type ClaudeAnswer,
  type ClaudeEvent,
  type ClaudeJob,
  type FixtureName,
  type MockOptions,
} from '.';

afterEach(() => {
  vi.useRealTimers();
});

/** A mock bridge with a finished scan of the system drive. */
async function scanned(name: FixtureName = 'windows', options: MockOptions = {}) {
  vi.useFakeTimers();
  const fixture = fixtures[name];
  const bridge = createMockBridge(fixture, { scanMs: 0, runMs: 0, ...options });
  const mountPoint = fixture.volumes[0]!.mountPoint;
  const scan = bridge.scan(mountPoint, () => {});
  await vi.advanceTimersByTimeAsync(10);
  const outcome = await scan;
  if (outcome.status !== 'done') throw new Error('the scan did not finish');
  return { bridge, mountPoint, outcome, fixture };
}

/** Starts a job and plays it to the end, keeping every event. */
async function play(bridge: Awaited<ReturnType<typeof scanned>>['bridge'], mountPoint: string, job: ClaudeJob, ms = 10_000) {
  const events: ClaudeEvent[] = [];
  const id = await bridge.claudeStart(mountPoint, job, (e) => events.push(e));
  await vi.advanceTimersByTimeAsync(ms);
  expect(events.every((e) => e.requestId === id)).toBe(true);
  return events;
}

/** A mock answer, with the fake clock moved on until it comes. */
async function settle<T>(promise: Promise<T>): Promise<T> {
  await vi.advanceTimersByTimeAsync(10);
  return promise;
}

function done(events: ClaudeEvent[]): ClaudeAnswer {
  const last = events.at(-1);
  if (last?.kind !== 'done') throw new Error(`the request ended ${JSON.stringify(last)}`);
  return last.answer;
}

describe('the mock bridge’s Claude', () => {
  it('is off until turned on, with privacy mode on and the always-hidden places listed', async () => {
    const bridge = createMockBridge(fixtures.windows, { claudeMs: 0 });
    const status = await bridge.claudeStatus();
    expect(status).toMatchObject({ state: 'off', privacy: true, fixture: false });
    expect(status.hidden.map((h) => h.group)).toEqual(['keys', 'browser', 'vaults']);
    expect((await bridge.claudeSetEnabled(true)).state).toBe('on');
    expect((await bridge.claudeSetPrivacy(false)).privacy).toBe(false);
    // Turning Claude off keeps the privacy choice.
    const off = await bridge.claudeSetEnabled(false);
    expect(off).toMatchObject({ state: 'off', privacy: false });
  });

  it.each([
    ['notfound', 'notFound'],
    ['signedout', 'signedOut'],
    ['on', 'on'],
    ['slow', 'on'],
    ['error', 'on'],
    ['timeout', 'on'],
  ] as const)('starts %s as %s', async (claude, state) => {
    const bridge = createMockBridge(fixtures.windows, { claude, claudeMs: 0 });
    if (claude === 'signedout') await bridge.claudeSetEnabled(true);
    expect((await bridge.claudeStatus()).state).toBe(state);
  });

  it('reads its state and privacy mode from the URL', async () => {
    const bridge = defaultBridge('?claude=signedout&privacy=off');
    const status = await bridge.claudeStatus();
    expect(status).toMatchObject({ state: 'signedOut', privacy: false });
  });

  it('reviews the plan: flags with reasons on its items, and adds nothing', async () => {
    const { bridge, mountPoint, outcome } = await scanned('windows', { claude: 'on' });
    const ids = outcome.proposals.filter((p) => p.defaultOn || p.id === 'w-installers').map((p) => p.id);
    const events = await play(bridge, mountPoint, { job: 'reviewPlan', ids });
    expect(events.map((e) => e.kind)).toEqual(['working', 'done']);
    const answer = done(events);
    if (answer.result.kind !== 'review') throw new Error('a review');
    expect(answer.result.flags.map((f) => f.id)).toContain('w-installers');
    expect(answer.result.flags.every((f) => ids.includes(f.id) && f.reason.length > 0)).toBe(true);
    expect(answer.refreshed).toBeNull();
    expect(answer.basis).toMatchObject({ privacy: true, opened: [] });
  });

  it('picks a plan: unticks, ticks and adds its own items unticked, with the Guard’s refusals named', async () => {
    const { bridge, mountPoint, outcome } = await scanned('windows', { claude: 'on' });
    const ids = outcome.proposals.filter((p) => p.defaultOn || p.id === 'w-installers').map((p) => p.id);
    const answer = done(await play(bridge, mountPoint, { job: 'pickPlan', ids }));
    if (answer.result.kind !== 'pick') throw new Error('a pick');
    const changes = answer.result.changes.map((c) => [c.id, c.change]);
    expect(changes).toContainEqual(['w-installers', 'untick']);
    expect(changes).toContainEqual(['w-steam-advice', 'tick']);
    expect(changes).toContainEqual(['claude-android', 'add']);
    expect(answer.result.plan).not.toContain('claude-android');
    expect(answer.result.bytesAfter).toBeLessThan(answer.result.bytesBefore);
    expect(answer.dropped.map((d) => d.name)).toContain('Windows Update downloads');
    const added = answer.refreshed!.proposals.find((p) => p.id === 'claude-android')!;
    expect(added).toMatchObject({ source: 'claude', risk: 'review', defaultOn: false });
    expect(added.claudeNote).toMatch(/^Suggested by Claude: /);
    // Its tile is carved out of the folder it is in.
    const tiles = answer.refreshed!.overview.categories.flatMap((c) => c.items);
    expect(tiles.some((t) => t.proposal === 'claude-android')).toBe(true);
    // A plan made with it marks the step Claude's; Keep mine takes it back out.
    const plan = await settle(bridge.buildPlan(mountPoint, ['claude-android']));
    expect(plan.steps[0]).toMatchObject({ id: 'claude-android', source: 'claude' });
    const kept = await bridge.claudeRemoveItems(mountPoint, ['claude-android', 'claude-public']);
    expect(kept.proposals.some((p) => p.source === 'claude')).toBe(false);
    await expect(bridge.claudeRemoveItems(mountPoint, ['w-node'])).rejects.toThrow('isn’t Claude’s');
  });

  it('chats about an item, saying what the answer was based on in each privacy mode', async () => {
    const { bridge, mountPoint } = await scanned('windows', { claude: 'on' });
    const events: ClaudeEvent[] = [];
    await bridge.claudeChat(mountPoint, { kind: 'proposal', id: 'w-node' }, [{ from: 'person', text: 'Why is this risky?', privacy: true }], (e) =>
      events.push(e),
    );
    await vi.advanceTimersByTimeAsync(2000);
    const answer = done(events);
    if (answer.result.kind !== 'ask') throw new Error('a reply');
    expect(answer.result.reply).toMatch(/61 JavaScript projects/);
    expect(answer.result.followUps.length).toBeGreaterThan(0);
    expect(answer.basis.text).toMatch(/names, sizes and dates/);
    await bridge.claudeSetPrivacy(false);
    const open = done(await play(bridge, mountPoint, { job: 'ask', about: { kind: 'proposal', id: 'w-node' }, messages: [{ from: 'person', text: 'What is it?', privacy: true }] }));
    expect(open.basis.privacy).toBe(false);
    expect(open.basis.opened[0]).toMatch(/package\.json/);
  });

  it('explains big folders and adds its suggestions unticked', async () => {
    const { bridge, mountPoint } = await scanned('windows', { claude: 'on' });
    const answer = done(await play(bridge, mountPoint, { job: 'explainFolders' }));
    if (answer.result.kind !== 'explain') throw new Error('an explanation');
    const sdk = answer.result.folders.find((f) => f.name === 'Sdk')!;
    expect(sdk.suggested).toEqual(['claude-android']);
    expect(answer.refreshed!.proposals.find((p) => p.id === 'claude-android')?.defaultOn).toBe(false);
  });

  it('suggests Tidy up layouts beside the built-in ones, and a plan uses the one picked', async () => {
    const { bridge, mountPoint, outcome } = await scanned('windows', { claude: 'on' });
    const answer = done(await play(bridge, mountPoint, { job: 'tidyLayout', ids: [] }));
    if (answer.result.kind !== 'tidy') throw new Error('layouts');
    expect(answer.result.layouts).toHaveLength(outcome.tidy.length);
    expect(answer.result.layouts.every((l) => l.layout === 'claude' && l.note)).toBe(true);
    expect(answer.refreshed!.tidy.every((t) => t.layout === 'builtin')).toBe(true);
    const sorts = await bridge.claudeChooseLayout(mountPoint, 'tidy-downloads', 'claude');
    expect(sorts.find((t) => t.id === 'tidy-downloads')!.layout).toBe('claude');
    const plan = await settle(bridge.buildPlan(mountPoint, ['tidy-downloads']));
    expect(plan.steps.find((s) => s.id === 'tidy-downloads')).toMatchObject({ kind: 'move', source: 'claude' });
    const back = await bridge.claudeChooseLayout(mountPoint, 'tidy-downloads', 'builtin');
    expect(back.find((t) => t.id === 'tidy-downloads')!.layout).toBe('builtin');
  });

  it('has no Tidy up layouts on a Mac', async () => {
    const { bridge, mountPoint } = await scanned('macos', { claude: 'on' });
    const events = await play(bridge, mountPoint, { job: 'tidyLayout', ids: [] });
    expect(events.at(-1)).toMatchObject({ kind: 'failed', failure: { kind: 'error' } });
    const review = done(await play(bridge, mountPoint, { job: 'reviewPlan', ids: ['m-archives', 'm-derived'] }));
    if (review.result.kind !== 'review') throw new Error('a review');
    expect(review.result.flags.map((f) => f.id)).toEqual(['m-archives']);
  });

  it.each([
    ['notfound', 'notFound'],
    ['signedout', 'signedOut'],
    ['timeout', 'timedOut'],
    ['error', 'error'],
  ] as const)('fails a request when Claude is %s', async (claude, kind) => {
    const { bridge, mountPoint } = await scanned('windows', { claude });
    const on = bridge.claudeSetEnabled(true);
    await vi.advanceTimersByTimeAsync(1500);
    await on;
    const events = await play(bridge, mountPoint, { job: 'explainFolders' });
    expect(events[0]!.kind).toBe('working');
    expect(events.at(-1)).toMatchObject({ kind: 'failed', failure: { kind } });
    if (kind === 'error') expect(JSON.stringify(events.at(-1))).toMatch(/429/);
  });

  it('says a slow answer is slow, and cancels', async () => {
    const { bridge, mountPoint } = await scanned('windows', { claude: 'slow' });
    const events: ClaudeEvent[] = [];
    const id = await bridge.claudeStart(mountPoint, { job: 'explainFolders' }, (e) => events.push(e));
    await vi.advanceTimersByTimeAsync(2000);
    expect(events.map((e) => e.kind)).toEqual(['working', 'slow']);
    await bridge.claudeCancel(id);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(events.map((e) => e.kind)).toEqual(['working', 'slow', 'failed']);
    expect(events.at(-1)).toMatchObject({ failure: { kind: 'cancelled' } });
  });
});

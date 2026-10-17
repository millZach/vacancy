import { describe, expect, it } from 'vitest';
import { NBSP } from '../format';
import { fullReport, hardLinked, report } from '../test/run';
import { boardText, diskSub, headlineSub, keptOf, slidesFor } from './deck';

const plain = (s: string) => s.replaceAll(NBSP, ' ');

describe('the slides a run gets', () => {
  it('all seven when something moved; no Moved folders when nothing did', () => {
    expect(slidesFor(fullReport).map((s) => s.id)).toEqual(['freed', 'disk', 'win', 'categories', 'moved', 'kept', 'checkout']);
    expect(slidesFor(report).map((s) => s.id)).toEqual(['freed', 'disk', 'win', 'categories', 'kept', 'checkout']);
  });

  it('no disk slide when the drive’s own figures couldn’t be read', () => {
    expect(slidesFor({ ...report, totalBytes: 0 }).map((s) => s.id)).not.toContain('disk');
  });
});

describe('what the slides say', () => {
  it('puts what was freed on the letterboard', () => {
    expect(boardText(26.89e9)).toBe('26.9 GB');
    expect(boardText(412e6)).toBe('412 MB');
  });

  it('says what the run did, or that it was stopped, or that nothing could go', () => {
    expect(plain(headlineSub(report))).toBe('Local Disk (C:) has room again. Vacancy deleted 460,216 items in 1 min 12 s.');
    expect(plain(headlineSub({ ...report, cancelled: true }))).toBe(
      'You stopped the run part way. Before it stopped, Vacancy deleted 460,216 items in 1 min 12 s.',
    );
    expect(headlineSub({ ...report, freedBytes: 0, deletedItems: 0 })).toBe(
      'Local Disk (C:) is as it was: nothing in the plan could go this time. The next slides say why.',
    );
    expect(plain(headlineSub({ ...report, deletedItems: 1 }))).toContain('deleted 1 item in');
  });

  it('says how much more room there is, and how full the drive is now', () => {
    expect(diskSub(report)).toBe('That’s 1.6 times the room you had. Local Disk (C:) is 85% full now, down from 91%.');
    const little = { ...report, freeAfter: 50e9, usedAfter: 426e9 };
    expect(plain(diskSub(little))).toBe('That’s 5.0 GB more room. Local Disk (C:) is 89% full now, down from 91%.');
    const none = { ...report, freeAfter: 44e9, usedAfter: 432e9 };
    expect(diskSub(none)).toBe(
      'Other apps wrote to the drive while it ran, so free space didn’t grow. Local Disk (C:) is 91% full now.',
    );
  });

  it('keeps notes about space still in use apart from what was left, and a stopped run’s places together', () => {
    const stopped = { path: 'C:\\b', reason: 'You stopped the run before it got here', kind: 'cancelled' as const, bytes: 1, items: 1 };
    const r = { ...fullReport, steps: [...fullReport.steps, { ...fullReport.steps[1]!, id: 'x', skipped: [stopped] }] };
    const kept = keptOf(r);
    expect(kept.notes).toEqual([hardLinked]);
    expect(kept.left.map((k) => k.kind)).toEqual(['inUse']);
    expect(kept.stopped).toEqual([stopped]);
  });
});

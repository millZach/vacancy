import { describe, expect, it } from 'vitest';
import type { RunEvent, RunReport, Skip } from '../bridge';
import { inUse, nodeDone, plan, stepReport } from '../test/run';
import { asSentence, foldEvents, inSentence, LOG_KEEP, settleRun, startRun, stepLine } from './progress';

const GB = 1e9;

describe('a run as the run panel shows it', () => {
  it('starts with every step waiting, nothing freed and an empty log', () => {
    const v = startRun(plan);
    expect(v.steps.map((s) => [s.id, s.status, s.plannedBytes])).toEqual([
      ['w-node', 'pending', 21 * GB],
      ['w-browser', 'pending', 6.3 * GB],
    ]);
    expect(v.freedBytes).toBe(0);
    expect(v.log).toEqual([]);
  });

  it('follows a step from start to finish, and logs each place and the step’s line', () => {
    const events: RunEvent[] = [
      { kind: 'stepStarted', step: 0, id: 'w-node' },
      { kind: 'progress', step: 0, freedBytes: 5 * GB, deletedItems: 1000, totalFreedBytes: 5 * GB, currentPath: 'C:\\a' },
      { kind: 'deleted', step: 0, path: 'C:\\Users\\alex\\code\\web-shop\\node_modules', bytes: 5 * GB, items: 1000 },
    ];
    const mid = foldEvents(startRun(plan), events);
    expect(mid.steps[0]).toMatchObject({ status: 'running', freedBytes: 5 * GB, deletedItems: 1000 });
    expect(mid.steps[1]!.status).toBe('pending');
    expect(mid.freedBytes).toBe(5 * GB);
    expect(mid.log.map((l) => l.text)).toEqual(['deleted C:\\Users\\alex\\code\\web-shop\\node_modules']);

    const done = foldEvents(mid, [{ kind: 'stepFinished', step: 0, report: nodeDone }]);
    expect(done.steps[0]).toMatchObject({ status: 'done', outcome: 'done', freedBytes: 21 * GB });
    expect(done.freedBytes).toBe(21 * GB);
    expect(done.log.at(-1)).toMatchObject({ kind: 'sum', text: 'node_modules in 61 idle projects: 21.0\u00a0GB freed' });
  });

  it('shows a skip as it happens, in its step and the log', () => {
    const v = foldEvents(startRun(plan), [
      { kind: 'stepStarted', step: 1, id: 'w-browser' },
      { kind: 'skipped', step: 1, skip: inUse },
      { kind: 'skipped', step: 1, skip: { ...inUse, path: 'C:\\x.exe', reason: 'It changed since the scan', kind: 'changed' } },
    ]);
    expect(v.current).toBe(1);
    expect(v.skips).toBe(2);
    expect(v.steps[1]!.skips.map((k) => k.reason)).toEqual(['212 files are open in Microsoft Edge', 'It changed since the scan']);
    expect(v.log.map((l) => [l.kind, l.text])).toEqual([
      ['skip', `skipped ${inUse.path} (212 files are open in Microsoft Edge)`],
      ['skip', 'skipped C:\\x.exe (it changed since the scan)'],
    ]);
  });

  it('keeps only the newest log lines, numbered across the run', () => {
    const many: RunEvent[] = Array.from({ length: LOG_KEEP + 30 }, (_, i) => ({
      kind: 'deleted',
      step: 0,
      path: `C:\\p${i}`,
      bytes: 1,
      items: 1,
    }));
    const v = foldEvents(startRun(plan), many);
    expect(v.log).toHaveLength(LOG_KEEP);
    expect(v.log[0]!.text).toBe('deleted C:\\p30');
    expect(v.written).toBe(LOG_KEEP + 30);
    expect(new Set(v.log.map((l) => l.id)).size).toBe(LOG_KEEP);
  });

  it('ignores events for steps the plan doesn’t have, and changes nothing for no events', () => {
    const v = startRun(plan);
    expect(foldEvents(v, [])).toBe(v);
    const after = foldEvents(v, [{ kind: 'stepStarted', step: 9, id: 'nope' }]);
    expect(after.steps).toEqual(v.steps);
  });

  it('settles every step from the report, including steps a stop never reached', () => {
    const stopped: Skip = { path: 'C:\\b', reason: 'You stopped the run before it got here', kind: 'cancelled', bytes: 6.3 * GB, items: 48_310 };
    const report = {
      freedBytes: 21 * GB,
      steps: [
        nodeDone,
        stepReport({ id: 'w-browser', title: 'Browser caches', category: 'caches', outcome: 'skipped', skipped: [stopped] }),
      ],
    } as RunReport;
    const v = settleRun(foldEvents(startRun(plan), [{ kind: 'stepStarted', step: 0, id: 'w-node' }]), report);
    expect(v.steps.map((s) => [s.status, s.outcome])).toEqual([
      ['done', 'done'],
      ['done', 'skipped'],
    ]);
    expect(v.freedBytes).toBe(21 * GB);
    expect(v.skips).toBe(1);
  });
});

describe('the words around a run', () => {
  it('says what a step did in one line', () => {
    expect(stepLine(nodeDone)).toBe('node_modules in 61 idle projects: 21.0\u00a0GB freed');
    expect(stepLine({ ...nodeDone, skipped: [inUse] })).toBe('node_modules in 61 idle projects: 21.0\u00a0GB freed, 212 files left');
    expect(stepLine({ ...nodeDone, outcome: 'skipped', freedBytes: 0, skipped: [{ ...inUse, items: 1 }] })).toBe(
      'node_modules in 61 idle projects: nothing freed, 1 file left',
    );
  });

  it('puts a reason inside a sentence, or makes it one', () => {
    expect(inSentence('It changed since the scan')).toBe('it changed since the scan');
    expect(inSentence('You stopped the run before it got here')).toBe('you stopped the run before it got here');
    expect(inSentence('212 files are open in Microsoft Edge')).toBe('212 files are open in Microsoft Edge');
    expect(asSentence('212 files are open in Microsoft Edge')).toBe('212 files are open in Microsoft Edge.');
    expect(asSentence('It changed since the scan.')).toBe('It changed since the scan.');
  });
});

// The UI's one door to the Rust core. Inside the app it talks to Tauri; anywhere else (a plain browser, tests,
// screenshot scripts) it is a mock fed by fixture data. Pick a fixture in the browser with `?fixture=macos`, and
// add `&delay=1500` or `&fail=message` to see loading and error states, `&scan=8000` for a slower mock scan,
// `&fda=granted` to change the Mac's Full Disk Access answer, `&unplug=20000` to unplug the last drive (the USB
// stick on Windows) 20 seconds in, `&run=6000` for a mock run of a plan that takes six seconds, `&dups=8000` for a
// duplicate search after the scan that takes eight seconds (default six), `&moves=1` for runs
// that also report the fixture's folder moves (to see the finish's Moved slide), `&unreadable=2` for a history
// with two kept runs it can't read, and `&history=0` to start with no past runs. `&advisor=notChecked` previews
// native fixture bypass; `&advisor=error` fails only the advisor status read. These browser fixtures never enable AI.
import { isTauri } from '@tauri-apps/api/core';
import { fixtures, isFixtureName } from './fixtures';
import { createMockBridge } from './mock';
import { createTauriBridge } from './tauri';
import type { Bridge, FullDiskAccess } from './types';

export { BridgeProvider } from './BridgeProvider';
export { useBridge } from './context';
export { createMockBridge, type MockBridge, type MockOptions } from './mock';
export { fixtures, type Fixture, type FixtureName } from './fixtures';
export type * from './types';

const ACCESS: readonly FullDiskAccess[] = ['granted', 'missing', 'unknown', 'notApplicable'];
const ADVISOR_PREVIEW = ['notChecked', 'error'] as const;

/** The bridge for wherever the UI is running. */
export function defaultBridge(search = window.location.search): Bridge {
  if (isTauri()) return createTauriBridge();
  const q = new URLSearchParams(search);
  const name = q.get('fixture');
  const delay = Number(q.get('delay'));
  const fail = q.get('fail');
  const scan = q.get('scan');
  const fda = ACCESS.find((a) => a === q.get('fda'));
  const unplug = Number(q.get('unplug'));
  const run = q.get('run');
  const dups = q.get('dups');
  const unreadable = Number(q.get('unreadable'));
  const advisor = ADVISOR_PREVIEW.find((a) => a === q.get('advisor'));
  return createMockBridge(fixtures[isFixtureName(name) ? name : 'windows'], {
    ...(delay > 0 ? { delayMs: delay } : {}),
    ...(fail !== null ? { fail } : {}),
    ...(scan !== null && Number.isFinite(Number(scan)) ? { scanMs: Number(scan) } : {}),
    ...(fda ? { fullDiskAccess: fda } : {}),
    ...(unplug > 0 ? { unplugMs: unplug } : {}),
    ...(run !== null && Number.isFinite(Number(run)) ? { runMs: Number(run) } : {}),
    ...(dups !== null && Number.isFinite(Number(dups)) ? { dupsMs: Number(dups) } : {}),
    ...(q.get('moves') === '1' ? { moves: true } : {}),
    ...(unreadable > 0 ? { unreadableRuns: unreadable } : {}),
    ...(q.get('history') === '0' ? { pastRuns: false } : {}),
    ...(advisor ? { advisor } : {}),
  });
}

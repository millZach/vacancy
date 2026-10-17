// The UI's one door to the Rust core. Inside the app it talks to Tauri; anywhere else (a plain browser, tests,
// screenshot scripts) it is a mock fed by fixture data. Pick a fixture in the browser with `?fixture=macos`, and
// add `&delay=1500` or `&fail=message` to see loading and error states, `&scan=8000` for a slower mock scan,
// `&fda=granted` to change the Mac's Full Disk Access answer, `&unplug=20000` to unplug the last drive (the USB
// stick on Windows) 20 seconds in, and `&run=6000` for a mock run of a plan that takes six seconds.
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
  return createMockBridge(fixtures[isFixtureName(name) ? name : 'windows'], {
    ...(delay > 0 ? { delayMs: delay } : {}),
    ...(fail !== null ? { fail } : {}),
    ...(scan !== null && Number.isFinite(Number(scan)) ? { scanMs: Number(scan) } : {}),
    ...(fda ? { fullDiskAccess: fda } : {}),
    ...(unplug > 0 ? { unplugMs: unplug } : {}),
    ...(run !== null && Number.isFinite(Number(run)) ? { runMs: Number(run) } : {}),
  });
}

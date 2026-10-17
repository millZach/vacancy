// The UI's one door to the Rust core. Inside the app it talks to Tauri; anywhere else (a plain browser, tests,
// screenshot scripts) it is a mock fed by fixture data. Pick a fixture in the browser with `?fixture=macos`, and
// add `&delay=1500` or `&fail=message` to see loading and error states.
import { isTauri } from '@tauri-apps/api/core';
import { fixtures, isFixtureName } from './fixtures';
import { createMockBridge } from './mock';
import { createTauriBridge } from './tauri';
import type { Bridge } from './types';

export { BridgeProvider } from './BridgeProvider';
export { useBridge } from './context';
export { createMockBridge } from './mock';
export { fixtures, type Fixture, type FixtureName } from './fixtures';
export type { Bridge, Platform, VolumeInfo } from './types';

/** The bridge for wherever the UI is running. */
export function defaultBridge(search = window.location.search): Bridge {
  if (isTauri()) return createTauriBridge();
  const q = new URLSearchParams(search);
  const name = q.get('fixture');
  const delay = Number(q.get('delay'));
  const fail = q.get('fail');
  return createMockBridge(fixtures[isFixtureName(name) ? name : 'windows'], {
    ...(delay > 0 ? { delayMs: delay } : {}),
    ...(fail !== null ? { fail } : {}),
  });
}

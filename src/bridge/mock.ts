import type { Fixture } from './fixtures';
import type { Bridge } from './types';

export interface MockOptions {
  /** Answer after this many milliseconds, to see loading states. Default: next tick. */
  delayMs?: number;
  /** Make every call fail with this message, to see error states. */
  fail?: string;
}

/** A bridge that answers from fixture data. Used in a plain browser (`npm run dev`), in tests and in screenshots. */
export function createMockBridge(fixture: Fixture, options: MockOptions = {}): Bridge {
  const answer = <T>(value: T) =>
    new Promise<T>((resolve, reject) => {
      setTimeout(() => {
        if (options.fail !== undefined) reject(new Error(options.fail));
        else resolve(structuredClone(value));
      }, options.delayMs ?? 0);
    });
  return {
    platform: fixture.platform,
    systemVolume: () => answer(fixture.systemVolume),
    machineName: () => answer(fixture.machineName),
  };
}

import type { ReactNode } from 'react';
import { BridgeContext } from './context';
import type { Bridge } from './types';

/** Gives everything below it the bridge to use: the real one in the app, a mock in tests and in a plain browser. */
export function BridgeProvider({ bridge, children }: { bridge: Bridge; children: ReactNode }) {
  return <BridgeContext value={bridge}>{children}</BridgeContext>;
}

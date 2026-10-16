import { createContext, use } from 'react';
import type { Bridge } from './types';

export const BridgeContext = createContext<Bridge | null>(null);

/** The bridge to the Rust core. Throws outside a `BridgeProvider`, so a missing provider fails loudly. */
export function useBridge(): Bridge {
  const bridge = use(BridgeContext);
  if (!bridge) throw new Error('useBridge() needs a <BridgeProvider> above it');
  return bridge;
}

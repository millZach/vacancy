// PROTOTYPE variant D "Atlas + Wrapped": A's map for scan, overview, plan and run (with a living dot field on the scan
// screen), then C's story slides restyled in A's look, handing off to A's before/after map summary. See Atlas.tsx.
// The drive picker (?picker=1|2|3) wraps Atlas: one scan, map, plan and summary per drive. See drives/Host.tsx.
import type { VariantModule } from '../../shared/types';
import { DriveAtlas } from './drives/Host';

export const variant: VariantModule = {
  key: 'D',
  name: 'Atlas + Wrapped',
  frame: { bg: '#12161c', fg: '#c3ccd8', border: '#232a34' },
  Component: DriveAtlas,
};

// PROTOTYPE variant A "Atlas": map-first. See Atlas.tsx.
import type { VariantModule } from '../../shared/types';
import { Atlas } from './Atlas';

export const variant: VariantModule = {
  key: 'A',
  name: 'Atlas',
  frame: { bg: '#12161c', fg: '#c3ccd8', border: '#232a34' },
  Component: Atlas,
};

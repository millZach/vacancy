// PROTOTYPE contract between the shell (App.tsx) and each variant.
import type { Dataset } from './data';

/** The flow every variant covers. A variant may merge steps visually, but must render something sensible for each key. */
export type Step = 'scan' | 'overview' | 'plan' | 'run' | 'summary';
export const STEPS: Step[] = ['scan', 'overview', 'plan', 'run', 'summary'];

export interface VariantProps {
  data: Dataset;
  step: Step;
  onStep: (s: Step) => void;
  /** Selection lives in the shell so jumping straight to ?step=summary shows the default plan's result. */
  proposals: Set<string>;
  setProposals: (next: Set<string>) => void;
  reorgs: Set<string>;
  setReorgs: (next: Set<string>) => void;
}

/** Colours for the fake OS title bar around the variant. */
export interface FrameTheme {
  bg: string;
  fg: string;
  border: string;
}

export interface VariantModule {
  key: string;
  name: string;
  frame: FrameTheme;
  Component: (p: VariantProps) => React.ReactNode;
}

import type { Proposal } from '../bridge';

/** What a plan adds up to: the delete proposals in it. */
export interface PlanTotals {
  /** Bytes it frees. */
  bytes: number;
  /** Files it removes. */
  items: number;
  /** Places they take things from: folders, files and folders' loose files. */
  places: number;
}

/** The totals of the proposals in `plan`. Advice and tools never count toward freed space. */
export function planTotals(proposals: readonly Proposal[], plan: readonly string[]): PlanTotals {
  const chosen = proposals.filter((p) => p.action === 'delete' && plan.includes(p.id));
  return {
    bytes: chosen.reduce((sum, p) => sum + p.bytes, 0),
    items: chosen.reduce((sum, p) => sum + p.itemCount, 0),
    places: chosen.reduce((sum, p) => sum + p.placeCount, 0),
  };
}

/** The map tile id of a proposal. The core names proposal tiles this way. */
export const tileOf = (proposalId: string) => `p:${proposalId}`;

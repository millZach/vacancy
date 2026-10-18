import type { DuplicateGroup, Proposal } from '../bridge';
import { DUPLICATES, groupTotals, included } from './duplicates';

/** What a plan adds up to: the delete proposals in it. */
export interface PlanTotals {
  /** Bytes it frees. */
  bytes: number;
  /** Files it removes. */
  items: number;
  /** Places they take things from: folders, files and folders' loose files. */
  places: number;
}

/** The duplicate groups found, and the ones the user unticked. */
export interface GroupChoice {
  groups: readonly DuplicateGroup[];
  skipped: readonly string[];
}

/**
 * The totals of the proposals in `plan`. Advice and tools never count toward freed space. Duplicate files count
 * only the groups `duplicates` keeps in: an unticked group deletes nothing.
 */
export function planTotals(
  proposals: readonly Proposal[],
  plan: readonly string[],
  duplicates?: GroupChoice,
): PlanTotals {
  const chosen = proposals.filter((p) => p.action === 'delete' && plan.includes(p.id));
  const of = (p: Proposal) => {
    if (p.id !== DUPLICATES || !duplicates) return { bytes: p.bytes, items: p.itemCount, places: p.placeCount };
    const t = groupTotals(included(duplicates.groups, duplicates.skipped));
    return { bytes: t.bytes, items: t.copies, places: t.copies };
  };
  return {
    bytes: chosen.reduce((sum, p) => sum + of(p).bytes, 0),
    items: chosen.reduce((sum, p) => sum + of(p).items, 0),
    places: chosen.reduce((sum, p) => sum + of(p).places, 0),
  };
}

/**
 * The map tile id of a proposal. The core names proposal tiles this way; a proposal drawn as several tiles (the
 * duplicates, one per folder) names each `p:<id>:<folder>`, and this id stands for all of them.
 */
export const tileOf = (proposalId: string) => `p:${proposalId}`;

// The duplicate finder as the plan sees it: which groups are in, what they add up to, and how to say it.
import type { DuplicateGroup, FileKind, TidyProposal } from '../bridge';

/** The duplicates proposal's id, as the core names it. */
export const DUPLICATES = 'duplicates';

/** Kinds as the duplicates panel's filter groups them. */
export type Bucket = 'Photos' | 'Videos' | 'Other files';
export const BUCKETS: readonly Bucket[] = ['Photos', 'Videos', 'Other files'];

export function bucketOf(kind: FileKind): Bucket {
  return kind === 'photo' ? 'Photos' : kind === 'video' ? 'Videos' : 'Other files';
}

/** The groups the user hasn't unticked. */
export function included(groups: readonly DuplicateGroup[], skipped: readonly string[]): DuplicateGroup[] {
  return groups.filter((g) => !skipped.includes(g.id));
}

/** What the included groups add up to: bytes freed and extra copies deleted. */
export function groupTotals(groups: readonly DuplicateGroup[]): { bytes: number; copies: number } {
  return {
    bytes: groups.reduce((s, g) => s + g.freed, 0),
    copies: groups.reduce((n, g) => n + g.extras.length, 0),
  };
}

/** "24 photos, 8 videos and 9 other files": extra copies, photos and videos counted first. */
export function countLine(groups: readonly DuplicateGroup[]): string {
  const files = (want: (k: FileKind) => boolean) =>
    groups.filter((g) => want(g.kind)).reduce((n, g) => n + g.extras.length, 0);
  const photos = files((k) => k === 'photo');
  const videos = files((k) => k === 'video');
  const other = files((k) => k !== 'photo' && k !== 'video');
  const n = (x: number) => x.toLocaleString('en-US');
  const parts = [
    photos ? `${n(photos)} ${photos === 1 ? 'photo' : 'photos'}` : null,
    videos ? `${n(videos)} ${videos === 1 ? 'video' : 'videos'}` : null,
    other ? `${n(other)} other ${other === 1 ? 'file' : 'files'}` : null,
  ].filter((p): p is string => p !== null);
  return parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}` : (parts[0] ?? 'No files');
}

/**
 * The sorts as a plan with duplicate files runs them: the core marks every extra copy's move as deleted by
 * duplicates, but an unticked group deletes nothing, so its extra copies are sorted like any file (delete wins over
 * sort only while a delete that takes the file is in the plan).
 */
export function sortable(
  tidy: readonly TidyProposal[],
  groups: readonly DuplicateGroup[],
  skipped: readonly string[],
): TidyProposal[] {
  const kept = new Set(
    groups.filter((g) => skipped.includes(g.id)).flatMap((g) => g.extras.map((e) => e.path.toLowerCase())),
  );
  if (kept.size === 0) return [...tidy];
  return tidy.map((t) => ({
    ...t,
    moves: t.moves.map((m) =>
      m.deletedBy.includes(DUPLICATES) && kept.has(`${t.path}\\${m.name}`.toLowerCase())
        ? { ...m, deletedBy: m.deletedBy.filter((id) => id !== DUPLICATES) }
        : m,
    ),
  }));
}

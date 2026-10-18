// What the highlight slides say, worked out from the run's report alone: which slides it gets, and their sentences.
// Kept apart from the components so it can be tested without rendering, and so the slides and the summary panel
// can never disagree: both read the same report fields, and nothing here adds anything up.
import type { CategoryId, MapCategory, RunReport, Skip, StepReport } from '../bridge';
import { formatBytes, formatCount, formatDuration, NBSP } from '../format';

export type SlideId = 'freed' | 'disk' | 'win' | 'categories' | 'moved' | 'kept' | 'checkout';

export interface Slide {
  id: SlideId;
  /** What a screen reader hears for it, and the dot's name. */
  label: string;
}

const ALL: readonly Slide[] = [
  { id: 'freed', label: 'Space freed' },
  { id: 'disk', label: 'Disk before and after' },
  { id: 'win', label: 'Biggest win' },
  { id: 'categories', label: 'Freed by category' },
  { id: 'moved', label: 'Moved folders' },
  { id: 'kept', label: 'Skipped and left for you' },
  { id: 'checkout', label: 'Checkout' },
];

/**
 * The slides for a run, in order. Moved folders shows only when the run moved something; the disk slide only when
 * the drive's own figures were read (they come from the OS, and a run can end without them).
 */
export function slidesFor(r: RunReport): Slide[] {
  return ALL.filter((s) => {
    if (s.id === 'moved') return r.movedItems > 0;
    if (s.id === 'disk') return r.totalBytes > 0;
    return true;
  });
}

/** "26.9 GB" as letterboard tiles: upper case, the number and its unit apart. */
export function boardText(bytes: number): string {
  return formatBytes(bytes).replaceAll(NBSP, ' ').toUpperCase();
}

/** A count of things: "1 item", "460,216 items". */
export function count(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}

/** A saved reminder run must have no actual file changes, including separately reported moves. */
export function remindersOnly(r: RunReport): boolean {
  return r.leftForYou.length > 0 && r.steps.length === 0 && r.deletedItems === 0 && r.freedBytes === 0 &&
    r.movedItems === 0 && r.reorganized.every((move) => move.movedItems === 0);
}

/** The sentence under the sign. */
export function headlineSub(r: RunReport): string {
  const took = formatDuration(r.durationMs);
  if (r.freedBytes <= 0 && r.movedItems <= 0 && r.deletedItems <= 0) {
    if (remindersOnly(r)) return `${count(r.leftForYou.length, 'reminder')} saved. No files were deleted or moved, and no disk space was freed.`;
    return `${r.volumeName} is as it was: nothing in the plan could go this time. The next slides say why.`;
  }
  const did = [
    r.deletedItems > 0 ? `deleted ${count(r.deletedItems, 'item')}` : null,
    r.movedItems > 0 ? `sorted ${count(r.movedItems, 'file')}` : null,
  ]
    .filter(Boolean)
    .join(' and ');
  if (r.cancelled) return `You stopped the run part way. Before it stopped, Vacancy ${did} in ${took}.`;
  if (r.freedBytes <= 0) return `Vacancy ${did} in ${took}. No disk space was freed. The next slides show what changed and what stayed.`;
  return `${r.volumeName} has room again. Vacancy ${did} in ${took}.`;
}

/** How full the drive was, then is, as whole percents. */
export function fullness(r: RunReport): { before: number; after: number } {
  const pct = (used: number) => (r.totalBytes > 0 ? Math.round((used / r.totalBytes) * 100) : 0);
  return { before: pct(r.usedBefore), after: pct(r.usedAfter) };
}

/** The disk slide's last line: how much more room, and how full the drive is now. */
export function diskSub(r: RunReport): string {
  const growth = r.freeBefore > 0 ? r.freeAfter / r.freeBefore : 0;
  const { before, after } = fullness(r);
  const room =
    growth >= 1.5
      ? `That’s ${growth.toFixed(1)} times the room you had.`
      : r.freeAfter > r.freeBefore
        ? `That’s ${formatBytes(r.freeAfter - r.freeBefore)} more room.`
        : r.freeAfter === r.freeBefore ? 'Free space stayed the same.' : 'Other apps wrote to the drive while it ran, so free space didn’t grow.';
  return `${room} ${r.volumeName} is ${after}% full now${after < before ? `, down from ${before}%` : ''}.`;
}

/** The step that freed the most, from the report's own pick. */
export function biggestWin(r: RunReport): StepReport | undefined {
  return r.biggestWin === null ? undefined : r.steps.find((s) => s.id === r.biggestWin);
}

/** The biggest win's share of everything freed, as a whole percent. */
export function share(part: number, whole: number): number {
  return whole > 0 ? Math.round((part / whole) * 100) : 0;
}

/** A category of the map the run started from, by id. */
export function categoryOf(r: RunReport, id: CategoryId): MapCategory | undefined {
  return r.map.categories.find((c) => c.id === id);
}

/** What the run left in place, sorted for the Skipped slide and the summary. */
export interface Kept {
  /** Left in place, each with its reason. */
  left: Skip[];
  /** Left because the run was stopped before it got there: told as one line. */
  stopped: Skip[];
  /** Deleted, but its space is still in use (hard links kept elsewhere, an app holding it open): a quiet note. */
  notes: Skip[];
}

export function keptOf(r: RunReport): Kept {
  const all = r.steps.flatMap((s) => s.skipped);
  return {
    left: all.filter((k) => k.kind !== 'cancelled' && !isNote(k)),
    stopped: all.filter((k) => k.kind === 'cancelled'),
    notes: all.filter(isNote),
  };
}

/** A note about something deleted, not something left (the core's `SkipKind::is_note`). */
export function isNote(k: Skip): boolean {
  return k.kind === 'hardLinked' || k.kind === 'heldOpen';
}

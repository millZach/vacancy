// What a Tidy up proposal shows, worked out from the core's proposal and the plan alone: which moves are in the
// plan (delete wins over sort), the folders they fill, the before and after trees, sample moves, and what stays.
// Pure, so the tray, the drawer, the mock bridge and the tests agree. Nothing here adds anything the core didn't say.
import type { Reorganized, RunReport, Skip, TidyMove, TidyProposal, TidyStay } from '../bridge';
import { formatCount } from '../format';

/** The moves a plan makes for `t`: every move but those of files a delete proposal in the plan takes. */
export function activeMoves(t: TidyProposal, plan: readonly string[]): TidyMove[] {
  return t.moves.filter((m) => !m.deletedBy || !plan.includes(m.deletedBy));
}

/** Files of `t` the plan deletes instead: a delete proposal in it takes them too. */
export function goingInPlan(t: TidyProposal, plan: readonly string[]): TidyMove[] {
  return t.moves.filter((m) => !!m.deletedBy && plan.includes(m.deletedBy));
}

/** The top folder a move goes into: `Images` for `Images\2026`. */
export const topOf = (dest: string) => dest.split('\\')[0] ?? dest;

/** The top folders the moves fill, most files first: what the folder shows at its top after the sort. */
export function kindFolders(moves: readonly TidyMove[]): [name: string, files: number][] {
  const m = new Map<string, number>();
  for (const mv of moves) m.set(topOf(mv.dest), (m.get(topOf(mv.dest)) ?? 0) + 1);
  return [...m].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

/** Folders the sort makes: every destination folder and its parents that isn't there yet. */
export function foldersMade(t: TidyProposal, moves: readonly TidyMove[]): string[] {
  const made = new Set<string>();
  for (const mv of moves) {
    const parts = mv.dest.split('\\');
    for (let i = 1; i <= parts.length; i++) {
      const dir = parts.slice(0, i).join('\\');
      if (!t.existing.includes(dir)) made.add(dir);
    }
  }
  return [...made].sort();
}

/** One row of a before or after tree. */
export interface TreeNode {
  name: string;
  count?: number;
  /** A folder the sort makes, one the user has, Sort later, or files staying loose. */
  tag?: 'new' | 'existing' | 'later' | 'stays' | 'going';
  file?: boolean;
  children?: TreeNode[];
}

/** The folder as it is: its subfolders, then its loose files in one row. */
export function beforeTree(t: TidyProposal): TreeNode {
  return {
    name: t.name,
    children: [
      ...t.subfolders.map((s) => ({ name: s.name, count: s.files })),
      { name: `${t.loose.toLocaleString('en-US')} loose ${t.loose === 1 ? 'file' : 'files'}`, file: true, count: t.loose },
    ],
  };
}

/**
 * The folder after the sort: each top folder with its year folders inside (new ones tagged), the subfolders it
 * leaves as they are, then what stays loose and what the plan deletes instead.
 */
export function afterTree(t: TidyProposal, plan: readonly string[]): TreeNode {
  const moves = activeMoves(t, plan);
  const tops = new Map<string, TreeNode>();
  const subs = new Map<string, Map<string, number>>();
  for (const mv of moves) {
    const [top = mv.dest, ...rest] = mv.dest.split('\\');
    const node = tops.get(top) ?? {
      name: top,
      count: 0,
      tag: t.existing.includes(top) ? 'existing' : mv.kind === 'later' ? 'later' : 'new',
    };
    node.count = (node.count ?? 0) + 1;
    tops.set(top, node);
    if (rest.length > 0) {
      const inner = subs.get(top) ?? new Map<string, number>();
      const sub = rest.join('\\');
      inner.set(sub, (inner.get(sub) ?? 0) + 1);
      subs.set(top, inner);
    }
  }
  const children: TreeNode[] = [...tops.values()]
    .sort((a, b) => (b.count ?? 0) - (a.count ?? 0) || a.name.localeCompare(b.name))
    .map((node) => {
      const inner = subs.get(node.name);
      if (!inner) return node;
      return {
        ...node,
        children: [...inner]
          .sort((a, b) => b[0].localeCompare(a[0]))
          .map(([name, count]) => ({
            name,
            count,
            tag: t.existing.includes(`${node.name}\\${name}`) ? ('existing' as const) : ('new' as const),
          })),
      };
    });
  for (const s of t.subfolders) if (!tops.has(s.name)) children.push({ name: s.name, count: s.files });
  const stays = t.stays.length;
  if (stays > 0) children.push({ name: `${stays} ${stays === 1 ? 'file stays' : 'files stay'} loose`, file: true, count: stays, tag: 'stays' });
  const going = goingInPlan(t, plan).length;
  if (going > 0) children.push({ name: `${going} going in this plan`, file: true, count: going, tag: 'going' });
  return { name: t.name, children };
}

/** A few moves to show: the first file of a few folders, and up to two renamed ones (a name already taken). */
export function sampleMoves(moves: readonly TidyMove[], shown = 5): TidyMove[] {
  const renamed = moves.filter((m) => m.renamed).slice(0, 2);
  const firsts: TidyMove[] = [];
  const seen = new Set<string>();
  for (const m of moves) {
    if (m.renamed || seen.has(m.dest)) continue;
    seen.add(m.dest);
    firsts.push(m);
  }
  const plain = firsts.slice(0, Math.max(0, shown - renamed.length));
  return [...plain.slice(0, 3), ...renamed, ...plain.slice(3)];
}

/** What stays, grouped by reason: one line per reason, with the files it covers. */
export function staysByReason(stays: readonly TidyStay[]): { reason: string; dropped: boolean; names: string[] }[] {
  const out: { reason: string; dropped: boolean; names: string[] }[] = [];
  for (const s of stays) {
    const at = out.find((g) => g.reason === s.reason && g.dropped === s.dropped);
    if (at) at.names.push(s.name);
    else out.push({ reason: s.reason, dropped: s.dropped, names: [s.name] });
  }
  return out;
}

/** "A", "A and B", "A, B and C". */
export function listOf(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  return `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
}

/** What a plan's sorts add up to: files moved, and the folders sorted. */
export function tidyTotals(tidy: readonly TidyProposal[], plan: readonly string[]): { moves: number; folders: string[] } {
  const chosen = tidy.filter((t) => plan.includes(t.id));
  const counts = chosen.map((t) => [t.name, activeMoves(t, plan).length] as const).filter(([, n]) => n > 0);
  return { moves: counts.reduce((n, [, c]) => n + c, 0), folders: counts.map(([name]) => name) };
}

/** Whether `id` names a Tidy up proposal: the core gives them all this prefix. */
export const isTidy = (id: string) => id.startsWith('tidy-');

/** Undo can run for a folder now: it has a journal and a moved file that isn't back. */
export function canUndo(r: Reorganized): boolean {
  return r.undoable && r.journal !== '';
}

/** What a run's sort of one folder left where it was, from its step: each file, and why. */
export function sortSkips(report: RunReport, id: string): Skip[] {
  return report.steps.find((s) => s.id === id && s.kind === 'move')?.skipped ?? [];
}

/** Files that stayed in a sort, not counting a stop's remainder, which is said once. */
export function stayedOf(skips: readonly Skip[]): number {
  return skips.filter((k) => k.kind !== 'cancelled').reduce((n, k) => n + k.items, 0);
}

/** "184 of 186 files moved into 7 folders." */
export function movedLine(m: Reorganized): string {
  const into = m.into.length;
  const of = m.plannedItems > m.movedItems ? `${formatCount(m.movedItems)} of ${formatCount(m.plannedItems)}` : formatCount(m.movedItems);
  const files = (m.plannedItems > m.movedItems ? m.plannedItems : m.movedItems) === 1 ? 'file' : 'files';
  return `${of} ${files} moved${into > 0 ? ` into ${formatCount(into)} ${into === 1 ? 'folder' : 'folders'}` : ''}.`;
}


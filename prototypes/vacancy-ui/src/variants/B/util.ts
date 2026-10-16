// PROTOTYPE helpers for variant B (Concierge). Pure functions only.
import { formatBytes, formatCount, type Dataset, type OS, type Proposal, type Risk } from '../../shared/data';
import type { RunResult } from '../../shared/run';

export interface PlanTotals {
  cleanups: Proposal[];
  bytes: number;
  items: number;
  hints: Proposal[];
  reorgCount: number;
  reorgFolders: string[];
  moves: number;
}

export function planTotals(d: Dataset, proposals: Set<string>, reorgs: Set<string>): PlanTotals {
  const cleanups = d.proposals.filter((p) => p.action === 'delete' && proposals.has(p.id));
  const hints = d.proposals.filter((p) => p.action === 'hint' && proposals.has(p.id));
  const rs = d.reorgs.filter((r) => reorgs.has(r.id));
  return {
    cleanups,
    bytes: cleanups.reduce((s, p) => s + p.bytes, 0),
    items: cleanups.reduce((s, p) => s + p.itemCount, 0),
    hints,
    reorgCount: rs.length,
    reorgFolders: rs.map((r) => r.folder),
    moves: rs.reduce((s, r) => s + r.moveCount, 0),
  };
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}

export function formatDuration(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return m ? `${m} min ${s} s` : `${s} s`;
}

export function joinWords(words: string[]): string {
  if (words.length <= 1) return words[0] ?? '';
  return `${words.slice(0, -1).join(', ')} and ${words[words.length - 1]}`;
}

export const RISK_LABEL: Record<Risk, string> = { safe: 'Safe', review: 'Review first', careful: 'Careful' };
export const WHY_LABEL: Record<Risk, string> = { safe: 'Why it is safe', review: 'Before you clean', careful: 'Worth knowing' };

/** Pastel motel palette derived from each category's suggested hue. */
export function catColor(hue: number, protectedCat?: boolean): string {
  return protectedCat ? `hsl(${hue} 18% 74%)` : `hsl(${hue} 52% 68%)`;
}

export function binName(os: OS): string {
  return os === 'mac' ? 'Trash' : 'Recycle Bin';
}

/** Split a move target into [unchanged prefix, new folders, file name] relative to its source. */
export function splitMove(from: string, to: string): [string, string, string] {
  const sep = to.includes('\\') ? '\\' : '/';
  const a = from.split(/[\\/]/);
  const b = to.split(/[\\/]/);
  let i = 0;
  while (i < a.length - 1 && i < b.length - 1 && a[i] === b[i]) i++;
  const prefix = b.slice(0, i).join(sep);
  const middle = b.slice(i, -1).join(sep);
  const file = b[b.length - 1];
  return [prefix ? prefix + sep : '', middle ? middle + sep : '', file];
}

export function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1];
}

/** Run playback is compressed so the whole run takes seconds, not minutes. */
export const RUN_SPEED = 0.36;

export function playbackDurations(result: RunResult): number[] {
  return result.steps.map((s) => Math.max(520, Math.round(s.durationMs * RUN_SPEED)));
}

export function gb(bytes: number): string {
  return formatBytes(bytes);
}

export function receiptDate(d: Date): { date: string; time: string; stamp: string } {
  return {
    date: d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }),
    time: d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' }),
    stamp: d.toLocaleDateString('en-US', { month: 'short', day: '2-digit', year: 'numeric' }).toUpperCase(),
  };
}

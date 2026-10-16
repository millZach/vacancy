// PROTOTYPE variant C ("Neon Ledger") helpers: plan math, category colours, formatting, run timeline.
import { formatBytes, formatCount, type Category, type Dataset, type Proposal } from '../../shared/data';
import type { RunResult, RunStep } from '../../shared/run';

export type Filter = 'all' | 'safe' | 'review' | 'ai' | 'reorg' | 'locked';

export const gb = (b: number) => (b === 0 ? '0 GB' : formatBytes(b, 1));
export const n = formatCount;

export const agentName = (d: Dataset) => d.scan.aiAgent ?? 'AI helper';
export const sourceLabel = (d: Dataset, p: { source: 'rule' | 'ai' }) => (p.source === 'ai' ? agentName(d) : 'Rule');
export const binName = (d: Dataset) => (d.os === 'mac' ? 'Trash' : 'Recycle Bin');

// Fixed categorical slots, validated against the dark surface (dataviz validate_palette, adjacent pairs in both
// datasets' bar order). Colour follows the category, never its rank. Magenta and red are left out on purpose:
// neon pink means "space this plan frees".
const CAT_HEX: Record<string, string> = {
  dev: '#3987e5',
  apps: '#d95926',
  media: '#199e70',
  docs: '#199e70',
  downloads: '#c98500',
  caches: '#9085e9',
  other: '#008300',
};

/** Category colour. Protected categories are slate so neon pink stays reserved for "free". */
export function catColor(c: Pick<Category, 'id' | 'hue' | 'protected'>, tone: 'bar' | 'seg' | 'dot' = 'bar'): string {
  if (c.protected) return tone === 'seg' ? 'hsl(226 20% 24%)' : 'hsl(226 16% 50%)';
  const hex = CAT_HEX[c.id] ?? `hsl(${c.hue} 46% 52%)`;
  // Capacity-bar segments carry white labels, so they sit a step darker on the night surface.
  if (tone === 'seg') return `color-mix(in oklab, ${hex} 62%, #0a0e1d)`;
  return hex;
}

export interface PlanTotals {
  delBytes: number;
  delItems: number;
  delCount: number;
  hints: Proposal[];
  moveCount: number;
  reorgCount: number;
  folders: string[];
}

export function planTotals(d: Dataset, proposals: Set<string>, reorgs: Set<string>): PlanTotals {
  let delBytes = 0;
  let delItems = 0;
  let delCount = 0;
  const hints: Proposal[] = [];
  for (const p of d.proposals) {
    if (!proposals.has(p.id)) continue;
    if (p.action === 'hint') hints.push(p);
    else {
      delBytes += p.bytes;
      delItems += p.itemCount;
      delCount++;
    }
  }
  const rs = d.reorgs.filter((r) => reorgs.has(r.id));
  return {
    delBytes,
    delItems,
    delCount,
    hints,
    moveCount: rs.reduce((s, r) => s + r.moveCount, 0),
    reorgCount: rs.length,
    folders: rs.map((r) => r.folder),
  };
}

/** Everything Vacancy could delete if every proposal were on. Drives how bright the sign gets. */
export const maxPlanBytes = (d: Dataset) => d.proposals.filter((p) => p.action === 'delete').reduce((s, p) => s + p.bytes, 0);

export function plannedByCategory(d: Dataset, proposals: Set<string>): Record<string, number> {
  const out: Record<string, number> = {};
  for (const p of d.proposals) if (p.action === 'delete' && proposals.has(p.id)) out[p.categoryId] = (out[p.categoryId] ?? 0) + p.bytes;
  return out;
}

/** Letters of VACANCY lit for a plan of this size. Any plan lights at least one. */
export function litFor(d: Dataset, bytes: number): number {
  if (bytes <= 0) return 0;
  return Math.max(1, Math.min(7, Math.round((bytes / maxPlanBytes(d)) * 7)));
}

// ---- last used: proposals don't carry it, so borrow it from the category item they clean ----
const STOP = new Set(['older', 'than', 'year', 'unused', 'idle', 'projects', 'your', 'from', 'with', 'folders', 'files', 'build', 'output', 'cache', 'caches', 'played', 'months', 'opened']);
const words = (s: string) => s.toLowerCase().split(/[^a-z0-9_]+/).filter((w) => w.length >= 4 && !STOP.has(w));
function globMatch(pattern: string, path: string): boolean {
  const esc = pattern.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*\*/g, '\u0000').replace(/\*/g, '[^\\\\/]*').replace(/\u0000/g, '.*');
  try {
    return new RegExp(`^${esc}$`, 'i').test(path);
  } catch {
    return false;
  }
}
const lastUsedCache = new Map<string, number | undefined>();
export function lastUsedFor(d: Dataset, p: Proposal): number | undefined {
  const key = `${d.os}:${p.id}`;
  if (lastUsedCache.has(key)) return lastUsedCache.get(key);
  const cat = d.categories.find((c) => c.id === p.categoryId);
  let best: { score: number; days: number } | undefined;
  const tw = words(p.title);
  for (const it of cat?.items ?? []) {
    if (it.lastUsedDays == null) continue;
    let score = p.samplePaths.some((sp) => globMatch(it.path, sp)) ? 10 : 0;
    const iw = words(it.name);
    for (const w of tw) if (iw.some((x) => x.startsWith(w) || w.startsWith(x))) score++;
    if (score > 0 && (!best || score > best.score)) best = { score, days: it.lastUsedDays };
  }
  lastUsedCache.set(key, best?.days);
  return best?.days;
}

export function formatAgo(days: number | undefined): string {
  if (days == null) return '';
  if (days < 1) return 'today';
  if (days < 14) return `${days} day${days === 1 ? '' : 's'} ago`;
  if (days < 60) return `${Math.round(days / 7)} weeks ago`;
  if (days < 730) return `${Math.round(days / 30.4)} months ago`;
  return `${(days / 365).toFixed(1)} years ago`;
}

export function formatDuration(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.round(sec % 60);
  return m ? `${m} min ${s} s` : `${s} s`;
}

export function clock(sec: number): string {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

// ---- run timeline: compresses buildRun's durations into a ~9 s show ----
export interface LogLine {
  t: number;
  text: string;
  kind: 'head' | 'del' | 'move' | 'skip' | 'done';
  step: number;
}
export interface Slot {
  step: RunStep;
  start: number;
  dur: number;
}
export interface Timeline {
  slots: Slot[];
  total: number;
  lines: LogLine[];
}

export function buildTimeline(run: RunResult, targetMs = 7600): Timeline {
  const raw = run.steps.reduce((s, x) => s + x.durationMs, 0);
  const scale = raw > 0 ? Math.min(1, targetMs / raw) : 1;
  const gap = 140;
  const slots: Slot[] = [];
  const lines: LogLine[] = [];
  let t = 250;
  run.steps.forEach((step, i) => {
    const dur = Math.max(420, step.durationMs * scale);
    slots.push({ step, start: t, dur });
    lines.push({
      t,
      step: i,
      kind: 'head',
      text: step.kind === 'delete' ? `${step.title}: deleting ${n(step.items)} items (${gb(step.plannedBytes)})` : `${step.title}: moving ${n(step.items)} files`,
    });
    step.log.forEach((l, j) => {
      lines.push({
        t: t + (dur * (j + 1)) / (step.log.length + 1),
        step: i,
        kind: l.startsWith('skipped') ? 'skip' : l.startsWith('moved') ? 'move' : 'del',
        text: l,
      });
    });
    lines.push({
      t: t + dur,
      step: i,
      kind: 'done',
      text:
        step.kind === 'delete'
          ? step.outcome === 'partial'
            ? `freed ${gb(step.freedBytes)} of ${gb(step.plannedBytes)}; ${step.skipped.length} skipped`
            : `freed ${gb(step.freedBytes)}`
          : `moved ${n(step.items)} files, journaled for undo`,
    });
    t += dur + gap;
  });
  return { slots, total: t + 200, lines };
}

export function progressAt(slot: Slot, t: number): number {
  return Math.max(0, Math.min(1, (t - slot.start) / slot.dur));
}

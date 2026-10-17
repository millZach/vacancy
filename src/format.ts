/** A no-break space, so "476 GB" never wraps between the number and its unit. */
const NBSP = '\u00a0';

/**
 * Bytes in decimal units, the way Finder, Explorer's drive view and most people count: "476 GB". Terabytes always
 * keep a decimal when it matters ("1.4 TB", but "1 TB"), because a whole TB is too coarse for a drive's numbers.
 */
export function formatBytes(bytes: number, digits = 1): string {
  const abs = Math.abs(bytes);
  if (abs >= 1e12) return `${trimZero((bytes / 1e12).toFixed(Math.max(1, digits)))}${NBSP}TB`;
  if (abs >= 1e9) return `${(bytes / 1e9).toFixed(digits)}${NBSP}GB`;
  if (abs >= 1e6) return `${(bytes / 1e6).toFixed(0)}${NBSP}MB`;
  if (abs >= 1e3) return `${(bytes / 1e3).toFixed(0)}${NBSP}KB`;
  return `${bytes}${NBSP}B`;
}

/** "1.0" → "1", "1.40" → "1.4". */
function trimZero(n: string): string {
  return n.includes('.') ? n.replace(/\.?0+$/, '') : n;
}

/** A count with thousands separators: "1,284,551". */
export function formatCount(n: number): string {
  return Math.round(n).toLocaleString('en-US');
}

const DAY_MS = 86_400_000;

/** How long ago a time was, the way people say it: "today", "12 days ago", "3 months ago", "1.2 years ago". */
export function ago(whenMs: number, nowMs = Date.now()): string {
  const days = Math.floor((nowMs - whenMs) / DAY_MS);
  if (days <= 0) return 'today';
  if (days === 1) return 'yesterday';
  if (days < 45) return `${days} days ago`;
  if (days < 548) return `${Math.round(days / 30.4)} months ago`;
  return `${(days / 365).toFixed(1).replace(/\.0$/, '')} years ago`;
}

/** A share of the disk for the hover card: "38", or "<1" when it rounds to nothing. */
export function percentOf(part: number, whole: number): string {
  if (whole <= 0) return '0';
  const p = (part / whole) * 100;
  return p < 1 ? '<1' : p.toFixed(0);
}

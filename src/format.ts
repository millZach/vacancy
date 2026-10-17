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

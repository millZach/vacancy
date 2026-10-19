// Fast scan (Windows, NTFS): what the scan screen needs to know about one, and the words it uses. The core asks
// Windows for admin, a small read-only helper reads the drive's file index, and the core sorts what it read into
// categories. Where it can't, the normal scan runs and says why (`FastFallback`).
import type { FastFallback, ScanUpdate, VolumeInfo } from '../bridge';

/** The share of the scan bar reading the file index takes; sorting into categories fills the rest. */
export const INDEX_SHARE = 0.6;

/** Scan progress shown before the scan says it is done: never quite full. */
export const ALMOST = 0.98;

/** The one line every fast-scan offer says about admin. */
export const ADMIN_LINE =
  'Windows asks for admin each time, and Vacancy uses it only for that read. Deleting still happens normally, with the same checks.';

/** Whether fast scan can read this drive: it has a file index only if it is NTFS. */
export const isNtfs = (volume: VolumeInfo) => volume.fileSystem.toUpperCase() === 'NTFS';

/** The fine print under the plate's two buttons, for the drive on screen. */
export function fastFine(volume: VolumeInfo): string {
  return isNtfs(volume)
    ? `Fast scan reads the drive’s file index directly instead of opening every folder. ${ADMIN_LINE}`
    : `Fast scan reads an NTFS drive’s file index directly. ${ADMIN_LINE}`;
}

/**
 * Whether a scan is waiting on Windows' admin prompt: the core says so, or a fast scan of an NTFS drive was asked
 * for and the core hasn't said anything yet. Nothing is scanning then.
 */
export function isAsking(scan: { fast: boolean; update: ScanUpdate | null }, volume: VolumeInfo): boolean {
  if (scan.update) return scan.update.stage === 'asking';
  return scan.fast && isNtfs(volume);
}

/**
 * How far a scan has got, 0 to 1. A normal scan goes by the bytes counted against the drive's used space. A fast
 * scan reads the file index first (its records read, into the first `INDEX_SHARE` of the bar), then sorts by bytes.
 */
export function scanProgress(update: ScanUpdate | null, usedBytes: number): number {
  const bytes = Math.min(1, (update?.bytes ?? 0) / Math.max(1, usedBytes));
  if (update?.method !== 'fast') return Math.min(ALMOST, bytes);
  if (update.stage === 'asking') return 0;
  if (update.stage === 'reading') {
    const read = update.index && update.index.total > 0 ? update.index.done / update.index.total : 0;
    return INDEX_SHARE * Math.min(1, Math.max(0, read));
  }
  return Math.min(ALMOST, INDEX_SHARE + (1 - INDEX_SHARE) * bytes);
}

/** The HUD's line under the counts during a fast scan, or null for a normal scan's folder path. */
export function fastPath(update: ScanUpdate | null, volume: VolumeInfo): string | null {
  if (update?.method !== 'fast') return null;
  if (update.stage === 'sorting') return `Sorting ${volume.name} into categories`;
  return `Reading the file index of ${volume.name}`;
}

/** The drive's file system as the notes name it ("exFAT"), or null if the OS didn't say. */
const fileSystemOf = (volume: VolumeInfo) => volume.fileSystem.trim() || null;

/** The map toolbar's short note for a normal scan that ran instead of a fast one. */
export function fallbackLabel(fallback: FastFallback, volume: VolumeInfo): string {
  switch (fallback) {
    case 'declined':
      return 'Normal scan: admin wasn’t given';
    case 'notNtfs': {
      const fs = fileSystemOf(volume);
      return fs ? `Normal scan: ${fs} drive` : 'Normal scan: not an NTFS drive';
    }
    case 'helperFailed':
      return 'Normal scan: the helper didn’t start';
    case 'indexUnreadable':
      return 'Normal scan: the index couldn’t be read';
  }
}

/** The HUD's note for a normal scan that runs instead of a fast one: its first sentence, then why. */
export function fallbackNote(fallback: FastFallback, volume: VolumeInfo): { lead: string; why: string } {
  const lead = 'Running the normal scan.';
  switch (fallback) {
    case 'declined':
      return { lead, why: 'Windows wasn’t given admin, so Vacancy can’t read the file index. This takes a few minutes.' };
    case 'notNtfs': {
      const fs = fileSystemOf(volume);
      return {
        lead,
        why: `${volume.name} ${fs ? `is ${fs}` : 'isn’t NTFS'}, and fast scan only reads NTFS drives. No admin was asked for.`,
      };
    }
    case 'helperFailed':
      return { lead, why: 'Vacancy’s scan helper didn’t start, so it can’t read the file index.' };
    case 'indexUnreadable':
      return { lead, why: 'The drive’s file index couldn’t be read (a drive BitLocker has locked, for one).' };
  }
}

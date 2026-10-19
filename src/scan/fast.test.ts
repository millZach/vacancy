import { describe, expect, it } from 'vitest';
import { fixtures, type ScanUpdate } from '../bridge';
import { ALMOST, INDEX_SHARE, fallbackLabel, fallbackNote, fastFine, fastPath, isAsking, scanProgress } from './fast';

const C = fixtures.windows.volumes[0]!;
const E = fixtures.windows.volumes[2]!;

const base: ScanUpdate = {
  files: 0,
  folders: 0,
  bytes: 0,
  unreadable: 0,
  currentPath: '',
  elapsedMs: 0,
  categories: null,
  method: 'fast',
  fallback: null,
  stage: 'asking',
  index: null,
};

describe('fast scan on the scan screen', () => {
  it('says what fast scan reads, and what admin is for, by the drive’s file system', () => {
    expect(fastFine(C)).toBe(
      'Fast scan reads the drive’s file index directly instead of opening every folder. Windows asks for admin each time, and Vacancy uses it only for that read. Deleting still happens normally, with the same checks.',
    );
    expect(fastFine(E)).toBe(
      'Fast scan reads an NTFS drive’s file index directly. Windows asks for admin each time, and Vacancy uses it only for that read. Deleting still happens normally, with the same checks.',
    );
  });

  it('waits on Windows’ prompt while the core says so, or before it says anything about an NTFS drive', () => {
    expect(isAsking({ fast: true, update: null }, C)).toBe(true);
    expect(isAsking({ fast: true, update: null }, E)).toBe(false);
    expect(isAsking({ fast: false, update: null }, C)).toBe(false);
    expect(isAsking({ fast: true, update: base }, C)).toBe(true);
    expect(isAsking({ fast: true, update: { ...base, stage: 'reading' } }, C)).toBe(false);
    expect(isAsking({ fast: true, update: { ...base, method: 'normal', stage: null, fallback: 'declined' } }, C)).toBe(false);
  });

  it('fills the bar with the index read first, then the sort by bytes, never quite full', () => {
    const used = 400;
    expect(scanProgress(base, used)).toBe(0);
    expect(scanProgress({ ...base, stage: 'reading', index: { done: 50, total: 100 } }, used)).toBeCloseTo(INDEX_SHARE / 2);
    expect(scanProgress({ ...base, stage: 'reading', index: { done: 100, total: 100 } }, used)).toBeCloseTo(INDEX_SHARE);
    expect(scanProgress({ ...base, stage: 'sorting', bytes: 0 }, used)).toBeCloseTo(INDEX_SHARE);
    expect(scanProgress({ ...base, stage: 'sorting', bytes: 200 }, used)).toBeCloseTo(INDEX_SHARE + (1 - INDEX_SHARE) / 2);
    expect(scanProgress({ ...base, stage: 'sorting', bytes: 400 }, used)).toBe(ALMOST);
    // A normal scan goes by bytes alone.
    expect(scanProgress({ ...base, method: 'normal', stage: null, bytes: 100 }, used)).toBe(0.25);
    expect(scanProgress(null, used)).toBe(0);
  });

  it('says what a fast scan is doing in place of a folder path', () => {
    expect(fastPath({ ...base, stage: 'reading' }, C)).toBe('Reading the file index of Local Disk (C:)');
    expect(fastPath({ ...base, stage: 'sorting' }, C)).toBe('Sorting Local Disk (C:) into categories');
    expect(fastPath({ ...base, method: 'normal', stage: null }, C)).toBeNull();
  });

  it('says why the normal scan runs, on the HUD and on the map', () => {
    const note = (f: Parameters<typeof fallbackNote>[0], v = C) => {
      const n = fallbackNote(f, v);
      return `${n.lead} ${n.why}`;
    };
    expect(note('declined')).toBe(
      'Running the normal scan. Windows wasn’t given admin, so Vacancy can’t read the file index. This takes a few minutes.',
    );
    expect(note('notNtfs', E)).toBe(
      'Running the normal scan. SanDisk (E:) is exFAT, and fast scan only reads NTFS drives. No admin was asked for.',
    );
    expect(note('notNtfs', { ...E, fileSystem: '' })).toBe(
      'Running the normal scan. SanDisk (E:) isn’t NTFS, and fast scan only reads NTFS drives. No admin was asked for.',
    );
    expect(note('helperFailed')).toBe(
      'Running the normal scan. Vacancy’s scan helper didn’t start, so it can’t read the file index.',
    );
    expect(note('indexUnreadable')).toBe(
      'Running the normal scan. The drive’s file index couldn’t be read (a drive BitLocker has locked, for one).',
    );
    expect(fallbackLabel('declined', C)).toBe('Normal scan: admin wasn’t given');
    expect(fallbackLabel('notNtfs', E)).toBe('Normal scan: exFAT drive');
    expect(fallbackLabel('helperFailed', C)).toBe('Normal scan: the helper didn’t start');
    expect(fallbackLabel('indexUnreadable', C)).toBe('Normal scan: the index couldn’t be read');
  });
});

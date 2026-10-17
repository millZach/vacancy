import { describe, expect, it } from 'vitest';
import { ago, formatBytes, formatCount, formatDuration, percentOf, timeAgo } from './format';

const plain = (s: string) => s.replace(/\u00a0/g, ' ');

describe('formatBytes', () => {
  it('counts in decimal units', () => {
    expect(plain(formatBytes(476e9, 0))).toBe('476 GB');
    expect(plain(formatBytes(12.34e9))).toBe('12.3 GB');
    expect(plain(formatBytes(850e6))).toBe('850 MB');
    expect(plain(formatBytes(0))).toBe('0 B');
  });

  it('keeps a decimal for terabytes when it matters', () => {
    expect(plain(formatBytes(1.024e12, 0))).toBe('1 TB');
    expect(plain(formatBytes(1.4e12, 0))).toBe('1.4 TB');
    expect(plain(formatBytes(2e12, 0))).toBe('2 TB');
    expect(plain(formatBytes(1.26e12, 2))).toBe('1.26 TB');
  });

  it('never breaks between the number and its unit', () => {
    expect(formatBytes(476e9, 0)).toBe('476\u00a0GB');
  });
});

describe('formatCount', () => {
  it('groups thousands', () => {
    expect(formatCount(1_284_551)).toBe('1,284,551');
    expect(formatCount(0)).toBe('0');
  });
});

describe('ago', () => {
  const now = Date.UTC(2026, 9, 16);
  const days = (n: number) => now - n * 86_400_000;
  it('says how long ago, coarser as it gets older', () => {
    expect(ago(now, now)).toBe('today');
    expect(ago(days(1), now)).toBe('yesterday');
    expect(ago(days(12), now)).toBe('12 days ago');
    expect(ago(days(96), now)).toBe('3 months ago');
    expect(ago(days(430), now)).toBe('14 months ago');
    expect(ago(days(730), now)).toBe('2 years ago');
    expect(ago(days(800), now)).toBe('2.2 years ago');
  });
});

describe('percentOf', () => {
  it('rounds, and never says 0 for something there', () => {
    expect(percentOf(38, 476)).toBe('8');
    expect(percentOf(1, 476)).toBe('<1');
    expect(percentOf(1, 0)).toBe('0');
  });
});

describe('timeAgo', () => {
  const now = Date.UTC(2026, 9, 17, 12);
  it('counts minutes, then hours, then days', () => {
    expect(timeAgo(now - 20_000, now)).toBe('just now');
    expect(timeAgo(now - 4 * 60_000, now)).toBe('4 min ago');
    expect(timeAgo(now - 59 * 60_000, now)).toBe('59 min ago');
    expect(timeAgo(now - 125 * 60_000, now)).toBe('2 h ago');
    expect(timeAgo(now - 3 * 86_400_000, now)).toBe('3 days ago');
  });
});

describe('formatDuration', () => {
  it('keeps a tenth of a second for short runs, then whole seconds, minutes and hours', () => {
    expect(plain(formatDuration(420))).toBe('0.4 s');
    expect(plain(formatDuration(8_240))).toBe('8.2 s');
    expect(plain(formatDuration(38_400))).toBe('38 s');
    expect(plain(formatDuration(125_000))).toBe('2 min 5 s');
    expect(plain(formatDuration(3_840_000))).toBe('1 h 4 min');
  });
});

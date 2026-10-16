import { describe, expect, it } from 'vitest';
import { formatBytes } from './format';

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

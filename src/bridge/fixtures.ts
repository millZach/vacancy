import type { Platform, VolumeInfo } from './types';

/** What the mock bridge answers with. One per platform, matching the prototype's datasets. */
export interface Fixture {
  platform: Platform;
  machineName: string;
  systemVolume: VolumeInfo;
}

const GB = 1e9;

export const fixtures = {
  windows: {
    platform: 'windows',
    machineName: 'DESKTOP-ALEX',
    systemVolume: { name: 'Local Disk (C:)', mountPoint: 'C:\\', totalBytes: 476 * GB, usedBytes: 431 * GB },
  },
  macos: {
    platform: 'macos',
    machineName: 'Alex’s MacBook Air',
    systemVolume: { name: 'Macintosh HD', mountPoint: '/', totalBytes: 494 * GB, usedBytes: 460 * GB },
  },
} satisfies Record<Platform, Fixture>;

export type FixtureName = keyof typeof fixtures;

export function isFixtureName(name: string | null): name is FixtureName {
  return name !== null && Object.hasOwn(fixtures, name);
}

import { HardDrive, Sparkles } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Platform, VolumeInfo } from '../bridge';
import { Wordmark } from '../design/Wordmark';
import { formatBytes } from '../format';
import './shell.css';

interface Props {
  platform: Platform;
  /** The drive the screen is about, once known. */
  volume: VolumeInfo | null;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
  children: ReactNode;
}

/**
 * The window's frame: the header (wordmark, the drive with how much is used and free, the AI agent) over the
 * current screen.
 * On macOS the title bar is an overlay, so the traffic lights sit in the header's left end and the header drags
 * the window. On Windows the native title bar does both.
 */
export function Shell({ platform, volume, agent, children }: Props) {
  const overlay = platform === 'macos';
  // "deep": anywhere in the header drags the window, except on its controls.
  const drag = overlay ? { 'data-tauri-drag-region': 'deep' } : {};
  return (
    <div className="shell" data-chrome={overlay ? 'overlay' : 'native'}>
      <header className="shell-head" {...drag}>
        <Wordmark />
        {volume && (
          <div className="head-disk">
            <HardDrive size={14} strokeWidth={1.8} aria-hidden />
            <span className="head-disk-name">{volume.name}</span>
            <span className="head-meter" aria-hidden>
              <i style={{ width: `${usedPercent(volume.usedBytes, volume.totalBytes)}%` }} />
            </span>
            <span className="head-disk-nums">
              {formatBytes(volume.usedBytes, 0)} of {formatBytes(volume.totalBytes, 0)} used
            </span>
            <span className="head-disk-free">
              {formatBytes(Math.max(0, volume.totalBytes - volume.usedBytes), 0)} free
            </span>
          </div>
        )}
        <div className="head-right">
          <span
            className={agent ? 'pill' : 'pill is-muted'}
            title={agent ? `${agent} explains unfamiliar folders and suggests reorganizations` : 'No AI agent found'}
          >
            <Sparkles size={12} strokeWidth={2.2} aria-hidden />
            {agent ?? 'Rules only'}
          </span>
        </div>
      </header>
      <div className="shell-body">{children}</div>
    </div>
  );
}

function usedPercent(used: number, total: number): number {
  return total > 0 ? Math.min(100, (used / total) * 100) : 0;
}

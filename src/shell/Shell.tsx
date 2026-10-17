import { HardDrive, Sparkles } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Platform } from '../bridge';
import { Wordmark } from '../design/Wordmark';
import type { DriveState } from '../drive';
import { formatBytes } from '../format';
import './shell.css';

interface Props {
  platform: Platform;
  drive: DriveState;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
  children: ReactNode;
}

/**
 * The window's frame: the header (wordmark, the drive and how full it is, the AI agent) over the current screen.
 * On macOS the title bar is an overlay, so the traffic lights sit in the header's left end and the header drags
 * the window. On Windows the native title bar does both.
 */
export function Shell({ platform, drive, agent, children }: Props) {
  const overlay = platform === 'macos';
  // "deep": anywhere in the header drags the window, except on its controls.
  const drag = overlay ? { 'data-tauri-drag-region': 'deep' } : {};
  return (
    <div className="shell" data-chrome={overlay ? 'overlay' : 'native'}>
      <header className="shell-head" {...drag}>
        <Wordmark />
        {drive.status === 'ready' && (
          <div className="head-disk">
            <HardDrive size={14} strokeWidth={1.8} aria-hidden />
            <span className="head-disk-name">{drive.volume.name}</span>
            <span className="head-meter" aria-hidden>
              <i style={{ width: `${usedPercent(drive.volume.usedBytes, drive.volume.totalBytes)}%` }} />
            </span>
            <span className="head-disk-nums">
              {formatBytes(drive.volume.usedBytes, 0)} of {formatBytes(drive.volume.totalBytes, 0)} used
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

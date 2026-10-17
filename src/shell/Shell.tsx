import { Sparkles } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Platform } from '../bridge';
import { Wordmark } from '../design/Wordmark';
import './shell.css';

interface Props {
  platform: Platform;
  /** The header's drive plate (the drive menu), once the drives are known. */
  drive: ReactNode;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
  children: ReactNode;
}

/**
 * The window's frame: the header (wordmark, the drive plate, the AI agent) over the current screen.
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
        {drive}
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

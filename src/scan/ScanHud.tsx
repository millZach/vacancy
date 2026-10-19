import { Info, Zap } from 'lucide-react';
import type { ScanUpdate, VolumeInfo } from '../bridge';
import { formatCount } from '../format';
import { fallbackNote, fastPath } from './fast';

interface Props {
  update: ScanUpdate | null;
  /** The drive being scanned, for the fast scan's lines and the fallback notes. */
  volume: VolumeInfo;
  /** 0 to 1. */
  progress: number;
  onCancel: () => void;
}

/**
 * The readout that floats over the map while it fills in: counts, the folder being read, progress, Cancel. A fast
 * scan is tagged as one, and says whether it is reading the file index or sorting it; a normal scan that runs in
 * place of a fast one says why under the bar.
 */
export function ScanHud({ update, volume, progress, onCancel }: Props) {
  const fast = update?.method === 'fast';
  const path = fastPath(update, volume) ?? update?.currentPath;
  const note = !fast && update?.fallback ? fallbackNote(update.fallback, volume) : null;
  return (
    <div className={fast ? 'hud is-fast' : 'hud'} role="status" aria-label="Scan progress" data-no-ripple>
      <div className="hud-nums">
        {fast && (
          <span className="hud-mode">
            <Zap size={11} strokeWidth={2.4} aria-hidden /> Fast scan
          </span>
        )}
        <span>
          <b>{formatCount(update?.files ?? 0)}</b> files
        </span>
        <span>
          <b>{formatCount(update?.folders ?? 0)}</b> folders
        </span>
        <button type="button" className="hud-cancel" onClick={onCancel}>
          Cancel
        </button>
      </div>
      <div className="hud-path" title={path}>
        {path || 'Starting…'}
      </div>
      <div
        className="hud-bar"
        role="progressbar"
        aria-label="Scanned"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(progress * 100)}
      >
        <i style={{ width: `${progress * 100}%` }} />
      </div>
      {note && (
        <p className="hud-note" role="note">
          <Info size={13} aria-hidden />
          <span>
            <b>{note.lead}</b> {note.why}
          </span>
        </p>
      )}
    </div>
  );
}

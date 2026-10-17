import type { ScanUpdate } from '../bridge';
import { formatCount } from '../format';

interface Props {
  update: ScanUpdate | null;
  /** 0 to 1. */
  progress: number;
  onCancel: () => void;
}

/** The readout that floats over the map while it fills in: counts, the folder being read, progress, Cancel. */
export function ScanHud({ update, progress, onCancel }: Props) {
  return (
    <div className="hud" role="status" aria-label="Scan progress" data-no-ripple>
      <div className="hud-nums">
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
      <div className="hud-path" title={update?.currentPath}>
        {update?.currentPath || 'Starting…'}
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
    </div>
  );
}

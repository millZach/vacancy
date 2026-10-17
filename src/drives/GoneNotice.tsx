import { Unplug, X } from 'lucide-react';
import { useEffect, useRef } from 'react';
import type { Gone } from './session';

/** How long the notice stays unless dismissed. */
const SHOWN_MS = 10_000;

/** A short note that the drive on screen went away (an unplugged USB stick), and what went with it. */
export function GoneNotice({ gone, onDismiss }: { gone: Gone; onDismiss: () => void }) {
  const dismiss = useRef(onDismiss);
  useEffect(() => {
    dismiss.current = onDismiss;
  });
  useEffect(() => {
    const timer = window.setTimeout(() => dismiss.current(), SHOWN_MS);
    return () => clearTimeout(timer);
  }, [gone]);

  const what =
    gone.lost === 'map' ? ', so its map was cleared' : gone.lost === 'scan' ? ', so its scan stopped' : '';
  return (
    <div className="drive-gone" role="status" data-no-ripple>
      <Unplug size={14} strokeWidth={2} aria-hidden />
      <span>
        <b>{gone.name}</b> is no longer connected{what}.
      </span>
      <button type="button" className="drive-gone-close" aria-label="Dismiss" onClick={onDismiss}>
        <X size={13} aria-hidden />
      </button>
    </div>
  );
}

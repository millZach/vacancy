import { useCallback, useEffect, useRef, useState } from 'react';
import { useBridge, type CategoryTotal, type Overview, type ScanUpdate, type VolumeInfo } from '../bridge';
import { errorMessage } from '../errors';

/** Where a scan is: not started, running (with the HUD's numbers and the map so far), finished, or failed. */
export type ScanState =
  | { phase: 'idle' }
  | {
      phase: 'scanning';
      volume: VolumeInfo;
      update: ScanUpdate | null;
      /** The categories so far, from the last update that carried a preview. */
      preview: CategoryTotal[];
    }
  | { phase: 'done'; volume: VolumeInfo; overview: Overview }
  | { phase: 'failed'; volume: VolumeInfo; message: string };

export interface Scan {
  state: ScanState;
  /** Starts scanning a volume (read-only). A scan already running is replaced. */
  start: (volume: VolumeInfo) => void;
  /** Stops the running scan and goes back to the start. */
  cancel: () => void;
  /** Leaves a finished or failed scan and goes back to the start. */
  reset: () => void;
}

/** One scan at a time, through the bridge. Leaving the screen mid-scan cancels it. */
export function useScan(): Scan {
  const bridge = useBridge();
  const [state, setState] = useState<ScanState>({ phase: 'idle' });
  const run = useRef(0);

  const start = useCallback(
    (volume: VolumeInfo) => {
      const id = ++run.current;
      setState({ phase: 'scanning', volume, update: null, preview: [] });
      bridge
        .scan(volume.mountPoint, (update) => {
          if (run.current !== id) return;
          setState((s) =>
            s.phase === 'scanning' ? { ...s, update, preview: update.categories ?? s.preview } : s,
          );
        })
        .then(
          (outcome) => {
            if (run.current !== id) return;
            setState(outcome.status === 'done' ? { phase: 'done', volume, overview: outcome.overview } : { phase: 'idle' });
          },
          (error: unknown) => {
            if (run.current === id) setState({ phase: 'failed', volume, message: errorMessage(error) });
          },
        );
    },
    [bridge],
  );

  const cancel = useCallback(() => {
    run.current++;
    setState({ phase: 'idle' });
    void bridge.cancelScan();
  }, [bridge]);

  const reset = useCallback(() => {
    run.current++;
    setState({ phase: 'idle' });
  }, []);

  // Leaving the screen mid-scan stops the scan; a finished one has nothing to stop.
  const scanning = useRef(false);
  useEffect(() => {
    scanning.current = state.phase === 'scanning';
  }, [state.phase]);
  useEffect(
    () => () => {
      if (scanning.current) void bridge.cancelScan();
    },
    [bridge],
  );

  return { state, start, cancel, reset };
}

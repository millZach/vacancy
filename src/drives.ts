import { useCallback, useEffect, useState } from 'react';
import { useBridge, type FullDiskAccess, type VolumeInfo } from './bridge';
import { errorMessage } from './errors';

/** The drives a scan can be pointed at, this computer's name, and (on macOS) whether Vacancy can see everything. */
export type DrivesState =
  | { status: 'loading' }
  | {
      status: 'ready';
      /** Local volumes, the system volume first. */
      volumes: VolumeInfo[];
      machineName: string;
      fullDiskAccess: FullDiskAccess;
    }
  | { status: 'error'; message: string };

/**
 * Reads the drives once, through the bridge. The system volume is always listed first, even if listing volumes
 * fails. Full Disk Access is checked again whenever the window comes back to the front, since people grant it in
 * System Settings and return.
 */
export function useDrives(): DrivesState & { recheckAccess: () => void } {
  const bridge = useBridge();
  const [state, setState] = useState<DrivesState>({ status: 'loading' });

  useEffect(() => {
    let live = true;
    Promise.all([
      bridge.systemVolume(),
      bridge.listVolumes().catch(() => [] as VolumeInfo[]),
      bridge.machineName(),
      bridge.fullDiskAccess().catch((): FullDiskAccess => 'unknown'),
    ]).then(
      ([system, listed, machineName, fullDiskAccess]) => {
        if (!live) return;
        const others = listed.filter((v) => v.mountPoint !== system.mountPoint);
        setState({ status: 'ready', volumes: [system, ...others], machineName, fullDiskAccess });
      },
      (error: unknown) => {
        if (live) setState({ status: 'error', message: errorMessage(error) });
      },
    );
    return () => {
      live = false;
    };
  }, [bridge]);

  const recheckAccess = useCallback(() => {
    bridge.fullDiskAccess().then(
      (fullDiskAccess) => setState((s) => (s.status === 'ready' ? { ...s, fullDiskAccess } : s)),
      () => {},
    );
  }, [bridge]);

  const missing = state.status === 'ready' && state.fullDiskAccess === 'missing';
  useEffect(() => {
    if (!missing) return;
    window.addEventListener('focus', recheckAccess);
    return () => window.removeEventListener('focus', recheckAccess);
  }, [missing, recheckAccess]);

  return { ...state, recheckAccess };
}

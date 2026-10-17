import { useEffect, useState } from 'react';
import { useBridge, type VolumeInfo } from './bridge';

/** The system drive and this computer's name, as the scan screen's plate and the header show them. */
export type DriveState =
  | { status: 'loading' }
  | { status: 'ready'; volume: VolumeInfo; machineName: string }
  | { status: 'error'; message: string };

/** Reads the system drive once, through the bridge. */
export function useSystemDrive(): DriveState {
  const bridge = useBridge();
  const [state, setState] = useState<DriveState>({ status: 'loading' });
  useEffect(() => {
    let live = true;
    Promise.all([bridge.systemVolume(), bridge.machineName()]).then(
      ([volume, machineName]) => {
        if (live) setState({ status: 'ready', volume, machineName });
      },
      (error: unknown) => {
        if (live) setState({ status: 'error', message: errorMessage(error) });
      },
    );
    return () => {
      live = false;
    };
  }, [bridge]);
  return state;
}

/** Tauri rejects with the command's error value (a string here); the mock rejects with an Error. */
function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  return 'Unknown error';
}

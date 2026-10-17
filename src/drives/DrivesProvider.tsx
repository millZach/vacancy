import { useCallback, useEffect, useReducer, useRef, useState, type Dispatch, type ReactNode } from 'react';
import { useBridge, type Bridge, type FullDiskAccess, type VolumeInfo } from '../bridge';
import { errorMessage } from '../errors';
import { DrivesContext, type DriveItem, type Drives } from './context';
import { EMPTY, reduce, sameDrive, scanOf, shownVolume, type Action, type Session } from './session';

/** How often to look for drives coming and going. Listing is read-only and takes about a millisecond. */
const POLL_MS = 3000;

/** Numbers each scan, so a scan's late events are never taken for those of the scan after it. */
let lastRun = 0;

type Info =
  | { status: 'loading' }
  | { status: 'error'; message: string }
  | { status: 'ready'; machineName: string; fullDiskAccess: FullDiskAccess };

/**
 * Holds the session's drives for everything below it (`useDrives()`): the listing, each drive's scan and map, the
 * drive on screen, and the lock a running plan holds.
 *
 * It lists the drives again every few seconds and whenever the window comes back to the front, so an unplugged
 * drive leaves the list and the core forgets its map. Full Disk Access is checked again on focus too, since people
 * grant it in System Settings and return.
 */
export function DrivesProvider({ children }: { children: ReactNode }) {
  const bridge = useBridge();
  const [info, setInfo] = useState<Info>({ status: 'loading' });
  const [session, dispatch] = useReducer(reduce, EMPTY);

  useEffect(() => {
    let live = true;
    Promise.all([
      bridge.listVolumes(),
      bridge.machineName(),
      bridge.fullDiskAccess().catch((): FullDiskAccess => 'unknown'),
    ]).then(
      ([drives, machineName, fullDiskAccess]) => {
        if (!live) return;
        if (drives.length === 0) {
          setInfo({ status: 'error', message: 'Vacancy found no drives to scan.' });
          return;
        }
        dispatch({ type: 'listed', drives });
        setInfo({ status: 'ready', machineName, fullDiskAccess });
      },
      (error: unknown) => {
        if (live) setInfo({ status: 'error', message: errorMessage(error) });
      },
    );
    return () => {
      live = false;
    };
  }, [bridge]);

  const ready = info.status === 'ready';
  const relist = useCallback(() => {
    bridge.listVolumes().then(
      (drives) => dispatch({ type: 'listed', drives }),
      () => {
        // Keep the drives we know and ask again next time; a failed listing is not a drive unplugged.
      },
    );
  }, [bridge]);
  useEffect(() => {
    if (!ready) return;
    const timer = window.setInterval(relist, POLL_MS);
    window.addEventListener('focus', relist);
    return () => {
      clearInterval(timer);
      window.removeEventListener('focus', relist);
    };
  }, [ready, relist]);

  // The core forgets drives that left the list: their maps, and a scan still running on one.
  const known = useRef<VolumeInfo[]>([]);
  useEffect(() => {
    for (const d of known.current) {
      if (!session.drives.some((n) => sameDrive(n, d))) bridge.forgetDrive(d.mountPoint).catch(() => {});
    }
    known.current = session.drives;
  }, [bridge, session.drives]);

  const recheckAccess = useCallback(() => {
    bridge.fullDiskAccess().then(
      (fullDiskAccess) => setInfo((i) => (i.status === 'ready' ? { ...i, fullDiskAccess } : i)),
      () => {},
    );
  }, [bridge]);
  const missing = info.status === 'ready' && info.fullDiskAccess === 'missing';
  useEffect(() => {
    if (!missing) return;
    window.addEventListener('focus', recheckAccess);
    return () => window.removeEventListener('focus', recheckAccess);
  }, [missing, recheckAccess]);

  const startScan = useCallback(
    (mountPoint: string) => {
      const run = ++lastRun;
      dispatch({ type: 'scanStarted', mountPoint, run });
      bridge.scan(mountPoint, (update) => dispatch({ type: 'scanUpdate', mountPoint, run, update })).then(
        (outcome) => dispatch({ type: 'scanEnded', mountPoint, run, outcome, at: Date.now() }),
        (error: unknown) => dispatch({ type: 'scanFailed', mountPoint, run, message: errorMessage(error) }),
      );
    },
    [bridge],
  );

  // Leaving the app mid-scan stops the scan; a finished one has nothing to stop.
  const latest = useRef(session);
  useEffect(() => {
    latest.current = session;
  });
  useEffect(
    () => () => {
      const s = latest.current;
      if (scanOf(s, s.current).phase === 'scanning') void bridge.cancelScan();
    },
    [bridge],
  );

  return <DrivesContext value={drivesValue(info, session, dispatch, bridge, startScan)}>{children}</DrivesContext>;
}

/** What `useDrives()` hands out: the drives as the screens show them, and what they can do. */
function drivesValue(
  info: Info,
  session: Session,
  dispatch: Dispatch<Action>,
  bridge: Bridge,
  startScan: (mountPoint: string) => void,
): Drives {
  if (info.status !== 'ready') return info;
  const list: DriveItem[] = session.drives.map((d) => ({
    volume: shownVolume(session, d),
    scan: scanOf(session, d.mountPoint),
    current: d.mountPoint === session.current,
  }));
  const current = list.find((d) => d.current);
  if (!current) return { status: 'loading' };
  /** Moves to another drive; a scan running on the drive being left stops. */
  const leave = (mountPoint: string, action: 'pick' | 'switch') => {
    if (session.locked) return;
    if (current.scan.phase === 'scanning' && mountPoint !== session.current) void bridge.cancelScan();
    dispatch({ type: action, mountPoint });
  };
  return {
    status: 'ready',
    machineName: info.machineName,
    fullDiskAccess: info.fullDiskAccess,
    list,
    current,
    screen: session.screen,
    locked: session.locked,
    gone: session.gone,
    pick: (mountPoint) => leave(mountPoint, 'pick'),
    switchTo: (mountPoint) => leave(mountPoint, 'switch'),
    openMap: () => dispatch({ type: 'openMap' }),
    showScanScreen: () => dispatch({ type: 'showScanScreen' }),
    scan: () => startScan(session.current),
    cancelScan: () => {
      dispatch({ type: 'scanCancelled', mountPoint: session.current });
      void bridge.cancelScan();
    },
    setRunning: (running) => dispatch({ type: 'lock', locked: running }),
    dismissGone: () => dispatch({ type: 'dismissGone' }),
  };
}

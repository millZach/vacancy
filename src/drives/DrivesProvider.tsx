import { useCallback, useEffect, useReducer, useRef, useState, type Dispatch, type ReactNode } from 'react';
import {
  useBridge,
  type Bridge,
  type FullDiskAccess,
  type PlanSummary,
  type RunEvent,
  type VolumeInfo,
} from '../bridge';
import { errorMessage } from '../errors';
import { DrivesContext, type DriveItem, type Drives } from './context';
import { EMPTY, reduce, sameDrive, scanOf, shownVolume, type Action, type Session } from './session';

/** How often to look for drives coming and going. Listing is read-only and takes about a millisecond. */
const POLL_MS = 3000;

/**
 * A run's events are handed to the screens in batches this far apart: a run reports every place it deletes, and
 * drawing the map once per event would fall behind.
 */
const BATCH_MS = 50;

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
  // One listing at a time: a drive slow to answer never piles up requests, and answers can't arrive out of order.
  const listing = useRef(false);
  const relist = useCallback(() => {
    if (listing.current) return;
    listing.current = true;
    bridge
      .listVolumes()
      .then(
        (drives) => dispatch({ type: 'listed', drives }),
        () => {
          // Keep the drives we know and ask again next time; a failed listing is not a drive unplugged.
        },
      )
      .finally(() => {
        listing.current = false;
      });
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
        (outcome) => {
          dispatch({ type: 'scanEnded', mountPoint, run, outcome, at: Date.now() });
          // The duplicate search starts as soon as the map is up, and never holds it up.
          if (outcome.status === 'done') {
            bridge
              .findDuplicates(mountPoint, (progress) => dispatch({ type: 'dupsProgress', mountPoint, run, progress }))
              .then(
                (found) => dispatch({ type: 'dupsEnded', mountPoint, run, outcome: found }),
                (error: unknown) => dispatch({ type: 'dupsFailed', mountPoint, run, message: errorMessage(error) }),
              );
          }
        },
        (error: unknown) => dispatch({ type: 'scanFailed', mountPoint, run, message: errorMessage(error) }),
      );
    },
    [bridge],
  );

  const runPlan = useCallback(
    (plan: PlanSummary) => {
      const { mountPoint, id: planId } = plan;
      dispatch({ type: 'runStarted', mountPoint, plan });
      let queue: RunEvent[] = [];
      let timer = 0;
      const flush = () => {
        clearTimeout(timer);
        timer = 0;
        if (queue.length === 0) return;
        const events = queue;
        queue = [];
        dispatch({ type: 'runEvents', mountPoint, planId, events });
      };
      const onEvent = (event: RunEvent) => {
        queue.push(event);
        if (!timer) timer = window.setTimeout(flush, BATCH_MS);
      };
      bridge.runPlan(mountPoint, planId, onEvent).then(
        (report) => {
          flush();
          dispatch({ type: 'runEnded', mountPoint, planId, report });
        },
        (error: unknown) => {
          flush();
          dispatch({ type: 'runFailed', mountPoint, planId, message: errorMessage(error) });
        },
      );
    },
    [bridge],
  );

  // Leaving the app mid-scan stops the scan, and mid-run stops the run; a finished one has nothing to stop.
  const latest = useRef(session);
  useEffect(() => {
    latest.current = session;
  });
  useEffect(
    () => () => {
      const s = latest.current;
      const scan = scanOf(s, s.current);
      if (scan.phase === 'scanning') void bridge.cancelScan();
      if (scan.phase === 'done' && scan.planRun?.phase === 'running') void bridge.cancelRun();
    },
    [bridge],
  );

  return (
    <DrivesContext value={drivesValue(info, session, dispatch, bridge, startScan, runPlan)}>{children}</DrivesContext>
  );
}

/** What `useDrives()` hands out: the drives as the screens show them, and what they can do. */
function drivesValue(
  info: Info,
  session: Session,
  dispatch: Dispatch<Action>,
  bridge: Bridge,
  startScan: (mountPoint: string) => void,
  runPlan: (plan: PlanSummary) => void,
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
    scan: () => {
      if (!session.locked) startScan(session.current);
    },
    cancelScan: () => {
      dispatch({ type: 'scanCancelled', mountPoint: session.current });
      void bridge.cancelScan();
    },
    setInPlan: (id, on) => dispatch({ type: 'plan', mountPoint: session.current, id, on }),
    setGroupSkipped: (id, skipped) => dispatch({ type: 'skipGroup', mountPoint: session.current, id, skipped }),
    setRunning: (running) => dispatch({ type: 'lock', locked: running }),
    runPlan,
    cancelRun: () => {
      dispatch({ type: 'runStopping', mountPoint: session.current });
      void bridge.cancelRun();
    },
    showSummary: () => dispatch({ type: 'showSummary', mountPoint: session.current }),
    summarySeen: () => dispatch({ type: 'summarySeen', mountPoint: session.current }),
    leaveRun: () => dispatch({ type: 'leaveRun', mountPoint: session.current }),
    dismissGone: () => dispatch({ type: 'dismissGone' }),
  };
}

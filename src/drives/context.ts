import { createContext, use } from 'react';
import type { ClaudeRefreshed, FullDiskAccess, PlanSummary, RunReport, VolumeInfo } from '../bridge';
import type { DriveScan, Gone, Screen } from './session';

/** The drives, this computer's name and (on macOS) whether Vacancy can see everything, once they are known. */
export type Drives = { status: 'loading' } | { status: 'error'; message: string } | ReadyDrives;

/** One drive as the screens show it. */
export interface DriveItem {
  /** Its figures: from its map once it has one (so they agree with the map), from the latest listing until then. */
  volume: VolumeInfo;
  scan: DriveScan;
  /** It is the drive on screen. */
  current: boolean;
}

export interface ReadyDrives {
  status: 'ready';
  machineName: string;
  fullDiskAccess: FullDiskAccess;
  /** Every drive to offer, the system drive first. */
  list: DriveItem[];
  /** The drive on screen. */
  current: DriveItem;
  /** Whether the drive on screen shows its scan screen or its map. */
  screen: Screen;
  /** A plan is running: switching drives waits until it ends. */
  locked: boolean;
  /** The drive on screen went away, for a short notice. */
  gone: Gone | null;
  /** Scan screen: choose the drive to scan, staying on the scan screen. */
  pick: (mountPoint: string) => void;
  /** Header: a scanned drive opens its map, an unscanned one its scan screen. Stops a scan of the drive being left. */
  switchTo: (mountPoint: string) => void;
  /** The drive on screen, from its scan screen back to its map. */
  openMap: () => void;
  /** The drive on screen, from its map to its scan screen. */
  showScanScreen: () => void;
  /**
   * Scans the drive on screen, read-only. A scan already running is replaced. With `fast` (Windows), asks for a fast
   * scan: Windows asks for admin, every time, and the normal scan runs where it can't be done.
   */
  scan: (options?: { fast?: boolean }) => void;
  /** Stops the scan of the drive on screen. */
  cancelScan: () => void;
  /** Puts a proposal in the plan of the drive on screen, or takes it out. */
  setInPlan: (id: string, on: boolean) => void;
  /** Unticks a duplicate group of the drive on screen (every copy of it stays), or ticks it again. */
  setGroupSkipped: (id: string, skipped: boolean) => void;
  /**
   * Claude's answer about a drive's scan `run` changed its map (its own items, unticked, or a layout): shows the map,
   * proposals and sorts it sent. Ignored once that scan has been replaced or has run its plan, and for a map older
   * than the one shown (a lower `revision`: a slow reply never puts back what a later change replaced).
   */
  claudeApplied: (mountPoint: string, run: number, refreshed: ClaudeRefreshed) => void;
  /**
   * Sets a drive's whole plan for its scan `run` (Let Claude pick, and Keep mine putting the plan back). With `from`,
   * only while the plan is still at that version (`planVersion`): an edit made meanwhile is never overwritten. With
   * `expect` too, only while the plan holds exactly those ids.
   */
  setPlan: (mountPoint: string, run: number, ids: string[], from?: number, expect?: string[]) => void;
  /** While a plan runs, switching drives is disabled. `runPlan` sets it; this is for anything else that must hold. */
  setRunning: (running: boolean) => void;
  /**
   * Runs a confirmed plan of the drive on screen. Locks drive switching, feeds the run's events to the drive as they
   * arrive, and unlocks when the run ends, either way. The drive keeps the run, then its summary.
   */
  runPlan: (plan: PlanSummary) => void;
  /** Stops the running plan after the file being deleted. The run then ends, saying it was stopped. */
  cancelRun: () => void;
  /** From the ended run of the drive on screen to its finish ("See what changed"): the slides, then the summary. */
  showSummary: () => void;
  /** The summary of the drive on screen came up; it opens there from now on, not at the slides. */
  summarySeen: () => void;
  /** From a failed run of the drive on screen back to its plan. */
  leaveRun: () => void;
  /** A kept run changed (an Undo ran): the drive whose run it is shows it as it is now. */
  reportUpdated: (report: RunReport) => void;
  dismissGone: () => void;
}

export const DrivesContext = createContext<Drives | null>(null);

/** The drives and the drive on screen. Throws outside a `DrivesProvider`, so a missing provider fails loudly. */
export function useDrives(): Drives {
  const drives = use(DrivesContext);
  if (!drives) throw new Error('useDrives() needs a <DrivesProvider> above it');
  return drives;
}

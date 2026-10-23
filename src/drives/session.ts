import type {
  CategoryTotal,
  ClaudeRefreshed,
  DuplicateProgress,
  Duplicates,
  DuplicatesOutcome,
  Overview,
  PlanSummary,
  Proposal,
  RunEvent,
  RunReport,
  ScanOutcome,
  ScanUpdate,
  TidyProposal,
  VolumeInfo,
} from '../bridge';
import { foldEvents, settleRun, startRun, type RunView } from '../run/progress';

/**
 * The drives for this session, one at a time on screen, each with its own scan, map, proposals, plan, run and
 * summary, so switching never mixes two drives' data. This file is the pure part: the state and how each event
 * changes it. `DrivesProvider` wires it to the bridge.
 */
export interface Session {
  /** The drives to offer, the system drive first, from the latest listing. */
  drives: VolumeInfo[];
  /** The drive on screen, by mount point. */
  current: string;
  /** Whether the drive on screen shows its scan screen or (once it has one) its map. */
  screen: Screen;
  /** Each drive's scan, by mount point. A drive with no entry hasn't been scanned. */
  scans: Record<string, DriveScan>;
  /** A plan is running: switching drives, and scanning, wait until it ends. */
  locked: boolean;
  /** The drive on screen went away, for a short notice. */
  gone: Gone | null;
}

export type Screen = 'scan' | 'map';

/** Where one drive's scan is. `run` tells a scan's events apart from those of a scan that came before it. */
export type DriveScan =
  | { phase: 'idle' }
  | {
      phase: 'scanning';
      run: number;
      /** A fast scan was asked for (Windows): until the core says otherwise, Windows' admin prompt may be up. */
      fast: boolean;
      update: ScanUpdate | null;
      /** The categories so far, from the last update that carried a preview. */
      preview: CategoryTotal[];
    }
  | {
      phase: 'done';
      /** Which scan this is (see `scanStarted`): the duplicate search of an earlier scan never lands on it. */
      run: number;
      /** The core's id for this scan (`ScanOutcome`'s `scan`), which every Claude command about it names. */
      scan: number;
      /** The newest `ClaudeRefreshed.revision` shown: an older one, arriving late, is ignored. */
      claudeRevision: number;
      overview: Overview;
      /** The cleanups proposed for this drive, biggest first. */
      proposals: Proposal[];
      /** The folders Tidy up can sort on this drive (Windows only). They start out of the plan. */
      tidy: TidyProposal[];
      /**
       * The ids of the proposals and sorts in this drive's plan. A new scan starts with the cleanups on by default;
       * sorts always start out of it.
       */
      plan: string[];
      /**
       * Goes up by one each time what is in the plan changes, however it changes: Claude's panel tells an answer about
       * the plan as it was sent from one about the plan as it is now.
       */
      planVersion: number;
      /** When it finished, in ms since the epoch. */
      at: number;
      /** The plan this scan ran, if it ran one. A drive keeps it, and its summary, until it is scanned again. */
      planRun: PlanRun | null;
      /** The duplicate search that runs after the scan, in the background. */
      duplicates: DuplicateSearch;
      /** Duplicate groups the user unticked, by id: every copy of them stays. */
      skippedGroups: string[];
    }
  | { phase: 'failed'; message: string };

/**
 * The duplicate search after a scan: checking (with how far it has got, once it says), done (with the groups it
 * found, maybe none), stopped (a run started, or the drive went), or failed.
 */
export type DuplicateSearch =
  | { phase: 'checking'; progress: DuplicateProgress | null }
  | { phase: 'done'; found: Duplicates }
  | { phase: 'stopped' }
  | { phase: 'failed'; message: string };

/**
 * A drive's plan once it is confirmed: running, ended (the run panel says Done, or that it was stopped), failed
 * (the run panel says why), and then the finish: its highlight slides, then the summary of what changed. `fresh`
 * until the summary has come up once; the drive opens at the summary after that.
 */
export type PlanRun =
  | { phase: 'running'; plan: PlanSummary; view: RunView; stopping: boolean }
  | { phase: 'ended'; plan: PlanSummary; view: RunView; report: RunReport }
  | { phase: 'failed'; plan: PlanSummary; view: RunView; message: string }
  | { phase: 'summary'; plan: PlanSummary; report: RunReport; fresh: boolean };

/** A drive that went away while it was on screen, and what went with it. */
export interface Gone {
  name: string;
  lost: 'map' | 'scan' | 'nothing';
}

export type Action =
  /** A fresh listing of the drives. Drives no longer in it leave, and their data with them. */
  | { type: 'listed'; drives: VolumeInfo[] }
  /** The scan screen's drive cards: choose the drive to scan, staying on the scan screen. */
  | { type: 'pick'; mountPoint: string }
  /** The header's drive menu: a scanned drive opens its map, an unscanned one its scan screen. */
  | { type: 'switch'; mountPoint: string }
  /** Back from the scan screen to the drive's map. */
  | { type: 'openMap' }
  /** From the map to the drive's scan screen ("Scan again"). */
  | { type: 'showScanScreen' }
  | { type: 'scanStarted'; mountPoint: string; run: number; fast?: boolean }
  | { type: 'scanUpdate'; mountPoint: string; run: number; update: ScanUpdate }
  | { type: 'scanEnded'; mountPoint: string; run: number; outcome: ScanOutcome; at: number }
  | { type: 'scanFailed'; mountPoint: string; run: number; message: string }
  /** The user stopped the scan of this drive. */
  | { type: 'scanCancelled'; mountPoint: string }
  /** Puts a proposal in this drive's plan, or takes it out. */
  | { type: 'plan'; mountPoint: string; id: string; on: boolean }
  /** How far the duplicate search of the scan `run` has got. */
  | { type: 'dupsProgress'; mountPoint: string; run: number; progress: DuplicateProgress }
  /** The duplicate search of the scan `run` ended: the map and proposals again, with what it found. */
  | { type: 'dupsEnded'; mountPoint: string; run: number; outcome: DuplicatesOutcome }
  | { type: 'dupsFailed'; mountPoint: string; run: number; message: string }
  /** Unticks a duplicate group (every copy stays), or ticks it again. */
  | { type: 'skipGroup'; mountPoint: string; id: string; skipped: boolean }
  /**
   * Claude's answer about the scan `run` changed the map (its own items, unticked, or a layout): the map, proposals
   * and sorts to show from now on, unless a newer one (a higher `revision`) is already shown. The plan keeps what is
   * still there.
   */
  | { type: 'claudeApplied'; mountPoint: string; run: number; refreshed: ClaudeRefreshed }
  /**
   * The plan of the scan `run` as a whole (Let Claude pick, or Keep mine putting the plan back). With `from`, only
   * while the plan is still at that version: an edit made meanwhile is never overwritten.
   */
  | { type: 'setPlan'; mountPoint: string; run: number; ids: string[]; from?: number }
  /** The user confirmed the plan and it started: drive switching and scanning wait until it ends. */
  | { type: 'runStarted'; mountPoint: string; plan: PlanSummary }
  /** What the run did since the last batch, in order. */
  | { type: 'runEvents'; mountPoint: string; planId: string; events: RunEvent[] }
  /** The user pressed Stop; the run ends after the file being deleted. */
  | { type: 'runStopping'; mountPoint: string }
  | { type: 'runEnded'; mountPoint: string; planId: string; report: RunReport }
  | { type: 'runFailed'; mountPoint: string; planId: string; message: string }
  /** From the ended run to the drive's finish ("See what changed"): the highlight slides, then the summary. */
  | { type: 'showSummary'; mountPoint: string }
  /** The drive's summary came up: next time the drive opens at it, not at the slides. */
  | { type: 'summarySeen'; mountPoint: string }
  /** From a failed run back to the drive's plan. */
  | { type: 'leaveRun'; mountPoint: string }
  /** A kept run changed (an Undo ran): the drive whose run it is shows it as it is now. */
  | { type: 'reportUpdated'; report: RunReport }
  | { type: 'lock'; locked: boolean }
  | { type: 'dismissGone' };

const IDLE: DriveScan = { phase: 'idle' };

/** Before the first listing: no drives yet. */
export const EMPTY: Session = { drives: [], current: '', screen: 'scan', scans: {}, locked: false, gone: null };

export function scanOf(s: Session, mountPoint: string): DriveScan {
  return s.scans[mountPoint] ?? IDLE;
}

/**
 * The same drive as before: same place, same volume id. A different stick plugged in where the last one was is a
 * different drive, and the last one's map must not carry over to it. A name or kind that reads differently once
 * (the OS was slow to answer) doesn't make it another drive.
 */
export function sameDrive(a: VolumeInfo, b: VolumeInfo): boolean {
  return a.mountPoint === b.mountPoint && a.id === b.id;
}

/** What a drive's delete proposals would free together: its reclaimable space. */
export function reclaimableBytes(proposals: readonly Proposal[]): number {
  return proposals.filter((p) => p.action === 'delete').reduce((sum, p) => sum + p.bytes, 0);
}

/** A drive's reclaimable space once it has a map; zero until then, and once its plan has run. */
export function reclaimableOf(scan: DriveScan): number {
  if (scan.phase !== 'done' || ranPlan(scan)) return 0;
  return reclaimableBytes(scan.proposals);
}

/** The report of the plan a drive's scan ran, once the run has ended. */
export function reportOf(scan: DriveScan): RunReport | null {
  if (scan.phase !== 'done' || !scan.planRun) return null;
  const r = scan.planRun;
  return r.phase === 'ended' || r.phase === 'summary' ? r.report : null;
}

/** The scan's plan has run, or is running: its proposals are spent. A run that failed leaves them to try again. */
function ranPlan(scan: Extract<DriveScan, { phase: 'done' }>): boolean {
  return scan.planRun !== null && scan.planRun.phase !== 'failed';
}

/**
 * The figures to show for a drive: those its map was drawn against once it has one, so they agree with the map;
 * once its plan has run, the drive's own figures from the end of the run.
 */
export function shownVolume(s: Session, drive: VolumeInfo): VolumeInfo {
  const scan = scanOf(s, drive.mountPoint);
  if (scan.phase !== 'done') return drive;
  const report = reportOf(scan);
  if (!report || report.totalBytes === 0) return scan.overview.volume;
  return { ...scan.overview.volume, usedBytes: report.usedAfter, freeBytes: report.freeAfter };
}

/** The screen a drive opens at: its map once it has one, its scan screen until then. */
function screenFor(s: Session, mountPoint: string): Screen {
  return scanOf(s, mountPoint).phase === 'done' ? 'map' : 'scan';
}

/** Puts `mountPoint` on screen. A scan running on the drive being left stops (the provider cancels it). */
function goTo(s: Session, mountPoint: string, screen: Screen): Session {
  if (s.locked || !s.drives.some((d) => d.mountPoint === mountPoint)) return s;
  let scans = s.scans;
  if (mountPoint !== s.current && scanOf(s, s.current).phase === 'scanning') {
    scans = { ...scans };
    delete scans[s.current];
  }
  return { ...s, scans, current: mountPoint, screen, gone: null };
}

type DoneScan = Extract<DriveScan, { phase: 'done' }>;

/** `scan` with the plan `plan`: its version goes up by one when what is in it changes, and stays when it doesn't. */
function withPlan(scan: DoneScan, plan: string[]): DoneScan {
  const same = plan.length === scan.plan.length && plan.every((id) => scan.plan.includes(id));
  return same ? scan : { ...scan, plan, planVersion: scan.planVersion + 1 };
}

/** Applies `change` to the plan run of a drive whose scan is done. `change` returns the run as it was to do nothing. */
function onPlanRun(s: Session, mountPoint: string, change: (run: PlanRun | null) => PlanRun | null): Session {
  const scan = scanOf(s, mountPoint);
  if (scan.phase !== 'done') return s;
  const next = change(scan.planRun);
  if (next === scan.planRun) return s;
  return { ...s, scans: { ...s.scans, [mountPoint]: { ...scan, planRun: next } } };
}

/** Applies `change` to the duplicate search of the drive's scan `run`, while its plan hasn't run. */
function onSearch(
  s: Session,
  mountPoint: string,
  run: number,
  change: (scan: Extract<DriveScan, { phase: 'done' }>) => Extract<DriveScan, { phase: 'done' }>,
): Session {
  const scan = scanOf(s, mountPoint);
  if (scan.phase !== 'done' || scan.run !== run || scan.duplicates.phase !== 'checking') return s;
  return { ...s, scans: { ...s.scans, [mountPoint]: change(scan) } };
}

/**
 * Applies `change` to the drive's finished scan `run` while its plan hasn't run. A later scan, a scan whose plan ran,
 * or none, ignores it.
 */
function onScan(
  s: Session,
  mountPoint: string,
  run: number,
  change: (scan: Extract<DriveScan, { phase: 'done' }>) => Extract<DriveScan, { phase: 'done' }>,
): Session {
  const scan = scanOf(s, mountPoint);
  if (scan.phase !== 'done' || scan.run !== run || ranPlan(scan)) return s;
  const next = change(scan);
  return next === scan ? s : { ...s, scans: { ...s.scans, [mountPoint]: next } };
}

/** Applies `change` to the drive's scan only if it is the scan `run` (a later scan, or none, ignores it). */
function onRun(
  s: Session,
  mountPoint: string,
  run: number,
  change: (scan: Extract<DriveScan, { phase: 'scanning' }>) => DriveScan,
): Session {
  const scan = scanOf(s, mountPoint);
  if (scan.phase !== 'scanning' || scan.run !== run) return s;
  return { ...s, scans: { ...s.scans, [mountPoint]: change(scan) } };
}

export function reduce(s: Session, a: Action): Session {
  switch (a.type) {
    case 'listed':
      return listed(s, a.drives);
    case 'pick':
      return goTo(s, a.mountPoint, 'scan');
    case 'switch':
      return goTo(s, a.mountPoint, screenFor(s, a.mountPoint));
    case 'openMap':
      return screenFor(s, s.current) === 'map' ? { ...s, screen: 'map' } : s;
    case 'showScanScreen':
      return s.locked ? s : { ...s, screen: 'scan' };
    case 'scanStarted':
      if (s.locked || !s.drives.some((d) => d.mountPoint === a.mountPoint)) return s;
      return {
        ...s,
        scans: { ...s.scans, [a.mountPoint]: { phase: 'scanning', run: a.run, fast: a.fast ?? false, update: null, preview: [] } },
      };
    case 'scanUpdate':
      return onRun(s, a.mountPoint, a.run, (scan) => ({
        ...scan,
        update: a.update,
        preview: a.update.categories ?? scan.preview,
      }));
    case 'scanEnded': {
      const next = onRun(s, a.mountPoint, a.run, () =>
        a.outcome.status === 'done'
          ? {
              phase: 'done',
              run: a.run,
              scan: a.outcome.scan,
              claudeRevision: 0,
              overview: a.outcome.overview,
              proposals: a.outcome.proposals,
              tidy: a.outcome.tidy,
              plan: a.outcome.proposals.filter((p) => p.defaultOn).map((p) => p.id),
              planVersion: 0,
              at: a.at,
              planRun: null,
              duplicates: { phase: 'checking', progress: null },
              skippedGroups: [],
            }
          : IDLE,
      );
      return next !== s && a.mountPoint === s.current && a.outcome.status === 'done' ? { ...next, screen: 'map' } : next;
    }
    case 'scanFailed':
      return onRun(s, a.mountPoint, a.run, () => ({ phase: 'failed', message: a.message }));
    case 'scanCancelled': {
      if (scanOf(s, a.mountPoint).phase !== 'scanning') return s;
      const scans = { ...s.scans };
      delete scans[a.mountPoint];
      return { ...s, scans };
    }
    case 'plan': {
      const scan = scanOf(s, a.mountPoint);
      const known = scan.phase === 'done' && (scan.proposals.some((p) => p.id === a.id) || scan.tidy.some((t) => t.id === a.id));
      if (scan.phase !== 'done' || !known || ranPlan(scan)) return s;
      if (scan.plan.includes(a.id) === a.on) return s;
      const plan = a.on ? [...scan.plan, a.id] : scan.plan.filter((id) => id !== a.id);
      return { ...s, scans: { ...s.scans, [a.mountPoint]: withPlan(scan, plan) } };
    }
    case 'dupsProgress':
      return onSearch(s, a.mountPoint, a.run, (scan) => ({
        ...scan,
        duplicates: { phase: 'checking', progress: a.progress },
      }));
    case 'dupsEnded':
      return onSearch(s, a.mountPoint, a.run, (scan) => {
        // Once its plan has run, a scan's map stays the one the run started from.
        if (a.outcome.status === 'cancelled' || ranPlan(scan)) return { ...scan, duplicates: { phase: 'stopped' } };
        const ids = new Set([...a.outcome.proposals.map((p) => p.id), ...a.outcome.tidy.map((t) => t.id)]);
        return {
          ...withPlan(scan, scan.plan.filter((id) => ids.has(id))),
          overview: a.outcome.overview,
          proposals: a.outcome.proposals,
          // The sorts again, with the extra copies marked as deleted by duplicates (delete wins over sort).
          tidy: a.outcome.tidy,
          duplicates: { phase: 'done', found: a.outcome.duplicates },
        };
      });
    case 'dupsFailed':
      return onSearch(s, a.mountPoint, a.run, (scan) => ({
        ...scan,
        duplicates: { phase: 'failed', message: a.message },
      }));
    case 'skipGroup': {
      const scan = scanOf(s, a.mountPoint);
      if (scan.phase !== 'done' || ranPlan(scan) || scan.skippedGroups.includes(a.id) === a.skipped) return s;
      const skippedGroups = a.skipped ? [...scan.skippedGroups, a.id] : scan.skippedGroups.filter((g) => g !== a.id);
      return { ...s, scans: { ...s.scans, [a.mountPoint]: { ...scan, skippedGroups } } };
    }
    case 'claudeApplied':
      return onScan(s, a.mountPoint, a.run, (scan) => {
        if (a.refreshed.revision <= scan.claudeRevision) return scan;
        const ids = new Set([...a.refreshed.proposals.map((p) => p.id), ...a.refreshed.tidy.map((t) => t.id)]);
        return {
          ...withPlan(scan, scan.plan.filter((id) => ids.has(id))),
          overview: a.refreshed.overview,
          proposals: a.refreshed.proposals,
          tidy: a.refreshed.tidy,
          claudeRevision: a.refreshed.revision,
        };
      });
    case 'setPlan':
      return onScan(s, a.mountPoint, a.run, (scan) => {
        if (a.from !== undefined && a.from !== scan.planVersion) return scan;
        const known = (id: string) => scan.proposals.some((p) => p.id === id) || scan.tidy.some((t) => t.id === id);
        return withPlan(scan, [...new Set(a.ids)].filter(known));
      });
    case 'runStarted': {
      if (s.locked || a.plan.mountPoint !== a.mountPoint) return s;
      const next = onPlanRun(s, a.mountPoint, (run) =>
        run === null || run.phase === 'failed'
          ? { phase: 'running', plan: a.plan, view: startRun(a.plan), stopping: false }
          : run,
      );
      if (next === s) return s;
      // A run stops a duplicate search still reading the drive: the core stops it too.
      const ran = scanOf(next, a.mountPoint);
      const scans =
        ran.phase === 'done' && ran.duplicates.phase === 'checking'
          ? { ...next.scans, [a.mountPoint]: { ...ran, duplicates: { phase: 'stopped' as const } } }
          : next.scans;
      return { ...next, scans, locked: true };
    }
    case 'runEvents':
      return onPlanRun(s, a.mountPoint, (run) =>
        run?.phase === 'running' && run.plan.id === a.planId ? { ...run, view: foldEvents(run.view, a.events) } : run,
      );
    case 'runStopping':
      return onPlanRun(s, a.mountPoint, (run) =>
        run?.phase === 'running' && !run.stopping ? { ...run, stopping: true } : run,
      );
    case 'runEnded':
    case 'runFailed': {
      const next = onPlanRun(s, a.mountPoint, (run) => {
        if (run?.phase !== 'running' || run.plan.id !== a.planId) return run;
        return a.type === 'runEnded'
          ? { phase: 'ended', plan: run.plan, view: settleRun(run.view, a.report), report: a.report }
          : { phase: 'failed', plan: run.plan, view: run.view, message: a.message };
      });
      // The run is over either way, even if its drive has gone since: unlock.
      return next.locked ? { ...next, locked: false } : next;
    }
    case 'showSummary':
      return onPlanRun(s, a.mountPoint, (run) =>
        run?.phase === 'ended' ? { phase: 'summary', plan: run.plan, report: run.report, fresh: true } : run,
      );
    case 'summarySeen':
      return onPlanRun(s, a.mountPoint, (run) => (run?.phase === 'summary' && run.fresh ? { ...run, fresh: false } : run));
    case 'leaveRun':
      return onPlanRun(s, a.mountPoint, (run) => (run?.phase === 'failed' ? null : run));
    case 'reportUpdated': {
      const mountPoint = Object.keys(s.scans).find((m) => reportOf(scanOf(s, m))?.id === a.report.id);
      if (mountPoint === undefined) return s;
      return onPlanRun(s, mountPoint, (run) =>
        run && (run.phase === 'ended' || run.phase === 'summary') && run.report.id === a.report.id ? { ...run, report: a.report } : run,
      );
    }
    case 'lock':
      return a.locked === s.locked ? s : { ...s, locked: a.locked };
    case 'dismissGone':
      return s.gone ? { ...s, gone: null } : s;
  }
}

/**
 * A fresh listing. Drives that left take their scans with them. If the drive on screen left, the system drive takes
 * its place (at its map if it has one) and `gone` says what happened.
 */
function listed(s: Session, drives: VolumeInfo[]): Session {
  const first = drives.find((d) => d.system) ?? drives[0];
  if (!first) return s; // the system drive is always listed; an empty listing is a failed one
  if (s.drives.length === 0) return { ...EMPTY, locked: s.locked, drives, current: first.mountPoint };
  if (drives.length === s.drives.length && drives.every((d, i) => sameVolume(d, s.drives[i]!))) return s;

  const left = s.drives.filter((d) => !drives.some((n) => sameDrive(n, d)));
  let scans = s.scans;
  if (left.length) {
    scans = { ...scans };
    for (const d of left) delete scans[d.mountPoint];
  }
  const next = { ...s, drives, scans };
  const onScreen = left.find((d) => d.mountPoint === s.current);
  if (!onScreen) return next;
  const scan = scanOf(s, s.current).phase;
  const lost = scan === 'done' ? 'map' : scan === 'scanning' ? 'scan' : 'nothing';
  return {
    ...next,
    current: first.mountPoint,
    screen: screenFor(next, first.mountPoint),
    gone: { name: onScreen.name, lost },
  };
}

function sameVolume(a: VolumeInfo, b: VolumeInfo): boolean {
  return (
    sameDrive(a, b) &&
    a.name === b.name &&
    a.kind === b.kind &&
    a.system === b.system &&
    a.totalBytes === b.totalBytes &&
    a.usedBytes === b.usedBytes &&
    a.freeBytes === b.freeBytes
  );
}

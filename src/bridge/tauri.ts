import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  AdvisorStatus,
  Bridge,
  DuplicateProgress,
  DuplicatesOutcome,
  FullDiskAccess,
  MapFolder,
  PlanSummary,
  Platform,
  RunEvent,
  RunHistory,
  RunReport,
  ScanOutcome,
  ScanUpdate,
  VolumeInfo,
} from './types';

/** The real bridge: Tauri commands registered in `src-tauri/src/lib.rs`. Keep the command names in step with it. */
export function createTauriBridge(): Bridge {
  return {
    platform: detectPlatform(),
    machineName: () => invoke<string>('machine_name'),
    advisorStatus: () => invoke<AdvisorStatus>('advisor_status'),
    listVolumes: () => invoke<VolumeInfo[]>('list_volumes'),
    forgetDrive: (mountPoint) => invoke<undefined>('forget_drive', { mountPoint }),
    fullDiskAccess: () => invoke<FullDiskAccess>('full_disk_access'),
    openFullDiskAccessSettings: () => invoke<undefined>('open_full_disk_access_settings'),
    scan: (mountPoint, onUpdate) => {
      const onUpdateChannel = new Channel<ScanUpdate>();
      onUpdateChannel.onmessage = onUpdate;
      return invoke<ScanOutcome>('scan_volume', { mountPoint, onUpdate: onUpdateChannel });
    },
    cancelScan: () => invoke<undefined>('cancel_scan'),
    browse: (mountPoint, id) => invoke<MapFolder>('browse', { mountPoint, id }),
    findDuplicates: (mountPoint, onProgress) => {
      const onProgressChannel = new Channel<DuplicateProgress>();
      onProgressChannel.onmessage = onProgress;
      return invoke<DuplicatesOutcome>('find_duplicates', { mountPoint, onProgress: onProgressChannel });
    },
    buildPlan: (mountPoint, ids, skippedGroups) =>
      invoke<PlanSummary>('build_plan', { mountPoint, ids, skippedGroups: skippedGroups ?? [] }),
    runPlan: (mountPoint, planId, onEvent) => {
      const onEventChannel = new Channel<RunEvent>();
      onEventChannel.onmessage = onEvent;
      return invoke<RunReport>('run_plan', { mountPoint, planId, onEvent: onEventChannel });
    },
    cancelRun: () => invoke<undefined>('cancel_run'),
    listRuns: () => invoke<RunHistory>('list_runs'),
    openRun: (id) => invoke<RunReport>('open_run', { id }),
    openHint: (mountPoint, proposalId) => invoke<undefined>('open_hint', { mountPoint, proposalId }),
    openGuide: (guide) => invoke<undefined>('open_guide', { guide }),
  };
}

/** WKWebView reports a Mac user agent; WebView2 reports Windows. */
function detectPlatform(): Platform {
  return /Mac/.test(navigator.userAgent) ? 'macos' : 'windows';
}

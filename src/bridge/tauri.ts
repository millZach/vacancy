import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  Bridge,
  FullDiskAccess,
  MapFolder,
  PlanSummary,
  Platform,
  RunEvent,
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
    buildPlan: (mountPoint, ids) => invoke<PlanSummary>('build_plan', { mountPoint, ids }),
    runPlan: (mountPoint, planId, onEvent) => {
      const onEventChannel = new Channel<RunEvent>();
      onEventChannel.onmessage = onEvent;
      return invoke<RunReport>('run_plan', { mountPoint, planId, onEvent: onEventChannel });
    },
    cancelRun: () => invoke<undefined>('cancel_run'),
    listRuns: () => invoke<RunReport[]>('list_runs'),
  };
}

/** WKWebView reports a Mac user agent; WebView2 reports Windows. */
function detectPlatform(): Platform {
  return /Mac/.test(navigator.userAgent) ? 'macos' : 'windows';
}

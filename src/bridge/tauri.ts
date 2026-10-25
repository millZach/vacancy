import { Channel, invoke } from '@tauri-apps/api/core';
import type {
  AdvisorStatus,
  Bridge,
  ClaudeChatStarted,
  ClaudeEvent,
  ClaudeRefreshed,
  ClaudeStatus,
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
    scan: (mountPoint, onUpdate, options) => {
      const onUpdateChannel = new Channel<ScanUpdate>();
      onUpdateChannel.onmessage = onUpdate;
      const fast = options?.fast ?? false;
      return invoke<ScanOutcome>('scan_volume', { mountPoint, fast, onUpdate: onUpdateChannel });
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
    undoTidy: (runId, tidyId) => invoke<RunReport>('undo_tidy', { runId, tidyId }),
    openHint: (mountPoint, proposalId) => invoke<undefined>('open_hint', { mountPoint, proposalId }),
    openGuide: (guide) => invoke<undefined>('open_guide', { guide }),
    claudeStatus: () => invoke<ClaudeStatus>('claude_status'),
    claudeSetEnabled: (on) => invoke<ClaudeStatus>('claude_set_enabled', { on }),
    claudeSetPrivacy: (on) => invoke<ClaudeStatus>('claude_set_privacy', { on }),
  claudeSetModel: (model, effort) => invoke<ClaudeStatus>('claude_set_model', { model, effort }),
    claudeCheck: (again) => invoke<ClaudeStatus>('claude_check', { again }),
    claudeStart: (mountPoint, scan, job, onEvent) => {
      const onEventChannel = new Channel<ClaudeEvent>();
      onEventChannel.onmessage = onEvent;
      return invoke<string>('claude_start', { mountPoint, scan, job, onEvent: onEventChannel });
    },
    claudeChat: (mountPoint, scan, conversation, about, text, onEvent) => {
      const onEventChannel = new Channel<ClaudeEvent>();
      onEventChannel.onmessage = onEvent;
      return invoke<ClaudeChatStarted>('claude_chat', { mountPoint, scan, conversation, about, text, onEvent: onEventChannel });
    },
    claudeCancel: (requestId) => invoke<undefined>('claude_cancel', { requestId }),
    claudeChooseLayout: (mountPoint, scan, tidyId, layout) =>
      invoke<ClaudeRefreshed>('claude_choose_layout', { mountPoint, scan, tidyId, layout }),
    claudeRemoveItems: (mountPoint, scan, ids) => invoke<ClaudeRefreshed>('claude_remove_items', { mountPoint, scan, ids }),
  };
}

/** WKWebView reports a Mac user agent; WebView2 reports Windows. */
function detectPlatform(): Platform {
  return /Mac/.test(navigator.userAgent) ? 'macos' : 'windows';
}

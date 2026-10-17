import { invoke } from '@tauri-apps/api/core';
import type { Bridge, Platform, VolumeInfo } from './types';

/** The real bridge: Tauri commands registered in `src-tauri/src/lib.rs`. Keep the command names in step with it. */
export function createTauriBridge(): Bridge {
  return {
    platform: detectPlatform(),
    systemVolume: () => invoke<VolumeInfo>('system_volume'),
    machineName: () => invoke<string>('machine_name'),
  };
}

/** WKWebView reports a Mac user agent; WebView2 reports Windows. */
function detectPlatform(): Platform {
  return /Mac/.test(navigator.userAgent) ? 'macos' : 'windows';
}

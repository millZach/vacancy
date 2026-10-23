import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// The UI runs in Tauri's webview (WebView2 on Windows, WKWebView on macOS) and, with the mock bridge, in any
// browser. Tauri expects a fixed dev port.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/target/**', '**/crates/**'] },
  },
  build: {
    target: ['es2022', 'chrome110', 'safari15'],
    sourcemap: false,
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['src/test/setup.ts'],
    css: false,
    restoreMocks: true,
    // Whole-app flows (scan, plan, run, summary, history) take about a second here but over five on CI's
    // Windows runners, past Vitest's default limit.
    testTimeout: 20_000,
  },
});

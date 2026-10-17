// Regenerates every app icon from tools/app-icon.html. Run: node tools/render-icon.mjs
//   1. Renders the macOS layout (Apple's grid) to src-tauri/icons/app-icon.png in Edge and runs `tauri icon` on it,
//      which writes every size: the PNGs, icon.icns and a first icon.ico.
//   2. Renders the full-bleed Windows layout and runs `tauri icon` on it in a temp folder, and keeps only its
//      icon.ico, because Windows icons fill their square.
//   3. Deletes what `tauri icon` writes that Vacancy doesn't ship (Android, iOS and Windows Store logos).
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { chromium } from 'playwright';

const html = pathToFileURL(fileURLToPath(new URL('./app-icon.html', import.meta.url))).href;
const iconsDir = fileURLToPath(new URL('../src-tauri/icons/', import.meta.url));
const tauriCli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url));
const temp = mkdtempSync(join(tmpdir(), 'vacancy-icon-'));

// The files src-tauri/tauri.conf.json bundles, plus the 512px icon.png and the rendered source.
const shipped = new Set(['32x32.png', '128x128.png', '128x128@2x.png', 'icon.icns', 'icon.ico', 'icon.png', 'app-icon.png']);

async function render(layout, out) {
  const browser = await chromium.launch({ channel: 'msedge' });
  try {
    const page = await browser.newPage({ viewport: { width: 1024, height: 1024 } });
    await page.goto(layout ? `${html}?layout=${layout}` : html);
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: out, omitBackground: true });
  } finally {
    await browser.close();
  }
}

function tauriIcon(source, outDir) {
  const run = spawnSync(process.execPath, [tauriCli, 'icon', source, '--output', outDir], { stdio: 'inherit' });
  if (run.status !== 0) throw new Error(`tauri icon failed on ${source}`);
}

try {
  const macSource = join(iconsDir, 'app-icon.png');
  await render(null, macSource);
  tauriIcon(macSource, iconsDir);

  const windowsSource = join(temp, 'app-icon-windows.png');
  const windowsOut = join(temp, 'windows');
  await render('windows', windowsSource);
  tauriIcon(windowsSource, windowsOut);
  copyFileSync(join(windowsOut, 'icon.ico'), join(iconsDir, 'icon.ico'));

  for (const entry of readdirSync(iconsDir)) {
    if (!shipped.has(entry)) rmSync(join(iconsDir, entry), { recursive: true, force: true });
  }
  console.log(`Icons written to ${iconsDir}`);
} finally {
  rmSync(temp, { recursive: true, force: true });
}

// Read-only saved-report proof: node tools/move-advice-shots.mjs --out shots/move-advice (inside the shared heavy-job wrapper).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';
const { values: o } = parseArgs({ options: { out: { type: 'string', default: 'shots/move-advice' } } });
await mkdir(o.out, { recursive: true });
const origin = 'http://127.0.0.1:1434';
const server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', '1434', '--strictPort'], { windowsHide: true, stdio: 'pipe' });
server.stdout.on('data', data => process.stdout.write(data));
server.stderr.on('data', data => process.stderr.write(data));
let browser;
const results = [];
try {
  const until = Date.now() + 15000;
  for (;;) {
    assert.equal(server.exitCode, null, 'Owned proof server must be running');
    try { const response = await fetch(`${origin}/tools/move-advice-proof.html`); if (response.ok && (await response.text()).includes('saved-move-advice')) break; } catch { /* Server is starting. */ }
    assert(Date.now() < until, 'Proof server startup timed out');
    await new Promise(resolve => setTimeout(resolve, 150));
  }
  browser = await chromium.launch({ channel: 'msedge' });
  for (const [fixture, width, height] of [['windows', 1280, 820], ['macos', 1100, 700]]) {
    const page = await browser.newPage({ viewport: { width, height } });
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    await page.goto(`${origin}/tools/move-advice-proof.html?fixture=${fixture}`);
    await page.getByRole('button', { name: 'Past runs' }).click();
    await page.getByRole('menuitem').click();
    const summary = page.getByRole('complementary', { name: 'What Vacancy did' });
    await summary.getByRole('button', { name: 'Close', exact: true }).waitFor();
    await page.evaluate(() => document.fonts.ready);
    assert(!(await summary.textContent()).includes('No files were deleted or moved.'));
    assert.equal(await summary.locator('[data-fly-target="deleted"]').textContent(), '0');
    assert.equal(await summary.locator('[data-fly-target="moved"]').textContent(), '4');
    assert((await summary.textContent()).includes('Downloads. 4 files moved.'));
    assert((await summary.locator('.advice-why').textContent()).includes('Uninstall it from Steam'));
    await page.waitForTimeout(1500);
    const path = join(o.out, `${fixture}-saved-move-advice.png`);
    await page.screenshot({ path }); console.log(path);
    assert.deepEqual(errors, []);
    results.push({ fixture, width, height, surface: 'Vite source proof fixture reopening a saved report through the real App history UI', deleted: 0, moved: 4, freed: 0 });
    await page.close();
  }
  await writeFile(join(o.out, 'results.json'), JSON.stringify(results, null, 2));
} finally {
  await browser?.close();
  if (server.exitCode === null) { const stopped = once(server, 'exit'); server.kill(); await stopped; }
}

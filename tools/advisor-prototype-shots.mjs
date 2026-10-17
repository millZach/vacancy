// THROWAWAY: after prototype:advisor:build, node tools/advisor-prototype-shots.mjs --serve --out <shots>; --serve owns/closes a static preview within the same heavy job.
import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const { values: options } = parseArgs({ options: { out: { type: 'string', default: 'shots' }, url: { type: 'string', default: 'http://127.0.0.1:1433' }, serve: { type: 'boolean', default: false } } });
await mkdir(options.out, { recursive: true });
const origin = new URL(options.url).origin;
const server = options.serve ? spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--config', 'prototypes/advisor-status/vite.config.ts', '--mode', 'prototype', '--host', '127.0.0.1', '--port', '1433', '--strictPort'], { cwd: fileURLToPath(new URL('..', import.meta.url)), windowsHide: true, stdio: 'pipe' }) : null;
if (server) {
  console.log(`Owned static preview PID ${server.pid}`);
  server.stderr.on('data', data => process.stderr.write(data));
  server.stdout.on('data', data => process.stdout.write(data));
}
let browser = null;
const results = [];
try {
  if (server) {
    const until = Date.now() + 15_000;
    while (true) {
      if (server.exitCode !== null) throw new Error(`Preview exited ${server.exitCode}`);
      try {
        const response = await fetch(origin);
        if (response.ok) {
          const expected = await readFile(fileURLToPath(new URL('../prototypes/advisor-status/dist/index.html', import.meta.url)), 'utf8');
          assert.equal(await response.text(), expected, 'Preview must serve the build being verified');
          break;
        }
      } catch (error) { if (error instanceof assert.AssertionError) throw error; /* Preview is still starting. */ }
      assert(Date.now() < until, 'Static preview did not start');
      await new Promise(done => setTimeout(done, 150));
    }
  }
  browser = await chromium.launch({ channel: 'msedge' });
  for (const [fixture, width, height] of [['windows', 1280, 820], ['macos', 1100, 700]]) {
    const context = await browser.newContext({ viewport: { width, height } });
    const external = [];
    await context.route('**/*', async route => {
      const url = route.request().url();
      if (url.startsWith(origin) || url.startsWith('data:')) await route.continue();
      else { external.push(url); await route.abort(); }
    });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    const shot = async name => { const path = join(options.out, `${fixture}-${name}.png`); await page.screenshot({ path }); console.log(path); };
    for (const variant of ['A', 'B', 'C']) {
      await page.goto(`${origin}/?fixture=${fixture}&variant=${variant}`);
      await page.locator('.drive-card').first().waitFor();
      assert.equal(await page.locator('.advisor-prototype').count(), 1, 'Expected the throwaway prototype host');
      const dialog = page.getByRole('dialog');
      await dialog.waitFor();
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(350);
      assert.equal(await page.locator('input').count(), 0, 'No credential/API fields');
      for (const control of await dialog.getByRole('switch').all()) {
        assert(await control.isDisabled());
        assert.equal(await control.getAttribute('aria-checked'), 'false');
      }
      assert.equal(await dialog.getByText('Claude Code', { exact: true }).count(), 1);
      assert.equal(await dialog.getByText('Codex', { exact: true }).count(), 1);
      assert(await page.getByRole('button', { name: 'Why can’t these apps be used?' }).evaluate(el => {
        const box = el.getBoundingClientRect(), clip = el.parentElement.getBoundingClientRect();
        return box.top >= clip.top - 1 && box.bottom <= clip.bottom + 1;
      }), 'Privacy explanation disclosure is visible before scrolling');
      await shot(`${variant}-scan-open`);
      await page.getByRole('button', { name: 'Why can’t these apps be used?' }).click();
      const explanation = page.getByText(/These versions may include other files/);
      await explanation.waitFor();
      await explanation.scrollIntoViewIfNeeded();
      await shot(`${variant}-details`);
      const explanationBounds = await explanation.evaluate(el => {
        const box = el.getBoundingClientRect(), clip = el.parentElement.getBoundingClientRect();
        return { top: box.top, bottom: box.bottom, clipTop: clip.top, clipBottom: clip.bottom };
      });
      assert(explanationBounds.top >= explanationBounds.clipTop - 1 && explanationBounds.bottom <= explanationBounds.clipBottom + 1,
        `Full privacy explanation remains accessible in its scroll region: ${JSON.stringify(explanationBounds)}`);
      if (variant !== 'A') {
        const dismissal = dialog.getByRole('button', { name: variant === 'B' ? 'Done' : 'Continue with built-in rules' });
        assert(await dismissal.evaluate(el => {
          const box = el.getBoundingClientRect();
          return box.top >= 0 && box.bottom <= innerHeight && el.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
        }), 'Expanded details keep the dismissal visible and unobscured');
      }
      await page.keyboard.press('Escape');
      assert.equal(await dialog.count(), 0);
      assert(await page.getByRole('button', { name: 'Rules only' }).evaluate(el => el === document.activeElement));
      await shot(`${variant}-scan-closed`);
      await page.getByRole('button', { name: 'AI options' }).click();
      await dialog.waitFor();
      await page.getByRole('button', { name: 'Close AI settings' }).click();
      await page.getByRole('button', { name: /^Scan / }).click();
      await page.locator('.atlas.is-map').waitFor();
      const driveBox = await page.locator('.drive-menu-button').boundingBox();
      const actionsBox = await page.locator('.head-right').boundingBox();
      assert(driveBox && actionsBox && driveBox.x + driveBox.width < actionsBox.x, 'Drive picker and header actions do not overlap');
      await page.getByRole('button', { name: 'Rules only' }).click();
      await dialog.waitFor();
      if (variant === 'A') {
        const popover = await dialog.boundingBox();
        const badge = await page.getByRole('button', { name: 'Rules only' }).boundingBox();
        assert(popover && badge && Math.abs(popover.x + popover.width - badge.x - badge.width) < 2, 'Popover remains anchored to the agent badge on the map');
      }
      await shot(`${variant}-map-open`);
      // Arrow switch and URL survive a full reload.
      const next = variant === 'A' ? 'B' : variant === 'B' ? 'C' : 'A';
      await page.keyboard.press('ArrowRight');
      assert.equal(new URL(page.url()).searchParams.get('variant'), next);
      await page.reload();
      await page.getByRole('dialog').waitFor();
      assert.equal(await page.locator('.advisor-prototype').getAttribute('data-variant'), next);
      results.push({ fixture, variant, width, height, checks: 'disabled controls, no API fields, complete disclosure, expanded dismissal visibility, Escape/refocus, scan-note reopen, mock scan to map, no header overlap, header reopen, keyboard cycling, reload' });
    }
    assert.deepEqual(errors, [], 'No page errors');
    assert.deepEqual(external, [], 'No external requests');
    await context.close();
  }
} finally {
  await browser?.close();
  if (server && server.exitCode === null) { const stopped = once(server, 'exit'); server.kill(); await stopped; }
}
await writeFile(join(options.out, 'interaction-results.json'), `${JSON.stringify(results, null, 2)}\n`);
console.log('All prototype interactions passed.');

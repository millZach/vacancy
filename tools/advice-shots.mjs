// Mock-only advice proof: after npm run build, node tools/advice-shots.mjs --out shots/advice. Owns/closes static preview and Edge; run inside Invoke-HeavyJob.ps1.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({ options: { out: { type: 'string', default: 'shots/advice' } } });
await mkdir(o.out, { recursive: true });
const origin = 'http://127.0.0.1:1434';
const server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--host', '127.0.0.1', '--port', '1434', '--strictPort'], { windowsHide: true, stdio: 'pipe' });
server.stdout.on('data', data => process.stdout.write(data));
server.stderr.on('data', data => process.stderr.write(data));
let browser;
const results = [];
try {
  const until = Date.now() + 15000;
  for (;;) {
    assert(server.exitCode === null, `Static preview exited ${server.exitCode}`);
    try {
      const response = await fetch(origin);
      if (response.ok) { assert.equal(await response.text(), await readFile('dist/index.html', 'utf8')); break; }
    } catch (error) { if (error instanceof assert.AssertionError) throw error; }
    assert(Date.now() < until, 'Static preview failed to start');
    await new Promise(resolve => setTimeout(resolve, 150));
  }
  browser = await chromium.launch({ channel: 'msedge' });
  for (const [fixture, width, height] of [['windows', 1280, 820], ['macos', 1100, 700]]) {
    for (const mode of ['advice-only', 'mixed']) {
      const context = await browser.newContext({ viewport: { width, height } });
      const external = [], errors = [];
      await context.route('**/*', async route => {
        const url = route.request().url();
        if (new URL(url).origin === origin || url.startsWith('data:')) await route.continue();
        else { external.push(url); await route.abort(); }
      });
      const page = await context.newPage();
      page.on('pageerror', error => errors.push(String(error)));
      const shot = async name => {
        await page.waitForTimeout(420); // Let the tray/dialog/slide entry finish before saving the settled view.
        const path = join(o.out, `${fixture}-${mode}-${name}.png`);
        await page.screenshot({ path }); console.log(path);
      };
      await page.goto(`${origin}/?fixture=${fixture}&scan=0&run=1500&fda=granted&history=0${mode === 'mixed' ? '&moves=1' : ''}`);
      await page.getByRole('button', { name: /^Scan / }).waitFor();
      await page.evaluate(() => document.fonts.ready);
      await shot('scan');
      await page.getByRole('button', { name: /^Scan / }).click();
      await page.locator('.atlas.is-map').waitFor();
      await page.getByRole('button', { name: 'Build a plan' }).click();
      const tray = page.getByRole('complementary', { name: 'Plan' });
      const hint = tray.getByRole('checkbox', { name: /^Remind me:/ });
      assert.equal(await hint.getAttribute('aria-checked'), 'false');
      const total = await tray.locator('.tray-total-num').textContent();
      await hint.click();
      assert.equal(await tray.locator('.tray-total-num').textContent(), total, 'Advice does not inflate total');
      if (mode === 'advice-only') {
        for (const box of await tray.getByRole('region', { name: 'Delete' }).getByRole('checkbox').all()) {
          if (await box.getAttribute('aria-checked') === 'true') await box.click();
        }
        assert.equal((await tray.locator('.tray-total-num').textContent()).replaceAll('\u00a0', ' '), '0 B');
      }
      const advice = tray.getByRole('region', { name: 'Left for you' });
      await advice.locator('.tray-row-title').click();
      if (fixture === 'windows') {
        await advice.getByRole('button', { name: 'Open Steam uninstall' }).click();
        assert((await advice.getByRole('alert').textContent()).includes('No app was launched.'));
      } else {
        assert.equal(await advice.getByRole('button', { name: 'Open Steam uninstall' }).count(), 0);
      }
      await advice.scrollIntoViewIfNeeded();
      await shot('tray');
      await tray.locator('.tray-cta').click();
      const confirm = page.getByRole('alertdialog');
      await confirm.waitFor();
      assert((await confirm.textContent()).includes('count them as freed space'));
      if (mode === 'advice-only') assert.equal(await confirm.locator('h2').textContent(), 'Save 1 reminder?');
      await shot('confirm');
      await confirm.getByRole('button', { name: 'Go back' }).click();
      assert.equal(await page.getByRole('complementary', { name: 'Run' }).count(), 0);
      await tray.locator('.tray-cta').click();
      await confirm.getByRole('button', { name: mode === 'advice-only' ? 'Save reminders' : /permanently$/ }).click();
      const run = page.getByRole('complementary', { name: 'Run' });
      await run.getByRole('heading', { name: mode === 'advice-only' ? 'Reminders saved' : 'Done' }).waitFor();
      if (mode === 'advice-only') assert((await run.textContent()).includes('No files were deleted or moved.'));
      await shot('done');
      await run.getByRole('button', { name: 'See what changed' }).click();
      const highlights = page.getByRole('region', { name: 'Highlights of this run' });
      await highlights.waitFor();
      await page.getByRole('button', { name: 'Pause the slides' }).click();
      await page.locator('.board').evaluate(el => Promise.all(el.getAnimations({ subtree: true }).map(animation => animation.finished.catch(() => {}))));
      await shot('headline');
      const dots = page.locator('.wrap-nav .dots button');
      assert.equal(await dots.count(), mode === 'mixed' ? 7 : 6, 'Existing slide deck preserved');
      await page.getByRole('button', { name: /Slide .*Skipped and left for you/ }).click();
      await page.waitForTimeout(500);
      await shot('left-for-you-slide');
      await page.getByRole('button', { name: 'Skip to recap' }).click();
      await page.locator('.finish.is-rest').waitFor();
      const summary = page.getByRole('complementary', { name: 'What Vacancy did' });
      await summary.waitFor();
      await summary.getByRole('heading', { name: 'Left for you' }).scrollIntoViewIfNeeded();
      await shot('summary');
      assert(await summary.locator('.advice-why').textContent());
      if (fixture === 'windows') {
        await summary.getByRole('button', { name: 'Open Steam uninstall' }).click();
        assert((await summary.getByRole('alert').textContent()).includes('No app was launched.'));
        await shot('summary-fixture-refusal');
      }
      if (mode === 'advice-only') assert.equal(await summary.locator('[data-fly-target="deleted"]').textContent(), '0');
      await summary.getByRole('button', { name: 'Replay highlights' }).click();
      await highlights.waitFor();
      await page.locator('.finish.is-slides').waitFor();
      await page.locator('.board').evaluate(el => Promise.all(el.getAnimations({ subtree: true }).map(animation => animation.finished.catch(() => {}))));
      await shot('replay');
      await page.getByRole('button', { name: 'Skip to recap' }).click();
      await page.locator('.finish.is-rest').waitFor();
      await summary.waitFor();
      await page.getByRole('button', { name: 'Past runs' }).click();
      await page.getByRole('menuitem').click();
      await summary.getByRole('button', { name: 'Close', exact: true }).waitFor();
      await summary.getByRole('heading', { name: 'Left for you' }).scrollIntoViewIfNeeded();
      await shot('history');
      assert(await summary.locator('.advice-why').textContent());
      if (fixture === 'windows') {
        await summary.getByRole('button', { name: 'Open Steam uninstall' }).click();
        assert((await summary.getByRole('alert').textContent()).includes('No app was launched.'));
        await shot('history-fixture-refusal');
      }
      assert.deepEqual(errors, []); assert.deepEqual(external, []);
      results.push({ fixture, mode, width, height, checks: 'default off, reminder selection, zero advice totals, expanded explanation, confirm/back, completed mock run, slide deck, summary, Replay, reopened history, no external requests or page errors' });
      await context.close();
    }
  }
  await writeFile(join(o.out, 'results.json'), JSON.stringify(results, null, 2));
} finally {
  await browser?.close();
  if (server.exitCode === null) { const stopped = once(server, 'exit'); server.kill(); await stopped; }
}

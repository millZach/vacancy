// After npm run build: node tools/advisor-shots.mjs --out shots/advisor [--baseline]. Owns its static preview and Edge; run under the shared heavy-job helper.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: options } = parseArgs({ options: {
  out: { type: 'string', default: 'shots/advisor' },
  baseline: { type: 'boolean', default: false },
} });
const cwd = fileURLToPath(new URL('..', import.meta.url));
const origin = 'http://127.0.0.1:1437';
const server = spawn(process.execPath, ['node_modules/vite/bin/vite.js', 'preview', '--host', '127.0.0.1', '--port', '1437', '--strictPort'], { cwd, windowsHide: true, stdio: 'pipe' });
server.stdout.on('data', (data) => process.stdout.write(data));
server.stderr.on('data', (data) => process.stderr.write(data));
console.log(`Owned advisor preview PID ${server.pid}`);
let browser;
const results = [];

async function visibleInside(locator, container) {
  const a = await locator.boundingBox(), b = await container.boundingBox();
  return a && b && a.y >= b.y - 1 && a.y + a.height <= b.y + b.height + 1;
}

async function off(dialog) {
  const switches = await dialog.getByRole('switch').all();
  assert.equal(switches.length, 2);
  for (const control of switches) {
    assert(await control.isDisabled());
    assert.equal(await control.getAttribute('aria-checked'), 'false');
  }
}

async function baseline(script, args) {
  const child = spawn(process.execPath, [script, ...args], { cwd, windowsHide: true, stdio: 'inherit' });
  const [code] = await once(child, 'exit');
  assert.equal(code, 0, `${script} passed`);
}

try {
  await mkdir(options.out, { recursive: true });
  const until = Date.now() + 15_000;
  while (true) {
    assert.equal(server.exitCode, null, 'Preview must stay alive');
    try {
      const response = await fetch(origin);
      if (response.ok) {
        assert.equal(await response.text(), await readFile(join(cwd, 'dist/index.html'), 'utf8'), 'Preview serves the current built artifact');
        break;
      }
    } catch (error) { if (error instanceof assert.AssertionError) throw error; }
    assert(Date.now() < until, 'Preview starts');
    await new Promise((done) => setTimeout(done, 150));
  }
  browser = await chromium.launch({ channel: 'msedge' });
  for (const [fixture, width, height] of [['windows', 1280, 820], ['macos', 1100, 700]]) {
    const context = await browser.newContext({ viewport: { width, height } });
    const errors = [], external = [];
    await context.route('**/*', async (route) => {
      const url = route.request().url();
      if (url.startsWith(`${origin}/`) || url.startsWith('data:')) await route.continue();
      else { external.push(url); await route.abort(); }
    });
    const page = await context.newPage();
    page.on('pageerror', (error) => errors.push(String(error)));
    page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
    const url = `${origin}/?fixture=${fixture}&fda=granted&scan=0`;
    const shot = async (name) => {
      const path = join(options.out, `${fixture}-${name}.png`);
      await page.screenshot({ path });
      console.log(path);
    };
    await page.goto(url);
    await page.getByRole('button', { name: 'AI options' }).waitFor();
    await page.evaluate(() => document.fonts.ready);
    await page.getByText('Preview status only. No apps were checked on this computer.').waitFor();
    await shot('scan-closed');
    const noteTrigger = page.getByRole('button', { name: 'AI options' });
    await noteTrigger.click();
    const dialog = page.getByRole('dialog');
    await dialog.waitFor();
    await off(dialog);
    assert.equal(await page.locator('input').count(), 0, 'No credential/API fields');
    assert.equal(await dialog.getByText('Preview: detected · Not available to Vacancy').count(), 1);
    assert.equal(await dialog.getByText('Preview: not detected').count(), 1);
    assert.equal(await dialog.getByText('Installed', { exact: false }).count(), 0, 'No installed assertion');
    const bounds = await dialog.boundingBox();
    assert(bounds && Math.abs(bounds.x + bounds.width / 2 - width / 2) < 2 && Math.abs(bounds.y + bounds.height / 2 - height / 2) < 2, 'Approved centered placement');
    const why = dialog.getByRole('button', { name: 'Why does AI stay off?' });
    assert(await visibleInside(why, page.locator('.advisor-dialog-body')), 'Explanation disclosure initially visible');
    await shot('scan-open');
    const close = dialog.getByRole('button', { name: 'Close AI options' });
    const resume = dialog.getByRole('button', { name: 'Continue with built-in rules' });
    assert(await close.evaluate((el) => el === document.activeElement));
    await page.keyboard.press('Shift+Tab');
    assert(await resume.evaluate((el) => el === document.activeElement));
    await page.keyboard.press('Tab');
    assert(await close.evaluate((el) => el === document.activeElement));
    await why.click();
    const explanation = dialog.getByText(/These apps may include other files or settings/);
    await explanation.scrollIntoViewIfNeeded();
    assert(await visibleInside(explanation, page.locator('.advisor-dialog-body')), 'Complete privacy explanation accessible');
    assert(await visibleInside(resume, dialog), 'Dismissal remains fixed and visible');
    await shot('details');
    await page.keyboard.press('Escape');
    assert.equal(await dialog.count(), 0);
    assert(await noteTrigger.evaluate((el) => el === document.activeElement), 'Focus returns to scan trigger');
    assert.equal(await page.locator('.shell[inert]').count(), 0);
    await page.getByRole('button', { name: /^Scan / }).click();
    await page.locator('.atlas.is-map').waitFor();
    const drive = await page.locator('.drive-menu-button').boundingBox(), actions = await page.locator('.head-right').boundingBox();
    assert(drive && actions && drive.x + drive.width < actions.x && actions.x + actions.width <= width, 'Drive/header controls do not overlap');
    await page.getByRole('button', { name: 'Rules only' }).click();
    await dialog.waitFor();
    await shot('map-open');
    await resume.click();
    assert(await page.getByRole('button', { name: 'Rules only' }).evaluate((el) => el === document.activeElement));
    // A plan check may finish after the read-only advisor opens. Only its visible modal may own focus.
    await page.goto(`${url}&delay=1800`);
    await page.getByRole('button', { name: /^Scan / }).click();
    await page.locator('.atlas.is-map').waitFor();
    await page.getByRole('button', { name: 'Build a plan' }).click();
    const free = page.getByRole('complementary', { name: 'Plan' }).getByRole('button', { name: /^Free / });
    await free.click();
    await page.locator('.tray-later').waitFor();
    await page.getByRole('button', { name: 'Rules only' }).click();
    await dialog.waitFor();
    await page.locator('.tray-later').waitFor({ state: 'detached' });
    assert.equal(await page.locator('[role="alertdialog"]').count(), 0, 'Pending confirmation is not mounted behind advisor');
    assert(await close.evaluate((el) => el === document.activeElement), 'Plan result cannot steal advisor focus');
    await shot('plan-ready-advisor');
    await page.keyboard.press('Tab');
    assert(await why.evaluate((el) => el === document.activeElement));
    await page.keyboard.press('Enter');
    assert.equal(await why.getAttribute('aria-expanded'), 'true');
    assert.equal(await page.getByRole('complementary', { name: 'Run' }).count(), 0, 'Advisor keys never start a run');
    await page.keyboard.press('Escape');
    const confirmation = page.getByRole('alertdialog');
    await confirmation.waitFor();
    const back = confirmation.getByRole('button', { name: 'Go back' });
    assert(await back.evaluate((el) => el === document.activeElement), 'Deferred confirmation focuses the safe answer after advisor cleanup');
    await confirmation.evaluate(async (el) => {
      await Promise.all(el.parentElement.getAnimations({ subtree: true }).map((animation) => animation.finished));
    });
    assert.equal(await confirmation.evaluate((el) => getComputedStyle(el).opacity), '1', 'Confirmation entrance is complete before capture');
    assert(await back.evaluate((el) => el === document.activeElement), 'Safe focus remains after the entrance');
    await shot('plan-ready-confirm');
    await page.keyboard.press('Enter');
    assert.equal(await confirmation.count(), 0);
    assert.equal(await page.getByRole('complementary', { name: 'Run' }).count(), 0, 'Safe answer leaves the plan unrun');
    await free.click();
    await confirmation.waitFor();
    assert(await back.evaluate((el) => el === document.activeElement), 'Ordinary confirmation retains safe focus');
    await page.keyboard.press('Escape');
    assert.equal(await confirmation.count(), 0);
    assert.equal(await page.getByRole('complementary', { name: 'Plan' }).count(), 1);
    await page.goto(`${url}&advisor=notChecked`);
    await page.getByRole('button', { name: 'AI options' }).click();
    await dialog.getByText('Not checked in fixture mode').first().waitFor();
    assert.equal(await dialog.getByText('Not checked in fixture mode').count(), 2);
    await off(dialog);
    await shot('not-checked');
    await page.goto(`${url}&advisor=error`);
    await page.getByRole('button', { name: 'AI options' }).click();
    await dialog.getByRole('alert').waitFor();
    await off(dialog);
    await shot('error');
    await dialog.getByRole('button', { name: 'Try again' }).click();
    await dialog.getByRole('alert').waitFor();
    await page.keyboard.press('Escape');
    assert(await page.getByRole('button', { name: /^Scan / }).isEnabled(), 'Status failure does not block rules');
    await page.goto(`${url}&delay=2000`);
    await page.getByRole('button', { name: 'Rules only' }).click();
    await dialog.getByRole('status').waitFor();
    await off(dialog);
    await shot('loading');
    await dialog.getByText('Preview: not detected').waitFor();
    await page.setViewportSize({ width, height: 560 });
    await dialog.getByRole('button', { name: 'Why does AI stay off?' }).click();
    await explanation.scrollIntoViewIfNeeded();
    assert(await visibleInside(resume, dialog), 'Short-window dismissal remains visible');
    await shot('short-expanded');
    assert.deepEqual(errors, [], 'No page/console errors');
    assert.deepEqual(external, [], 'No external requests');
    results.push({ fixture, width, height, checks: 'synthetic rows, disabled switches, privacy disclosure, centered placement, loading/error/unchecked, retry, keyboard trap/Escape/refocus, deferred-plan modal race/keyboard containment/safe focus/no run, ordinary confirm/Escape, fixed short-window dismissal, rule scan/map, header fit, no external requests' });
    await context.close();
  }
  await browser.close();
  browser = null;
  if (options.baseline) {
    for (const [fixture, viewport] of [['windows', '1280x820'], ['macos', '1100x700']]) {
      const url = `${origin}/?fixture=${fixture}&fda=granted&run=1500`;
      await baseline('tools/ripple-strip.mjs', ['--url', url, '--out', options.out, '--name', `${fixture}-baseline`, '--viewport', viewport]);
      await baseline('tools/map-shots.mjs', ['--url', url, '--out', options.out, '--name', `${fixture}-baseline`, '--viewport', viewport]);
    }
  }
  await writeFile(join(options.out, 'interaction-results.json'), `${JSON.stringify(results, null, 2)}\n`);
} finally {
  await browser?.close();
  if (server.exitCode === null) { const stopped = once(server, 'exit'); server.kill(); await stopped; }
}

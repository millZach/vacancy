// THROWAWAY proof. Run only inside the shared heavy-job wrapper, after `npm run prototype:dups:build`.
// Serves the built route on a temporary Vite preview, drives each variant through the whole flow in Edge, saves
// screenshots and fails on page errors.
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../..', import.meta.url));
const output = process.env.DUPS_SHOTS ?? 'D:\\tmp\\vacancy-proto-dups\\shots';
const PORT = 1437;
const BASE = `http://127.0.0.1:${PORT}/`;
await mkdir(output, { recursive: true });
const server = spawn(process.execPath, [fileURLToPath(new URL('../../node_modules/vite/bin/vite.js', import.meta.url)), 'preview', '--config', 'prototypes/duplicates/vite.config.ts', '--port', String(PORT), '--host', '127.0.0.1', '--strictPort'], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let log = '';
server.stdout.on('data', (x) => { log += x; });
server.stderr.on('data', (x) => { log += x; });

const passes = [
  { platform: 'windows', tag: 'win', viewport: { width: 1280, height: 820 }, wedding: 'Wedding highlights.mp4' },
  { platform: 'macos', tag: 'mac', viewport: { width: 1100, height: 700 }, wedding: 'Wedding highlights.mov' },
];
const errors = [];
const results = [];
const shots = [];
let browser;

const fail = (m) => { throw new Error(m); };

try {
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) fail(`Server exited: ${log}`);
    try { if ((await fetch(BASE)).ok) break; } catch { /* starting */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  browser = await chromium.launch({ channel: 'msedge', headless: true });
  for (const pass of passes) {
    const page = await browser.newPage({ viewport: pass.viewport, deviceScaleFactor: 1, reducedMotion: 'reduce' });
    page.on('pageerror', (e) => errors.push(`${pass.tag}: ${e.message}`));
    // Failed loads are reported by URL; the browser's own favicon probe is the only one allowed.
    page.on('response', (r) => r.status() >= 400 && !r.url().endsWith('/favicon.ico') && errors.push(`${pass.tag} ${r.status()}: ${r.url()}`));
    page.on('console', (m) => m.type() === 'error' && !m.text().startsWith('Failed to load resource') && errors.push(`${pass.tag} console: ${m.text()}`));
    const shot = async (name) => {
      const path = `${output}\\${name}.png`;
      await page.screenshot({ path });
      shots.push(path);
    };
    for (const v of ['A', 'B', 'C']) {
      const id = `${v}-${pass.tag}`;
      await page.goto(`${BASE}?review=1&variant=${v}&platform=${pass.platform}&hash=6000&run=3000`);
      await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();

      // 1. Hashing in the background; the plan stays usable.
      await page.getByRole('status', { name: 'Duplicate check progress' }).waitFor();
      const browserRow = page.locator('.tray .tray-row:not(.dup-row) [role=checkbox]').first();
      const before = await browserRow.getAttribute('aria-checked');
      await browserRow.click();
      if ((await browserRow.getAttribute('aria-checked')) === before) fail(`${id}: plan not usable while hashing`);
      await browserRow.click();
      await page.locator('.map-region-head').first().hover();
      await page.locator('.map-tip').waitFor();
      await page.waitForTimeout(500);
      if (!(await page.locator('.state-label').textContent()).startsWith('Hashing')) fail(`${id}: hashing finished too early`);
      await shot(`${id}-1-progress`);

      // 2. Proposal ready.
      await page.locator('.state-label', { hasText: 'Ready' }).waitFor({ timeout: 10_000 });
      await page.mouse.move(5, 300);
      await page.waitForTimeout(700);
      await shot(`${id}-2-ready`);

      // 3. Review groups, untick one (the wedding video), and put duplicates in the plan.
      const wedding = page.getByRole('checkbox', { name: `Remove extra copies of ${pass.wedding}` });
      if (v === 'A') {
        await page.getByRole('button', { name: /^Review \d+ groups$/ }).click();
        await page.getByRole('checkbox', { name: 'Include Duplicate files' }).click();
        await wedding.scrollIntoViewIfNeeded();
        await wedding.click();
        await page.locator('.dupA-bucket[aria-label="Videos"]').evaluate((el) => el.scrollIntoView({ block: 'start' }));
        await page.waitForTimeout(200);
        await shot(`${id}-3-review`);
      } else if (v === 'B') {
        await page.locator('.dupB-pill.is-ready').click();
        await page.getByRole('heading', { name: 'Duplicate files' }).waitFor();
        await wedding.scrollIntoViewIfNeeded();
        await wedding.click();
        await page.locator('.dupB-card', { hasText: pass.wedding }).hover();
        await page.waitForTimeout(300);
        await shot(`${id}-3-review`);
        if (!(await page.locator('.dupB-links path').count())) fail(`${id}: no link lines on the map`);
        await page.locator('.dupB-card', { hasText: 'GX010231' }).click();
        await page.mouse.move(5, 300);
        await page.waitForTimeout(200);
        await shot(`${id}-3b-links`);
        await page.getByRole('button', { name: /^Add \d+ groups to the plan$/ }).click();
      } else {
        await page.getByRole('button', { name: 'Compare copies' }).first().click();
        await page.getByRole('dialog', { name: 'Duplicate files' }).waitFor();
        await page.waitForTimeout(250);
        await shot(`${id}-3-review`);
        await page.getByRole('tab', { name: new RegExp(`: ${pass.wedding.replace('.', '\\.')}$`) }).click();
        await page.getByRole('button', { name: 'Skip group', exact: true }).click();
        await page.getByRole('tab', { name: new RegExp(`: ${pass.wedding.replace('.', '\\.')}, skipped$`) }).click();
        await page.waitForTimeout(150);
        await shot(`${id}-3b-skipped`);
        await page.getByRole('button', { name: /^Add \d+ groups to the plan$/ }).click();
      }
      const state = (await page.locator('.state-label').textContent()) ?? '';
      const m = state.match(/(\d+) of (\d+) groups in plan/);
      if (!m || Number(m[1]) !== Number(m[2]) - 1) fail(`${id}: expected one group left out, got "${state}"`);
      if (v !== 'A') await shot(`${id}-3c-plan`);

      // 4. Confirm: only the extra copies go, one copy of each group stays.
      await page.getByRole('button', { name: /^Free / }).click();
      const dialog = page.getByRole('alertdialog');
      await dialog.waitFor();
      const text = await dialog.innerText();
      if (!/One copy of each of the \d+ groups stays where it is/.test(text)) fail(`${id}: confirm doesn't say a copy stays`);
      if (!text.includes('1 group you unticked is left alone')) fail(`${id}: confirm doesn't mention the unticked group`);
      await shot(`${id}-4-confirm`);

      // 5. Run.
      await dialog.getByRole('button', { name: /permanently$/ }).click();
      await page.getByRole('heading', { name: 'Freeing space' }).waitFor();
      await page.waitForTimeout(1300);
      await shot(`${id}-5-run`);
      await page.getByRole('button', { name: 'See what changed' }).waitFor({ timeout: 15_000 });
      await shot(`${id}-5b-done`);

      // 6. Recap.
      await page.getByRole('button', { name: 'See what changed' }).click();
      const recap = page.getByRole('region', { name: 'Duplicate files' });
      await recap.waitFor();
      const recapText = await recap.innerText();
      if (!/extra copies deleted/.test(recapText) || !recapText.includes(`You left ${pass.wedding} alone`)) fail(`${id}: recap is missing duplicates`);
      await page.waitForTimeout(400);
      await shot(`${id}-6-recap`);

      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth || document.documentElement.scrollHeight > innerHeight);
      if (overflow) fail(`${id}: page overflows the window`);
      results.push({ variant: v, platform: pass.platform, viewport: pass.viewport, state, recap: recapText.split('\n').slice(0, 3).join(' | ') });
    }
    // The built route hides the switcher without ?review=1.
    await page.goto(`${BASE}?variant=A&platform=${pass.platform}`);
    await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
    if (await page.locator('.prototype-switcher').count()) fail('Switcher shown in a build without review=1');
    await page.close();
  }
  if (errors.length) fail(errors.join('\n'));
  await writeFile(`${output}\\proof.json`, JSON.stringify({ date: new Date().toISOString(), surface: 'built output, vite preview, Edge headless', results, shots, pageErrors: errors, status: 'passed' }, null, 2));
  console.log(JSON.stringify({ status: 'passed', output, cases: results.length, shots: shots.length, results }, null, 2));
} finally {
  await browser?.close();
  if (server.exitCode === null) { server.kill(); await new Promise((r) => server.once('exit', r)); }
}

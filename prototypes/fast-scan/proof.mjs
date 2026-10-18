// Run only inside the shared heavy-job wrapper, after `prototype:fast-scan:build`. The preview server is temporary.
// Drives every variant through the whole flow in Edge at 1280×820 (Windows): plate, choosing fast scan, the prompt
// stand-in, the fast scan, the map with Locked folders (hover and opened), then both fallbacks (admin declined; an
// exFAT drive). Then a 1100×700 macOS pass that must show today's scan with no fast scan anywhere. Fails on any page
// error, bridge call, page overflow or missing line.
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../..', import.meta.url));
const output = process.argv[2] ?? 'D:\\tmp\\vacancy-proto-fast-scan\\shots';
const base = 'http://127.0.0.1:1437/';
await mkdir(output, { recursive: true });
const server = spawn(process.execPath, [fileURLToPath(new URL('../../node_modules/vite/bin/vite.js', import.meta.url)), 'preview', '--config', 'prototypes/fast-scan/vite.config.ts'], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let log = '';
server.stdout.on('data', (x) => { log += x; });
server.stderr.on('data', (x) => { log += x; });

let browser;
const errors = [];
const results = [];
const shots = {};
const fail = (m) => { throw new Error(m); };

try {
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) fail(`Server exited: ${log}`);
    try { if ((await fetch(base)).ok) break; } catch { /* starting */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  browser = await chromium.launch({ channel: 'msedge', headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 820 }, deviceScaleFactor: 1, reducedMotion: 'reduce' });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });

  const shot = async (name) => {
    const path = `${output}\\${name}.png`;
    await page.waitForTimeout(450);
    await page.screenshot({ path });
    (shots[name.split('-')[0]] ??= []).push(path);
  };
  const noOverflow = async (where) => {
    const o = await page.evaluate(() => [document.documentElement.scrollWidth > innerWidth, document.documentElement.scrollHeight > innerHeight]);
    if (o[0] || o[1]) fail(`${where}: page overflows the window`);
    // The plate must fit above the switcher strip.
    const clipped = await page.evaluate(() => {
      const c = document.querySelector('.plate-content');
      if (!c) return false;
      const r = c.getBoundingClientRect();
      const stage = document.querySelector('.atlas-stage').getBoundingClientRect();
      return r.top < stage.top - 1 || r.bottom > stage.bottom + 1;
    });
    if (clipped) fail(`${where}: the plate doesn't fit its stage`);
  };
  const has = async (text, where) => {
    const body = await page.locator('body').innerText();
    if (!body.includes(text)) fail(`${where}: missing “${text}”`);
  };
  const lacks = async (re, where) => {
    const body = await page.locator('.shell').innerText();
    if (re.test(body)) fail(`${where}: unexpected ${re}`);
  };
  const hold = (x) => page.evaluate((v) => { window.__proto.hold = v; }, x);
  const bridgeCalls = () => page.evaluate(() => window.__bridgeCalls.length);
  const uac = () => page.getByRole('dialog', { name: 'User Account Control' });
  const plate = () => page.getByRole('heading', { name: 'Which drive should Vacancy scan?' }).waitFor();
  const hud = () => page.getByRole('status', { name: 'Scan progress' });
  const toMap = async () => {
    await hold(null);
    await page.getByRole('button', { name: 'Finish scan' }).click();
    await page.locator('.atlas.is-map').waitFor({ timeout: 5000 });
  };

  /** Each variant's way of asking for a fast scan of the drive on screen. */
  async function askFast(v, drive) {
    if (v === 'A') await page.getByRole('button', { name: 'Fast scan (asks for admin)' }).click();
    else if (v === 'B') {
      await page.getByRole('radio', { name: /Fast scan/ }).click();
      await page.locator('.plate-actions button').last().click();
    } else {
      await page.getByRole('button', { name: new RegExp(`^(Scan again|Scan ${drive.replace(/[()]/g, '\\$&')})$`) }).click();
      await hud().waitFor();
      if (drive.includes('(C:)') && !(await page.locator('.hud-offer').count())) fail('C: no offer in the HUD');
    }
  }

  for (const v of ['A', 'B', 'C']) {
    await page.goto(`${base}?review=1&variant=${v}&platform=windows`);
    await plate();
    await noOverflow(`${v} plate`);
    await shot(`${v}-win-1-plate`);
    if (v === 'C') await lacks(/Fast scan|admin/, 'C plate');
    if (v === 'B') {
      await page.getByRole('radio', { name: /Fast scan/ }).click();
      await has('Windows will ask for admin, for this scan only.', v);
      await noOverflow('B fast plate');
      await shot(`${v}-win-1b-fast-mode`);
      await page.getByRole('radio', { name: /Normal scan/ }).click();
    }

    // Fast scan: ask, the prompt, Yes, the fast scan, the map with Locked folders.
    await askFast(v, 'Local Disk (C:)');
    if (v === 'C') {
      await hold(0.14);
      await has('Fast scan reads its file index instead', v);
      await shot(`${v}-win-2-offer`);
      await page.getByRole('button', { name: 'Switch to fast scan' }).click();
    }
    await uac().waitFor();
    await has('PROTOTYPE STAND-IN', v);
    await has('Do you want to allow this app to make changes to your device?', v);
    await shot(`${v}-win-3-uac`);
    await uac().getByRole('button', { name: 'Yes' }).click();
    await hold(0.62);
    await hud().waitFor();
    if (!/fast scan/i.test(await hud().innerText())) fail(`${v}: the HUD doesn't say fast scan`);
    await shot(`${v}-win-4-fast-scanning`);
    await toMap();
    await page.locator('[data-tile="r:locked"]').waitFor();
    await has('in locked folders, totals only', v);
    await noOverflow(`${v} map`);
    await shot(`${v}-win-5-map`);
    await page.locator('[data-tile="locked:accounts"]').hover();
    const tip = page.getByRole('tooltip');
    await tip.waitFor();
    const ttext = await tip.innerText();
    if (!/total only/.test(ttext) || !/Protected/.test(ttext) || !/Locked folders/.test(ttext)) fail(`${v}: the locked hover card is wrong: ${ttext}`);
    await shot(`${v}-win-6-locked-hover`);
    await page.mouse.move(640, 30);
    await page.locator('[data-tile="r:locked"]').click();
    await page.getByRole('complementary', { name: 'About locked folders' }).waitFor();
    await page.mouse.move(640, 30);
    await has('no names, nothing to propose, nothing to delete', v);
    await shot(`${v}-win-7-locked-open`);
    await page.keyboard.press('Escape');

    // Fallback 1: admin declined; the normal scan runs, saying why.
    await page.getByRole('button', { name: 'Scan again' }).click();
    await plate();
    await askFast(v, 'Local Disk (C:)');
    if (v === 'C') {
      await hold(0.2);
      await page.getByRole('button', { name: 'Switch to fast scan' }).click();
    }
    await uac().waitFor();
    await uac().getByRole('button', { name: 'No' }).click();
    await hold(0.3);
    await hud().waitFor();
    if (/fast scan/i.test(await hud().innerText())) fail(`${v}: declined, but the HUD says fast scan`);
    await has('Windows wasn’t given admin', v);
    await shot(`${v}-win-8-declined`);
    if (v === 'A') {
      await toMap();
      await has('Normal scan: admin wasn’t given', v);
      if (await page.locator('[data-tile="r:locked"]').count()) fail('A: a normal scan shows Locked folders');
      await shot(`${v}-win-8b-normal-map`);
      await page.getByRole('button', { name: 'Scan again' }).click();
    } else {
      await hud().getByRole('button', { name: 'Cancel' }).click();
    }

    // Fallback 2: an exFAT drive. No prompt; the normal scan runs, saying why.
    await plate();
    await page.getByRole('radio', { name: 'SanDisk (E:)' }).click();
    if (v === 'B') {
      await page.getByRole('radio', { name: /Fast scan/ }).click();
      await has('SanDisk (E:) gets the normal scan.', v);
      await noOverflow('B exFAT plate');
      await shot(`${v}-win-9a-exfat-plate`);
    }
    await askFast(v, 'SanDisk (E:)');
    await hud().waitFor();
    if (await uac().count()) fail(`${v}: the prompt showed for an exFAT drive`);
    await hold(0.4);
    await has('exFAT, and fast scan only reads NTFS drives', v);
    await shot(`${v}-win-9-not-ntfs`);
    if (await bridgeCalls()) fail(`${v}: the bridge was called`);
    results.push({ variant: v, platform: 'windows', state: await page.locator('.state-label').innerText() });
    await hold(null);
  }

  // Keyboard arrows switch variants and keep the URL.
  await page.goto(`${base}?review=1&variant=A&platform=windows`);
  await plate();
  await page.mouse.click(640, 120);
  await page.keyboard.press('ArrowLeft');
  if (!page.url().includes('variant=C')) fail('ArrowLeft did not wrap to C');
  await page.keyboard.press('ArrowRight');
  if (!page.url().includes('variant=A')) fail('ArrowRight did not return to A');

  // macOS, 1100×700: today's scan, no fast scan anywhere.
  await page.setViewportSize({ width: 1100, height: 700 });
  for (const v of ['A', 'B', 'C']) {
    await page.goto(`${base}?review=1&variant=${v}&platform=macos`);
    await plate();
    await lacks(/Fast|admin|Locked folders/, `${v} mac plate`);
    await noOverflow(`${v} mac plate`);
    await shot(`${v}-mac-1-plate`);
    await page.getByRole('button', { name: 'Scan Macintosh HD' }).click();
    await hold(0.4);
    await hud().waitFor();
    await lacks(/Fast|admin|Switch/, `${v} mac scanning`);
    await shot(`${v}-mac-2-scanning`);
    await toMap();
    await lacks(/Fast|admin|Locked folders/, `${v} mac map`);
    await shot(`${v}-mac-3-map`);
    if (await bridgeCalls()) fail(`${v} mac: the bridge was called`);
    results.push({ variant: v, platform: 'macos', fastScan: 'none' });
  }

  // A build without review=1 hides the switcher.
  await page.goto(`${base}?variant=A`);
  await plate();
  if (await page.locator('.prototype-switcher').count()) fail('Built switcher appeared without review=1');

  if (errors.length) fail(errors.join('\n'));
  await writeFile(`${output}\\proof.json`, JSON.stringify({ date: new Date().toISOString(), surface: 'built output, review=1', results, shots, pageErrors: errors, status: 'passed' }, null, 2));
  console.log(JSON.stringify({ status: 'passed', output, results, pageErrors: errors }, null, 2));
} finally {
  await browser?.close();
  if (server.exitCode === null) {
    server.kill();
    await new Promise((r) => server.once('exit', r));
  }
}

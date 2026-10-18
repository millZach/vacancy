// Run only inside the shared heavy-job wrapper, after `prototype:reorg:build`. The preview server is temporary.
// Drives every variant through the whole flow in Edge at 1280×820 (Windows), then a 1100×700 macOS pass, and fails
// on any page error, bridge call, horizontal overflow or missing honesty line.
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../..', import.meta.url));
const output = process.argv[2] ?? 'D:\\tmp\\vacancy-proto-reorg\\shots';
const base = 'http://127.0.0.1:1435/';
await mkdir(output, { recursive: true });
const server = spawn(process.execPath, [fileURLToPath(new URL('../../node_modules/vite/bin/vite.js', import.meta.url)), 'preview', '--config', 'prototypes/reorg/vite.config.ts'], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
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
    await page.waitForTimeout(250);
    await page.screenshot({ path });
    (shots[name.split('-')[0]] ??= []).push(path);
  };
  const noOverflow = async (where) => {
    const o = await page.evaluate(() => [document.documentElement.scrollWidth > innerWidth, document.documentElement.scrollHeight > innerHeight]);
    if (o[0] || o[1]) fail(`${where}: page overflows the window`);
  };
  const has = async (text, where) => {
    const body = await page.locator('body').innerText();
    if (!body.includes(text)) fail(`${where}: missing “${text}”`);
  };
  const lacks = async (re, where) => {
    const body = await page.locator('.shell').innerText();
    if (re.test(body)) fail(`${where}: unexpected ${re}`);
  };
  const bridgeCalls = () => page.evaluate(() => window.__bridgeCalls.length);

  for (const v of ['A', 'B', 'C']) {
    await page.goto(`${base}?review=1&variant=${v}&platform=windows`);
    await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
    // Reorganizations start unchecked, like every review item.
    const t0 = await page.locator('.tray-foot').innerText();
    if (/moved|sorted/i.test(t0)) fail(`${v}: a reorganization started in the plan`);
    if (await page.getByRole('checkbox', { checked: true, name: /Sort / }).count()) fail(`${v}: a sort is ticked at the start`);
    await page.locator('section[aria-label="Tidy up"]').evaluate((el) => el.scrollIntoView({ block: 'start' }));
    await shot(`${v}-win-1-plan`);

    if (v === 'A') {
      await page.getByRole('checkbox', { name: 'Include Sort Downloads' }).click();
      await page.getByRole('checkbox', { name: 'Include Sort Desktop' }).click();
      await page.getByRole('button', { name: 'Sort Downloads', exact: true }).click();
      await page.getByRole('dialog', { name: 'Sort Downloads' }).waitFor();
      await has('Built-in rule: sorted by kind and year', v);
      await has('Docker Desktop Installer (1).exe', v);
      await has('Undo any time', v);
      await shot(`${v}-win-2-drawer`);
      await page.getByRole('dialog', { name: 'Sort Downloads' }).getByRole('button', { name: 'Close' }).click();
    } else if (v === 'B') {
      await page.getByRole('button', { name: /See them on the map/ }).click();
      await page.getByRole('button', { name: 'Add Sort Downloads' }).click();
      await page.getByRole('button', { name: 'Add Sort Desktop' }).click();
      const knob = page.getByRole('slider', { name: 'As it is to sorted' });
      await knob.focus();
      await page.keyboard.press('Home');
      await page.waitForTimeout(700);
      await shot(`${v}-win-2a-tidy-as-it-is`);
      for (let i = 0; i < 5; i++) await page.keyboard.press('ArrowRight');
      await page.waitForTimeout(700);
      await shot(`${v}-win-2-tidy-map`);
      await page.getByRole('button', { name: 'Sort Downloads', exact: true }).click();
      await page.waitForTimeout(700);
      await has('Docker Desktop Installer (1).exe', v);
      await has('Undo any time', v);
      await shot(`${v}-win-3-drawer`);
    } else {
      await page.getByRole('button', { name: 'Review sorting' }).click();
      const sheet = page.getByRole('dialog', { name: 'Review sorting' });
      await sheet.waitFor();
      await sheet.getByRole('checkbox', { name: 'Include Sort Downloads' }).click();
      await sheet.getByRole('checkbox', { name: 'Include Downloads\\Audio\\2025' }).click();
      await has('Docker Desktop Installer (1).exe', v);
      await has('Sort later', v);
      await has('181 files into 9 folders', v);
      await sheet.locator('.rs-body').evaluate((el) => { el.scrollTop = 0; });
      await shot(`${v}-win-2-drawer`);
      await sheet.getByRole('tab', { name: /Desktop/ }).click();
      await sheet.getByRole('checkbox', { name: 'Include Sort Desktop' }).click();
      await shot(`${v}-win-3-drawer-desktop`);
      await sheet.getByRole('button', { name: 'Done' }).click();
    }

    const foot = await page.locator('.tray-foot').innerText();
    if (!/frees 0\sB/.test(foot)) fail(`${v}: the plan's moves don't say they free nothing`);
    if (!/will be freed/.test(foot)) fail(`${v}: no freed total`);
    await shot(`${v}-win-4-plan-picked`);
    await noOverflow(`${v} plan`);

    await page.locator('.tray-cta').click();
    const dialog = page.getByRole('alertdialog');
    await dialog.waitFor();
    const dtext = await dialog.innerText();
    if (!/files will be moved/.test(dtext) || !/undo/i.test(dtext)) fail(`${v}: the confirm dialog lacks the move count or Undo`);
    await shot(`${v}-win-5-confirm`);
    await dialog.locator('.cta').click();
    await page.getByRole('button', { name: 'See what changed' }).waitFor({ timeout: 8000 });
    await has('Sort Downloads', v);
    await shot(`${v}-win-6-run-done`);
    await page.getByRole('button', { name: 'See what changed' }).click();
    await page.getByRole('group', { name: '5 of 7: Moved folders' }).waitFor();
    await has('found a home', v);
    await has('no space was freed', v);
    await shot(`${v}-win-7-moved-slide`);
    await page.getByRole('button', { name: 'Skip to recap' }).click();
    await page.getByRole('complementary', { name: 'What Vacancy did' }).waitFor();
    await has('files moved', v);
    await shot(`${v}-win-8-recap`);

    // Undo Downloads from the recap (B: from its chip on the map).
    const scope = v === 'B' ? page.locator('.tidy-stage') : page.getByRole('complementary', { name: 'What Vacancy did' });
    await scope.getByRole('button', { name: 'Undo Downloads' }).click();
    const back = v === 'C' ? '177 of 179' : '182 of 184';
    await page.getByText(`${back} files are back in Downloads.`).first().waitFor({ timeout: 5000 });
    await has('2 files couldn’t go back: a file now has the original name.', v);
    await has(`Removed the ${v === 'C' ? 8 : 10} empty folders Vacancy made.`, v);
    await shot(`${v}-win-9-undo-result`);
    await noOverflow(`${v} recap`);

    await page.getByRole('button', { name: 'Past runs' }).click();
    const pop = page.getByRole('menu', { name: 'Past runs' });
    await pop.waitFor();
    const ptext = await pop.innerText();
    if (!/Moved back · 2 stayed/.test(ptext)) fail(`${v}: history doesn't show the Downloads Undo`);
    await shot(`${v}-win-10-history`);
    await pop.getByRole('button', { name: 'Undo Desktop' }).click();
    await pop.getByText('Moved back', { exact: true }).waitFor({ timeout: 5000 });
    await shot(`${v}-win-11-history-undone`);
    await page.keyboard.press('Escape');
    await page.mouse.click(5, 300);
    await has('All 49 files are back in Desktop.', v);
    if (await bridgeCalls()) fail(`${v}: the bridge was called`);
    const state = await page.locator('.state-label').innerText();
    results.push({ variant: v, platform: 'windows', state });
  }

  // Keyboard arrows switch variants and keep the URL.
  await page.goto(`${base}?review=1&variant=A&platform=windows`);
  await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
  await page.keyboard.press('ArrowLeft');
  if (!page.url().includes('variant=C')) fail('ArrowLeft did not wrap to C');
  await page.keyboard.press('ArrowRight');
  if (!page.url().includes('variant=A')) fail('ArrowRight did not return to A');

  // macOS, 1100×700: no reorganization anywhere, and cleanup exactly as today.
  await page.setViewportSize({ width: 1100, height: 700 });
  for (const v of ['A', 'B', 'C']) {
    await page.goto(`${base}?review=1&variant=${v}&platform=macos`);
    await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
    await lacks(/Tidy|Sort Downloads|Reorganiz|Undo/, `${v} mac plan`);
    await shot(`${v}-mac-1-plan`);
    await noOverflow(`${v} mac plan`);
  }
  await page.locator('.tray-cta').click();
  const mdialog = page.getByRole('alertdialog');
  await mdialog.waitFor();
  if (/moved/i.test(await mdialog.innerText())) fail('mac: confirm mentions moves');
  await shot('C-mac-2-confirm');
  await mdialog.locator('.cta').click();
  await page.getByRole('button', { name: 'See what changed' }).waitFor({ timeout: 8000 });
  await shot('C-mac-3-run-done');
  await page.getByRole('button', { name: 'See what changed' }).click();
  await page.getByRole('complementary', { name: 'What Vacancy did' }).waitFor();
  await lacks(/Reorganized|Undo/, 'mac recap');
  await shot('C-mac-4-recap');
  await page.getByRole('button', { name: 'Past runs' }).click();
  await page.getByRole('menu', { name: 'Past runs' }).waitFor();
  await lacks(/Undo/, 'mac history');
  await shot('C-mac-5-history');
  if (await bridgeCalls()) fail('mac: the bridge was called');
  results.push({ variant: 'all', platform: 'macos', reorganization: 'none' });

  // A build without review=1 hides the switcher.
  await page.goto(`${base}?variant=A`);
  await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
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

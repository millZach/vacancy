// Run only inside the shared heavy-job wrapper. The Vite server is temporary.
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
const root = fileURLToPath(new URL('../..', import.meta.url));
const output = fileURLToPath(new URL('../../shots/tools-advice', import.meta.url));
await mkdir(output, { recursive: true });
const server = spawn(process.execPath, [fileURLToPath(new URL('../../node_modules/vite/bin/vite.js', import.meta.url)), 'preview', '--config', 'prototypes/tools-advice/vite.config.ts', '--port', '1434', '--host', '127.0.0.1'], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let log = '';
server.stdout.on('data', x => { log += x; });
server.stderr.on('data', x => { log += x; });
let browser;
const errors = [];
const results = [];
try {
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) throw new Error(`Server exited: ${log}`);
    try { if ((await fetch('http://127.0.0.1:1434/')).ok) break; } catch { /* startup */ }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  browser = await chromium.launch({ channel: 'msedge', headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 950 }, deviceScaleFactor: 1, reducedMotion: 'reduce' });
  page.on('pageerror', e => errors.push(e.message));
  for (const variant of ['A', 'B', 'C']) {
    for (const platform of ['windows', 'macos']) {
      await page.goto(`http://127.0.0.1:1434/?review=1&variant=${variant}&platform=${platform}`);
      await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
      await page.waitForTimeout(650);
      if (/\bWER\b/.test(await page.locator('.atlas').innerText())) throw new Error('Unsupported WER fixture label returned');
      await page.screenshot({ path: `${output}/${variant}-${platform}-plan.png` });
      const title = platform === 'windows' ? 'Review Windows cleanup' : 'Review Xcode components';
      if (variant !== 'B') await page.getByRole('button', { name: title, exact: true }).click();
      await page.getByRole('checkbox', { name: `Remind me: ${title}` }).click();
      if (!(await page.getByRole('checkbox', { name: `Remind me: ${title}` }).getAttribute('aria-checked') === 'true')) throw new Error('Checkbox was not selected');
      await page.screenshot({ path: `${output}/${variant}-${platform}-instructions.png` });
      await page.getByRole('button', { name: 'Review reminders', exact: true }).click();
      await page.getByRole('dialog').waitFor();
      const reviewText = await page.getByRole('dialog').innerText();
      if (!reviewText.includes('0 B freed · 0 deletions · 0 moves')) throw new Error('Review savings changed');
      await page.screenshot({ path: `${output}/${variant}-${platform}-review.png` });
      await page.getByRole('button', { name: 'Simulate recap', exact: true }).click();
      await page.getByRole('heading', { name: 'Recap', exact: true }).waitFor();
      if (!(await page.getByRole('checkbox', { name: `Remind me: ${title}` }).getAttribute('aria-checked') === 'true')) throw new Error('Reminder lost in recap');
      await page.screenshot({ path: `${output}/${variant}-${platform}-recap.png` });
      await page.getByRole('button', { name: 'Back to plan', exact: true }).click();
      await page.getByRole('button', { name: 'Next variant', exact: true }).click();
      if (!page.url().includes(`variant=${variant === 'A' ? 'B' : variant === 'B' ? 'C' : 'A'}`)) throw new Error('Variant URL was not updated');
      if (!(await page.locator('.state-label').innerText()).includes('1 reminder · 0 B')) throw new Error('Reminder lost across variant switch');
      results.push({ variant, platform, selected: title, recapBytes: 0, recapFiles: 0, remindersPreserved: true });
    }
  }
  // Every guide expands; keyboard arrows preserve URL and selected reminder state.
  await page.goto('http://127.0.0.1:1434/?review=1&variant=A&platform=macos');
  for (const title of ['Review Xcode components', 'Preview Homebrew cleanup', 'Review Docker images', 'Review Docker build cache']) {
    const button = page.getByRole('button', { name: title, exact: true });
    await button.click();
    await button.locator('..').getByRole('link').waitFor();
  }
  await page.keyboard.press('ArrowLeft');
  if (!page.url().includes('variant=C')) throw new Error('ArrowLeft did not wrap to C');
  await page.getByRole('button', { name: /Left for you.*guides/ }).click();
  if (await page.locator('.drawer-content').count()) throw new Error('Drawer did not collapse');
  await page.screenshot({ path: `${output}/C-macos-collapsed.png` });
  await page.setViewportSize({ width: 1024, height: 768 });
  for (const variant of ['A', 'B', 'C']) {
    await page.goto(`http://127.0.0.1:1434/?review=1&variant=${variant}&platform=windows`);
    await page.waitForTimeout(450);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
    if (overflow) throw new Error(`${variant} has horizontal overflow at 1024px`);
    await page.screenshot({ path: `${output}/${variant}-windows-1024.png` });
  }
  await page.goto('http://127.0.0.1:1434/?variant=A');
  await page.getByRole('heading', { name: 'Plan', exact: true }).waitFor();
  if (await page.locator('.prototype-switcher').count()) throw new Error('Built switcher appeared without explicit review=1');
  if (errors.length) throw new Error(errors.join('\n'));
  await writeFile(`${output}/proof.json`, JSON.stringify({ date: new Date().toISOString(), surface: 'built output; explicit review=1 comparison toolbar', serverPid: server.pid, results, pageErrors: errors, status: 'passed' }, null, 2));
  console.log(JSON.stringify({ status: 'passed', output, serverPid: server.pid, temporaryServerStoppedOnExit: true, cases: results.length, pageErrors: errors }, null, 2));
} finally {
  await browser?.close();
  if (server.exitCode === null) { server.kill(); await new Promise(resolve => server.once('exit', resolve)); }
}

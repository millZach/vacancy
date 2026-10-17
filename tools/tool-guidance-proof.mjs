// Mock-only Edge proof: npm run build, then node tools/tool-guidance-proof.mjs --out <directory>. Serves built dist.
// Run through Invoke-HeavyJob.ps1. Never attaches to an installed app or executes a real plan.
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve, join } from 'node:path';
import { parseArgs } from 'node:util';
import { spawn } from 'node:child_process';
import { preview } from 'vite';
import { chromium } from 'playwright';

const { values } = parseArgs({ options: { out: { type: 'string', default: 'shots/tool-guidance' }, 'map-ripple': { type: 'boolean', default: false } } });
const out = resolve(values.out);
await mkdir(out, { recursive: true });
const server = await preview({ preview: { host: '127.0.0.1', port: 1438, strictPort: true } });
const errors = [];
const observations = { pid: process.pid, mockOnly: true, startedAt: new Date().toISOString(), journeys: [] };
let browser;
try {
  const index = await readFile('dist/index.html');
  const assetPaths = ['index.html', ...Array.from(index.toString().matchAll(/(?:src|href)="(\/assets\/[^"?#]+)"/g), match => match[1].slice(1))];
  observations.builtAssets = [];
  for (const path of assetPaths) {
    const disk = await readFile(join('dist', path));
    const response = await fetch(`http://127.0.0.1:1438/${path}`);
    const served = Buffer.from(await response.arrayBuffer());
    if (!response.ok || !disk.equals(served)) throw new Error(`Preview differs from built asset: ${path}`);
    observations.builtAssets.push({ path, bytes: disk.length, sha256: createHash('sha256').update(disk).digest('hex'), modifiedAt: (await stat(join('dist', path))).mtime.toISOString() });
  }
  await writeFile(join(out, 'asset-attestation.json'), JSON.stringify(observations.builtAssets, null, 2));
  browser = await chromium.launch({ channel: 'msedge' });
  for (const platform of ['windows', 'macos']) {
    for (const mixed of [false, true]) {
      const name = `${platform}-${mixed ? 'mixed' : 'reminders'}`;
      const journeyStarted = performance.now();
      const page = await browser.newPage({ viewport: { width: 1280, height: 820 }, reducedMotion: 'reduce' });
      page.on('pageerror', error => errors.push(`${name}: ${error}`));
      page.on('console', message => { if (message.type() === 'error') errors.push(`${name}: ${message.text()}`); });
      await page.goto(`http://127.0.0.1:1438/?fixture=${platform}&scan=60&run=60&history=0&fda=granted`);
      await page.getByRole('button', { name: /^Scan / }).click();
      await page.getByRole('button', { name: 'Build a plan' }).click();
      const tray = page.getByRole('complementary', { name: 'Plan' });
      const boxes = tray.getByRole('region', { name: 'Delete' }).getByRole('checkbox');
      if (!mixed) for (const box of await boxes.all()) {
        if (await box.getAttribute('aria-checked') === 'true') await box.click();
      }
      const names = platform === 'windows'
        ? ['Review Windows cleanup', 'Review Delivery Optimization', 'Review Docker images', 'Review Docker build cache']
        : ['Review Docker images', 'Review Docker build cache', 'Review Xcode simulators and runtimes', 'Preview Homebrew cleanup'];
      for (let i = 0; i < names.length; i++) {
        const title = names[i];
        const checkbox = tray.getByRole('checkbox', { name: `Remind me: ${title}` });
        if (await checkbox.getAttribute('aria-checked') !== 'false') throw new Error(`Guide started checked: ${title}`);
        await tray.getByRole('button', { name: title, exact: true }).click();
        const row = checkbox.locator('..');
        if (await row.locator('ol > li').count() !== 3) throw new Error(`Missing ordered steps: ${title}`);
        await row.getByRole('button', { name: /↗/ }).click();
        await row.getByRole('alert').filter({ hasText: 'No app was launched.' }).waitFor();
        await row.locator('.hint-action').screenshot({ path: join(out, `${name}-reference-${i + 1}.png`) });
        await checkbox.click();
        await row.screenshot({ path: join(out, `${name}-guide-${i + 1}.png`) });
        await tray.getByRole('button', { name: title, exact: true }).click();
      }
      await tray.screenshot({ path: join(out, `${name}-plan.png`) });
      await tray.getByRole('button', { name: mixed ? /^Free / : 'Review reminders' }).click();
      const confirm = page.getByRole('alertdialog');
      await confirm.waitFor();
      await confirm.screenshot({ path: join(out, `${name}-confirm.png`) });
      await confirm.getByRole('button', { name: mixed ? /permanently$/ : 'Save reminders' }).click();
      await page.getByRole('button', { name: 'See what changed' }).click();
      await page.getByRole('button', { name: 'Pause the slides' }).click();
      await page.getByRole('button', { name: /Slide .*Skipped and left for you/ }).click();
      const kept = page.locator('.slide--kept');
      await kept.waitFor();
      await page.setViewportSize({ width: 1100, height: 620 });
      const keyboardReferenceReachMs = [];
      for (const disclosure of await kept.locator('summary').all()) {
        const interactionStarted = performance.now();
        await disclosure.focus();
        await page.keyboard.press('Space');
        if (!await disclosure.locator('..').getAttribute('open').then(value => value !== null)) throw new Error('Space did not open guide');
        await page.keyboard.press('Enter');
        if (await disclosure.locator('..').getAttribute('open') !== null) throw new Error('Enter did not close guide');
        await page.keyboard.press('Space');
        await page.keyboard.press('Tab');
        const focused = await page.evaluate(() => ({ label: document.activeElement?.textContent, rect: document.activeElement?.getBoundingClientRect().toJSON() }));
        if (!focused.label?.includes('↗') || focused.rect.top < 0 || focused.rect.bottom > 620) throw new Error(`Keyboard reference not reachable: ${JSON.stringify(focused)}`);
        keyboardReferenceReachMs.push(performance.now() - interactionStarted);
      }
      if (await kept.locator('ol > li').count() !== 12) throw new Error('Highlights lost instructions');
      const scroll = await kept.evaluate(el => ({ height: el.clientHeight, content: el.scrollHeight, overflow: getComputedStyle(el).overflowY }));
      if (scroll.overflow !== 'auto' || scroll.content <= scroll.height) throw new Error('Short highlights have no reading scroll boundary');
      await page.screenshot({ path: join(out, `${name}-short-highlights.png`) });
      await page.setViewportSize({ width: 1280, height: 820 });
      await page.getByRole('button', { name: 'Skip to recap' }).click();
      const summary = page.getByRole('complementary', { name: 'What Vacancy did' });
      await summary.waitFor();
      await page.locator('.wrap').waitFor({ state: 'detached' });
      for (const toggle of await summary.getByText('Instructions and reference', { exact: true }).all()) await toggle.click();
      if (await summary.locator('ol > li').count() !== 12) throw new Error('Recap lost guide steps');
      await page.screenshot({ path: join(out, `${name}-recap-instructions.png`) });
      await summary.getByRole('heading', { level: 1 }).scrollIntoViewIfNeeded();
      await page.screenshot({ path: join(out, `${name}-recap.png`) });
      const summaryText = await summary.innerText();
      if (!mixed && !summaryText.includes('No files were deleted or moved.')) throw new Error('Wrong reminder totals');
      // Leave the report first: otherwise an immediate assertion can accidentally read the old recap
      // while the asynchronous history menu is still opening this same saved report.
      await summary.getByRole('button', { name: 'Scan again' }).click();
      await page.getByRole('button', { name: /^Scan / }).waitFor();
      await summary.waitFor({ state: 'detached' });
      await page.getByRole('button', { name: 'Past runs' }).click();
      await page.getByRole('menuitem').click();
      await page.getByRole('menu', { name: 'Past runs' }).waitFor({ state: 'hidden' });
      await summary.waitFor();
      if (await summary.getByText('Instructions and reference', { exact: true }).count() !== 4) throw new Error('History lost guides');
      await page.screenshot({ path: join(out, `${name}-history.png`) });
      for (const toggle of await summary.getByText('Instructions and reference', { exact: true }).all()) await toggle.click();
      if (await summary.locator('ol > li').count() !== 12) throw new Error('Reopened history lost instructions');
      await page.screenshot({ path: join(out, `${name}-history-instructions.png`) });
      observations.journeys.push({ platform, mixed, guides: names, summaryText, orderedSteps: 12, fixtureReferencesRefused: 4, shortHighlights: scroll, keyboardReferencesReached: 4, keyboardReferenceReachMs, historyReopenedFromScan: true, historyOrderedSteps: 12, elapsedMs: performance.now() - journeyStarted });
      await page.close();
    }
  }
  await browser.close();
  browser = null;
  if (values['map-ripple']) {
    for (const platform of ['windows', 'macos']) for (const tool of ['ripple-strip', 'map-shots']) {
      await new Promise((done, reject) => {
        const child = spawn(process.execPath, [`tools/${tool}.mjs`, '--url', `http://127.0.0.1:1438/?fixture=${platform}&fda=granted&run=1500&scan=2500`, '--out', out, '--name', `${platform}-${tool}`], { stdio: 'inherit' });
        child.once('error', reject);
        child.once('exit', code => code === 0 ? done() : reject(new Error(`${platform} ${tool} exited ${code}`)));
      });
    }
  }
  if (errors.length) throw new Error(errors.join('\n'));
  observations.finishedAt = new Date().toISOString();
  observations.errors = errors;
  await writeFile(join(out, 'observations.json'), JSON.stringify(observations, null, 2));
} finally {
  await browser?.close();
  await new Promise((done, reject) => server.httpServer.close(error => error ? reject(error) : done()));
}

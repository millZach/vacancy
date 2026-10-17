// Mock-only Edge proof: node tools/tool-guidance-proof.mjs --out <directory>. Starts/stops its own Vite server.
// Run through Invoke-HeavyJob.ps1. Never attaches to an installed app or executes a real plan.
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { parseArgs } from 'node:util';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const { values } = parseArgs({ options: { out: { type: 'string', default: 'shots/tool-guidance' } } });
const out = resolve(values.out);
await mkdir(out, { recursive: true });
const server = await createServer({ server: { host: '127.0.0.1', port: 1438, strictPort: true } });
const errors = [];
const observations = { pid: process.pid, mockOnly: true, startedAt: new Date().toISOString(), journeys: [] };
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ channel: 'msedge' });
  for (const platform of ['windows', 'macos']) {
    for (const mixed of [false, true]) {
      const name = `${platform}-${mixed ? 'mixed' : 'reminders'}`;
      const page = await browser.newPage({ viewport: { width: 1280, height: 820 }, reducedMotion: 'reduce' });
      page.on('pageerror', error => errors.push(`${name}: ${error}`));
      page.on('console', message => { if (message.type() === 'error') errors.push(`${name}: ${message.text()}`); });
      await page.goto(`http://127.0.0.1:1438/?fixture=${platform}&scan=60&run=60&history=0&fda=granted`);
      await page.getByRole('button', { name: /^Scan / }).click();
      await page.getByRole('button', { name: 'Build a plan' }).click();
      const tray = page.getByRole('complementary', { name: 'Plan' });
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
        await checkbox.click();
        await row.screenshot({ path: join(out, `${name}-guide-${i + 1}.png`) });
        await tray.getByRole('button', { name: title, exact: true }).click();
      }
      const boxes = tray.getByRole('region', { name: 'Delete' }).getByRole('checkbox');
      if (!mixed) for (const box of await boxes.all()) {
        if (await box.getAttribute('aria-checked') === 'true') await box.click();
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
      for (const disclosure of await kept.locator('summary').all()) {
        await disclosure.focus();
        await page.keyboard.press('Space');
        if (!await disclosure.locator('..').getAttribute('open').then(value => value !== null)) throw new Error('Space did not open guide');
        await page.keyboard.press('Enter');
        if (await disclosure.locator('..').getAttribute('open') !== null) throw new Error('Enter did not close guide');
        await page.keyboard.press('Space');
        await page.keyboard.press('Tab');
        const focused = await page.evaluate(() => ({ label: document.activeElement?.textContent, rect: document.activeElement?.getBoundingClientRect().toJSON() }));
        if (!focused.label?.includes('↗') || focused.rect.top < 0 || focused.rect.bottom > 620) throw new Error(`Keyboard reference not reachable: ${JSON.stringify(focused)}`);
      }
      if (await kept.locator('ol > li').count() !== 12) throw new Error('Highlights lost instructions');
      const scroll = await kept.evaluate(el => ({ height: el.clientHeight, content: el.scrollHeight, overflow: getComputedStyle(el).overflowY }));
      if (scroll.overflow !== 'auto' || scroll.content <= scroll.height) throw new Error('Short highlights have no reading scroll boundary');
      await page.screenshot({ path: join(out, `${name}-short-highlights.png`) });
      await page.setViewportSize({ width: 1280, height: 820 });
      await page.getByRole('button', { name: 'Skip to recap' }).click();
      for (const toggle of await page.getByText('Instructions and reference', { exact: true }).all()) await toggle.click();
      const summary = page.getByRole('complementary', { name: 'What Vacancy did' });
      await summary.waitFor();
      if (await summary.locator('ol > li').count() !== 12) throw new Error('Recap lost guide steps');
      await page.screenshot({ path: join(out, `${name}-recap.png`) });
      const summaryText = await summary.innerText();
      if (!mixed && !summaryText.includes('No files were deleted or moved.')) throw new Error('Wrong reminder totals');
      await page.getByRole('button', { name: 'Past runs' }).click();
      await page.getByRole('menuitem').click();
      if (await page.getByText('Instructions and reference', { exact: true }).count() !== 4) throw new Error('History lost guides');
      await page.screenshot({ path: join(out, `${name}-history.png`) });
      observations.journeys.push({ platform, mixed, guides: names, summaryText, orderedSteps: 12, fixtureReferencesRefused: 4, shortHighlights: scroll, keyboardReferencesReached: 4 });
      await page.close();
    }
  }
  if (errors.length) throw new Error(errors.join('\n'));
  observations.finishedAt = new Date().toISOString();
  observations.errors = errors;
  await writeFile(join(out, 'observations.json'), JSON.stringify(observations, null, 2));
} finally {
  await browser?.close();
  await server.close();
}

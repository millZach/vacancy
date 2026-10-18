// Screenshots the duplicate finder in Edge (ticket 09, variant B "Map links"): the progress pill while the search
// runs after the scan, the ready pill with the plan tray, the duplicates panel with a group's links on the map, a
// hovered group, an unticked group, the plan with the duplicates in it, the confirm dialog, the run mid-way and done,
// and the summary with its duplicates section.
//
//   npm run dev                         (the UI on the mock bridge, http://localhost:1420)
//   npm run dups-shots -- [options]
//
// Options (all optional):
//   --url <url>        page to open (default http://localhost:1420/?fixture=windows&dups=6000&run=6000)
//   --out <dir>        where PNGs go (default shots)
//   --name <prefix>    file name prefix (default dups)
//   --viewport <WxH>   browser viewport (default 1280x820, the app window's default size)
//   --untick <name>    the group to untick, by its kept copy's name (default: the second group listed)
//   --only             take every other proposal out of the plan first, so the run deletes duplicates only
//   --timeout <ms>     how long the scan, the search or the run may take (default 600000)
//   --cdp <url>        drive an already running page over CDP (the installed app started with
//                      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 and --cdp http://127.0.0.1:9333)
//   --run              over CDP, also free the plan. The app deletes for real, so only ever pass this to an app in
//                      fixture mode (VACANCY_FIXTURE_ROOT). Against the mock bridge the plan is always freed.
//
// Writes <name>-1-progress.png, -2-ready.png, -3-links.png, -4-hover.png, -5-untick.png, -6-plan.png,
// -7-confirm.png, -8-run.png, -9-run-done.png, -10-summary.png and -11-summary-dups.png, and prints what the confirm
// dialog and the summary said. Fails if the page logs an error, or if a panel shows an image (no thumbnails).
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?fixture=windows&dups=6000&run=6000' },
    out: { type: 'string', default: 'shots' },
    name: { type: 'string', default: 'dups' },
    viewport: { type: 'string', default: '1280x820' },
    untick: { type: 'string' },
    only: { type: 'boolean', default: false },
    timeout: { type: 'string', default: '600000' },
    cdp: { type: 'string' },
    run: { type: 'boolean', default: false },
  },
});
const [vw, vh] = o.viewport.split('x').map(Number);
const timeout = Number(o.timeout);
await mkdir(o.out, { recursive: true });
const shot = (page, step) => page.screenshot({ path: join(o.out, `${o.name}-${step}.png`) });
const settle = (page, ms = 700) => page.waitForTimeout(ms);
const said = {};

const remote = o.cdp ? await chromium.connectOverCDP(o.cdp) : null;
const browser = remote ? null : await chromium.launch({ channel: 'msedge' });
const errors = [];
try {
  const page = remote ? remote.contexts()[0].pages()[0] : await browser.newPage({ viewport: { width: vw, height: vh } });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  if (!remote) await page.goto(o.url);

  // Scan, and catch the search's pill while it runs.
  const scan = page.locator('button.cta', { hasText: /^\s*Scan / });
  await scan.waitFor({ timeout: 30_000 });
  await scan.click();
  await page.locator('.atlas.is-map').waitFor({ timeout });
  const pill = page.locator('.dup-pill');
  await pill.first().waitFor({ timeout });
  const checking = page.locator('.dup-pill[role="progressbar"]');
  if (await checking.count()) {
    await page.waitForFunction(
      () => Number(document.querySelector('.dup-pill b')?.textContent?.replace('%', '') ?? 0) >= 20,
      null,
      { timeout },
    ).catch(() => {});
    await shot(page, '1-progress');
    said.progress = (await checking.textContent())?.trim();
  }

  // Done: the pill says what it found; the plan tray lists the proposal, off by default.
  const ready = page.locator('button.dup-pill.is-ready');
  await ready.waitFor({ timeout });
  said.pill = (await ready.textContent())?.trim();
  const build = page.getByRole('button', { name: 'Build a plan' });
  if (await build.count()) await build.click();
  await page.locator('.tray').waitFor({ timeout: 10_000 });
  await settle(page, 900);
  await shot(page, '2-ready');

  // The panel in place of the tray, the first group's links on the map.
  await ready.click();
  const panel = page.locator('.dup-panel');
  await panel.waitFor({ timeout: 10_000 });
  await page.locator('svg.dup-links').waitFor({ timeout: 10_000 });
  await page.mouse.move(2, 2);
  await settle(page);
  await shot(page, '3-links');
  if ((await panel.locator('img').count()) > 0) throw new Error('the duplicates panel shows an image: no thumbnails');

  // Another group under the pointer links that one.
  const cards = panel.locator('.dup-card');
  const hoverAt = Math.min(2, (await cards.count()) - 1);
  await cards.nth(hoverAt).hover();
  await settle(page, 400);
  await shot(page, '4-hover');

  // Untick a group: every copy of it stays.
  const target = o.untick ? cards.filter({ hasText: o.untick }).first() : cards.nth(1);
  said.unticked = (await target.locator('.dup-text b').textContent())?.trim();
  await target.locator('[role="checkbox"]').click();
  await target.hover();
  await settle(page, 400);
  await shot(page, '5-untick');

  // Into the plan.
  await panel.getByRole('button', { name: /^Add \d+ groups? to the plan$/ }).click();
  await page.locator('.tray:not(.dup-panel)').waitFor({ timeout: 10_000 });
  if (o.only) {
    // Everything else out of the plan: the run then deletes the duplicates' extra copies only.
    const others = page.locator('.tray [role="checkbox"][aria-checked="true"]:not([aria-label="Include Duplicate files"])');
    while ((await others.count()) > 0) await others.first().click();
  }
  await page.mouse.move(2, 2);
  await settle(page);
  await shot(page, '6-plan');

  const free = page.locator('.tray-cta');
  if ((!remote || o.run) && (await free.isEnabled())) {
    await free.click();
    const dialog = page.getByRole('alertdialog');
    await dialog.waitFor({ timeout: 60_000 });
    await settle(page, 500);
    await shot(page, '7-confirm');
    said.confirm = (await dialog.textContent())?.replace(/\s+/g, ' ').trim();

    await page.getByRole('button', { name: /permanently$/ }).click();
    await page.locator('.run').waitFor({ timeout: 30_000 });
    await page
      .waitForFunction(() => document.querySelectorAll('.run-step.is-done, .run-steps li.is-done').length > 0, null, {
        timeout: 15_000,
      })
      .catch(() => {});
    await shot(page, '8-run');
    const see = page.getByRole('button', { name: 'See what changed' });
    await see.waitFor({ timeout });
    await settle(page, 900);
    await shot(page, '9-run-done');
    await see.click();
    await settle(page, 600);
    await page.keyboard.press('Escape');
    const summary = page.locator('.tray.summary');
    await summary.waitFor({ timeout: 30_000 });
    await settle(page, 2600);
    await shot(page, '10-summary');
    const recap = summary.locator('.dup-recap');
    if (await recap.count()) {
      await recap.scrollIntoViewIfNeeded();
      await settle(page, 400);
      await shot(page, '11-summary-dups');
      said.summary = (await recap.textContent())?.replace(/\s+/g, ' ').trim();
      if ((await recap.locator('img').count()) > 0) throw new Error('the summary shows an image: no thumbnails');
    }
    said.freed = (await summary.locator('.summary-num').getAttribute('aria-label'))?.trim();
  }
} finally {
  await writeFile(join(o.out, `${o.name}-said.json`), JSON.stringify(said, null, 2));
  console.log(JSON.stringify(said, null, 2));
  if (browser) await browser.close();
  if (remote) await remote.close();
}
if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}

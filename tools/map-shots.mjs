// Screenshots the map's screens in Edge: the scan plate, scanning (the HUD over the map filling in), the overview,
// a hover card, a category zoomed in, a folder opened from it, the plan tray beside the map with its tiles lit, and
// then freeing the plan: the confirm dialog, the run mid-way and done, and the summary (past the highlight slides,
// which npm run wrapped-shots takes).
//
//   npm run dev                       (the UI on the mock bridge, http://localhost:1420)
//   npm run map-shots -- [options]
//
// Options (all optional):
//   --url <url>          page to open             (default http://localhost:1420/?fixture=windows; add &run=<ms>
//                        to change how long the mock run takes)
//   --out <dir>          where PNGs go            (default shots)
//   --name <prefix>      file name prefix         (default map)
//   --viewport <WxH>     browser viewport         (default 1280x820, the app window's default size)
//   --scanning-at <ms>   when to take the scanning shot after pressing Scan (default 1600)
//   --midway <share>     take the mid-run shot once this share of the plan is freed (default 0.45)
//   --timeout <ms>       how long a scan, or a run, may take (default 600000: a real drive)
//   --cdp <url>          drive an already running page over CDP instead of opening --url, e.g. the installed
//                        app started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
//                        and --cdp http://127.0.0.1:9333 (the window keeps its own size; --viewport is ignored)
//   --run                over CDP, also free the plan. The app deletes for real, so only ever pass this to an app
//                        in fixture mode (VACANCY_FIXTURE_ROOT). Against the mock bridge the plan is always freed.
//   --fast               press "Fast scan (asks for admin)" instead of "Scan <drive>" (Windows). The app waits on
//                        Windows' admin prompt first: answer it on the real desktop (the mock answers itself); the
//                        scanning shot is taken once the HUD shows, and the map may take up to --timeout. Works
//                        over CDP with --run, to free a plan made from a fast scan's map.
//
// Writes <name>-1-plate.png, -2-scanning.png, -3-map.png, -4-hover.png, -5-category.png, -6-folder.png,
// -7-plan.png (the tray open, a reclaimable tile's hover card), -8-plan-why.png (the biggest proposal's why and
// sample paths), -9-confirm.png, -10-run.png (mid-way), -11-run-done.png and -12-summary.png. Fails if the page
// logs an error.
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?fixture=windows' },
    out: { type: 'string', default: 'shots' },
    name: { type: 'string', default: 'map' },
    viewport: { type: 'string', default: '1280x820' },
    'scanning-at': { type: 'string', default: '1600' },
    midway: { type: 'string', default: '0.45' },
    timeout: { type: 'string', default: '600000' },
    cdp: { type: 'string' },
    run: { type: 'boolean', default: false },
    fast: { type: 'boolean', default: false },
  },
});
const [vw, vh] = o.viewport.split('x').map(Number);
await mkdir(o.out, { recursive: true });
const shot = (page, step) => page.screenshot({ path: join(o.out, `${o.name}-${step}.png`) });
const settle = (page, ms = 900) => page.waitForTimeout(ms);

const remote = o.cdp ? await chromium.connectOverCDP(o.cdp) : null;
const browser = remote ? null : await chromium.launch({ channel: 'msedge' });
const errors = [];
try {
  const page = remote ? remote.contexts()[0].pages()[0] : await browser.newPage({ viewport: { width: vw, height: vh } });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  if (!remote) await page.goto(o.url);

  const scan = o.fast
    ? page.getByRole('button', { name: 'Fast scan (asks for admin)' })
    : page.locator('button.cta', { hasText: /^\s*Scan / });
  await scan.waitFor({ timeout: 30_000 });
  await settle(page, 1200);
  await shot(page, '1-plate');

  const started = Date.now();
  await scan.click();
  // A fast scan waits on Windows' admin prompt before the HUD shows, as long as a person takes to answer it; a
  // small fixture folder's may then be done before the HUD is caught, so its map will do too.
  if (o.fast) await page.locator('.hud, .atlas.is-map').first().waitFor({ timeout: Number(o.timeout) });
  else await page.locator('.hud').waitFor({ timeout: 30_000 });
  await settle(page, Number(o['scanning-at']));
  await shot(page, '2-scanning');

  await page.locator('.atlas.is-map').waitFor({ timeout: Number(o.timeout) });
  const how = (await page.locator('.scan-note.is-fast').count()) ? 'a fast scan' : 'a normal scan';
  console.log(`The scan took ${((Date.now() - started) / 1000).toFixed(1)} s from pressing Scan to the map (${how}).`);
  await settle(page, 1400);
  await shot(page, '3-map');

  // The hover card over the biggest tile that isn't free space.
  const biggest = async (selector) =>
    page.evaluate((sel) => {
      let best = null;
      for (const el of document.querySelectorAll(sel)) {
        const r = el.getBoundingClientRect();
        if (r.width * r.height > (best?.a ?? 0)) best = { a: r.width * r.height, x: r.x + r.width / 2, y: r.y + r.height / 2 };
      }
      return best;
    }, selector);
  const tile = await biggest('.map-tile:not(.map-tile--free)');
  if (tile) {
    await page.mouse.move(tile.x, tile.y);
    await settle(page, 300);
    await shot(page, '4-hover');
  }

  // Zoom into the biggest region, then open its biggest folder.
  const head = await biggest('.map-region-head');
  if (head) {
    await page.mouse.click(head.x, head.y);
    await settle(page);
    await page.mouse.move(2, 2);
    await shot(page, '5-category');
    // Reclaimable tiles are clickable too, but go in the plan rather than open.
    const folder = await biggest('.map-tile.is-clickable:not(.map-tile--proposal)');
    if (folder) {
      const crumbs = await page.locator('.crumb').count();
      await page.mouse.click(folder.x, folder.y);
      await page.waitForFunction((n) => document.querySelectorAll('.crumb').length > n, crumbs, { timeout: 30_000 });
      await settle(page);
      const inner = await biggest('.map-tile:not(.map-tile--free)');
      if (inner) await page.mouse.move(inner.x, inner.y);
      await settle(page, 300);
      await shot(page, '6-folder');
    }
  }

  // Back to the whole disk, then the plan: the tray beside the map, reclaimable tiles pinned.
  await page.mouse.move(2, 2);
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  const build = page.getByRole('button', { name: 'Build a plan' });
  if (await build.count()) {
    await build.click();
    await page.locator('.tray').waitFor({ timeout: 10_000 });
    await settle(page);
    const reclaim = await biggest('.map-tile--proposal');
    if (reclaim) await page.mouse.move(reclaim.x, reclaim.y);
    await settle(page, 300);
    await shot(page, '7-plan');
    await page.mouse.move(2, 2);
    await page.locator('.tray-row-title').first().click();
    await settle(page, 400);
    await shot(page, '8-plan-why');

    // Free the plan. Over CDP that deletes for real, so only with --run (an app in fixture mode).
    const free = page.locator('.tray-cta');
    if ((!remote || o.run) && (await free.isEnabled())) {
      await free.click();
      await page.getByRole('alertdialog').waitFor({ timeout: 60_000 });
      await settle(page, 500);
      await shot(page, '9-confirm');

      await page.getByRole('button', { name: /permanently$/ }).click();
      await page.locator('.tray.run').waitFor({ timeout: 10_000 });
      const share = Number(o.midway) * 100;
      await page.waitForFunction(
        (at) => parseFloat(document.querySelector('.run-meter i')?.style.width ?? '0') >= at,
        share,
        { timeout: Number(o.timeout) },
      );
      // Over a draining tile, so its hover card says how far it has got.
      const running = await biggest('.map-tile.is-running');
      if (running) await page.mouse.move(running.x, running.y);
      await settle(page, 250);
      await shot(page, '10-run');

      await page.mouse.move(2, 2);
      const seeChanges = page.getByRole('button', { name: 'See what changed' });
      await seeChanges.waitFor({ timeout: Number(o.timeout) });
      await settle(page, 1300);
      await shot(page, '11-run-done');

      await seeChanges.click();
      // The highlight slides come first (npm run wrapped-shots takes them); skip to the summary.
      await page.getByRole('button', { name: 'Skip to recap' }).click();
      await page.locator('.tray.summary').waitFor({ timeout: 10_000 });
      // The swipe sweeps to After and the Vacant signs catch.
      await settle(page, 2800);
      await shot(page, '12-summary');
    }
  }
} finally {
  await browser?.close();
}
if (errors.length) {
  console.error(`The page logged errors:\n${errors.join('\n')}`);
  process.exit(1);
}
console.log(`Wrote ${o.out}/${o.name}-*.png`);
// Over CDP the open connection would keep Node alive; the app itself is left running.
process.exit(0);

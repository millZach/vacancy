// Screenshots the map's screens in Edge: the scan plate, scanning (the HUD over the map filling in), the overview,
// a hover card, a category zoomed in, and a folder opened from it.
//
//   npm run dev                       (the UI on the mock bridge, http://localhost:1420)
//   npm run map-shots -- [options]
//
// Options (all optional):
//   --url <url>          page to open             (default http://localhost:1420/?fixture=windows)
//   --out <dir>          where PNGs go            (default shots)
//   --name <prefix>      file name prefix         (default map)
//   --viewport <WxH>     browser viewport         (default 1280x820, the app window's default size)
//   --scanning-at <ms>   when to take the scanning shot after pressing Scan (default 1600)
//   --timeout <ms>       how long a scan may take (default 600000: a real drive)
//   --cdp <url>          drive an already running page over CDP instead of opening --url, e.g. the installed
//                        app started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
//                        and --cdp http://127.0.0.1:9333 (the window keeps its own size; --viewport is ignored)
//
// Writes <name>-1-plate.png, -2-scanning.png, -3-map.png, -4-hover.png, -5-category.png, -6-folder.png. Fails if
// the page logs an error.
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
    timeout: { type: 'string', default: '600000' },
    cdp: { type: 'string' },
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

  const scan = page.locator('button.cta', { hasText: /^\s*Scan / });
  await scan.waitFor({ timeout: 30_000 });
  await settle(page, 1200);
  await shot(page, '1-plate');

  const started = Date.now();
  await scan.click();
  await page.locator('.hud').waitFor({ timeout: 30_000 });
  await settle(page, Number(o['scanning-at']));
  await shot(page, '2-scanning');

  await page.locator('.atlas.is-map').waitFor({ timeout: Number(o.timeout) });
  console.log(`The scan took ${((Date.now() - started) / 1000).toFixed(1)} s from pressing Scan to the map.`);
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
    const folder = await biggest('.map-tile.is-clickable');
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

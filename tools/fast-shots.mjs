// Screenshots fast scan (Windows) in Edge, against the mock bridge: the plate with both buttons, the wait while
// Windows' admin prompt is up, the fast scan reading the file index and sorting it, the map with its Locked folders
// block (a locked tile's hover card, the block opened with its card, the toolbar's notes), then the normal scan in
// place of a fast one, with its note in the HUD and on the map: admin declined, a drive that isn't NTFS (the USB
// stick), the helper not starting and the index unreadable. With --mac, also the Mac's plate, which must have no fast
// scan.
//
//   npm run dev                       (the UI on the mock bridge, http://localhost:1420)
//   npm run fast-shots -- [options]
//
// Options (all optional):
//   --url <url>          the Windows page         (default http://localhost:1420/?fixture=windows&scan=12000&uac=2500:
//                        a fast scan takes half of scan=, after the mock prompt's uac=)
//   --mac <url>          also check a Mac's plate, e.g. "http://localhost:1420/?fixture=macos"
//   --out <dir>          where PNGs go            (default shots)
//   --name <prefix>      file name prefix         (default fast)
//   --viewport <WxH>     browser viewport         (default 1280x820, the app window's default size)
//
// Writes <name>-1-plate.png, -2-asking.png, -3-reading.png, -4-sorting.png, -5-map.png, -6-locked-hover.png,
// -7-locked-open.png, -8-notes.png (the toolbar), -9-declined-hud.png, -10-declined-map.png, -11-exfat-hud.png,
// -12-exfat-map.png, -13-failed-hud.png, -14-failed-map.png, -15-unreadable-hud.png, -16-unreadable-map.png and,
// with --mac, -17-mac-plate.png. Fails if the page logs an error, or a screen doesn't say what it should.
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?fixture=windows&scan=12000&uac=2500' },
    mac: { type: 'string' },
    out: { type: 'string', default: 'shots' },
    name: { type: 'string', default: 'fast' },
    viewport: { type: 'string', default: '1280x820' },
  },
});
const [vw, vh] = o.viewport.split('x').map(Number);
await mkdir(o.out, { recursive: true });
const shot = (page, step, clip) => page.screenshot({ path: join(o.out, `${o.name}-${step}.png`), ...(clip ? { clip } : {}) });
const settle = (page, ms = 900) => page.waitForTimeout(ms);
const FAST = 'Fast scan (asks for admin)';
const MINUTE = 60_000;

/** The page at `url` with `fast=` set to `fast` (or left out). */
const withFast = (url, fast) => {
  const u = new URL(url);
  if (fast) u.searchParams.set('fast', fast);
  else u.searchParams.delete('fast');
  return u.toString();
};

/** Waits until `locator` shows text matching `text`; fails if it doesn't within a minute. */
async function says(locator, text, what) {
  await locator.waitFor({ timeout: MINUTE });
  const until = Date.now() + MINUTE;
  let got = '';
  while (Date.now() < until) {
    got = ((await locator.count()) ? await locator.innerText() : '').replace(/\s+/g, ' ');
    if (typeof text === 'string' ? got.includes(text) : text.test(got)) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`${what} should say ${text}, but says: ${got}`);
}

const browser = await chromium.launch({ channel: 'msedge' });
const errors = [];
const open = async (url) => {
  const page = await browser.newPage({ viewport: { width: vw, height: vh } });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.goto(url);
  await page.getByRole('heading', { name: 'Which drive should Vacancy scan?' }).waitFor({ timeout: 30_000 });
  await settle(page, 1200);
  return page;
};
/** The biggest element matching `selector`, by its centre. */
const biggest = (page, selector) =>
  page.evaluate((sel) => {
    let best = null;
    for (const el of document.querySelectorAll(sel)) {
      const r = el.getBoundingClientRect();
      if (r.width * r.height > (best?.a ?? 0)) best = { a: r.width * r.height, x: r.x + r.width / 2, y: r.y + r.height / 2 };
    }
    return best;
  }, selector);

try {
  // ---------- the fast scan ----------
  const page = await open(withFast(o.url, null));
  const fast = page.getByRole('button', { name: FAST });
  await fast.waitFor({ timeout: 10_000 });
  await says(page.locator('.fast-fine'), 'Fast scan reads the drive’s file index directly', 'The plate’s fine print');
  await shot(page, '1-plate');

  await fast.click();
  await says(page.locator('.plate-wait'), 'Waiting for Windows’ admin prompt…', 'The plate, while Windows asks');
  if (!(await fast.isDisabled())) throw new Error('The fast scan button should wait while Windows asks for admin');
  if (await page.locator('.hud').count()) throw new Error('Nothing should be scanning while Windows asks for admin');
  await settle(page, 400);
  await shot(page, '2-asking');

  await says(page.locator('.hud.is-fast .hud-path'), 'Reading the file index of Local Disk (C:)', 'The HUD, reading');
  await says(page.locator('.hud-mode'), /fast scan/i, 'The HUD’s tag');
  await settle(page, 900);
  await shot(page, '3-reading');

  await says(page.locator('.hud.is-fast .hud-path'), 'Sorting Local Disk (C:) into categories', 'The HUD, sorting');
  await settle(page, 1300);
  await shot(page, '4-sorting');

  await page.locator('.atlas.is-map').waitFor({ timeout: MINUTE });
  await settle(page, 1400);
  await says(page.locator('.scan-notes'), /Fast scan, \d+ s/, 'The toolbar');
  await says(page.locator('.scan-notes'), 'in locked folders, totals only', 'The toolbar');
  await shot(page, '5-map');

  const lockedTile = await biggest(page, '.map-tile--locked');
  if (!lockedTile) throw new Error('The map should have a Locked folders block');
  await page.mouse.move(lockedTile.x, lockedTile.y);
  await settle(page, 300);
  await says(page.getByRole('tooltip'), 'shows their total only', 'A locked tile’s hover card');
  await shot(page, '6-locked-hover');

  await page.mouse.click(lockedTile.x, lockedTile.y);
  await says(page.locator('.locked-card'), /in [\d,]+ locked folders/, 'The opened block’s card');
  await page.mouse.move(2, 2);
  await settle(page);
  await shot(page, '7-locked-open');

  await page.keyboard.press('Escape');
  await settle(page, 700);
  const bar = await page.locator('.atlas-toolbar').boundingBox();
  await shot(page, '8-notes', { x: 0, y: 0, width: vw, height: Math.ceil(bar.y + bar.height + 8) });
  await page.close();

  // ---------- the normal scan in place of a fast one ----------
  const fallbacks = [
    ['declined', null, 'Windows wasn’t given admin', 'Normal scan: admin wasn’t given'],
    ['exfat', 'SanDisk (E:)', 'is exFAT, and fast scan only reads NTFS drives', 'Normal scan: exFAT drive'],
    ['failed', null, 'scan helper didn’t start', 'Normal scan: the helper didn’t start'],
    ['unreadable', null, 'file index couldn’t be read', 'Normal scan: the index couldn’t be read'],
  ];
  let step = 9;
  for (const [mode, drive, note, label] of fallbacks) {
    const p = await open(withFast(o.url, mode === 'exfat' ? null : mode));
    if (drive) {
      await p.getByRole('radio', { name: drive }).click();
      await settle(p, 500);
    }
    await p.getByRole('button', { name: FAST }).click();
    await says(p.locator('.hud .hud-note'), note, `The HUD (${mode})`);
    await settle(p, 1500);
    if (await p.locator('.hud.is-fast').count()) throw new Error(`The HUD (${mode}) should be a normal scan's`);
    await shot(p, `${step++}-${mode}-hud`);
    await p.locator('.atlas.is-map').waitFor({ timeout: MINUTE });
    await settle(p, 1400);
    await says(p.locator('.scan-notes'), label, `The toolbar (${mode})`);
    await shot(p, `${step++}-${mode}-map`);
    await p.close();
  }

  // ---------- a Mac: no fast scan ----------
  if (o.mac) {
    const mac = await open(o.mac);
    await mac.locator('button.cta', { hasText: /^\s*Scan / }).waitFor({ timeout: 10_000 });
    if (await mac.getByRole('button', { name: /Fast scan/ }).count()) throw new Error('A Mac should offer no fast scan');
    if (await mac.locator('.fast-fine').count()) throw new Error('A Mac should say nothing about fast scan');
    await shot(mac, `${step++}-mac-plate`);
    await mac.close();
  }
} finally {
  await browser.close();
}
if (errors.length) {
  console.error(`The page logged errors:\n${errors.join('\n')}`);
  process.exit(1);
}
console.log(`Wrote ${o.out}/${o.name}-*.png`);

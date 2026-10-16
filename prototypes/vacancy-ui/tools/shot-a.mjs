// PROTOTYPE helper for variant A: screenshots of in-between states (scanning, zoom, tooltip, drawer, confirm, run mid-way, small window).
// node tools/shot-a.mjs <baseUrl>
import { chromium } from 'playwright';
const [base = 'http://localhost:5201/'] = process.argv.slice(2);
const browser = await chromium.launch({ channel: 'msedge' });
const errors = [];
const out = [];

async function open(page, os, step) {
  await page.goto(`${base}?variant=A&os=${os}&step=${step}`);
  await page.waitForTimeout(900);
}
async function shot(page, name) {
  const f = `shots/A-x-${name}.png`;
  await page.screenshot({ path: f });
  out.push(f);
}

const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });

// scanning animation, mid-way
await open(page, 'win', 'scan');
await page.click('.va-cta--scan');
await page.waitForTimeout(2000);
await shot(page, 'win-scanning');
await page.waitForTimeout(3200);
await shot(page, 'win-after-scan');

// zoom into a category, then hover a tile for the tooltip
await open(page, 'win', 'overview');
await page.click('.va-region-head >> text=Developer');
await page.waitForTimeout(1200);
await shot(page, 'win-zoom');
const t = await page.locator('[data-tile="p:w-node"]').boundingBox();
await page.mouse.move(t.x + t.width / 2, t.y + t.height / 2);
await page.waitForTimeout(300);
await shot(page, 'win-zoom-tip');

// overview tooltip on an AI item (mac)
await open(page, 'mac', 'overview');
const ai = await page.locator('[data-tile="p:m-vscode-ai"]').boundingBox();
await page.mouse.move(ai.x + ai.width / 2, ai.y + ai.height / 2);
await page.waitForTimeout(300);
await shot(page, 'mac-tip-ai');

// plan: toggle a tile, open the reorg drawer, then the confirm
await open(page, 'win', 'plan');
await page.click('[data-tile="p:w-docker"]');
await page.waitForTimeout(400);
await page.click('.va-reorg-viz >> nth=0');
await page.waitForTimeout(600);
await shot(page, 'win-drawer');
await page.keyboard.press('Escape');
await page.waitForTimeout(400);
await page.click('.va-tray-foot .va-cta');
await page.waitForTimeout(600);
await shot(page, 'win-confirm');
await page.click('.va-dialog .va-cta');
await page.waitForTimeout(4500);
await shot(page, 'win-run-mid');
await page.waitForTimeout(9000);
await shot(page, 'win-run-done');
await page.click('.va-run-foot .va-cta');
await page.waitForTimeout(3000);
await shot(page, 'win-summary-settled');
await page.locator('.va-summary .va-side-scroll').evaluate((el) => (el.scrollTop = el.scrollHeight));
await page.click('.va-undo button >> nth=0');
await page.waitForTimeout(1500);
await shot(page, 'win-summary-scrolled');

await open(page, 'mac', 'run');
await page.waitForTimeout(6000);
await shot(page, 'mac-run-mid');

// small window: content area about 1100x700
await page.setViewportSize({ width: 1140, height: 830 });
for (const [os, step] of [['win', 'overview'], ['win', 'plan'], ['mac', 'run'], ['mac', 'summary'], ['win', 'scan']]) {
  await open(page, os, step);
  await page.waitForTimeout(step === 'summary' ? 2500 : 800);
  await shot(page, `small-${os}-${step}`);
}

await browser.close();
console.log(out.join('\n'));
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');

// PROTOTYPE helper for variant D's drive picker: for each ?picker= design, the Windows scan screen, the header
// control open, switching to D: (its scan screen, then its map after scanning), the header with two scanned drives,
// switching back to C: (its map, no rescan), and the macOS scan screen. Plus a few in-between states.
// VP=1440x900 sets the viewport. node tools/shot-drives.mjs <baseUrl> <outDir> [pickers=1,2,3]
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const [base = 'http://localhost:5231/', out = 'shots', ps = '1,2,3'] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const [vw, vh] = (process.env.VP || '1440x900').split('x').map(Number);
const browser = await chromium.launch({ channel: 'msedge' });
const page = await browser.newPage({ viewport: { width: vw, height: vh } });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => {
  if (m.type() === 'error') errors.push(m.text());
});

const open = async (p, os, step, wait = 1400) => {
  await page.goto(`${base}?variant=D&picker=${p}&os=${os}&step=${step}`);
  await page.waitForTimeout(wait);
};
const shot = async (name) => {
  const f = `${out}/${name}.png`;
  await page.screenshot({ path: f });
  console.log(f);
};
const rest = () => page.mouse.move(vw - 4, 4);

/** Opens the header's drive control (design 2 has nothing to open: hover a chip for its card). */
async function openHeader(p, nth) {
  if (p === 1) await page.click('.vd-dm-btn');
  if (p === 2) await page.hover(`.vd-chip-wrap >> nth=${nth}`);
  if (p === 3) await page.click('.vd-crumb-drive');
  await page.waitForTimeout(350);
}
/** Switches drives from the header. */
async function headerSwitch(p, nth) {
  if (p === 1) {
    await page.click('.vd-dm-btn');
    await page.click(`.vd-dm-row >> nth=${nth}`);
  }
  if (p === 2) await page.click(`.vd-chip >> nth=${nth}`);
  if (p === 3) {
    await page.click('.vd-crumb-drive');
    await page.click(`.vd-crumb-row >> nth=${nth}`);
  }
  await rest();
  await page.waitForTimeout(1300);
}

for (const p of ps.split(',').map(Number)) {
  const n = `p${p}`;
  // Fresh launch: nothing scanned yet.
  await open(p, 'win', 'scan');
  await shot(`${n}-win-1-scan`);
  // The scan screen's own picker (designs 1 and 2) picks without leaving; design 3 picks on the shelf.
  if (p === 1) await page.click('.vd-card >> nth=2');
  if (p === 2) await page.click('.vd-flip-tab >> nth=2');
  if (p === 3) await page.click('.vd-shelf-item >> nth=1');
  await rest();
  await page.waitForTimeout(700);
  await shot(`${n}-win-1b-scan-picked-other`);

  // After a scan of C: (the prototype seeds it 4 min ago).
  await open(p, 'win', 'overview');
  await openHeader(p, 1);
  await shot(`${n}-win-2-header-open`);
  await page.keyboard.press('Escape');
  await rest();
  await page.waitForTimeout(200);

  // Switch to D: (unscanned): its scan screen, then scan it.
  await headerSwitch(p, 1);
  await shot(`${n}-win-3-switched-to-D-scan`);
  if (p !== 3) {
    // On the scan screen, picking a drive that was already scanned offers its map instead of a rescan.
    await page.click(p === 1 ? '.vd-card >> nth=0' : '.vd-flip-tab >> nth=0');
    await rest();
    if (p === 2) {
      await page.waitForTimeout(90);
      await shot(`${n}-win-3a-flip-mid`);
    }
    await page.waitForTimeout(700);
    await shot(`${n}-win-3b-scan-screen-on-scanned-C`);
    await page.click(p === 1 ? '.vd-card >> nth=1' : '.vd-flip-tab >> nth=1');
    await rest();
    await page.waitForTimeout(600);
  }
  await page.click('.va-cta--scan');
  await page.waitForTimeout(5400);
  await rest();
  await page.waitForTimeout(500);
  await shot(`${n}-win-4-D-map`);

  // Both scanned now.
  await openHeader(p, 0);
  await shot(`${n}-win-5-header-both-scanned`);
  await page.keyboard.press('Escape');
  await rest();
  await page.waitForTimeout(200);

  // Back to C:: straight to its map, no rescan.
  await headerSwitch(p, 0);
  await shot(`${n}-win-6-back-to-C-map`);

  // A run locks switching.
  await open(p, 'win', 'run', 2500);
  if (p === 1) await page.hover('.vd-dm-btn');
  await shot(`${n}-win-7-run-locked`);

  // After a run: the summary at rest, with the header control showing the cleaned drive.
  await open(p, 'win', 'summary', 1500);
  await page.click('.vc-wrap-skip');
  await rest();
  await page.waitForTimeout(1600);
  await openHeader(p, 0);
  await shot(`${n}-win-8-summary-header`);
  await page.keyboard.press('Escape');
  if (p === 3) {
    await open(p, 'win', 'plan', 1500);
    await shot(`${n}-win-9-plan`);
  }

  // Ripples: a background click on the scan screen ripples; a click on the picker (between its controls) does not.
  await open(p, 'win', 'scan', 1500);
  const bg = { x: Math.round(vw * 0.82), y: Math.round(vh * 0.7) };
  await page.mouse.click(bg.x, bg.y);
  await page.waitForTimeout(260);
  await page.screenshot({ path: `${out}/${n}-ripple-background.png`, clip: { x: bg.x - 220, y: bg.y - 150, width: 440, height: 300 } });
  console.log(`${out}/${n}-ripple-background.png`);
  if (p !== 3) {
    await page.waitForTimeout(3200);
    const box = await page.locator(p === 1 ? '.vd-card >> nth=0' : '.vd-flip-tabs').boundingBox();
    const at = p === 1 ? { x: Math.round(box.x + box.width + 7), y: Math.round(box.y + box.height / 2) } : { x: Math.round(box.x + 2), y: Math.round(box.y + box.height / 2) };
    await page.mouse.click(at.x, at.y);
    await page.waitForTimeout(260);
    await page.screenshot({ path: `${out}/${n}-ripple-picker-gap.png`, clip: { x: at.x - 220, y: at.y - 150, width: 440, height: 300 } });
    console.log(`${out}/${n}-ripple-picker-gap.png`, at, 'step after:', new URL(page.url()).searchParams.get('step'));
  }

  await open(p, 'mac', 'scan');
  await shot(`${n}-mac-1-scan`);
}

await browser.close();
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');

// PROTOTYPE helper for variant C: screenshots of interactive states the step shots can't reach.
// node tools/shot-c.mjs <baseUrl> [os=win,mac] [groups=scan,plan,confirm,run,summary,narrow]
import { chromium } from 'playwright';

const [base = 'http://localhost:5203/', oss = 'win,mac', gs = 'scan,plan,confirm,run,summary,narrow'] = process.argv.slice(2);
const groups = new Set(gs.split(','));
const browser = await chromium.launch({ channel: 'msedge' });
const errors = [];
const url = (os, step) => `${base}?variant=C&os=${os}&step=${step}`;

async function open(page, os, step, wait = 1200) {
  await page.goto(url(os, step));
  await page.waitForTimeout(wait);
}
const shot = async (page, name) => {
  const f = `shots/C-${name}.png`;
  await page.screenshot({ path: f });
  console.log(f);
};

for (const os of oss.split(',')) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });

  if (groups.has('scan')) {
    await open(page, os, 'scan');
    await page.click('.vc-scan-btn');
    await page.waitForTimeout(1900);
    await shot(page, `${os}-scanning`);
  }
  if (groups.has('plan')) {
    await open(page, os, 'plan');
    await page.focus('.vc-tbody');
    for (let i = 0; i < 2; i++) await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(300);
    await shot(page, `${os}-plan-keys`);
    await page.click('.vc-rail-f:has-text("AI suggestions")');
    await page.focus('.vc-tbody');
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(300);
    await shot(page, `${os}-plan-ai`);
    await page.click('.vc-rail-f:has-text("Reorganize")');
    await page.waitForTimeout(400);
    await shot(page, `${os}-plan-reorg`);
    await page.click('.vc-rail-f:has-text("Locked")');
    await page.waitForTimeout(300);
    await shot(page, `${os}-plan-locked`);
    await open(page, os, 'overview');
    await page.focus('.vc-tbody');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(300);
    await shot(page, `${os}-overview-open`);
  }
  if (groups.has('confirm')) {
    await open(page, os, 'plan');
    await page.click('button:has-text("Review and run")');
    await page.waitForTimeout(1600);
    await shot(page, `${os}-confirm`);
  }
  if (groups.has('run')) {
    await open(page, os, 'run', 4600);
    await shot(page, `${os}-run-mid`);
    await page.waitForTimeout(6300);
    await shot(page, `${os}-run-end`);
  }
  if (groups.has('summary')) {
    await open(page, os, 'summary', 1800);
    for (let k = 2; k <= 7; k++) {
      await page.click(`.vc-dotbtn:nth-child(${k})`);
      await page.mouse.move(5, 5);
      await page.waitForTimeout(k === 5 ? 1600 : 1400);
      await shot(page, `${os}-sum-${k}`);
    }
  }
  if (groups.has('narrow')) {
    await page.setViewportSize({ width: 1140, height: 830 });
    for (const s of ['scan', 'overview', 'plan', 'run']) {
      await open(page, os, s, s === 'run' ? 4000 : 1200);
      await shot(page, `${os}-narrow-${s}`);
    }
    await open(page, os, 'summary', 1500);
    await shot(page, `${os}-narrow-sum-1`);
    for (const k of [2, 3, 7]) {
      await page.click(`.vc-dotbtn:nth-child(${k})`);
      await page.mouse.move(5, 5);
      await page.waitForTimeout(1400);
      await shot(page, `${os}-narrow-sum-${k}`);
    }
    await open(page, os, 'plan');
    await page.click('.vc-rail-f:has-text("Reorganize")');
    await page.waitForTimeout(400);
    await shot(page, `${os}-narrow-reorg`);
  }
  await page.close();
}
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');
await browser.close();

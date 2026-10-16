// PROTOTYPE helper for variant B: extra screenshots (scanning, confirm, mid-run, scrolled feeds, small window).
// node tools/shot-b.mjs [--isolated] [--base=http://localhost:5202/] [--only=name,name]
//   --isolated  starts its own Vite server on 5202 with variants A and C stubbed out, so B can be checked
//               while the other variants are mid-build. Without it, point --base at a running dev server.
import { chromium } from 'playwright';

const args = Object.fromEntries(process.argv.slice(2).map((a) => {
  const s = a.replace(/^--/, '');
  const i = s.indexOf('=');
  return i < 0 ? [s, true] : [s.slice(0, i), s.slice(i + 1)];
}));
const base = args.base ?? 'http://localhost:5202/';
const only = args.only ? String(args.only).split(',') : null;

let server = null;
if (args.isolated) {
  const { createServer } = await import('vite');
  const react = (await import('@vitejs/plugin-react')).default;
  const stub = (key) =>
    `export const variant = { key: '${key}', name: '${key} (stub)', frame: { bg: '#222', fg: '#ddd', border: '#333' }, Component: () => null };`;
  server = await createServer({
    configFile: false,
    root: process.cwd(),
    base: './',
    logLevel: 'error',
    plugins: [
      react(),
      {
        name: 'stub-other-variants',
        enforce: 'pre',
        resolveId(id) {
          const m = /^\.\/variants\/(A|C)$/.exec(id);
          return m ? `\0stub-variant-${m[1]}` : null;
        },
        load(id) {
          return id.startsWith('\0stub-variant-') ? stub(id.slice(-1)) : null;
        },
      },
    ],
    server: { port: 5202, strictPort: true, hmr: false, watch: { ignored: ['**/src/variants/A/**', '**/src/variants/C/**'] } },
  });
  await server.listen();
}

const browser = await chromium.launch({ channel: 'msedge' });
const errors = [];
const feedScroll = (page, y) => page.evaluate((dy) => { const f = document.querySelector('.vb-feed'); if (f) f.scrollTop += dy; }, y);

const scrollToSel = (page, sel) => page.evaluate((q) => {
  const f = document.querySelector('.vb-feed');
  const el = document.querySelector(q);
  if (f && el) f.scrollTop += el.getBoundingClientRect().top - f.getBoundingClientRect().top - 90;
}, sel);

const steps = ['scan', 'overview', 'plan', 'run', 'summary'];
const shots = [];
for (const os of ['win', 'mac']) {
  for (const s of steps) {
    shots.push({ name: `B-${os}-${s}`, url: `?variant=B&os=${os}&step=${s}`, wait: 1500 });
  }
  shots.push({ name: `B-${os}-scanning`, url: `?variant=B&os=${os}&step=scan`, wait: 600, act: async (p) => { await p.click('.vb-primary-lg'); await p.waitForTimeout(2300); } });
  shots.push({ name: `B-${os}-confirm`, url: `?variant=B&os=${os}&step=plan`, wait: 1200, act: async (p) => { await p.click('.vb-composer .vb-primary'); await p.waitForTimeout(1300); } });
  shots.push({ name: `B-${os}-run-mid`, url: `?variant=B&os=${os}&step=run`, wait: 4200 });
  shots.push({ name: `B-${os}-run-end`, url: `?variant=B&os=${os}&step=run`, wait: 9800 });
  shots.push({ name: `B-${os}-overview-2`, url: `?variant=B&os=${os}&step=overview`, wait: 3500, act: (p) => feedScroll(p, 640) });
  shots.push({ name: `B-${os}-overview-3`, url: `?variant=B&os=${os}&step=overview`, wait: 3500, act: (p) => feedScroll(p, 1700) });
  shots.push({ name: `B-${os}-overview-ai`, url: `?variant=B&os=${os}&step=overview`, wait: 3500, act: (p) => scrollToSel(p, '.vb-find .vb-agent-chip') });
  shots.push({ name: `B-${os}-overview-hint`, url: `?variant=B&os=${os}&step=overview`, wait: 3500, act: (p) => scrollToSel(p, '.vb-find-hint') });
  shots.push({ name: `B-${os}-plan-2`, url: `?variant=B&os=${os}&step=plan`, wait: 1500, act: (p) => feedScroll(p, 620) });
  shots.push({ name: `B-${os}-summary-2`, url: `?variant=B&os=${os}&step=summary`, wait: 1800, act: (p) => feedScroll(p, 560) });
  shots.push({ name: `B-${os}-summary-3`, url: `?variant=B&os=${os}&step=summary`, wait: 1800, act: (p) => feedScroll(p, 1180) });
  shots.push({ name: `B-${os}-summary-4`, url: `?variant=B&os=${os}&step=summary`, wait: 1800, act: (p) => feedScroll(p, 3000) });
}
shots.push({ name: 'B-win-small-plan', url: '?variant=B&os=win&step=plan', wait: 1500, viewport: { width: 1140, height: 796 }, act: (p) => feedScroll(p, 300) });
shots.push({ name: 'B-win-small-summary', url: '?variant=B&os=win&step=summary', wait: 1800, viewport: { width: 1140, height: 796 } });

const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
if (args.probe) {
  // --probe=<url query> --js=<expression>: print the value of an expression after the page settles.
  await page.goto(`${base}${args.probe}`);
  await page.waitForTimeout(Number(args.wait ?? 2000));
  console.log(JSON.stringify(await page.evaluate(String(args.js)), null, 1));
  shots.length = 0;
}
for (const s of shots) {
  if (only && !only.some((o) => s.name.includes(o))) continue;
  await page.setViewportSize(s.viewport ?? { width: 1440, height: 900 });
  await page.goto(`${base}${s.url}`);
  await page.waitForTimeout(s.wait);
  if (s.act) {
    await s.act(page);
    await page.waitForTimeout(350);
  }
  const f = `shots/${s.name}.png`;
  await page.screenshot({ path: f });
  console.log(f);
}
if (args.flow) {
  // Walk the whole flow by clicking, and print what the composer says at each point.
  const say = async (label) => console.log(label.padEnd(26), (await page.textContent('.vb-composer').catch(() => '(no composer)'))?.replace(/\s+/g, ' ').trim());
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`${base}?variant=B&os=win&step=scan`);
  await page.waitForTimeout(800);
  await page.click('.vb-primary-lg');
  await page.waitForURL(/step=overview/, { timeout: 8000 });
  await page.waitForTimeout(2500);
  await say('overview default');
  await page.locator('.vb-find .vb-choice button', { hasText: 'Keep' }).first().click();
  await say('after Keep on first');
  await page.locator('.vb-find-hint .vb-choice button', { hasText: 'Add to my list' }).click();
  await say('after adding hint');
  await page.click('.vb-composer .vb-primary');
  await page.waitForURL(/step=plan/);
  await page.waitForTimeout(1200);
  await page.locator('.vb-reorg').nth(1).locator('.vb-choice button', { hasText: 'Reorganize' }).click();
  await say('plan, both reorgs');
  await page.click('.vb-composer .vb-primary');
  await page.waitForTimeout(900);
  console.log('confirm says'.padEnd(26), (await page.textContent('.vb-confirm'))?.replace(/\s+/g, ' ').trim());
  await page.screenshot({ path: 'shots/B-flow-confirm.png' });
  const hold = page.locator('.vb-hold');
  await hold.hover();
  await page.mouse.down();
  await page.waitForTimeout(500);
  await page.mouse.up();
  await page.waitForTimeout(400);
  console.log('short press keeps plan'.padEnd(26), page.url().includes('step=plan'));
  await hold.hover();
  await page.mouse.down();
  await page.waitForTimeout(1600);
  await page.mouse.up();
  await page.waitForURL(/step=run/, { timeout: 3000 });
  console.log('long press starts run'.padEnd(26), page.url().includes('step=run'));
  await page.waitForTimeout(3500);
  await say('mid run');
  await page.waitForURL(/step=summary/, { timeout: 20000 });
  await page.waitForTimeout(1500);
  await say('summary');
  console.log('left for you'.padEnd(26), (await page.locator('.vb-r-item', { hasText: 'Cyberpunk' }).count()) > 0);
  await page.locator('.vb-r-undo').first().click();
  await page.waitForTimeout(1400);
  console.log('undo buttons'.padEnd(26), await page.locator('.vb-r-undo').allTextContents());
  const [dl] = await Promise.all([page.waitForEvent('download'), page.click('.vb-composer .vb-primary')]);
  console.log('save receipt'.padEnd(26), dl.suggestedFilename());
  await page.click('.vb-composer .vb-secondary');
  await page.waitForURL(/step=scan/);
  console.log('start again'.padEnd(26), page.url().includes('step=scan'), await page.isVisible('.vb-primary-lg'));
}
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');
await browser.close();
if (server) await server.close();

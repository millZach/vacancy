// Screenshots the scan screen and a frame strip of the dot field's ripple, in Edge, from a running UI.
//
//   npm run dev                       (the UI on the mock bridge, http://localhost:1420)
//   npm run ripple-strip -- [options]
//
// Options (all optional):
//   --url <url>          page to open             (default http://localhost:1420/?fixture=windows)
//   --out <dir>          where PNGs go            (default shots)
//   --name <prefix>      file name prefix         (default app)
//   --canvas <selector>  the dot field's canvas   (default .dot-field)
//   --scan <selector>    the Scan button          (default "button.cta")
//   --viewport <WxH>     browser viewport         (default 1280x820, the app window's default size)
//   --at <x,y>           where to click the water (default 300,560)
//   --frames <n>         frames per strip         (default 6)
//   --gap <ms>           time between frames      (default 120)
//   --reduced            emulate prefers-reduced-motion: reduce
//   --cdp <url>          drive an already running page over CDP instead of opening --url, e.g. the installed
//                        app started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
//                        and --cdp http://127.0.0.1:9333 (the window keeps its own size; --viewport is ignored)
//
// Writes <name>-scan.png, <name>-ripple-sheet.png (a background click) and <name>-splash-sheet.png (the Scan
// button). Frames are copied from the canvas inside the page on a timer, because page screenshots are too slow for
// a 120 ms strip; each crop is 600x500 CSS px around the impact point.
import { mkdir } from 'node:fs/promises';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?fixture=windows' },
    out: { type: 'string', default: 'shots' },
    name: { type: 'string', default: 'app' },
    canvas: { type: 'string', default: '.dot-field' },
    scan: { type: 'string', default: 'button.cta' },
    viewport: { type: 'string', default: '1280x820' },
    at: { type: 'string', default: '300,560' },
    frames: { type: 'string', default: '6' },
    gap: { type: 'string', default: '120' },
    reduced: { type: 'boolean', default: false },
    cdp: { type: 'string' },
  },
});
const [vw, vh] = o.viewport.split('x').map(Number);
const [ax, ay] = o.at.split(',').map(Number);
const frames = Number(o.frames);
const gap = Number(o.gap);
await mkdir(o.out, { recursive: true });

// Edge lays out the contact sheets; the page under test is either a fresh Edge tab or a running app over CDP.
const browser = await chromium.launch({ channel: 'msedge' });
const remote = o.cdp ? await chromium.connectOverCDP(o.cdp) : null;
const errors = [];
try {
  const page = remote
    ? remote.contexts()[0].pages()[0]
    : await browser.newPage({ viewport: { width: vw, height: vh }, reducedMotion: o.reduced ? 'reduce' : 'no-preference' });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  if (remote && o.reduced) await page.emulateMedia({ reducedMotion: 'reduce' });
  if (!remote) await page.goto(o.url);
  await page.waitForSelector(o.canvas);
  await page.waitForTimeout(1800);
  await page.mouse.move(2, 2);
  await page.screenshot({ path: `${o.out}/${o.name}-scan.png` });
  console.log(`${o.out}/${o.name}-scan.png`);

  await strip(page, 'ripple', { x: ax, y: ay }, () => page.mouse.click(ax, ay));
  await page.waitForTimeout(3200);
  const button = await page.locator(o.scan).first().boundingBox();
  if (button) {
    const at = { x: button.x + button.width / 2, y: button.y + button.height / 2 };
    await strip(page, 'splash', at, () => page.locator(o.scan).first().click());
  }
} finally {
  await remote?.close(); // disconnects; the app keeps running
  await browser.close();
}
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');
if (errors.length) process.exitCode = 1;

async function strip(page, label, at, act) {
  await act();
  const shots = await page.evaluate(
    ({ selector, at, frames, gap }) =>
      new Promise((done) => {
        const c = document.querySelector(selector);
        const box = c.getBoundingClientRect();
        const k = c.width / box.width;
        const out = [];
        const t0 = performance.now();
        // Copy now, encode later: encoding inside the loop would stretch the intervals.
        const grab = () => {
          const crop = document.createElement('canvas');
          crop.width = 600;
          crop.height = 500;
          const g = crop.getContext('2d');
          g.fillStyle = '#0f1318';
          g.fillRect(0, 0, 600, 500);
          g.drawImage(c, (at.x - box.left - 300) * k, (at.y - box.top - 250) * k, 600 * k, 500 * k, 0, 0, 600, 500);
          out.push({ ms: Math.round(performance.now() - t0), crop });
          if (out.length < frames) setTimeout(grab, t0 + out.length * gap - performance.now());
          else done(out.map((x) => ({ ms: x.ms, url: x.crop.toDataURL('image/png') })));
        };
        grab();
      }),
    { selector: o.canvas, at, frames, gap },
  );
  const sheet = await browser.newPage({ viewport: { width: 1808, height: 900 } });
  await sheet.setContent(
    '<body style="margin:0;background:#000;display:grid;grid-template-columns:repeat(3,600px);gap:4px;font:13px sans-serif;color:#fff">' +
      shots
        .map(
          (x) =>
            `<div style="position:relative"><img style="display:block" src="${x.url}"><span style="position:absolute;left:8px;top:6px">+${x.ms} ms</span></div>`,
        )
        .join('') +
      '</body>',
  );
  await sheet.waitForTimeout(100);
  const file = `${o.out}/${o.name}-${label}-sheet.png`;
  await sheet.screenshot({ path: file, fullPage: true });
  await sheet.close();
  console.log(file, shots.map((x) => x.ms).join(' '));
}

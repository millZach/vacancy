// PROTOTYPE helper for variant D: in-between states the step shots can't reach. Ripple strip after a background click,
// the Scan splash and scan in progress, the confirm, the run mid-way, the summary slides, the hand-off into the map
// summary, Replay highlights, and a small window.
// node tools/shot-d.mjs <baseUrl> [groups=ripple,scan,confirm,run,slides,handoff,small,reduced] [os=win]
import { chromium } from 'playwright';

const [base = 'http://localhost:5204/', gs = 'ripple,scan,confirm,run,slides,handoff,small,reduced', os = 'win'] = process.argv.slice(2);
const groups = new Set(gs.split(','));
const browser = await chromium.launch({ channel: 'msedge' });
const errors = [];
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => {
  if (m.type() === 'error') errors.push(m.text());
});

async function open(step, wait = 1200) {
  await page.goto(`${base}?variant=D&os=${os}&step=${step}`);
  await page.waitForTimeout(wait);
}
async function shot(name, clip) {
  const f = `shots/D-x-${os}-${name}.png`;
  const buf = await page.screenshot({ path: f, clip });
  console.log(f);
  return buf;
}
/** Frames `gap` ms apart, measured from the first, so slow screenshots don't stretch the strip. Also writes one contact sheet. */
async function strip(name, frames, gap, clip, cols = 3) {
  const t0 = Date.now();
  const bufs = [];
  for (let k = 0; k < frames; k++) {
    const due = t0 + k * gap;
    const wait = due - Date.now();
    if (wait > 0) await page.waitForTimeout(wait);
    const ms = Date.now() - t0;
    bufs.push({ ms, b64: (await shot(`${name}-${k}`, clip)).toString('base64') });
  }
  const sheet = await browser.newPage({ viewport: { width: 1800, height: 900 } });
  await sheet.setContent(
    `<body style="margin:0;background:#000;display:grid;grid-template-columns:repeat(${cols},1fr);gap:4px;font:12px sans-serif;color:#fff">` +
      bufs.map((x) => `<div style="position:relative"><img style="width:100%;display:block" src="data:image/png;base64,${x.b64}"><span style="position:absolute;left:6px;top:4px">+${x.ms} ms</span></div>`).join('') +
      '</body>',
  );
  await sheet.waitForTimeout(100);
  const f = `shots/D-x-${os}-${name}-sheet.png`;
  await sheet.screenshot({ path: f, fullPage: true });
  await sheet.close();
  console.log(f);
}

/**
 * Exact-interval frames of the dot field: copies the canvas inside the page on a timer (page screenshots take
 * ~250 ms here, too slow for a 120 ms strip), then lays the crops out as one sheet.
 */
async function canvasStrip(name, at, frames = 6, gap = 120) {
  await page.mouse.click(at.x, at.y);
  const shots = await page.evaluate(
    ({ at, frames, gap }) =>
      new Promise((done) => {
        const c = document.querySelector('.vd-field');
        const box = c.getBoundingClientRect();
        const k = c.width / box.width;
        const out = [];
        const t0 = performance.now();
        // Copy now, encode later: PNG encoding inside the loop would stretch the intervals.
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
    { at, frames, gap },
  );
  const sheet = await browser.newPage({ viewport: { width: 1800, height: 900 } });
  await sheet.setContent(
    '<body style="margin:0;background:#000;display:grid;grid-template-columns:repeat(3,1fr);gap:4px;font:12px sans-serif;color:#fff">' +
      shots.map((x) => `<div style="position:relative"><img style="width:100%;display:block" src="${x.url}"><span style="position:absolute;left:6px;top:4px">+${x.ms} ms</span></div>`).join('') +
      '</body>',
  );
  await sheet.waitForTimeout(100);
  const f = `shots/D-x-${os}-${name}-sheet.png`;
  await sheet.screenshot({ path: f, fullPage: true });
  await sheet.close();
  console.log(f, shots.map((x) => x.ms).join(' '));
}

if (groups.has('ripple')) {
  await open('scan', 1500);
  const at = { x: 330, y: 560 };
  await canvasStrip('ripple', at);
  await page.waitForTimeout(2600);
  await canvasStrip('ripple-late', at, 6, 300);
  await page.waitForTimeout(2600);
  await page.mouse.click(at.x, at.y);
  await strip('ripple-page', 6, 120, { x: at.x - 300, y: at.y - 250, width: 600, height: 500 });
  // two drops close together, to see them interfere
  await page.waitForTimeout(2600);
  await page.mouse.click(1040, 330);
  await page.waitForTimeout(260);
  await page.mouse.click(1200, 420);
  await page.waitForTimeout(520);
  await shot('ripple-pair', { x: 760, y: 120, width: 640, height: 560 });
  await page.waitForTimeout(2600);
  await shot('ripple-calm');
}

if (groups.has('scan')) {
  await open('scan', 1500);
  await page.click('.va-cta--scan');
  await page.waitForTimeout(200);
  await shot('scan-splash-200ms');
  await page.waitForTimeout(400);
  await shot('scan-splash-600ms');
  await page.waitForTimeout(1300);
  await shot('scanning-1900ms');
  await page.waitForTimeout(3000);
  console.log('after scan, step =', new URL(page.url()).searchParams.get('step'));
}

if (groups.has('confirm')) {
  await open('plan', 1500);
  await page.click('.va-tray-foot .va-cta');
  await page.waitForTimeout(700);
  await shot('confirm');
  await page.click('.va-dialog .va-cta');
  await page.waitForTimeout(4500);
  await shot('run-mid');
}

if (groups.has('run')) {
  await open('run', 13500);
  await shot('run-done');
  await page.click('.va-run-foot .va-cta');
  await page.waitForTimeout(1600);
  await shot('run-to-slides');
}

if (groups.has('slides')) {
  await open('summary', 2200);
  await page.mouse.move(5, 5);
  await shot('slide-1');
  for (const k of [2, 3, 4, 5, 6, 7]) {
    await page.click(`.vc-dotbtn:nth-child(${k})`);
    await page.mouse.move(5, 5);
    await page.waitForTimeout(k === 2 ? 1900 : 1500);
    await shot(`slide-${k}`);
  }
}

if (groups.has('handoff')) {
  await open('summary', 900);
  await page.click('.vc-dotbtn:nth-child(7)');
  await page.mouse.move(5, 5);
  await page.waitForTimeout(1500);
  await page.click('.vd-co-actions .va-cta');
  await page.mouse.move(5, 5);
  await strip('handoff', 5, 230);
  await page.waitForTimeout(1600);
  await shot('handoff-rest');
  // the receipt auto-advances into the map too
  await open('summary', 900);
  await page.click('.vc-dotbtn:nth-child(7)');
  await page.mouse.move(5, 5);
  await page.waitForTimeout(5200);
  await shot('handoff-auto');
  // skip, then replay
  await open('summary', 1500);
  await page.click('.vc-wrap-skip');
  await page.waitForTimeout(300);
  await shot('skip-mid');
  await page.waitForTimeout(2200);
  await shot('skip-rest');
  await page.locator('.va-summary .va-side-scroll').evaluate((el) => (el.scrollTop = el.scrollHeight));
  await page.click('.va-undo button >> nth=0');
  await page.waitForTimeout(1400);
  await shot('rest-undone');
  await page.click('.vd-replay');
  await page.waitForTimeout(200);
  await shot('replay-in');
  await page.click('.vc-dotbtn:nth-child(5)');
  await page.mouse.move(5, 5);
  await page.waitForTimeout(1200);
  await shot('replay-moved-undone');
}

if (groups.has('small')) {
  await page.setViewportSize({ width: 1140, height: 830 });
  await open('scan', 1500);
  await page.mouse.click(260, 520);
  await page.waitForTimeout(360);
  await shot('small-scan-ripple');
  await open('summary', 2200);
  await page.mouse.move(5, 5);
  await shot('small-slide-1');
  for (const k of [2, 3, 5, 6, 7]) {
    await page.click(`.vc-dotbtn:nth-child(${k})`);
    await page.mouse.move(5, 5);
    await page.waitForTimeout(1500);
    await shot(`small-slide-${k}`);
  }
  await page.click('.vd-co-actions .va-cta');
  await page.mouse.move(5, 5);
  await page.waitForTimeout(2600);
  await shot('small-rest');
}

if (groups.has('reduced')) {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await open('scan', 1200);
  await page.mouse.click(330, 560);
  await page.waitForTimeout(150);
  await shot('reduced-ripple', { x: 30, y: 310, width: 600, height: 500 });
  await open('summary', 7600);
  await shot('reduced-slide-still-1');
  await page.click('.vc-dotbtn:nth-child(7)');
  await page.waitForTimeout(600);
  await page.click('.vd-co-actions .va-cta');
  await page.waitForTimeout(700);
  await shot('reduced-handoff');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
}

await browser.close();
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');

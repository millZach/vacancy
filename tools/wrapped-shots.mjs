// Screenshots the finish in Edge: each highlight slide, the hand-off into the map summary with a frame strip of the
// numbers flying (the page's animations slowed down so every frame is caught), the summary at rest, Replay
// highlights, the run history and a run reopened from it, and reduced motion (slides fading, the hand-off
// cross-fading). It also times frames while the slides play, to check they stay smooth.
//
//   npm run dev                         (the UI on the mock bridge, http://localhost:1420)
//   npm run wrapped-shots -- [options]
//
// Options (all optional):
//   --url <url>          page to open             (default http://localhost:1420/?fixture=windows&run=1500)
//   --out <dir>          where PNGs go            (default shots)
//   --name <prefix>      file name prefix         (default wrapped)
//   --viewport <WxH>     browser viewport         (default 1280x820, the app window's default size)
//   --groups <list>      what to take             (default slides,handoff,replay,history,reduced,fps)
//   --timeout <ms>       how long a scan, or a run, may take (default 600000: a real drive)
//   --cdp <url>          drive an already running page over CDP instead of opening --url, e.g. the installed app
//                        started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333 and
//                        --cdp http://127.0.0.1:9333 (the window keeps its own size; --viewport is ignored)
//   --run                over CDP, scan and free the plan the app proposes, then play its finish. The app deletes
//                        for real, so only ever pass this to an app in fixture mode (VACANCY_FIXTURE_ROOT).
//                        Without it, over CDP the tool starts from the run on screen ("See what changed"), or else
//                        the newest run in the history. Against the mock bridge the plan is always freed.
//
// Writes <name>-slide-<n>-<id>.png for each slide, -handoff-<k>.png frames and -handoff-sheet.png, -rest.png,
// -replay-in.png and -replay-slide-1.png, -history.png, -past.png and -past-replay.png, -reduced-*.png, and
// -fps.json with frame times. Fails if the page logs an error.
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?fixture=windows&run=1500' },
    out: { type: 'string', default: 'shots' },
    name: { type: 'string', default: 'wrapped' },
    viewport: { type: 'string', default: '1280x820' },
    groups: { type: 'string', default: 'slides,handoff,replay,history,reduced,fps' },
    timeout: { type: 'string', default: '600000' },
    cdp: { type: 'string' },
    run: { type: 'boolean', default: false },
  },
});
const groups = new Set(o.groups.split(','));
const [vw, vh] = o.viewport.split('x').map(Number);
await mkdir(o.out, { recursive: true });
const file = (step) => join(o.out, `${o.name}-${step}.png`);
const shot = async (page, step) => {
  const buf = await page.screenshot({ path: file(step) });
  console.log(file(step));
  return buf;
};
const wait = (page, ms) => page.waitForTimeout(ms);
/** Moves the pointer off the slides, so it never holds one. */
const away = (page) => page.mouse.move(4, 4);

const remote = o.cdp ? await chromium.connectOverCDP(o.cdp) : null;
const browser = remote ? null : await chromium.launch({ channel: 'msedge' });
const errors = [];
try {
  const page = remote ? remote.contexts()[0].pages()[0] : await browser.newPage({ viewport: { width: vw, height: vh } });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Animation.enable');
  /** Slows every animation on the page (CSS and Web Animations) by `rate`, so a strip catches each frame. */
  const speed = (rate) => cdp.send('Animation.setPlaybackRate', { playbackRate: rate });
  if (!remote) await page.goto(o.url);

  const slides = page.getByRole('region', { name: 'Highlights of this run' });
  const summary = page.getByRole('complementary', { name: 'What Vacancy did' });

  // ---------- to the finish ----------
  if (!remote || o.run) {
    // From the scan screen, or from a map the app already shows.
    const scan = page.locator('button.cta', { hasText: /^\s*Scan / });
    const build = page.getByRole('button', { name: 'Build a plan' });
    await scan.or(build).first().waitFor({ timeout: 30_000 });
    if (await scan.count()) {
      await scan.click();
      await page.locator('.atlas.is-map').waitFor({ timeout: Number(o.timeout) });
      await wait(page, 600);
    }
    await build.click();
    await page.locator('.tray-cta').click();
    await page.getByRole('alertdialog').waitFor({ timeout: 60_000 });
    await page.getByRole('button', { name: /permanently$/ }).click();
  }
  const seeChanges = page.getByRole('button', { name: 'See what changed' });
  if (!remote || o.run || (await seeChanges.count())) {
    await seeChanges.waitFor({ timeout: Number(o.timeout) });
    await wait(page, 600);
    await away(page);
    await seeChanges.click();
  } else {
    // Over CDP with no run on screen: the newest kept run, replayed.
    await page.getByRole('button', { name: 'Past runs' }).click();
    await page.getByRole('menuitem').first().click();
    await summary.waitFor({ timeout: 10_000 });
    await page.getByRole('button', { name: 'Replay highlights' }).click();
  }
  await slides.waitFor({ timeout: 10_000 });
  await away(page);

  // ---------- each slide, held still ----------
  const dots = page.locator('.wrap-nav .dot');
  const count = await dots.count();
  if (groups.has('slides')) {
    await page.getByRole('button', { name: 'Pause the slides' }).click();
    await away(page);
    for (let k = 0; k < count; k++) {
      if (k > 0) await page.keyboard.press('ArrowRight');
      // Long enough for the slide's own motion (the board's tiles, the fob, the bars) to settle.
      await wait(page, k === 0 ? 2600 : 2300);
      const id = await page.locator('.slide').getAttribute('class').then((c) => /slide--(\w+)/.exec(c ?? '')?.[1]);
      await shot(page, `slide-${k + 1}-${id}`);
    }
  } else {
    await dots.nth(count - 1).click();
    await wait(page, 1500);
  }

  // ---------- the hand-off: the checkout's numbers fly into the summary ----------
  if (groups.has('handoff')) {
    const rate = 0.2;
    await speed(rate);
    await page.getByRole('button', { name: 'See it on the map' }).click();
    const frames = [];
    const t0 = Date.now();
    // The flight takes about 1.1 s; slowed down, a frame every 70 ms of animation time.
    for (let k = 0; k < 16; k++) {
      const due = t0 + (k * 70) / rate;
      const left = due - Date.now();
      if (left > 0) await wait(page, left);
      const ms = Math.round((Date.now() - t0) * rate);
      frames.push({ ms, b64: (await shot(page, `handoff-${String(k).padStart(2, '0')}`)).toString('base64') });
    }
    await speed(1);
    await sheet(`handoff-sheet`, frames, 4);
    await summary.waitFor({ timeout: 10_000 });
    await wait(page, 2600);
    await shot(page, 'rest');
  } else {
    await page.keyboard.press('Escape');
    await summary.waitFor({ timeout: 10_000 });
    await wait(page, 2000);
  }

  // ---------- Replay highlights ----------
  if (groups.has('replay')) {
    await page.getByRole('button', { name: 'Replay highlights' }).click();
    await wait(page, 160);
    await shot(page, 'replay-in');
    await away(page);
    await wait(page, 2600);
    await shot(page, 'replay-slide-1');
    await page.keyboard.press('Escape');
    await summary.waitFor({ timeout: 10_000 });
    await wait(page, 1500);
  }

  // ---------- frame times while the slides play ----------
  if (groups.has('fps')) {
    await page.getByRole('button', { name: 'Replay highlights' }).click();
    await slides.waitFor({ timeout: 10_000 });
    await away(page);
    // Through the first slides' own motion and two slide changes.
    const stats = await page.evaluate(
      (ms) =>
        new Promise((done) => {
          const times = [];
          let last = performance.now();
          const end = last + ms;
          const tick = (now) => {
            times.push(now - last);
            last = now;
            if (now < end) requestAnimationFrame(tick);
            else {
              const sorted = [...times].sort((a, b) => a - b);
              const at = (p) => sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
              done({
                seconds: ms / 1000,
                frames: times.length,
                fps: Math.round((times.length / ms) * 1000 * 10) / 10,
                meanMs: Math.round((times.reduce((s, t) => s + t, 0) / times.length) * 100) / 100,
                p95Ms: Math.round(at(0.95) * 100) / 100,
                p99Ms: Math.round(at(0.99) * 100) / 100,
                maxMs: Math.round(sorted.at(-1) * 100) / 100,
                over20Ms: times.filter((t) => t > 20).length,
                over34Ms: times.filter((t) => t > 34).length,
              });
            }
          };
          requestAnimationFrame((now) => {
            last = now;
            requestAnimationFrame(tick);
          });
        }),
      15_000,
    );
    console.log('Frame times while the slides play:', JSON.stringify(stats));
    await writeFile(join(o.out, `${o.name}-fps.json`), `${JSON.stringify(stats, null, 2)}\n`);
    await page.keyboard.press('Escape');
    await summary.waitFor({ timeout: 10_000 });
    await wait(page, 1500);
  }

  // ---------- the run history, and a run reopened from it ----------
  if (groups.has('history')) {
    await page.getByRole('button', { name: 'Past runs' }).click();
    await page.getByRole('menu', { name: 'Past runs' }).waitFor();
    await page.getByRole('menuitem').first().waitFor({ timeout: 10_000 });
    await wait(page, 400);
    await shot(page, 'history');
    const rows = page.getByRole('menuitem');
    await rows.nth((await rows.count()) > 1 ? 1 : 0).click();
    await summary.waitFor({ timeout: 10_000 });
    await away(page);
    await wait(page, 2600);
    await shot(page, 'past');
    await page.getByRole('button', { name: 'Replay highlights' }).click();
    await away(page);
    await wait(page, 2600);
    await shot(page, 'past-replay');
    await page.keyboard.press('Escape');
    await summary.waitFor({ timeout: 10_000 });
    await wait(page, 800);
    await page.getByRole('button', { name: 'Close' }).click();
    await wait(page, 800);
  }

  // ---------- reduced motion: slides fade, the hand-off cross-fades ----------
  if (groups.has('reduced')) {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    const back = page.getByRole('button', { name: 'Replay highlights' });
    if (!(await back.count())) {
      await page.getByRole('button', { name: 'Past runs' }).click();
      await page.getByRole('menuitem').first().click();
      await summary.waitFor({ timeout: 10_000 });
    }
    await page.getByRole('button', { name: 'Replay highlights' }).click();
    await slides.waitFor({ timeout: 10_000 });
    await away(page);
    await wait(page, 1200);
    await shot(page, 'reduced-slide-1');
    // Half way through a slide change: the next slide fades in where it will rest, without moving.
    await speed(0.1);
    await page.keyboard.press('ArrowRight');
    await wait(page, 4200);
    await shot(page, 'reduced-slide-change');
    await speed(1);
    await wait(page, 1200);
    await shot(page, 'reduced-slide-2');
    await dots.nth(count - 1).click();
    await away(page);
    await wait(page, 1200);
    const rate = 0.2;
    await speed(rate);
    await page.getByRole('button', { name: 'See it on the map' }).click();
    const frames = [];
    const t0 = Date.now();
    for (let k = 0; k < 8; k++) {
      const due = t0 + (k * 80) / rate;
      const left = due - Date.now();
      if (left > 0) await wait(page, left);
      frames.push({ ms: Math.round((Date.now() - t0) * rate), b64: (await shot(page, `reduced-handoff-${k}`)).toString('base64') });
    }
    await speed(1);
    await sheet('reduced-handoff-sheet', frames, 4);
    await summary.waitFor({ timeout: 10_000 });
    await wait(page, 1000);
    await shot(page, 'reduced-rest');
    await page.emulateMedia({ reducedMotion: 'no-preference' });
  }

  /** Lays frames out as one contact sheet, each marked with its time into the animation. */
  async function sheet(name, frames, cols) {
    // The app's webview can't open pages; over CDP the frames stay as they are.
    if (!browser) return;
    const p = await browser.newPage();
    await p.setViewportSize({ width: 1800, height: 900 });
    await p.setContent(
      `<body style="margin:0;background:#000;display:grid;grid-template-columns:repeat(${cols},1fr);gap:4px;font:13px sans-serif;color:#fff">` +
        frames
          .map((x) => `<div style="position:relative"><img style="width:100%;display:block" src="data:image/png;base64,${x.b64}"><span style="position:absolute;left:6px;top:4px;background:#000a;padding:1px 4px">+${x.ms} ms</span></div>`)
          .join('') +
        '</body>',
    );
    await p.waitForTimeout(200);
    await p.screenshot({ path: file(name), fullPage: true });
    await p.close();
    console.log(file(name));
  }
} finally {
  await browser?.close();
}
if (errors.length) {
  console.error(`The page logged errors:\n${errors.join('\n')}`);
  process.exit(1);
}
console.log(`Wrote ${o.out}/${o.name}-*`);
// Over CDP the open connection would keep Node alive; the app itself is left running.
process.exit(0);

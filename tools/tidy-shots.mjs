// Screenshots Tidy up in Edge: the plan tray's Tidy up group, a folder's drawer with its before and after trees, the
// confirm dialog with the move count, the run while it moves files and when it is done, the Moved slide, the
// summary's Reorganized section and an Undo there, Past runs with Undo, a run reopened from Past runs, and (with
// --mac) the macOS fixture, which has no Tidy up.
//
//   npm run dev                       (the UI on the mock bridge, http://localhost:1420)
//   node tools/tidy-shots.mjs -- [options]
//
// Options (all optional):
//   --url <url>        page to open               (default http://localhost:1420/?fixture=windows&run=6000)
//   --mac <url>        also open this macOS page and check it shows no Tidy up
//                      (e.g. http://localhost:1420/?fixture=macos)
//   --out <dir>        where PNGs go              (default shots)
//   --name <prefix>    file name prefix           (default tidy)
//   --viewport <WxH>   browser viewport           (default 1280x820)
//   --phase <which>    all (default), run (up to the summary, no Undo), or undo (Past runs: Undo every folder of
//                      the newest run, then reopen it). run then undo, with the app restarted between, proves Undo
//                      from the journal after a restart.
//   --cdp <url>        drive the installed app over CDP (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=
//                      --remote-debugging-port=9333, then --cdp http://127.0.0.1:9333)
//   --run              over CDP, approve and run the plan. The app deletes and moves for real, so only ever pass
//                      this to an app in fixture mode (VACANCY_FIXTURE_ROOT).
//   --hold <path>      hold this file open (no sharing) from the scan until the run ends: its move is skipped
//   --touch <path>     append to this file once the scan is done: it changed since Vacancy looked
//   --timeout <ms>     how long a scan, a run or an Undo may take (default 600000)
//
// Fails if the page logs an error, or if the macOS page shows Tidy up.
import { spawn } from 'node:child_process';
import { appendFile, mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    url: { type: 'string', default: 'http://localhost:1420/?fixture=windows&run=6000' },
    mac: { type: 'string' },
    out: { type: 'string', default: 'shots' },
    name: { type: 'string', default: 'tidy' },
    viewport: { type: 'string', default: '1280x820' },
    phase: { type: 'string', default: 'all' },
    cdp: { type: 'string' },
    run: { type: 'boolean', default: false },
    hold: { type: 'string' },
    touch: { type: 'string' },
    timeout: { type: 'string', default: '600000' },
  },
  allowPositionals: true,
});
const [vw, vh] = o.viewport.split('x').map(Number);
const timeout = Number(o.timeout);
await mkdir(o.out, { recursive: true });
const shot = (page, step) => page.screenshot({ path: join(o.out, `${o.name}-${step}.png`) });
const settle = (page, ms = 700) => page.waitForTimeout(ms);
const notes = { steps: [] };
const note = (step, data) => {
  notes.steps.push({ step, at: new Date().toISOString(), ...data });
  console.log(step, JSON.stringify(data));
};

const remote = o.cdp ? await chromium.connectOverCDP(o.cdp) : null;
const browser = remote ? null : await chromium.launch({ channel: 'msedge' });
const errors = [];
let holder = null;

/** The newest Past runs entry's Undo buttons, pressed one after another; then the run reopened. */
async function undoFromHistory(page, prefix) {
  await page.getByRole('button', { name: 'Past runs' }).click();
  await page.locator('.history-pop .history-row').first().waitFor({ timeout: 30_000 });
  await settle(page, 500);
  await shot(page, `${prefix}-history`);
  const first = page.locator('.history-run').first();
  const listed = await first.locator('.history-undo').allInnerTexts();
  note('history', { folders: listed });
  for (;;) {
    const undo = first.getByRole('menuitem', { name: /^Undo / }).first();
    if (!(await undo.count())) break;
    const label = await undo.getAttribute('aria-label');
    await undo.click();
    await first.getByRole('menuitem', { name: label, exact: true }).waitFor({ state: 'detached', timeout });
    await first.locator('[role="menuitem"][disabled]').first().waitFor({ state: 'detached', timeout });
    note('history-undo', { label });
    await settle(page, 300);
  }
  await shot(page, `${prefix}-history-undone`);
  note('history-after', { folders: await first.locator('.history-undo').allInnerTexts() });
  await first.locator('.history-row').click();
  await page.locator('.tray.summary').waitFor({ timeout: 30_000 });
  await settle(page, 2600);
  const recap = page.locator('.reorg-recap');
  if (await recap.count()) await recap.scrollIntoViewIfNeeded();
  await settle(page, 300);
  await shot(page, `${prefix}-history-reopen`);
  note('reopen', { recap: (await recap.count()) ? await recap.innerText() : null });
}

try {
  const page = remote ? remote.contexts()[0].pages()[0] : await browser.newPage({ viewport: { width: vw, height: vh } });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  if (!remote) await page.goto(o.url);

  if (o.phase === 'undo') {
    await page.getByRole('button', { name: 'Past runs' }).waitFor({ timeout: 30_000 });
    await undoFromHistory(page, '20');
  } else {
    const scan = page.locator('button.cta', { hasText: /^\s*Scan / });
    await scan.waitFor({ timeout: 30_000 });
    await scan.click();
    await page.locator('.atlas.is-map').waitFor({ timeout });
    await settle(page, 1200);
    if (o.touch) {
      await appendFile(o.touch, ' changed after the scan');
      note('touched', { path: o.touch });
    }
    if (o.hold) {
      const path = o.hold.replaceAll("'", "''");
      holder = spawn('powershell.exe', ['-NoProfile', '-Command', `$f=[IO.File]::Open('${path}','Open','Read','None'); Start-Sleep -Seconds 900; $f.Close()`], { stdio: 'ignore' });
      await settle(page, 1500);
      note('holding', { path: o.hold });
    }

    await page.getByRole('button', { name: 'Build a plan' }).click();
    await page.locator('.tray').waitFor({ timeout: 10_000 });
    const group = page.getByRole('region', { name: 'Tidy up' }).or(page.locator('section[aria-label="Tidy up"]'));
    await group.first().waitFor({ timeout: 10_000 });
    await group.first().scrollIntoViewIfNeeded();
    await settle(page);
    await shot(page, '1-tray');

    const rows = await page.getByRole('checkbox', { name: /^Include Sort / }).all();
    const folders = [];
    for (const box of rows) {
      folders.push(await box.getAttribute('aria-label'));
      await box.click();
    }
    note('ticked', { folders });
    await page.getByRole('button', { name: 'Sort Downloads' }).first().click();
    await page.getByRole('dialog', { name: 'Sort Downloads' }).waitFor({ timeout: 10_000 });
    await settle(page, 600);
    await shot(page, '2-drawer');
    note('drawer', { text: await page.getByRole('dialog', { name: 'Sort Downloads' }).innerText() });
    await page.keyboard.press('Escape');
    await page.locator('.tray-foot').scrollIntoViewIfNeeded();
    await settle(page, 300);
    await shot(page, '3-tray-picked');

    if (!remote || o.run) {
      await page.locator('.tray-cta').click();
      const dialog = page.getByRole('alertdialog');
      await dialog.waitFor({ timeout: 60_000 });
      await settle(page, 500);
      await shot(page, '4-confirm');
      note('confirm', { text: await dialog.innerText() });
      await dialog.locator('button.cta').click();
      await page.locator('.tray.run').waitFor({ timeout: 10_000 });
      // While a sort runs: its row counts the files it has handled.
      const moving = page.locator('.run-step.is-running', { hasText: /^Sort / });
      try {
        await moving.first().waitFor({ timeout });
        await settle(page, 350);
        await shot(page, '5-run-moving');
      } catch {
        note('run-moving', { missed: true });
      }
      const seeChanges = page.getByRole('button', { name: 'See what changed' });
      await seeChanges.waitFor({ timeout });
      if (holder) {
        holder.kill();
        holder = null;
      }
      await settle(page, 1200);
      await shot(page, '6-run-done');
      note('run-done', { panel: await page.locator('.tray.run').innerText() });

      await seeChanges.click();
      const movedDot = page.getByRole('button', { name: /Moved folders$/ });
      if (await movedDot.count()) {
        await movedDot.click();
        await page.locator('.slide--moved').waitFor({ timeout: 10_000 });
        await page.getByRole('button', { name: 'Pause the slides' }).click();
        await settle(page, 900);
        await shot(page, '7-moved-slide');
        note('moved-slide', { text: await page.locator('.slide--moved').innerText() });
      }
      await page.getByRole('button', { name: 'Skip to recap' }).click();
      await page.locator('.tray.summary').waitFor({ timeout: 10_000 });
      await settle(page, 2800);
      const recap = page.locator('.reorg-recap');
      await recap.scrollIntoViewIfNeeded();
      await settle(page, 300);
      await shot(page, '8-summary');
      note('summary', { panel: await page.locator('.tray.summary').innerText() });

      if (o.phase === 'all') {
        await recap.getByRole('button', { name: 'Undo Downloads' }).click();
        await recap.locator('.undo-result').first().waitFor({ timeout });
        await settle(page, 400);
        await recap.locator('.undo-result').first().scrollIntoViewIfNeeded();
        await shot(page, '9-undo-recap');
        note('undo-recap', { text: await recap.innerText() });
        await undoFromHistory(page, '10');
      }
    }
  }

  if (o.mac) {
    const mac = await browser.newPage({ viewport: { width: vw, height: vh } });
    mac.on('pageerror', (e) => errors.push(String(e)));
    await mac.goto(o.mac);
    await mac.locator('button.cta', { hasText: /^\s*Scan / }).click();
    await mac.locator('.atlas.is-map').waitFor({ timeout });
    await mac.getByRole('button', { name: 'Build a plan' }).click();
    await mac.locator('.tray').waitFor({ timeout: 10_000 });
    await settle(mac, 900);
    await shot(mac, '30-mac-tray');
    const tidy = await mac.locator('section[aria-label="Tidy up"]').count();
    note('mac', { tidyGroups: tidy, sortRows: await mac.getByRole('checkbox', { name: /^Include Sort / }).count() });
    if (tidy > 0) errors.push('The macOS fixture shows Tidy up.');
  }
} finally {
  holder?.kill();
  await writeFile(join(o.out, `${o.name}-notes.json`), JSON.stringify(notes, null, 2));
  await browser?.close();
}
if (errors.length) {
  console.error(`The page logged errors:\n${errors.join('\n')}`);
  process.exit(1);
}
console.log(`Wrote ${o.out}/${o.name}-*.png`);
process.exit(0);

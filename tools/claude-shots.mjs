// Screenshots the Claude panel (ADR 0008, prototype/ai-advisor variant B) in Edge, on the mock bridge's Claude, which
// never calls Claude: first beside the scan screen (prototype/claude-entry, variant A), folded and off, opened from
// the header pill, on with its job chips and chat waiting for a scan, the model and reasoning picker, privacy mode off
// before a scan, then a chat about the whole scan with "Answered by" (and a model change that applies to the next
// answer only) and a row's Ask Claude still focusing the chat; then on the map: off and folded to its rail (the map
// gets the room), opened with the privacy notice and what
// always stays hidden, turning on, on, each job's answer
// (Explain big folders with its hover card, a folder chat with a follow-up and a typed question, Review my plan, a
// row's Ask Claude, Let Claude pick's changes with Keep mine and Use Claude's plan, Suggest Tidy up layouts with
// Claude's layout picked and its drawer), what Vacancy left out, the privacy confirm and an answer with privacy mode
// off (ending the conversation so far, which the log says), Claude's marks in the confirm dialog, the summary and
// Past runs, then each other state on a fresh page (not found, not signed in, a sign-in check that timed out or
// failed, slow with Cancel, cancelled, an error and a timeout in Claude's words, off again with Claude's items still
// in the plan and the panel folded again, a pick set aside because the plan changed while Claude picked, and one
// settled by a later edit, and privacy mode off with reads unavailable, or with a scan whose rules are too many for
// Claude Code's settings, each saying why; the check of Claude Code before privacy mode off lets Claude read, running
// and failed with Check again), a macOS pass (no Tidy up
// layouts) and a 1024x700 pass.
//
//   npm run dev                       (the UI on the mock bridge, http://localhost:1420)
//   npm run claude-shots -- [options]
//
// Options (all optional):
//   --base <url>       the dev server              (default http://localhost:1420/)
//   --out <dir>        where PNGs go               (default shots/claude)
//   --name <prefix>    file name prefix            (default claude)
//   --viewport <WxH>   browser viewport            (default 1280x800)
//   --only <which>     all (default), entry, win, states, mac or small
//
// Fails on a page error, or a screen that doesn't say what it should.
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({
  options: {
    base: { type: 'string', default: 'http://localhost:1420/' },
    out: { type: 'string', default: 'shots/claude' },
    name: { type: 'string', default: 'claude' },
    viewport: { type: 'string', default: '1280x800' },
    only: { type: 'string', default: 'all' },
  },
  allowPositionals: true,
});
const [vw, vh] = o.viewport.split('x').map(Number);
await mkdir(o.out, { recursive: true });
const browser = await chromium.launch({ channel: 'msedge' });
const errors = [];
const notes = [];
const settle = (page, ms = 500) => page.waitForTimeout(ms);
const want = (which) => o.only === 'all' || o.only === which;

function check(ok, what) {
  if (!ok) throw new Error(`check failed: ${what}`);
  notes.push({ check: what });
}

async function open(query, viewport = { width: vw, height: vh }, { scan = true } = {}) {
  const page = await browser.newPage({ viewport });
  page.on('pageerror', (e) => errors.push(`${query}: ${String(e)}`));
  page.on('console', (m) => m.type() === 'error' && errors.push(`${query}: ${m.text()}`));
  await page.goto(`${o.base}?${query}`);
  const button = page.locator('button.cta', { hasText: /^\s*Scan / });
  await button.waitFor({ timeout: 30_000 });
  if (scan) await scanNow(page);
  else await settle(page, 900);
  return page;
}

/** Presses Scan on the plate and waits for the map. */
async function scanNow(page) {
  await page.locator('button.cta', { hasText: /^\s*Scan / }).click();
  await page.locator('.atlas.is-map').waitFor({ timeout: 60_000 });
  await settle(page, 900);
}

const shot = (page, step) => page.screenshot({ path: join(o.out, `${o.name}-${step}.png`) });
const panel = (page) => page.getByRole('complementary', { name: 'Claude' });
const has = async (loc) => (await loc.count()) > 0;
/** Scrolls the panel's log so `loc` is in view. */
const into = async (loc) => {
  await loc.scrollIntoViewIfNeeded();
  await settle(loc.page(), 400);
};
/** Presses a job chip and waits for its answer. */
async function job(page, name, kind, timeout = 20_000) {
  const before = await panel(page).locator(`.cp-entry[data-job="${kind}"]`).count();
  await panel(page).getByRole('button', { name, exact: true }).click();
  const entry = panel(page).locator(`.cp-entry[data-job="${kind}"]`).nth(before);
  await entry.locator('.cp-msg').waitFor({ timeout });
  await settle(page, 600);
  return entry;
}

// ---------------------------------------------------------------- Windows: every job, then the marks after a run
async function windows() {
  const page = await open('fixture=windows&claudeMs=1200&run=1500&dups=1200');
  const p = panel(page);
  check(await has(page.locator('.cp.is-folded')), 'off: the panel is folded to its rail');
  check(await has(p.getByRole('button', { name: 'Show Claude' })), 'off: the rail has a Claude button');
  check(!(await has(p.getByRole('heading', { name: 'Turn on Claude?' }))), 'off: folded, it shows nothing else');
  await shot(page, '00-off-folded');
  await p.getByRole('button', { name: 'Show Claude' }).click();
  await settle(page);
  check(await has(p.getByRole('heading', { name: 'Turn on Claude?' })), 'off: opened, the panel is the turn-on step');
  check(await has(page.getByRole('button', { name: /^Rules only/ })), 'off: the header says Rules only');
  await shot(page, '01-off');
  await p.getByRole('button', { name: 'What always stays hidden' }).click();
  await settle(page);
  check(await has(p.getByText('Password managers and wallets')), 'off: the hidden places are listed');
  await shot(page, '02-off-hidden');
  await p.getByRole('button', { name: 'Turn on Claude' }).click();
  await p.getByText('Checking that Claude Code answers…').waitFor({ timeout: 2_000 });
  await shot(page, '03-turning-on');
  await p.getByRole('button', { name: 'Turn off Claude' }).waitFor({ timeout: 10_000 });
  await settle(page);
  check(await has(p.getByRole('switch', { name: 'Privacy mode', checked: true })), 'on: privacy mode is on by default');
  await shot(page, '04-on');

  // The plan: Review my plan (working first), a row's Ask Claude.
  await page.getByRole('button', { name: 'Build a plan' }).click();
  const tray = page.getByRole('complementary', { name: 'Plan' });
  await tray.waitFor();
  await settle(page);
  await p.getByRole('button', { name: 'Review my plan', exact: true }).click();
  await p.getByText('Claude is reviewing your plan…').waitFor({ timeout: 2_000 });
  await shot(page, '05-working');
  const review = p.locator('.cp-entry[data-job="reviewPlan"]').first();
  await review.locator('.cp-msg').waitFor({ timeout: 20_000 });
  await settle(page, 700);
  check(await has(review.locator('.cl-flag')), 'review: flags risky items');
  check(await has(review.locator('.cl-basis')), 'review: the answer says what it was based on');
  await into(review.locator('.cp-you'));
  await shot(page, '06-review');
  await review.getByRole('button', { name: 'Keep it' }).first().click();
  await settle(page, 300);
  await tray.locator('.tray-row', { hasText: 'Rust build output' }).getByRole('button', { name: 'Ask Claude' }).click();
  await p.locator('.cp-entry[data-chat="Rust build output in 12 projects"] .cl-first').waitFor({ timeout: 20_000 });
  await settle(page, 700);
  await shot(page, '07-row-chat');

  // Let Claude pick: its changes go in at once; Keep mine puts the plan back; again, then Use Claude's plan.
  const planBefore = await tray.locator('.tray-total-num').innerText();
  const pick = await job(page, 'Let Claude pick', 'pickPlan');
  check(await has(pick.getByRole('button', { name: 'Keep mine' })), 'pick: Keep mine is offered');
  check(await has(tray.getByText('Android emulator images')), 'pick: Claude’s own items are in the tray at once');
  check(!(await tray.getByRole('checkbox', { name: 'Include Android emulator images' }).isChecked()), 'pick: its own items start unticked');
  check(await has(pick.getByText('Left out by Vacancy').first()), 'pick: what the Guard refused is shown with why');
  await into(pick.locator('.cl-diff-total'));
  await shot(page, '08-pick-changes');
  await into(pick.locator('.cl-diff-actions'));
  await shot(page, '09-pick-left-out');
  await pick.getByRole('button', { name: 'Keep mine' }).click();
  await pick.getByText('You kept your plan.', { exact: false }).waitFor({ timeout: 10_000 });
  await settle(page);
  check((await tray.locator('.tray-total-num').innerText()) === planBefore, 'keep mine: the plan is as it was');
  check(!(await has(tray.getByText('Installers in Public Downloads'))), 'keep mine: Claude’s own items are out again');
  await shot(page, '10-keep-mine');
  const pick2 = await job(page, 'Let Claude pick', 'pickPlan');
  await pick2.getByRole('button', { name: 'Use Claude’s plan' }).click();
  await pick2.getByText('You’re using my plan.', { exact: false }).waitFor();
  await tray.locator('.tray-row.is-ai').first().scrollIntoViewIfNeeded();
  await settle(page);
  check(await has(tray.locator('.tray-row.is-ai .cl-note')), 'tray: Claude’s items carry its note');
  await shot(page, '11-use-claudes-plan');

  // Explain big folders: the answer, a ✳ tile's hover card, then a folder chat with a follow-up and a typed question.
  await page.getByRole('button', { name: 'Hide the plan' }).click();
  const explain = await job(page, 'Explain big folders', 'explainFolders');
  check(await has(explain.getByText('in the tray unticked').first()), 'explain: suggestions are added unticked');
  await into(explain.locator('.cp-you'));
  await shot(page, '12-explain');
  const dropped = explain.locator('.cl-dropped');
  if (await has(dropped)) {
    await into(dropped);
    await shot(page, '13-explain-left-out');
  }
  const tile = page.locator('.map-tile.is-explained').first();
  check(await has(tile), 'explain: explained folders have a ✳ on the map');
  await tile.hover();
  await page.locator('.map-tip .cl-tip').waitFor({ timeout: 5_000 });
  await settle(page, 300);
  await shot(page, '14-explain-hover');
  await tile.click();
  const chat = p.locator('.cp-entry[data-chat]').last();
  await chat.locator('.cl-first').waitFor({ timeout: 20_000 });
  await settle(page);
  await shot(page, '15-folder-chat');
  await chat.locator('.cl-chips .cl-chip').first().click();
  await chat.locator('.cl-msg:not(.is-you)').first().waitFor({ timeout: 20_000 });
  const input = p.locator('.cl-compose input');
  await input.fill('Can you look inside it?');
  await input.press('Enter');
  await page.waitForFunction(() => {
    const c = [...document.querySelectorAll('.cp-entry[data-chat]')].at(-1);
    return c && c.querySelectorAll('.cl-msg:not(.is-you)').length >= 2;
  }, null, { timeout: 20_000 });
  await settle(page, 700);
  await shot(page, '16-chat-follow-up');
  await page.getByRole('button', { name: 'Build a plan' }).click();
  await tray.waitFor();
  await settle(page, 300);

  // Suggest Tidy up layouts: Claude's layout for Downloads, and its drawer.
  const tidy = await job(page, 'Suggest Tidy up layouts', 'tidyLayout');
  await into(tidy.locator('.cp-tidy').first());
  await shot(page, '17-tidy-layouts');
  await tidy.getByRole('radiogroup', { name: 'Layout for Downloads' }).getByRole('radio', { name: /Claude’s/ }).click();
  await page.waitForFunction(() => document.querySelector('.tidy-row.is-ai'), null, { timeout: 10_000 });
  await settle(page);
  await tray.getByRole('checkbox', { name: 'Include Sort Downloads' }).click();
  await tray.locator('.tidy-row.is-ai').scrollIntoViewIfNeeded();
  await settle(page);
  await shot(page, '18-tidy-chosen');
  await tidy.getByRole('button', { name: 'See the trees' }).first().click();
  await page.getByRole('dialog', { name: 'Sort Downloads' }).waitFor();
  await settle(page);
  check(await has(page.getByRole('dialog', { name: 'Sort Downloads' }).getByText('Claude’s layout:')), 'tidy: the drawer says it is Claude’s layout');
  await shot(page, '19-tidy-drawer');
  await page.keyboard.press('Escape');
  await settle(page, 300);

  // Privacy mode off: the confirm, then an answer that read files.
  await p.getByRole('switch', { name: 'Privacy mode' }).click();
  const confirm = page.getByRole('alertdialog', { name: 'Let Claude read your files?' });
  await confirm.waitFor();
  await settle(page, 300);
  check(await has(confirm.getByText('Keys and sign-ins')), 'privacy confirm: lists what always stays hidden');
  check(
    await has(confirm.getByText('Claude can open and read files on the drive you scanned, but not on your other drives', { exact: false })),
    'privacy confirm: Claude reads the drive you scanned only',
  );
  check(!(await has(confirm.getByText('anywhere', { exact: false }))), 'privacy confirm: never "anywhere"');
  await shot(page, '20-privacy-confirm');
  await confirm.getByRole('button', { name: 'Turn privacy mode off' }).click();
  await settle(page);
  check(await has(p.getByRole('switch', { name: 'Privacy mode', checked: false })), 'privacy: off after the confirm');
  check(await has(p.getByText('Privacy mode is off now, so Vacancy ended the conversation so far.', { exact: false })), 'privacy: changing it ends the conversation, and says so');
  check((await p.locator('.cp-entry').count()) === 0, 'privacy: nothing of the conversation is left');
  await shot(page, '20b-privacy-ended');
  check(await has(page.getByRole('button', { name: /^Claude on · reads files/ })), 'privacy off: the header says so');
  check(
    await has(p.getByText('Off: Claude can read files on the drive you scanned, except hidden places.', { exact: false })),
    'privacy off: the privacy row says the drive you scanned',
  );
  check(await has(p.getByText('I can open files on the drive you scanned', { exact: false })), 'privacy off: the panel says the drive you scanned');
  const review2 = await job(page, 'Review my plan', 'reviewPlan');
  check(await has(review2.locator('.cl-basis.is-open')), 'privacy off: the answer says which files Claude opened');
  check(await has(review2.locator('.cl-basis').getByText('from the drive you scanned', { exact: false })), 'privacy off: the answer says it read the drive you scanned');
  await into(review2.locator('.cl-basis'));
  await shot(page, '21-review-privacy-off');
  await p.getByRole('switch', { name: 'Privacy mode' }).click();
  await settle(page, 300);

  // Claude's marks: the confirm dialog, the run, the summary, Past runs.
  const android = tray.getByRole('checkbox', { name: 'Include Android emulator images' });
  if (!(await android.isChecked())) await android.click();
  await settle(page, 300);
  await tray.locator('.tray-cta').click();
  const dialog = page.getByRole('alertdialog', { name: /^Permanently delete/ });
  await dialog.waitFor({ timeout: 10_000 });
  await settle(page, 400);
  check(await has(dialog.getByText('1 item was suggested by Claude.')), 'confirm: marks Claude’s item');
  check(await has(dialog.getByText('sorted into Claude’s folders', { exact: false })), 'confirm: marks Claude’s layout');
  await shot(page, '22-confirm');
  await dialog.getByRole('button', { name: /permanently$/ }).click();
  await page.getByRole('button', { name: 'See what changed' }).waitFor({ timeout: 60_000 });
  await page.getByRole('button', { name: 'See what changed' }).click();
  const summary = page.getByRole('complementary', { name: 'What Vacancy did' });
  for (let i = 0; i < 12 && !(await summary.isVisible().catch(() => false)); i++) {
    await page.keyboard.press('Escape');
    await settle(page, 400);
  }
  await summary.waitFor({ timeout: 10_000 });
  await settle(page, 2600);
  const sec = summary.getByRole('region', { name: 'Suggested by Claude' });
  check(await has(sec), 'summary: has a Suggested by Claude section');
  await sec.scrollIntoViewIfNeeded();
  await settle(page, 300);
  await shot(page, '23-summary');
  await page.getByRole('button', { name: 'Past runs' }).click();
  await page.getByRole('menu', { name: 'Past runs' }).waitFor();
  await settle(page, 600);
  check(await has(page.locator('.history-tag.is-claude')), 'Past runs: marks the run with Claude’s steps');
  await shot(page, '24-past-runs');
  await page.close();
}

// ---------------------------------------------------------------- the other states, each on a fresh page
async function states() {
  {
    const page = await open('fixture=windows&claude=notfound');
    check(await has(page.locator('.cp.is-folded')), 'not found: folded, like off');
    await panel(page).getByRole('button', { name: 'Show Claude' }).click();
    await settle(page);
    check(await has(panel(page).getByRole('heading', { name: 'Claude Code isn’t on this PC.' })), 'not found');
    await shot(page, '30-not-found');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=signedout');
    check(await has(panel(page).getByRole('heading', { name: 'Sign in to Claude Code first.' })), 'not signed in');
    await shot(page, '31-not-signed-in');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=checktimeout');
    check(await has(panel(page).getByRole('heading', { name: 'Claude Code didn’t answer.' })), 'check timed out: its own state');
    check(await has(panel(page).getByText('within 30 seconds', { exact: false })), 'check timed out: says why');
    check(!(await has(panel(page).getByText('Found · not signed in'))), 'check timed out: never "not signed in"');
    await shot(page, '31b-check-timed-out');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=checkerror');
    check(await has(panel(page).getByRole('heading', { name: 'Vacancy couldn’t check Claude Code.' })), 'check failed: its own state');
    check(await has(panel(page).getByText('Access is denied', { exact: false })), 'check failed: in its own words');
    await shot(page, '31c-check-failed');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=slow');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    await panel(page).getByRole('button', { name: 'Review my plan', exact: true }).click();
    await panel(page).getByText('Still working.').waitFor({ timeout: 5_000 });
    await settle(page, 300);
    check(await has(panel(page).getByText('On · replies are slow')), 'slow: the head says so');
    await shot(page, '32-slow');
    await panel(page).getByRole('button', { name: 'Cancel' }).click();
    await panel(page).getByText('Cancelled. Nothing changed.').waitFor();
    await settle(page, 300);
    await shot(page, '33-cancelled');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=error');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    await panel(page).getByRole('button', { name: 'Review my plan', exact: true }).click();
    await panel(page).getByText('Claude Code stopped with an error.', { exact: false }).waitFor({ timeout: 10_000 });
    await settle(page, 300);
    check(await has(panel(page).getByText('rate_limit_error', { exact: false })), 'error: shows Claude’s message');
    await shot(page, '34-error');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=timeout');
    await panel(page).getByRole('button', { name: 'Explain big folders', exact: true }).click();
    await panel(page).getByText('took too long', { exact: false }).waitFor({ timeout: 10_000 });
    await settle(page, 300);
    await shot(page, '35-timed-out');
    await page.close();
  }
  {
    const page = await open('fixture=windows&claude=on&claudeMs=300');
    await job(page, 'Explain big folders', 'explainFolders');
    await panel(page).getByRole('button', { name: 'Turn off Claude' }).click();
    await page.locator('.cp.is-folded').waitFor();
    check(await has(panel(page).getByRole('button', { name: 'Show Claude' })), 'off again: the panel folds to its rail');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    const tray = page.getByRole('complementary', { name: 'Plan' });
    await tray.locator('.tray-row.is-ai').first().scrollIntoViewIfNeeded();
    await settle(page);
    check(await has(tray.getByText('Android emulator images')), 'off again: Claude’s items stay in the plan');
    check(!(await has(page.locator('.map-tile.is-explained'))), 'off again: no ✳ on the map');
    await shot(page, '36-off-again-folded');
    await panel(page).getByRole('button', { name: 'Show Claude' }).click();
    await panel(page).getByRole('heading', { name: 'Turn on Claude?' }).waitFor();
    await settle(page, 300);
    await shot(page, '37-off-again-opened');
    await page.close();
  }
  {
    // The plan changes while Claude picks: its answer is set aside, then a later answer is settled by an edit.
    const page = await open('fixture=windows&claude=on&claudeMs=1500');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    const tray = page.getByRole('complementary', { name: 'Plan' });
    await tray.waitFor();
    await panel(page).getByRole('button', { name: 'Let Claude pick', exact: true }).click();
    await tray.getByRole('checkbox', { name: 'Include Temp folders' }).click();
    const stale = panel(page).locator('.cp-entry[data-job="pickPlan"]').first();
    await stale.getByText('my own items are out of the tray again', { exact: false }).waitFor({ timeout: 10_000 });
    await settle(page);
    check(!(await has(stale.getByRole('button', { name: 'Keep mine' }))), 'pick set aside: no Keep mine');
    check(!(await tray.getByRole('checkbox', { name: 'Include Temp folders' }).isChecked()), 'pick set aside: the edit stays');
    check(!(await has(tray.getByText('Android emulator images'))), 'pick set aside: Claude’s items are out again');
    await shot(page, '38-pick-set-aside');
    const pick = await job(page, 'Let Claude pick', 'pickPlan');
    await tray.getByRole('checkbox', { name: 'Include Temp folders' }).click();
    await settle(page);
    check(await has(pick.getByText('Your plan changed since, so these changes are settled.', { exact: false })), 'pick settled by an edit');
    check(!(await has(pick.getByRole('button', { name: 'Keep mine' }))), 'pick settled: no Keep mine');
    await into(pick.locator('.cl-diff-settled'));
    await shot(page, '39-pick-settled');
    await page.close();
  }
  {
    // Privacy mode off, but no rule could keep Claude where it may read: the panel says why, and answers are from
    // names, sizes and dates.
    const page = await open('fixture=windows&claude=on&privacy=off&reads=unavailable&claudeMs=300');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    const note = panel(page).getByRole('note');
    check(await has(note.getByText('Claude can’t open files for now', { exact: false })), 'reads unavailable: the panel says why');
    check(await has(panel(page).getByText('Off, but Claude can’t open files for now', { exact: false })), 'reads unavailable: the privacy row says so');
    const review = await job(page, 'Review my plan', 'reviewPlan');
    check(await has(review.getByText('Claude gets names, sizes, counts and dates only', { exact: false })), 'reads unavailable: the answer says why');
    check(!(await has(review.getByText('Claude opened', { exact: false }))), 'reads unavailable: Claude opened nothing');
    await shot(page, '39b-reads-unavailable');
    await page.close();
  }
  {
    // Privacy mode off, but this scan's rules are too many for Claude Code's settings: the answer says why, and from
    // then on the panel's privacy row does too.
    const page = await open('fixture=windows&claude=on&privacy=off&reads=toobig&claudeMs=300');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    check(await has(panel(page).getByText('Off: Claude can read files on the drive you scanned', { exact: false })), 'too big: off until an answer says otherwise');
    const review = await job(page, 'Review my plan', 'reviewPlan');
    check(await has(review.getByText('Claude can’t open files on this scan', { exact: false })), 'too big: the answer says why');
    check(!(await has(review.getByText('Claude opened', { exact: false }))), 'too big: Claude opened nothing');
    check(await has(panel(page).getByText('Off, but Claude can’t open files for now', { exact: false })), 'too big: the privacy row says so');
    check(await has(panel(page).getByRole('note').getByText('Claude can’t open files on this scan', { exact: false })), 'too big: the panel says why');
    check(await has(page.getByRole('button', { name: /^Claude on: show/ })), 'too big: the header no longer says Claude reads files');
    await shot(page, '39c-reads-too-big');
    await page.close();
  }
  {
    // Turning privacy mode off checks this Claude Code first (one small call, once per Claude Code): the privacy row
    // says so while it runs, and Claude reads files once it has passed.
    const page = await open('fixture=windows&claude=on&canary=slow&claudeMs=300');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    await panel(page).getByRole('switch', { name: 'Privacy mode' }).click();
    await page.getByRole('button', { name: 'Turn privacy mode off' }).click();
    const checking = panel(page).getByText('Checking Claude Code…');
    await checking.waitFor({ timeout: 5000 });
    check(await has(panel(page).getByText('Off: Vacancy is checking Claude Code before Claude opens files', { exact: false })), 'canary: the privacy row says it is checking');
    await settle(page, 400);
    await shot(page, '39d-canary-checking');
    await checking.waitFor({ state: 'detached', timeout: 10_000 });
    check(await has(panel(page).getByText('Off: Claude can read files on the drive you scanned', { exact: false })), 'canary: passed, Claude reads files');
    await page.close();
  }
  {
    // A Claude Code that fails the check: privacy mode off can't let Claude open files, the privacy row says why with
    // Check again, and answers are from names, sizes and dates.
    const page = await open('fixture=windows&claude=on&privacy=off&canary=fail&claudeMs=300');
    await page.getByRole('button', { name: 'Build a plan' }).click();
    const note = panel(page).getByRole('note');
    await note.getByRole('button', { name: 'Check again' }).waitFor({ timeout: 10_000 });
    check(await has(note.getByText('couldn’t verify this version of Claude Code', { exact: false })), 'canary failed: the row says why');
    check(await has(panel(page).getByText('Off, but Claude can’t open files for now', { exact: false })), 'canary failed: the privacy row says so');
    const review = await job(page, 'Review my plan', 'reviewPlan');
    check(await has(review.getByText('couldn’t verify this version of Claude Code', { exact: false })), 'canary failed: the answer says why');
    check(!(await has(review.getByText('Claude opened', { exact: false }))), 'canary failed: Claude opened nothing');
    check(await has(page.getByRole('button', { name: /^Claude on: show/ })), 'canary failed: the header doesn’t say Claude reads files');
    await shot(page, '39e-canary-failed');
    await note.getByRole('button', { name: 'Check again' }).click();
    await panel(page).getByText('Checking Claude Code…').waitFor({ timeout: 5000 });
    await panel(page).getByRole('note').getByRole('button', { name: 'Check again' }).waitFor({ timeout: 10_000 });
    check(await has(panel(page).getByRole('note').getByText('couldn’t verify', { exact: false })), 'canary failed: Check again fails again');
    await page.close();
  }
}

// ---------------------------------------------------------------- Before a scan, the picker, the whole-scan chat
// (prototype/claude-entry, variant A): the panel beside the scan screen, off and on; the job chips and the chat wait
// for a scan; the model and reasoning picker; then, after the scan, a chat about the whole scan with "Answered by",
// and Ask on a row still focusing the chat on that row.
async function entry() {
  const page = await open('fixture=windows&claudeMs=800', { width: vw, height: vh }, { scan: false });
  const p = panel(page);
  const note = page.locator('.advisor-note');
  check(await has(page.locator('.atlas.is-scan')), 'before a scan: the scan screen is up');
  check(await has(page.locator('.cp.is-folded')), 'before a scan, off: the panel is folded to its rail beside the scan screen');
  check(await has(note.getByText('Turn it on in the Claude panel on the left', { exact: false })), 'before a scan, off: the plate says Claude is in the panel on the left');
  const pill = page.getByRole('button', { name: /^Rules only: show the Claude panel/ });
  check(await has(pill), 'before a scan: the header pill is a button');
  await shot(page, 'p0-scan-off-folded');
  await pill.click();
  await settle(page);
  check(await has(p.getByRole('heading', { name: 'Turn on Claude?' })), 'before a scan: the pill unfolds the turn-on step');
  await shot(page, 'p1-scan-off-open');
  await p.getByRole('button', { name: 'Turn on Claude' }).click();
  await p.getByRole('button', { name: 'Turn off Claude' }).waitFor({ timeout: 10_000 });
  await settle(page);
  check(await has(p.getByRole('switch', { name: 'Privacy mode', checked: true })), 'before a scan, on: privacy mode is on');
  for (const chip of ['Review my plan', 'Let Claude pick', 'Explain big folders', 'Suggest Tidy up layouts'])
    check(await p.getByRole('button', { name: chip, exact: true }).isDisabled(), `before a scan: ${chip} waits for a scan`);
  check(await has(p.getByText('Scan a drive first — Claude works from the scan.')), 'before a scan: the chips say why');
  check(await p.getByRole('textbox', { name: 'Ask Claude, after the scan' }).isDisabled(), 'before a scan: the chat waits for a scan');
  check(await has(note.getByText('Built-in rules, and Claude in the panel on the left.')), 'before a scan, on: the plate says Claude is on, on the left');
  check((await p.locator('.cp-head p').innerText()) === 'On · through your Claude Code', 'before a scan: the head line with the defaults');
  await shot(page, 'p2-scan-on');

  // The picker: Opus and High, remembered by the core; the head line says it at once.
  await p.getByRole('combobox', { name: 'Model' }).selectOption('opus');
  await p.getByRole('combobox', { name: 'Reasoning' }).selectOption('high');
  await settle(page);
  check((await p.locator('.cp-head p').innerText()) === 'On · Opus · High, through your Claude Code', 'picker: the head line names the alias and the reasoning');
  const models = await p.getByRole('combobox', { name: 'Model' }).locator('option').allInnerTexts();
  check(models.join('|') === 'Your default|Opus|Sonnet|Haiku|Fable', `picker: the models are the aliases (${models.join(', ')})`);
  const efforts = await p.getByRole('combobox', { name: 'Reasoning' }).locator('option').allInnerTexts();
  check(efforts.join('|') === 'Default|Low|Medium|High|Extra high|Max', `picker: the reasoning levels (${efforts.join(', ')})`);
  await shot(page, 'p3-picker');

  // Privacy mode switches before a scan too (it asks first), and back on.
  await p.getByRole('switch', { name: 'Privacy mode' }).click();
  await page.getByRole('button', { name: 'Turn privacy mode off' }).click();
  await p.getByRole('switch', { name: 'Privacy mode', checked: false }).waitFor({ timeout: 5_000 });
  await settle(page);
  check(await has(p.getByText('Off: Claude can read files on the drive you scanned', { exact: false })), 'before a scan: privacy mode goes off');
  await shot(page, 'p4-scan-privacy-off');
  await p.getByRole('switch', { name: 'Privacy mode' }).click();
  await p.getByRole('switch', { name: 'Privacy mode', checked: true }).waitFor({ timeout: 5_000 });
  await settle(page);

  // After the scan: the jobs and the chat work; a question about the whole scan, no row picked first.
  await scanNow(page);
  check(!(await p.getByRole('button', { name: 'Review my plan', exact: true }).isDisabled()), 'after the scan: the chips work');
  const box = p.getByRole('textbox', { name: 'Ask Claude about this scan' });
  check(!(await box.isDisabled()), 'after the scan: the chat sends about the whole scan, no row picked first');
  await shot(page, 'p5-map-on');
  await box.fill('What should I clean up?');
  await box.press('Enter');
  const whole = p.locator('.cp-entry[data-chat="the whole scan"]');
  await whole.locator('.cl-first').waitFor({ timeout: 20_000 });
  await settle(page, 700);
  check(await has(whole.getByText('What should I clean up?')), 'whole scan: the question is in the log');
  check(await has(whole.getByText('The biggest things on', { exact: false })), 'whole scan: Claude answers about the drive');
  check(await has(whole.locator('.cl-answered', { hasText: 'Answered by Opus 5.5 · High' })), 'whole scan: the answer says which model made it');
  check((await p.locator('.cp-head p').innerText()) === 'On · Opus 5.5 · High, through your Claude Code', 'whole scan: the head line names the model once known');
  check(!(await has(p.locator('.cp-about'))), 'whole scan: no row chip on the composer');
  await into(whole.locator('.cp-you'));
  await shot(page, 'p6-whole-scan-chat');

  // Ask on a row still focuses the chat on it, with its chip and ×; the × goes back to the whole scan.
  await page.getByRole('button', { name: 'Build a plan' }).click();
  const tray = page.getByRole('complementary', { name: 'Plan' });
  await tray.waitFor();
  await settle(page);
  await tray.locator('.tray-row', { hasText: 'Rust build output' }).getByRole('button', { name: 'Ask Claude' }).click();
  const row = p.locator('.cp-entry[data-chat="Rust build output in 12 projects"]');
  await row.locator('.cl-first').waitFor({ timeout: 20_000 });
  await settle(page, 700);
  check(await has(p.getByRole('textbox', { name: 'Ask about Rust build output in 12 projects' })), 'row: Ask focuses the chat on the row');
  check(await has(p.locator('.cp-about', { hasText: 'Rust build output in 12 projects' })), 'row: the composer has the row chip');
  check(await has(row.locator('.cl-answered', { hasText: 'Answered by Opus 5.5 · High' })), 'row: the answer says which model made it');
  await shot(page, 'p7-row-chat');
  await p.getByRole('button', { name: 'Stop asking about it' }).click();
  await settle(page, 300);
  const back = p.getByRole('textbox', { name: 'Ask Claude about this scan' });
  check(await has(back), 'row: the × goes back to the whole scan');
  // A change of model applies to the next question.
  await p.getByRole('combobox', { name: 'Model' }).selectOption('sonnet');
  await p.getByRole('combobox', { name: 'Reasoning' }).selectOption('default');
  await settle(page, 300);
  await back.fill('Can I get them back after?');
  await back.press('Enter');
  await page.waitForFunction(() => {
    const c = document.querySelector('.cp-entry[data-chat="the whole scan"]');
    return c && c.querySelectorAll('.cl-answered').length >= 2;
  }, null, { timeout: 20_000 });
  await settle(page, 700);
  const lines = await whole.locator('.cl-answered').allInnerTexts();
  check(lines.at(-1)?.trim() === 'Answered by Sonnet 5.5', `whole scan: the next answer is the new model's (${lines.join(' / ')})`);
  check(lines[0]?.trim() === 'Answered by Opus 5.5 · High', 'whole scan: the earlier answer keeps its own');
  await shot(page, 'p8-whole-scan-follow-up');
  await page.close();
}

// ---------------------------------------------------------------- macOS: the same jobs, no Tidy up layouts
async function mac() {
  const page = await open('fixture=macos&claude=on&claudeMs=400');
  const p = panel(page);
  check(!(await has(p.getByRole('button', { name: 'Suggest Tidy up layouts' }))), 'macOS: no Tidy up layouts job');
  await shot(page, '40-mac-on');
  await job(page, 'Explain big folders', 'explainFolders');
  await shot(page, '41-mac-explain');
  await page.getByRole('button', { name: 'Build a plan' }).click();
  await job(page, 'Review my plan', 'reviewPlan');
  await shot(page, '42-mac-review');
  const pick = await job(page, 'Let Claude pick', 'pickPlan');
  await into(pick.locator('.cl-diff-total'));
  await shot(page, '43-mac-pick');
  await page.close();
}

// ---------------------------------------------------------------- 1024x700: the panel, the map and the tray together
async function small() {
  const page = await open('fixture=windows&claude=on&claudeMs=300', { width: 1024, height: 700 });
  await page.getByRole('button', { name: 'Build a plan' }).click();
  const pick = await job(page, 'Let Claude pick', 'pickPlan');
  await into(pick.locator('.cl-diff-total'));
  await shot(page, '50-small-pick');
  await page.close();
}

try {
  if (want('entry')) await entry();
  if (want('win')) await windows();
  if (want('states')) await states();
  if (want('mac')) await mac();
  if (want('small')) await small();
} catch (e) {
  errors.push(String(e?.stack ?? e));
} finally {
  await browser.close();
  await writeFile(join(o.out, `${o.name}-notes.json`), JSON.stringify({ notes, errors }, null, 2));
}
if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log(`${notes.length} checks passed; shots in ${o.out}`);

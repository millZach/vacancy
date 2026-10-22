// Run only inside the shared heavy-job wrapper, after `prototype:ai-advisor:build`. The preview server is temporary.
// Drives every variant through every job in Edge at 1280×800: turning Claude on (the privacy notice, the hidden
// areas, the check, on), explaining big folders (hover card, chat with follow-ups), Review my plan, a chat about a
// row, Let Claude pick (the diff, then accepting it), Claude's Tidy up layout (the drawer's trees), privacy mode off
// (its confirm, then a richer answer and a hidden folder), the confirm dialog with Claude's line, and the end. Then
// each variant's advisor states (not found, signed out, slow with Cancel, error, turned off again) and a 1024×700
// pass. Fails on any page error, bridge call, page overflow, a panel that leaves the window, or a missing line.
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../..', import.meta.url));
const output = process.argv[2] ?? fileURLToPath(new URL('./shots', import.meta.url));
const base = 'http://127.0.0.1:1438/';
await mkdir(output, { recursive: true });
const server = spawn(process.execPath, [fileURLToPath(new URL('../../node_modules/vite/bin/vite.js', import.meta.url)), 'preview', '--config', 'prototypes/ai-advisor/vite.config.ts'], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let log = '';
server.stdout.on('data', (x) => { log += x; });
server.stderr.on('data', (x) => { log += x; });

let browser;
const errors = [];
const results = [];
const shots = [];
const fail = (m) => { throw new Error(m); };
const DOCKER = 'fx-dev-3';
const EDGE = 'fx-edge-profile';

try {
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) fail(`Server exited: ${log}`);
    try { if ((await fetch(base)).ok) break; } catch { /* starting */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  browser = await chromium.launch({ channel: 'msedge', headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1, reducedMotion: 'reduce' });
  const page = await context.newPage();
  page.setDefaultTimeout(9000);
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });

  const fits = async (where) => {
    const bad = await page.evaluate(() => {
      const out = [];
      const d = document.documentElement;
      if (d.scrollWidth > innerWidth + 1 || d.scrollHeight > innerHeight + 1) out.push('the page overflows the window');
      for (const sel of ['.tray', '.cp', '.rv', '.reorg-drawer', '.dialog', '.advisor-dialog', '.cl-card', '.atlas-toolbar', '.shell-head', '.tray-foot', '.rv-foot', '.cp-foot']) {
        for (const el of document.querySelectorAll(sel)) {
          const r = el.getBoundingClientRect();
          if (r.width === 0) continue;
          if (r.left < -1 || r.top < -1 || r.right > innerWidth + 1 || r.bottom > innerHeight + 1) out.push(`${sel} leaves the window`);
        }
      }
      // Toolbar contents must not spill out of it.
      const tb = document.querySelector('.atlas-toolbar');
      if (tb && tb.scrollWidth > tb.clientWidth + 2) out.push('the map toolbar is clipped');
      const head = document.querySelector('.shell-head');
      if (head && head.scrollWidth > head.clientWidth + 2) out.push('the header is clipped');
      return out;
    });
    if (bad.length) fail(`${where}: ${bad.join('; ')}`);
  };
  const shot = async (name, where = name) => {
    await page.waitForTimeout(350);
    await fits(where);
    const path = `${output}\\${name}.png`;
    await page.screenshot({ path });
    shots.push(path);
  };
  const has = async (text, where) => {
    try { await page.getByText(text).first().waitFor({ timeout: 9000 }); } catch { fail(`${where}: missing “${text}”`); }
  };
  const lacks = async (text, where) => { if (await page.getByText(text).count()) fail(`${where}: unexpected “${text}”`); };
  const click = (role, name, scope = page) => scope.getByRole(role, { name }).first().click();
  const bridgeCalls = () => page.evaluate(() => window.__bridgeCalls.length);
  const open = async (v, extra = '') => { await page.goto(`${base}?review=1&variant=${v}&delay=300${extra}`); await page.locator('.atlas.is-map').waitFor(); };
  const row = (id) => page.locator(`[data-row="${id}"]`).first();

  async function turnOn(v, p) {
    if (v === 'B') {
      await has('Turn on Claude?', `${v} panel off`);
      await has('The contents of any file.', `${v} panel off`);
      await click('button', 'What always stays hidden');
      await has('KeePass .kdbx files', `${v} hidden list`);
      await shot(`${p}-02-turn-on`);
      await page.locator('.cp').getByRole('button', { name: 'Turn on Claude' }).click();
      await has('Checking that Claude Code answers', `${v} checking`);
      await shot(`${p}-03-checking`);
      await page.getByRole('button', { name: 'Turn off Claude' }).waitFor();
      await has('Privacy mode', `${v} on`);
      await shot(`${p}-04-on`);
      return;
    }
    await click('button', /Rules only/);
    const d = page.getByRole('dialog', { name: 'Turn on Claude?' });
    await d.waitFor();
    await has('The contents of any file.', `${v} dialog`);
    await click('button', 'What always stays hidden', d);
    await has('KeePass .kdbx files', `${v} hidden list`);
    await shot(`${p}-02-turn-on`);
    await click('button', /Turn on Claude/, d);
    await has('Checking that Claude Code answers', `${v} checking`);
    await shot(`${p}-03-checking`);
    await page.getByRole('dialog', { name: 'Claude is on.' }).waitFor();
    await page.getByRole('switch', { name: 'Privacy mode' }).waitFor();
    await shot(`${p}-04-on`);
    await click('button', 'Done');
    await page.getByRole('button', { name: /Claude on/ }).waitFor();
  }

  async function privacyOff(v, p) {
    if (v === 'B') await page.getByRole('switch', { name: 'Privacy mode' }).click();
    else await page.locator('.cl-pchip').first().click();
    const c = page.getByRole('alertdialog', { name: 'Turn privacy mode off?' });
    await c.waitFor();
    await has('goes to Anthropic through your Claude Code', `${v} privacy confirm`);
    await has('Bitwarden', `${v} privacy confirm`);
    await shot(`${p}-30-privacy-confirm`);
    await click('button', 'Turn privacy mode off', c);
    await page.getByRole('button', { name: /Claude on · reads files/ }).waitFor();
    await has(v === 'B' ? 'Off: Claude can read files' : 'Privacy off · reads files', `${v} privacy off`);
  }

  for (const v of ['A', 'B', 'C']) {
    const p = `${v}`;
    await page.setViewportSize({ width: 1280, height: 800 });
    await open(v, '&state=off');
    await shot(`${p}-01-map-off`);
    if (v !== 'B') await click('button', 'Build a plan');
    if (v === 'A') await has('Claude can review this plan or build one.', 'A tray off');
    if (v === 'C') await has('Turn on Claude to check the plan first', 'C tray off');

    // 1. Turn Claude on.
    await turnOn(v, p);
    if (v === 'B') await click('button', 'Build a plan');

    // 5. Explain the big folders the rules don't recognise.
    if (v === 'A') await click('button', /Explain big folders/);
    if (v === 'B') await click('button', 'Explain big folders', page.locator('.cp'));
    if (v === 'C') await click('button', /folders the rules don’t know/);
    await page.locator('.ai-pin').first().waitFor();
    if (v === 'B') { await has('big folders the rules don’t recognise', 'B explain'); await shot(`${p}-05-explain`); }
    await page.locator(`[data-tile="${DOCKER}"]`).hover();
    const tip = page.getByRole('tooltip');
    await tip.waitFor();
    if (!/Claude says|Docker Desktop’s virtual disk/.test(await tip.innerText())) fail(`${v}: the Docker hover card has no Claude section`);
    await shot(`${p}-06-explain-hover`);
    await page.locator(`[data-tile="${DOCKER}"]`).click();
    if (v === 'C') { await page.getByRole('complementary', { name: /Claude on Docker/ }).waitFor(); await click('button', 'Ask a follow-up'); }
    await has('What you’d lose', `${v} folder chat`);
    await has('From names, sizes and dates only', `${v} folder chat basis`);
    await click('button', 'Can I get it back?');
    await has('deleting is permanent', `${v} follow-up`);
    if (v !== 'B') {
      await page.getByRole('textbox', { name: /Ask about Docker/ }).fill('Can you look inside it?');
      await page.getByRole('button', { name: 'Send' }).first().click();
    } else {
      await page.getByRole('textbox', { name: /Ask about Docker/ }).fill('Can you look inside it?');
      await page.locator('.cp').getByRole('button', { name: 'Send' }).click();
    }
    await has('With privacy mode on I only see names', `${v} typed follow-up`);
    await shot(`${p}-07-folder-chat`);
    if (v === 'A') await page.getByRole('dialog', { name: /Ask Claude about Docker/ }).getByRole('button', { name: 'Close' }).click();
    if (v === 'C') await page.locator('.cl-card').getByRole('button', { name: 'Close' }).click();

    // 2. Review my plan.
    if (v === 'A') { await click('button', 'Review my plan'); await has('Claude flagged 2 of', 'A review'); }
    if (v === 'B') { await click('button', 'Review my plan', page.locator('.cp')); await has('I’d look at 2 of these', 'B review'); }
    if (v === 'C') { await click('button', /Review with Claude/); await page.locator('.rv').waitFor(); await has('Claude flags 2', 'C review'); }
    await has('portfolio-2024 has no package-lock.json', `${v} review flag`);
    await shot(`${p}-08-review`);

    // 4. Why is this risky? A chat about a plan item.
    if (v === 'C') await row('w-installers').getByRole('button', { name: 'Ask' }).click();
    else await row('w-installers').getByRole('button', { name: 'Ask Claude' }).click();
    await has('47 setup files', `${v} row chat`);
    await click('button', 'Which ones should I keep?');
    await has('Docker Desktop Installer.exe and node-v22.9.0-x64.msi are versions people keep', `${v} row follow-up`);
    await shot(`${p}-09-row-chat`);
    if (v === 'A') await page.getByRole('dialog', { name: /Ask Claude about Old installers/ }).getByRole('button', { name: 'Close' }).click();
    if (v === 'C') await row('w-installers').getByRole('button', { name: 'Hide' }).click();

    // 3. Let Claude pick: the diff, then accept it.
    if (v === 'A') { await click('button', 'Let Claude pick'); await has('Claude’s plan, shown below.', 'A pick'); await has('Claude adds this, unticked', 'A pick'); }
    if (v === 'B') { await click('button', 'Let Claude pick', page.locator('.cp')); await has('Refused by the Guard', 'B pick'); }
    if (v === 'C') { await click('radio', /Claude’s picks/); await has('Use Claude’s picks', 'C pick'); }
    await has('Android emulator images', `${v} pick`);
    await has('Windows Update downloads', `${v} pick refused`);
    await shot(`${p}-10-pick-diff`);
    await click('button', v === 'C' ? 'Use Claude’s picks' : 'Use Claude’s plan');
    await row('ai-android').waitFor();
    if (await row('w-installers').locator('[role=checkbox][aria-checked=true]').count()) fail(`${v}: accepting the pick left Old installers ticked`);
    // Claude's own item is unticked; Zach ticks it.
    await row('ai-android').getByRole('checkbox').click();
    await shot(`${p}-11-pick-accepted`);

    // 6. Claude's Tidy up layout.
    if (v === 'A') {
      await row('tidy-downloads').getByRole('button', { name: /Sort Downloads/ }).click();
      await page.getByRole('dialog', { name: 'Sort Downloads' }).waitFor();
      await click('radio', /Claude’s layout/);
      await has('Claude’s layout:', 'A tidy');
    }
    if (v === 'B') {
      await click('button', 'Suggest Tidy up layouts', page.locator('.cp'));
      await has('Here’s how I’d sort them instead', 'B tidy');
      await page.locator('.cp-tidy').first().getByRole('radio', { name: /Claude’s/ }).click();
      await shot(`${p}-12a-tidy-panel`);
      await page.locator('.cp-tidy').first().getByRole('button', { name: 'See the trees' }).click();
      await page.getByRole('dialog', { name: 'Sort Downloads' }).waitFor();
    }
    if (v === 'C') {
      await page.getByRole('radiogroup', { name: 'Layout for Downloads' }).getByRole('radio', { name: /Claude’s layout/ }).click();
      await page.locator('.rv-tidy-f').first().getByText('Money & paperwork').waitFor();
      await page.locator('.rv-tidy-f').first().getByRole('checkbox').click();
      await has('Grouped by what the files are for', 'C tidy');
    }
    await has('Money & paperwork', `${v} tidy tree`);
    await has('Sort later', `${v} tidy tree`);
    await shot(`${p}-12-tidy`);
    if (v !== 'C') {
      await page.getByRole('dialog', { name: 'Sort Downloads' }).getByRole('button', { name: 'Add to plan' }).click();
      await has('Every move is journaled', `${v} drawer undo`);
      await page.getByRole('dialog', { name: 'Sort Downloads' }).getByRole('button', { name: 'Close' }).click();
      await has('sorted Claude’s way', `${v} tray tidy row`);
    }

    // Confirm: the real dialog, with the proposed line about Claude.
    if (v === 'C') await click('button', /^Continue: free/);
    else await page.locator('.tray-cta').click();
    await page.getByRole('alertdialog').waitFor();
    await has('Proposed line', `${v} confirm`);
    await has('suggested by Claude: Android emulator images', `${v} confirm`);
    await has('sorted into Claude’s folders', `${v} confirm`);
    await shot(`${p}-13-confirm`);
    await page.getByRole('alertdialog').getByRole('button', { name: /permanently/ }).click();
    await has('The prototype stops here.', `${v} end`);
    await has('Proposed for the summary', `${v} end`);
    await shot(`${p}-14-end`);
    await click('button', 'Back to the map');

    // Privacy mode off: its confirm, then a richer answer, and a folder that stays hidden.
    await privacyOff(v, p);
    await page.locator(`[data-tile="${EDGE}"]`).click();
    if (v === 'C') { await page.getByRole('complementary', { name: /Claude on User Data/ }).waitFor(); await click('button', 'Ask a follow-up'); }
    await has('I can’t look inside the rest: Login Data, Cookies', `${v} hidden folder`);
    await shot(`${p}-31-hidden-folder`);
    if (v === 'A') await page.getByRole('dialog', { name: /Ask Claude about User Data/ }).getByRole('button', { name: 'Close' }).click();
    if (v === 'C') await page.locator('.cl-card').getByRole('button', { name: 'Close' }).click();
    if (!(await page.locator('aside.tray').count())) await click('button', 'Build a plan');
    if (v === 'C') { await click('button', /Review with Claude/); await page.locator('.rv').waitFor(); await row('w-rust').getByRole('button', { name: 'Ask' }).click(); }
    else await row('w-rust').getByRole('button', { name: 'Ask Claude' }).click();
    await has('I opened Cargo.toml in each project', `${v} privacy-off answer`);
    await has('1.2 GB of .png images', `${v} privacy-off answer`);
    await shot(`${p}-32-privacy-off-answer`);
    // The review again, with privacy mode off: reading files finds a flag names alone couldn't.
    const againIn = v === 'B' ? page.locator('.cp-entry').filter({ hasText: 'Review my plan' }) : page;
    await againIn.getByRole('button', { name: 'Ask again with privacy mode off' }).first().click();
    await has('its renders folder holds 1.2 GB', `${v} review with privacy off`);
    await shot(`${p}-33-review-privacy-off`);
    if (v === 'C') await click('button', /^Plan$/);

    if (await bridgeCalls()) fail(`${v}: the bridge was called`);
    results.push({ variant: v, state: await page.locator('.state-label').innerText() });

    // The advisor's states.
    for (const s of ['notfound', 'signedout']) {
      await open(v, `&state=${s}`);
      if (v !== 'B') await click('button', /Rules only/);
      await has(s === 'notfound' ? 'Claude Code isn’t on this PC.' : 'Sign in to Claude Code first.', `${v} ${s}`);
      await page.getByRole('button', { name: 'Check again' }).click();
      await has('Checking…', `${v} ${s} recheck`);
      await shot(`${p}-40-${s}`);
    }
    for (const s of ['slow', 'error']) {
      await open(v, `&state=${s}`);
      await click('button', 'Build a plan');
      if (v === 'B') await click('button', 'Review my plan', page.locator('.cp'));
      else if (v === 'A') await click('button', 'Review my plan');
      else await click('button', /Review with Claude/);
      if (s === 'slow') {
        await has('Still working.', `${v} slow`);
        await shot(`${p}-41-slow`);
        await page.getByRole('button', { name: 'Cancel' }).first().click();
        await has('Cancelled. Nothing changed.', `${v} cancel`);
        await shot(`${p}-42-cancelled`);
      } else {
        await has('Claude Code didn’t answer', `${v} error`);
        await shot(`${p}-43-error`);
      }
    }
    // Turning Claude off again.
    await open(v, '&state=on');
    if (v === 'B') await click('button', 'Turn off Claude');
    else { await click('button', /Claude on/); await click('button', 'Turn off Claude'); await click('button', 'Close AI options'); }
    await page.getByRole('button', { name: /Rules only/ }).waitFor();
    if (v === 'B') await has('Turn on Claude?', 'B off again');
    await shot(`${p}-44-off-again`);
  }

  // 1024×700: each variant mid-job, with the tray, the panel or the review step up.
  await page.setViewportSize({ width: 1024, height: 700 });
  for (const v of ['A', 'B', 'C']) {
    await open(v, '&state=on');
    await click('button', 'Build a plan');
    if (v === 'A') { await click('button', /Explain big folders/); await page.locator('.ai-pin').first().waitFor(); await click('button', 'Let Claude pick'); await has('Claude’s plan, shown below.', 'A small'); }
    if (v === 'B') { await click('button', 'Let Claude pick', page.locator('.cp')); await has('Refused by the Guard', 'B small'); }
    if (v === 'C') { await click('button', /Review with Claude/); await has('Claude flags 2', 'C small'); }
    await shot(`${v}-50-small`);
    if (v === 'A') { await row('tidy-downloads').getByRole('button', { name: /Sort Downloads/ }).click(); await click('radio', /Claude’s layout/); await has('Claude’s layout:', 'A small tidy'); await shot(`${v}-51-small-tidy`); }
    if (v === 'B') { await click('button', 'Explain big folders', page.locator('.cp')); await has('big folders the rules don’t recognise', 'B small explain'); await shot(`${v}-51-small-explain`); }
    if (v === 'C') { await click('radio', /Claude’s picks/); await has('Use Claude’s picks', 'C small pick'); await shot(`${v}-51-small-pick`); }
    await open(v, '&state=on&privacy=off');
    if (v !== 'B') { await click('button', /Claude on/); await has('Off: Claude can read files', `${v} small dialog`); await shot(`${v}-52-small-dialog-privacy-off`); }
    else { await has('Off: Claude can read files', 'B small panel'); await shot(`${v}-52-small-panel-privacy-off`); }
  }

  // Keyboard arrows switch variants; a build without review=1 hides the switcher.
  await page.setViewportSize({ width: 1280, height: 800 });
  await open('A');
  await page.mouse.click(640, 20);
  await page.keyboard.press('ArrowLeft');
  if (!page.url().includes('variant=C')) fail('ArrowLeft did not wrap to C');
  await page.keyboard.press('ArrowRight');
  if (!page.url().includes('variant=A')) fail('ArrowRight did not return to A');
  await page.goto(`${base}?variant=A`);
  await page.locator('.atlas.is-map').waitFor();
  if (await page.locator('.prototype-switcher').count()) fail('Built switcher appeared without review=1');
  if (await bridgeCalls()) fail('the bridge was called');

  if (errors.length) fail(errors.join('\n'));
  await writeFile(`${output}\\proof.json`, JSON.stringify({ date: new Date().toISOString(), surface: 'built output, review=1', results, shots, pageErrors: errors, status: 'passed' }, null, 2));
  console.log(JSON.stringify({ status: 'passed', output, results, shots: shots.length }, null, 2));
} finally {
  await browser?.close();
  if (server.exitCode === null) {
    server.kill();
    await new Promise((r) => server.once('exit', r));
  }
}

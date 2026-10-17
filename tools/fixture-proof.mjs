// Installed Windows fixture proof: node tools/fixture-proof.mjs --fixture D:\tmp\vacancy-fixture-new --out shots --cdp http://127.0.0.1:9333
// Requires the app already started with VACANCY_FIXTURE_ROOT and a held-open fixture file.
// Build the handle-pinned mutation helper first: cargo build -p vacancy-core --example fixture_mutator
// --mutator <exe> overrides target/debug/examples/fixture_mutator.exe (VACANCY_FIXTURE_MUTATOR also works).
// --reminders also selects the single default-off reminder from a fresh fixture built with --advice.
// Drives scan/plan/confirm/run with UI clicks; stops on the completed run for wrapped-shots (without --run).
import assert from 'node:assert/strict';
import { lstat, mkdir, realpath, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { homedir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({ options: {
  fixture: { type: 'string' }, out: { type: 'string' },
  cdp: { type: 'string', default: 'http://127.0.0.1:9333' },
  mutator: { type: 'string', default: process.env.VACANCY_FIXTURE_MUTATOR || resolve('target/debug/examples/fixture_mutator.exe') },
  reminders: { type: 'boolean', default: false },
} });
assert.equal(process.platform, 'win32', 'Installed fixture proof is Windows only');
assert(o.fixture && o.out, '--fixture and --out are required');
const root = resolve(o.fixture);
const contains = (parent, child) => {
  const r = relative(parent, child);
  return r === '' || (!r.startsWith('..') && !isAbsolute(r));
};
assert(!contains(homedir(), root) && !contains(root, homedir()), 'Fixture must be outside home');
assert(!contains(root, resolve(o.out)), 'Evidence must be written outside the fixture');
assert(dirname(root) !== root, 'A drive root is forbidden');
assert.equal((await realpath(root)).toLowerCase(), root.toLowerCase(), 'Fixture path resolves elsewhere');
for (let p = root; dirname(p) !== p; p = dirname(p)) {
  assert(!(await lstat(p)).isSymbolicLink(), `Fixture ancestor is a link: ${p}`);
}
assert((await lstat(join(root, 'vacancy-fixture-profile.txt'))).isFile(), 'Fixture marker required');
const modify = join(root, 'Users/fixture/AppData/Local/Google/Chrome/User Data/Profile 1/Cache/Cache_Data/modified-later.bin');
assert((await lstat(modify)).isFile(), 'Expected fixture file absent');
for (let p = modify; p !== root; p = dirname(p)) {
  assert(!(await lstat(p)).isSymbolicLink(), `Modification path is a link: ${p}`);
}
await mkdir(o.out, { recursive: true });
const browser = await chromium.connectOverCDP(o.cdp);
const page = browser.contexts()[0].pages()[0];
const errors = [];
page.on('pageerror', e => errors.push(String(e)));
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
const saveText = async (name, text) => writeFile(join(o.out, name), text);
const shot = name => page.screenshot({ path: join(o.out, `${name}.png`) });
let mutation = null;
let mutationLines = null;
try {
  const scan = page.locator('button.cta', { hasText: /^\s*Scan / });
  await scan.waitFor({ timeout: 30_000 });
  const names = await page.locator('.drive-card-name').allTextContents();
  const fixtureName = `Fixture (${basename(root)})`;
  assert.equal((await scan.innerText()).replace(/\s+/g, ' ').trim(), `Scan ${fixtureName}`, 'Visible scan target must be the named fixture');
  // The one-drive fixture layout has no cards. If cards are shown, they must still offer exactly this fixture.
  if (names.length) assert.deepEqual(names.map(n => n.trim()), [fixtureName], 'App must offer only the named fixture');
  // A read-only check of the public bridge adds the exact mount path to the visible fixture-name check.
  const volumes = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_volumes'));
  assert.equal(volumes.length, 1);
  assert.equal(resolve(volumes[0].mountPoint).toLowerCase(), root.toLowerCase(), 'App is not confined to this fixture');
  await saveText('fixture-volume.json', JSON.stringify(volumes, null, 2));
  // Arm before Scan: the helper pins every real ancestor and opens this existing file exclusively, then keeps
  // those exact handles until its change command. Node never reopens or creates the target by path.
  mutation = spawn(o.mutator, [root], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  console.log(`Fixture mutation helper PID ${mutation.pid}`);
  let mutationError = '';
  mutation.stderr.on('data', chunk => { mutationError += chunk; });
  const exited = new Promise((done, reject) => {
    mutation.once('error', reject);
    mutation.once('exit', code => code === 0 ? done() : reject(new Error(`Mutation helper exited ${code}: ${mutationError}`)));
  });
  mutationLines = createInterface({ input: mutation.stdout });
  const [readyLine] = await Promise.race([
    once(mutationLines, 'line'),
    exited.then(() => { throw new Error('Mutation helper exited before READY'); }),
  ]);
  const ready = JSON.parse(readyLine);
  assert.equal(ready.state, 'READY');
  assert.equal(resolve(ready.path).toLowerCase(), modify.toLowerCase(), 'Mutation helper armed the wrong file');
  await saveText('mutation-ready.json', JSON.stringify(ready, null, 2));
  await shot('installed-1-plate');
  await scan.click();
  await page.locator('.atlas.is-map').waitFor({ timeout: 120_000 });
  await shot('installed-2-map');
  mutation.stdin.end('change\n');
  await exited; // It has written via the held handle and released every pin before planning.
  await page.getByRole('button', { name: 'Build a plan' }).click();
  if (o.reminders) {
    const hints = page.getByRole('complementary', { name: 'Plan', exact: true })
      .getByRole('region', { name: 'Left for you' }).getByRole('checkbox', { name: /^Remind me:/ });
    assert.equal(await hints.count(), 1, 'The fresh Windows advice fixture must have exactly one reminder');
    assert.equal(await hints.getAttribute('aria-checked'), 'false', 'Advice must start unselected');
    await hints.click();
    assert.equal(await hints.getAttribute('aria-checked'), 'true');
  }
  const rows = page.locator('.tray-row');
  for (const row of await rows.all()) {
    const title = row.locator('.tray-row-title');
    if (await title.getAttribute('aria-expanded') === 'false') await title.click();
  }
  await saveText('selected-proposals.txt', await page.getByRole('complementary', { name: 'Plan', exact: true }).innerText());
  await shot('installed-3-plan');
  await page.locator('.tray-cta').click();
  const dialog = page.getByRole('alertdialog');
  await dialog.waitFor({ timeout: 60_000 });
  await saveText('confirmation.txt', await dialog.innerText());
  await shot('installed-4-confirm');
  await page.getByRole('button', { name: /permanently$/ }).click();
  await page.locator('.tray.run').waitFor({ timeout: 15_000 });
  await shot('installed-5-run');
  await page.getByRole('button', { name: 'See what changed' }).waitFor({ timeout: 120_000 });
  await saveText('installed-run-panel.txt', await page.locator('.tray.run').innerText());
  await shot('installed-6-run-done');
  assert.equal(errors.length, 0, errors.join('\n'));
  console.log('Fixture run completed; capture finish with wrapped-shots --cdp (NO --run).');
} finally {
  mutationLines?.close();
  if (mutation && mutation.exitCode === null) mutation.kill();
  await saveText('page-errors.json', JSON.stringify(errors));
  await browser.close();
}

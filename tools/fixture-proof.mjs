// Installed Windows fixture proof: node tools/fixture-proof.mjs --fixture D:\tmp\vacancy-fixture-new --out shots --cdp http://127.0.0.1:9333
// Requires the app already started with VACANCY_FIXTURE_ROOT and a held-open fixture file.
// Drives scan/plan/confirm/run with UI clicks; stops on the completed run for wrapped-shots (without --run).
import assert from 'node:assert/strict';
import { appendFile, lstat, mkdir, realpath, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { chromium } from 'playwright';

const { values: o } = parseArgs({ options: {
  fixture: { type: 'string' }, out: { type: 'string' },
  cdp: { type: 'string', default: 'http://127.0.0.1:9333' },
} });
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
try {
  const scan = page.locator('button.cta', { hasText: /^\s*Scan / });
  await scan.waitFor({ timeout: 30_000 });
  const names = await page.locator('.drive-card-name').allTextContents();
  assert.deepEqual(names.map(n => n.trim()), [`Fixture (${basename(root)})`], 'App must offer only the named fixture');
  // A read-only check of the public bridge adds the exact mount path to the visible fixture-name check.
  const volumes = await page.evaluate(() => window.__TAURI_INTERNALS__.invoke('list_volumes'));
  assert.equal(volumes.length, 1);
  assert.equal(resolve(volumes[0].mountPoint).toLowerCase(), root.toLowerCase(), 'App is not confined to this fixture');
  await saveText('fixture-volume.json', JSON.stringify(volumes, null, 2));
  await shot('installed-1-plate');
  await scan.click();
  await page.locator('.atlas.is-map').waitFor({ timeout: 120_000 });
  await shot('installed-2-map');
  await appendFile(modify, 'written after the scan\n');
  await page.getByRole('button', { name: 'Build a plan' }).click();
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
  await saveText('page-errors.json', JSON.stringify(errors));
  await browser.close();
}

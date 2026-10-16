// PROTOTYPE helper: open each variant's plan, press its primary run button, screenshot the confirm. node tools/shot-confirm.mjs <baseUrl>
import { chromium } from 'playwright';
const base = process.argv[2] ?? 'http://localhost:5199/';
const b = await chromium.launch({ channel: 'msedge' });
const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
for (const [v, label] of [['A', /^Free /], ['B', /Review & run/], ['C', /Review and run/]]) {
  await p.goto(`${base}?variant=${v}&os=win&step=plan`);
  await p.waitForTimeout(1500);
  await p.locator('.pf-content button', { hasText: label }).last().click();
  await p.waitForTimeout(1800);
  await p.screenshot({ path: `shots/confirm-${v}.png` });
  console.log(v, 'step now', new URL(p.url()).searchParams.get('step'));
}
await b.close();

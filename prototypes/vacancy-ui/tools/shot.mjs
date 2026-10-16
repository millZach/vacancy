// PROTOTYPE helper: screenshot every variant x os x step from a running server.
// VP=1140x796 sets the viewport (default 1440x900).
// node tools/shot.mjs <baseUrl> [variants=A,B,C] [os=win,mac] [steps=scan,overview,plan,run,summary] [waitMs=1500]
import { chromium } from 'playwright';
const [base = 'http://localhost:5199/', vs = 'A,B,C', oss = 'win,mac', ss = 'scan,overview,plan,run,summary', wait = '1500'] = process.argv.slice(2);
const browser = await chromium.launch({ channel: 'msedge' });
const page = await browser.newPage({ viewport: { width: Number((process.env.VP || '1440x900').split('x')[0]), height: Number((process.env.VP || '1440x900').split('x')[1]) } });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
for (const v of vs.split(',')) for (const os of oss.split(',')) for (const s of ss.split(',')) {
  await page.goto(`${base}?variant=${v}&os=${os}&step=${s}`);
  await page.waitForTimeout(Number(wait));
  const f = `shots/${process.env.VP ? process.env.VP + '-' : ''}${v}-${os}-${s}.png`;
  await page.screenshot({ path: f });
  console.log(f);
}
console.log(errors.length ? `ERRORS:\n${errors.join('\n')}` : 'no console errors');
await browser.close();

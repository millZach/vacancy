// Renders tools/app-icon.html to src-tauri/icons/app-icon.png (1024x1024, transparent corners) in Edge.
// Then regenerate the platform icons with:  npm run tauri icon src-tauri/icons/app-icon.png
import { fileURLToPath, pathToFileURL } from 'node:url';
import { chromium } from 'playwright';

const html = fileURLToPath(new URL('./app-icon.html', import.meta.url));
const out = fileURLToPath(new URL('../src-tauri/icons/app-icon.png', import.meta.url));

const browser = await chromium.launch({ channel: 'msedge' });
try {
  const page = await browser.newPage({ viewport: { width: 1024, height: 1024 } });
  await page.goto(pathToFileURL(html).href);
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: out, omitBackground: true });
  console.log(out);
} finally {
  await browser.close();
}

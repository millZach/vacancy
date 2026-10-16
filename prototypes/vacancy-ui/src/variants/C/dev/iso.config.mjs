// PROTOTYPE dev-only: run variant C's own dev server with variants A and B stubbed out,
// so their in-progress edits can't break or reload C's screenshots. Not used by the real build.
// npx vite --config src/variants/C/dev/iso.config.mjs --port 5203 --strictPort
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

const here = (p) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig({
  base: './',
  plugins: [
    {
      name: 'vc-isolate',
      enforce: 'pre',
      resolveId(source, importer) {
        const from = importer ? importer.split(String.fromCharCode(92)).join('/') : '';
        if (from.endsWith('/src/App.tsx') && (source === './variants/A' || source === './variants/B')) {
          return here(`./stub${source.slice(-1)}.tsx`);
        }
      },
    },
    react(),
  ],
  server: { watch: { ignored: ['**/src/variants/A/**', '**/src/variants/B/**'] } },
});

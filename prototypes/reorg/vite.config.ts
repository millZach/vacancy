import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)), base: './', plugins: [react()],
  server: { host: '127.0.0.1', port: 1435, strictPort: true, fs: { allow: [fileURLToPath(new URL('../..', import.meta.url))] } },
  preview: { host: '127.0.0.1', port: 1435, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true, target: ['es2022', 'chrome110', 'safari15'] },
});

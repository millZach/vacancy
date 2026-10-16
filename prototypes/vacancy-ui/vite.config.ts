import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// PROTOTYPE: static build with relative paths so any folder server can host it.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: { port: 5199, strictPort: true },
});

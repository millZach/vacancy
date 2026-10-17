import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, vi } from 'vitest';

// The finish's script reads its timings from the design tokens. Tests load no CSS, so the timing tokens are read
// from tokens.css and set on the root element. (Only those: a whole stylesheet makes every style lookup in jsdom,
// and so every query by role, many times slower.)
const tokens = readFileSync(join(process.cwd(), 'src/design/tokens.css'), 'utf8');
for (const [, name, value] of tokens.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
  if (name && value && /-ms$|^--ease-|^--slide-shift$/.test(name)) document.documentElement.style.setProperty(name, value.trim());
}

// jsdom has no layout engine, canvas, ResizeObserver or matchMedia. Give the UI harmless stand-ins; tests that
// care about one of them replace it.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= StubResizeObserver;

if (typeof window.matchMedia !== 'function') {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string): MediaQueryList => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

// Without this jsdom prints "Not implemented: HTMLCanvasElement.prototype.getContext" for every canvas.
HTMLCanvasElement.prototype.getContext = vi.fn(() => null);

afterEach(() => {
  cleanup();
});

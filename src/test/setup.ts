import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

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

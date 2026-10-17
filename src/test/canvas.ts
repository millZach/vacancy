// Stand-ins for what the dot field needs from a browser, for tests that watch it: a 2D context, the design tokens,
// animation frames the test runs by hand, and the reduced-motion setting.
import { vi } from 'vitest';

/** A 2D context that accepts every call, counts the dots drawn and keeps every fill colour set. */
export function fakeContext() {
  const calls = { arc: 0 };
  const fills = new Set<string>();
  const ctx = new Proxy(
    {},
    {
      get: (_target, key) => (key === 'arc' ? () => void calls.arc++ : () => {}),
      set: (_target, key, value) => {
        if (key === 'fillStyle') fills.add(String(value));
        return true;
      },
    },
  ) as CanvasRenderingContext2D;
  return { ctx, calls, fills };
}

const realComputedStyle = window.getComputedStyle.bind(window);

/** The design tokens the field reads its colours from (the test DOM doesn't load tokens.css). */
export function designTokens(tokens: Record<string, string>) {
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
    const style = realComputedStyle(el, pseudo);
    const read = style.getPropertyValue.bind(style);
    style.getPropertyValue = (name: string) => tokens[name] ?? read(name);
    return style;
  });
}

/** Animation frames held until the test runs them, on the performance clock the field uses. */
export function manualFrames() {
  const pending = new Map<number, FrameRequestCallback>();
  let nextId = 1;
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
    pending.set(nextId, cb);
    return nextId++;
  });
  const cancel = vi.spyOn(window, 'cancelAnimationFrame').mockImplementation((id) => {
    pending.delete(id);
  });
  return {
    get waiting() {
      return pending.size;
    },
    cancel,
    /** Runs every waiting frame `afterMs` from now. */
    run(afterMs = 16) {
      const due = [...pending.values()];
      pending.clear();
      for (const cb of due) cb(performance.now() + afterMs);
    },
  };
}

export function prefersReducedMotion(reduce: boolean) {
  vi.spyOn(window, 'matchMedia').mockImplementation(
    (query: string) =>
      ({
        matches: reduce && query.includes('reduce'),
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
      }) as unknown as MediaQueryList,
  );
}

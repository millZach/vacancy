import { fireEvent, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DotField } from './DotField';

/** A 2D context that accepts every call, counts the dots drawn and keeps every fill colour set. */
function fakeContext() {
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
function designTokens(tokens: Record<string, string>) {
  vi.spyOn(window, 'getComputedStyle').mockImplementation((el, pseudo) => {
    const style = realComputedStyle(el, pseudo);
    const read = style.getPropertyValue.bind(style);
    style.getPropertyValue = (name: string) => tokens[name] ?? read(name);
    return style;
  });
}

/** Animation frames held until the test runs them, on the performance clock the field uses. */
function manualFrames() {
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

function prefersReducedMotion(reduce: boolean) {
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

function renderField() {
  return render(
    <div data-testid="water" style={{ position: 'relative', width: 800, height: 600 }}>
      <DotField />
      <button type="button">Scan</button>
    </div>,
  );
}

describe('the dot field', () => {
  let drawn: ReturnType<typeof fakeContext>;

  beforeEach(() => {
    drawn = fakeContext();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(drawn.ctx);
    designTokens({ '--dot-rgb': '196, 205, 219', '--neon-rgb': '255, 79, 179' });
  });

  it('keeps animating while mounted and stops when unmounted', () => {
    prefersReducedMotion(false);
    const frames = manualFrames();
    const { unmount } = renderField();

    frames.run();
    frames.run();
    expect(drawn.calls.arc).toBeGreaterThan(0);
    expect(frames.waiting).toBe(1);

    unmount();
    expect(frames.waiting).toBe(0);
    expect(frames.cancel).toHaveBeenCalled();
  });

  it('with reduced motion, sleeps until the water is touched, then fades once and sleeps again', () => {
    prefersReducedMotion(true);
    const frames = manualFrames();
    const { getByTestId } = renderField();

    frames.run(); // the first still frame
    frames.run(); // nothing moving: no more frames
    expect(frames.waiting).toBe(0);

    fireEvent.pointerDown(getByTestId('water'), { button: 0, clientX: 200, clientY: 200 });
    expect(frames.waiting).toBe(1);
    frames.run(16);
    expect(frames.waiting).toBe(1); // still fading
    frames.run(1500);
    frames.run(1516);
    expect(frames.waiting).toBe(0); // calm again
  });

  it('with reduced motion, redraws the calm field when frames resume after a fade has ended', () => {
    prefersReducedMotion(true);
    const frames = manualFrames();
    const { getByTestId } = renderField();
    frames.run();
    frames.run();

    fireEvent.pointerDown(getByTestId('water'), { button: 0, clientX: 200, clientY: 200 });
    frames.run(200); // mid-fade, then frames pause (a hidden window) until well after the fade's end
    const lit = drawn.calls.arc;
    frames.run(1500);
    expect(drawn.calls.arc).toBeGreaterThan(lit); // the lit frame is replaced by the calm one
    frames.run(1516);
    expect(frames.waiting).toBe(0);
  });

  it('paints with the colours from the design tokens', () => {
    designTokens({ '--dot-rgb': '10, 20, 30', '--neon-rgb': '255, 79, 179' });
    prefersReducedMotion(true);
    const frames = manualFrames();
    renderField();
    frames.run(); // a still frame: every dot is the resting colour

    expect(drawn.fills.size).toBeGreaterThan(0);
    for (const fill of drawn.fills) expect(fill).toMatch(/^rgba\(10,20,30,/);
  });

  it('is not drawn when the design tokens are missing', () => {
    designTokens({ '--dot-rgb': '', '--neon-rgb': '' });
    vi.spyOn(console, 'error').mockImplementation(() => {});
    prefersReducedMotion(true);
    const frames = manualFrames();
    renderField();

    expect(frames.waiting).toBe(0);
    expect(console.error).toHaveBeenCalled();
  });

  it('does not ripple for clicks on controls or other mouse buttons', () => {
    prefersReducedMotion(true);
    const frames = manualFrames();
    const { getByRole, getByTestId } = renderField();
    frames.run();
    frames.run();
    expect(frames.waiting).toBe(0);

    fireEvent.pointerDown(getByRole('button', { name: 'Scan' }), { button: 0 });
    fireEvent.pointerDown(getByTestId('water'), { button: 2 });

    expect(frames.waiting).toBe(0);
  });
});

describe('the dot field while scanning', () => {
  let drawn: ReturnType<typeof fakeContext>;

  beforeEach(() => {
    clock = 0;
    drawn = fakeContext();
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(drawn.ctx);
    vi.spyOn(HTMLCanvasElement.prototype, 'getBoundingClientRect').mockReturnValue(
      DOMRect.fromRect({ x: 0, y: 0, width: 800, height: 600 }),
    );
    designTokens({ '--dot-rgb': '196, 205, 219', '--neon-rgb': '255, 79, 179' });
  });

  /** Fill colours other than the resting dot colour: dots tinted by a ripple's neon wavefront. */
  const tinted = () => [...drawn.fills].filter((f) => !f.startsWith('rgba(196,205,219,'));

  // Frames run on a clock that only moves forward, as in a browser.
  let clock = 0;
  function runFor(frames: ReturnType<typeof manualFrames>, ms: number) {
    for (const end = clock + ms; clock < end; ) frames.run((clock += 16));
  }

  it('drops droplets as the scan sweeps across, and none without a scan', () => {
    prefersReducedMotion(false);
    const frames = manualFrames();
    const { rerender } = render(<DotField scan={null} />);
    runFor(frames, 800);
    expect(tinted()).toEqual([]);

    rerender(<DotField scan={0.4} />);
    runFor(frames, 800);
    expect(tinted().length).toBeGreaterThan(0);
  });

  it('fades out when the map has covered it', () => {
    prefersReducedMotion(false);
    manualFrames();
    const { container, rerender } = render(<DotField scan={0.5} />);
    expect(container.querySelector('canvas')).not.toHaveClass('is-out');
    rerender(<DotField scan={0.9} out />);
    expect(container.querySelector('canvas')).toHaveClass('is-out');
  });
});

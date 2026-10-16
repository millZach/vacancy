import { fireEvent, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DotField } from './DotField';

/** A 2D context that accepts every call and counts the dots drawn. */
function fakeContext() {
  const calls = { arc: 0 };
  const ctx = new Proxy(
    {},
    {
      get: (_target, key) => (key === 'arc' ? () => void calls.arc++ : () => {}),
      set: () => true,
    },
  ) as CanvasRenderingContext2D;
  return { ctx, calls };
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
    expect(frames.waiting).toBe(0); // calm again
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

// The finish's motion, done with the Web Animations API so it runs on the compositor and never sets React state per
// frame: each slide's countdown, slides leaving and arriving, the overlay fading, and the numbers flying into the
// summary. Timings come from tokens.css. Where `element.animate` is missing (tests), timers stand in.
import { useSyncExternalStore } from 'react';

/** A duration token from tokens.css, in milliseconds ("6500ms" or "0.6s"). 0 if it isn't set. */
export function tokenMs(name: string): number {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const n = parseFloat(v);
  if (!Number.isFinite(n)) return 0;
  return v.endsWith('ms') ? n : v.endsWith('s') ? n * 1000 : n;
}

/** Any other token from tokens.css, as written: an easing, a distance. */
export function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

const QUERY = '(prefers-reduced-motion: reduce)';

function subscribe(onChange: () => void) {
  const m = window.matchMedia?.(QUERY);
  m?.addEventListener('change', onChange);
  return () => m?.removeEventListener('change', onChange);
}

/** Whether the OS asks for less motion. Slides then fade instead of moving, and the number flight cross-fades. */
export function prefersReducedMotion(): boolean {
  return window.matchMedia?.(QUERY).matches ?? false;
}

/** `prefersReducedMotion`, kept up to date as the setting changes. */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribe, prefersReducedMotion, () => false);
}

/** Runs keyframes on `el`, or does nothing where the Web Animations API is missing. */
export function play(el: Element | null, keyframes: Keyframe[], options: KeyframeAnimationOptions): Animation | null {
  if (!el || typeof el.animate !== 'function') return null;
  return el.animate(keyframes, options);
}

/** When `a` has finished; at once for no animation. A cancelled animation never counts as finished. */
export function whenDone(a: Animation | null): Promise<boolean> {
  if (!a) return Promise.resolve(true);
  return a.finished.then(
    () => true,
    () => false,
  );
}

/** A slide's countdown: the bar in its dot filling up, then `onDone`. Pauses and resumes where it was. */
export interface Countdown {
  play(): void;
  pause(): void;
  cancel(): void;
}

/**
 * Fills `bar` over `ms`, then calls `onDone` once. Starts paused. The fill is a compositor animation of a scale, so
 * nothing is laid out or rendered by React while it runs.
 */
export function countdown(bar: HTMLElement | null, ms: number, onDone: () => void): Countdown {
  const fill = play(bar, [{ transform: 'scaleX(0)' }, { transform: 'scaleX(1)' }], {
    duration: ms,
    easing: 'linear',
    fill: 'forwards',
  });
  if (fill) {
    fill.pause();
    void whenDone(fill).then((done) => done && onDone());
    return { play: () => fill.play(), pause: () => fill.pause(), cancel: () => fill.cancel() };
  }
  // No Web Animations: a timer that keeps what was left when paused.
  let left = ms;
  let since = 0;
  let timer = 0;
  let over = false;
  return {
    play() {
      if (timer || over) return;
      since = performance.now();
      timer = window.setTimeout(() => {
        timer = 0;
        over = true;
        onDone();
      }, left);
    },
    pause() {
      if (!timer) return;
      clearTimeout(timer);
      timer = 0;
      left = Math.max(0, left - (performance.now() - since));
    },
    cancel() {
      clearTimeout(timer);
      timer = 0;
      over = true;
    },
  };
}

/** A slide going (`out`) or coming (`in`), `dir` 1 forwards and -1 back. With reduced motion it only fades. */
export function slideMotion(el: Element | null, how: 'in' | 'out', dir: number, reduce: boolean): Animation | null {
  const shift = reduce ? 0 : parseFloat(token('--slide-shift')) || 0;
  const away = how === 'in' ? shift * dir : -shift * dir;
  const moved = { opacity: 0, transform: `translateX(${away}px)` };
  const still = { opacity: 1, transform: 'translateX(0)' };
  return play(el, how === 'in' ? [moved, still] : [still, moved], {
    duration: tokenMs('--slide-move-ms'),
    easing: token('--ease-slide') || 'ease',
    fill: how === 'out' ? 'forwards' : 'none',
  });
}

/** Fades `el` in or out over a duration token. Used with reduced motion too: a fade isn't movement. */
export function fade(el: Element | null, how: 'in' | 'out', msToken: string): Animation | null {
  const shown = { opacity: 1 };
  const gone = { opacity: 0 };
  return play(el, how === 'in' ? [gone, shown] : [shown, gone], {
    duration: tokenMs(msToken),
    easing: token('--ease-fade') || 'ease',
    fill: 'forwards',
  });
}

/**
 * Flies each `[data-fly]` number under `root` onto its `[data-fly-target]` twin in the summary panel. Both use the
 * same markup, so a translate plus a uniform scale (the ratio of their font sizes) lands each glyph on its twin.
 * The freed number leaves first; the counts follow right to left, so none overtakes another. Resolves when all
 * have landed.
 */
export function flyNumbers(root: ParentNode): Promise<void> {
  const sources = [...root.querySelectorAll<HTMLElement>('[data-fly]')];
  const flights = sources.map((src, i) => {
    const dst = root.querySelector<HTMLElement>(`[data-fly-target="${src.dataset.fly}"]`);
    if (!dst) return Promise.resolve(true);
    const a = src.getBoundingClientRect();
    const b = dst.getBoundingClientRect();
    const from = parseFloat(getComputedStyle(src).fontSize);
    const to = parseFloat(getComputedStyle(dst).fontSize);
    const scale = from > 0 && to > 0 ? to / from : 1;
    const order = i === 0 ? 0 : sources.length - i;
    return whenDone(
      play(src, [{ transform: 'translate(0, 0) scale(1)' }, { transform: `translate(${b.left - a.left}px, ${b.top - a.top}px) scale(${scale})` }], {
        duration: tokenMs(order === 0 ? '--fly-ms' : '--fly-stat-ms'),
        delay: order === 0 ? 0 : tokenMs('--fly-delay-ms') + tokenMs('--fly-stagger-ms') * order,
        easing: token('--ease-fly') || 'ease-in-out',
        fill: 'forwards',
      }),
    );
  });
  return Promise.all(flights).then(() => undefined);
}

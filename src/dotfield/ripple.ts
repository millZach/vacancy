// Ripple physics for the dot field. Pure functions of time, so they can be reasoned about (and tested) apart from the
// canvas. Ported unchanged from the prototype's variant D: changing a constant here changes how the water feels.

export type DropKind = 'tap' | 'splash' | 'drip';

export interface Ripple {
  /** Impact point, canvas-local CSS px. */
  x: number;
  y: number;
  /** Impact time, seconds on the performance clock. */
  t0: number;
  amp: number;
  /** Front speed, px/s. */
  v: number;
  /** Wavelength at impact, px. Grows as the ring spreads (dispersion). */
  lam: number;
  /** Seconds until fully calm. */
  life: number;
  /** Outward push per unit of height, px. */
  push: number;
  /** Reduced motion: a soft brightening that fades in place, no travelling ring. */
  soft: boolean;
}

/** A tap is a background click; a splash is the Scan button's big drop; a drip is the scan finding folders. */
const PRESETS: Record<DropKind, Pick<Ripple, 'amp' | 'v' | 'lam' | 'life' | 'push'>> = {
  tap: { amp: 1.35, v: 300, lam: 60, life: 2.3, push: 6.5 },
  splash: { amp: 1.6, v: 470, lam: 80, life: 2.9, push: 9 },
  drip: { amp: 0.42, v: 230, lam: 46, life: 1.5, push: 4 },
};
const SOFT_PRESETS: Record<DropKind, Pick<Ripple, 'amp' | 'lam' | 'life'>> = {
  tap: { amp: 0.75, lam: 120, life: 0.9 },
  splash: { amp: 0.9, lam: 190, life: 1.3 },
  drip: { amp: 0.75, lam: 120, life: 0.9 },
};

export const TAU = Math.PI * 2;

export const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : v > hi ? hi : v);

export const smooth = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a), 0, 1);
  return t * t * (3 - 2 * t);
};

export function makeRipple(x: number, y: number, t0: number, kind: DropKind, reducedMotion: boolean): Ripple {
  if (reducedMotion) return { x, y, t0, v: 0, push: 0, soft: true, ...SOFT_PRESETS[kind] };
  return { x, y, t0, soft: false, ...PRESETS[kind] };
}

/**
 * A ripple's per-frame values, the ones that don't depend on the dot. Each ripple is a wave packet: a few rings
 * under a Gaussian envelope that trails the leading edge, rings spreading apart as it travels (dispersion), crests
 * running outward through the packet the way capillary ripples do, and amplitude falling with distance and age.
 */
export interface RippleFrame {
  ripple: Ripple;
  /** Radius of the leading edge, px. */
  R: number;
  /** Current wavelength, px. */
  lam: number;
  /** Ring amplitude (or, for a soft ripple, its brightness). */
  A: number;
  /** The bob at the impact point. */
  Ac: number;
  /** Envelope centre behind the edge, and its width. */
  c: number;
  w: number;
  phase: number;
  /** 1 while the ripple is young: the neon tint on the wavefront fades with it. */
  youth: number;
  /** Squared outer and inner radii of the annulus the packet occupies; dots outside it skip the maths. */
  o2: number;
  i2: number;
}

export function rippleFrame(ripple: Ripple, t: number): RippleFrame {
  const age = t - ripple.t0;
  const k = age / ripple.life;
  if (ripple.soft) {
    const A = ripple.amp * (1 - smooth(0, 1, k)) * smooth(0, 0.12, k);
    return { ripple, R: 0, lam: ripple.lam, A, Ac: 0, c: 0, w: 1, phase: 0, youth: 0, o2: Infinity, i2: 0 };
  }
  const R = ripple.v * age * (1 - 0.3 * k);
  const lam = ripple.lam * (1 + 0.6 * k);
  const A = (ripple.amp * smooth(0, 0.05, age) * Math.exp(-1.25 * k) * (1 - k * k * k * k)) / Math.sqrt(1 + R / 420);
  const Ac = ripple.amp * 0.85 * Math.exp(-age * 4) * Math.cos(age * TAU * 2.6);
  const c = 1.25 * lam;
  const w = 1.65 * lam;
  const outer = R + 0.7 * lam;
  const inner = Math.max(0, R - c - 2.4 * w);
  return {
    ripple,
    R,
    lam,
    A,
    Ac,
    c,
    w,
    phase: age * TAU * 0.9,
    youth: 1 - smooth(0.08, 0.66, k),
    o2: outer * outer,
    i2: inner * inner,
  };
}

/** True once the ripple has fully calmed and can be dropped. */
export const isCalm = (ripple: Ripple, t: number) => t - ripple.t0 > ripple.life;

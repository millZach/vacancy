// The dot field's renderer, without React: one canvas, one requestAnimationFrame loop, typed arrays reused between
// frames. The field breathes slowly; a drop sends a water ripple (a crest that pushes dots outward, a damped wake
// behind it, a bob at the impact point); dots near the calm area (the scan plate) stay quiet. With reduced motion
// there is no ambient movement, a drop is a single soft fade, and the loop sleeps whenever nothing is fading.
//
// Look and feel are ported unchanged from the prototype's variant D.

import { clamp, isCalm, makeRipple, rippleFrame, TAU, type DropKind, type Ripple } from './ripple';

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** What the field needs from the page. Injected so tests can drive frames by hand. */
export interface FieldEnv {
  /** Milliseconds on the same clock as requestAnimationFrame's timestamps. */
  now(): number;
  requestFrame(callback: (nowMs: number) => void): number;
  cancelFrame(handle: number): void;
  devicePixelRatio(): number;
}

/** Red, green and blue channels, 0 to 255. */
export type Rgb = readonly [number, number, number];

/** The field's two colours, from the design tokens (`--dot-rgb` and `--neon-rgb`). */
export interface Palette {
  dot: Rgb;
  neon: Rgb;
}

export interface DotField {
  /** Re-reads the canvas size and pixel ratio and rebuilds the grid. Call when the canvas resizes. */
  resize(): void;
  /** Drops a ripple at a canvas-local point (CSS px). */
  drop(x: number, y: number, kind: DropKind): void;
  /**
   * The area to keep quiet, canvas-local: the plate's content. The field dims an ellipse around it so the text
   * reads cleanly. `null` centres the quiet pool in the canvas.
   */
  setCalm(area: Rect | null): void;
  setReducedMotion(reduce: boolean): void;
  /** Stops the loop for good. */
  destroy(): void;
}

const MAX_DOTS = 4200;
const MIN_SPACING = 15;
const MAX_RIPPLES = 12;
const ALPHA_LEVELS = 28;
const NEON_LEVELS = 7;
/** Half-width of the quiet pool around the plate, px. */
const POOL_RX = 330;
/** Light from the upper left, so each ring has a lit flank and a shaded one. */
const LIGHT_X = -0.5;
const LIGHT_Y = -0.86;
const MAX_GLOW = 1200;

export const browserEnv: FieldEnv = {
  now: () => performance.now(),
  requestFrame: (cb) => requestAnimationFrame(cb),
  cancelFrame: (h) => cancelAnimationFrame(h),
  devicePixelRatio: () => window.devicePixelRatio || 1,
};

/** One fill style per (neon level, alpha level) bucket, so a frame is at most 196 fills however many dots move. */
function bucketFills({ dot, neon }: Palette): string[] {
  const fills: string[] = [];
  for (let k = 0; k < NEON_LEVELS; k++) {
    const m = k / (NEON_LEVELS - 1);
    const rgb = dot.map((c, i) => Math.round(c + (neon[i]! - c) * m));
    for (let a = 0; a < ALPHA_LEVELS; a++) {
      const alpha = Math.pow(a / (ALPHA_LEVELS - 1), 2);
      fills.push(`rgba(${rgb[0]},${rgb[1]},${rgb[2]},${alpha.toFixed(3)})`);
    }
  }
  return fills;
}

export function createDotField(
  canvas: HTMLCanvasElement,
  ctx: CanvasRenderingContext2D,
  options: { reducedMotion: boolean; palette: Palette; env?: FieldEnv },
): DotField {
  const env = options.env ?? browserEnv;
  let reduce = options.reducedMotion;

  let W = 0;
  let H = 0;
  let dpr = 1;
  let n = 0;
  let bx = new Float32Array(0);
  let by = new Float32Array(0);
  let seed = new Float32Array(0);
  let px = new Float32Array(0);
  let py = new Float32Array(0);
  let pr = new Float32Array(0);
  let bucket = new Uint16Array(0);
  let order = new Int32Array(0);
  const counts = new Int32Array(ALPHA_LEVELS * NEON_LEVELS);
  const starts = new Int32Array(ALPHA_LEVELS * NEON_LEVELS);
  const fills = bucketFills(options.palette);
  const bloom = `rgba(${options.palette.neon.join(',')},0.045)`;
  const glow = new Int32Array(MAX_GLOW);
  let glowN = 0;

  let calmArea: Rect | null = null;
  const pool = { x: 0, y: 0, rx: POOL_RX, ry: 190 };
  const ripples: Ripple[] = [];

  let handle = 0;
  let scheduled = false;
  let dirty = true;
  let dead = false;

  const currentDpr = () => Math.min(3, env.devicePixelRatio());

  const placePool = () => {
    if (calmArea) {
      pool.x = calmArea.left + calmArea.width / 2;
      pool.y = calmArea.top + calmArea.height / 2;
      pool.rx = POOL_RX;
      pool.ry = Math.max(150, calmArea.height * 0.62);
    } else {
      pool.x = W / 2;
      pool.y = H / 2;
      pool.rx = POOL_RX;
      pool.ry = 190;
    }
  };

  const build = () => {
    const box = canvas.getBoundingClientRect();
    W = Math.max(1, Math.round(box.width));
    H = Math.max(1, Math.round(box.height));
    dpr = currentDpr();
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    // Staggered rows: still reads as a dot grid, but samples round ripples far better than a square grid.
    const s = Math.max(MIN_SPACING, Math.sqrt((W * H) / (0.866 * MAX_DOTS)));
    const sy = s * 0.866;
    const cols = Math.floor(W / s) + 2;
    const rows = Math.floor(H / sy) + 2;
    const ox = (W - (cols - 1) * s) / 2;
    const oy = (H - (rows - 1) * sy) / 2;
    n = cols * rows;
    bx = new Float32Array(n);
    by = new Float32Array(n);
    seed = new Float32Array(n);
    px = new Float32Array(n);
    py = new Float32Array(n);
    pr = new Float32Array(n);
    bucket = new Uint16Array(n);
    order = new Int32Array(n);
    let i = 0;
    for (let r = 0; r < rows; r++)
      for (let c = 0; c < cols; c++, i++) {
        bx[i] = ox + c * s + (r % 2 ? s / 2 : 0) - s / 4;
        by[i] = oy + r * sy;
        seed[i] = Math.random();
      }
    placePool();
    dirty = true;
  };

  const frame = (nowMs: number) => {
    handle = 0;
    scheduled = false;
    if (dead) return;
    const t = nowMs / 1000;
    if (currentDpr() !== dpr) build();

    for (let k = ripples.length - 1; k >= 0; k--) {
      if (!isCalm(ripples[k]!, t)) continue;
      ripples.splice(k, 1);
      dirty = true; // draw the calm field once, even if frames paused past the ripple's end
    }

    // Reduced motion with nothing fading: stay idle until the next drop.
    if (reduce && ripples.length === 0 && !dirty) return;

    const rs = ripples.map((r) => rippleFrame(r, t));
    const base = Math.max(1.05, Math.min(1.35, W / 1100));

    counts.fill(0);
    glowN = 0;
    for (let i = 0; i < n; i++) {
      const x0 = bx[i]!;
      const y0 = by[i]!;
      let amb = 0.5;
      let dx = 0;
      let dy = 0;
      if (!reduce) {
        const a1 = Math.sin(x0 * 0.0058 + y0 * 0.0039 - t * 0.5);
        const a2 = Math.sin(-x0 * 0.0031 + y0 * 0.0068 - t * 0.29 + 1.7);
        amb = 0.5 + 0.25 * (a1 + a2);
        const sd = seed[i]! * 6.28;
        dx = Math.sin(y0 * 0.011 + t * 0.42 + sd) * 0.45;
        dy = Math.cos(x0 * 0.01 + t * 0.35 + sd) * 0.45;
      }

      let h = 0;
      let shade = 0;
      let f = 0;
      let lift = 0;
      for (let k = 0; k < rs.length; k++) {
        const q = rs[k]!;
        if (q.A === 0 && q.Ac === 0) continue;
        const ex = x0 - q.ripple.x;
        const ey = y0 - q.ripple.y;
        const d2 = ex * ex + ey * ey;
        if (q.ripple.soft) {
          lift += q.A * Math.exp(-d2 / (q.lam * q.lam));
          continue;
        }
        if (d2 > q.o2 || (d2 < q.i2 && d2 > 4900)) continue;
        const d = Math.sqrt(d2) || 0.001;
        const u = q.R - d; // distance behind the leading edge
        const lam = q.lam;
        let hk = 0;
        let sk = 0;
        if (u > -0.7 * lam && u < q.c + 2.4 * q.w) {
          const z = (u - q.c) / q.w;
          const edge = u < 0 ? Math.exp(-(u * u) / (0.035 * lam * lam)) : 1;
          const e = Math.exp(-z * z) * edge;
          const ph = (TAU * u) / lam + q.phase;
          const cs = Math.cos(ph);
          const sn = Math.sin(ph);
          hk = q.A * e * cs;
          // Slope along the radius, in height units (d/du scaled by lam / 2pi).
          sk = q.A * e * (((-2 * z * lam) / (q.w * TAU)) * cs - sn);
          const fu = (u - 0.22 * lam) / (0.32 * lam);
          f += q.A * q.youth * Math.exp(-fu * fu) * (0.55 + 0.45 * cs);
        }
        if (d < 70) hk += q.Ac * Math.exp(-d2 / 500);
        if (hk !== 0 || sk !== 0) {
          h += hk;
          const lr = (ex * LIGHT_X + ey * LIGHT_Y) / d;
          shade += sk * lr;
          // Seen through moving water the grid bends with the slope; crests also push dots outward.
          const push = ((0.55 * sk + 0.45 * hk) * q.ripple.push) / d;
          dx += ex * push;
          dy += ey * push;
        }
      }

      const pd = Math.exp(-(((x0 - pool.x) / pool.rx) ** 2) - ((y0 - pool.y) / pool.ry) ** 2);
      const quiet = 1 - 0.55 * pd;
      const crest = clamp(h, -1, 1.3);
      const lit = clamp(shade, -1, 1);
      const neon = clamp(f * 1.2, 0, 0.72);
      const alpha = clamp(
        (0.08 + 0.1 * amb) * quiet * Math.max(0, 1 + 1.1 * Math.min(0, crest) + 0.5 * Math.min(0, lit)) +
          0.62 * Math.max(0, crest) +
          0.42 * Math.max(0, lit) +
          0.5 * lift +
          0.3 * neon,
        0,
        1,
      );
      const rad = base * (0.78 + 0.42 * amb) * (1 + 0.85 * crest + 0.25 * lit + 0.6 * lift) + 0.4 * neon;

      px[i] = x0 + dx;
      py[i] = y0 + dy;
      pr[i] = Math.max(0.3, rad);
      const bi = Math.round(neon * (NEON_LEVELS - 1)) * ALPHA_LEVELS + Math.round(Math.sqrt(alpha) * (ALPHA_LEVELS - 1));
      bucket[i] = bi;
      counts[bi]!++;
      if (neon > 0.4 && alpha > 0.3 && glowN < MAX_GLOW) glow[glowN++] = i;
    }

    // Sort dots into fill buckets (counting sort), so each bucket is one path and one fill.
    let acc = 0;
    for (let b = 0; b < counts.length; b++) {
      starts[b] = acc;
      acc += counts[b]!;
    }
    for (let i = 0; i < n; i++) order[starts[bucket[i]!]!++] = i;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);

    // A soft neon bloom under the wavefront dots.
    if (glowN) {
      ctx.fillStyle = bloom;
      ctx.beginPath();
      for (let g = 0; g < glowN; g++) {
        const i = glow[g]!;
        const r = pr[i]! * 2.4;
        ctx.moveTo(px[i]! + r, py[i]!);
        ctx.arc(px[i]!, py[i]!, r, 0, TAU);
      }
      ctx.fill();
    }

    let at = 0;
    for (let b = 0; b < counts.length; b++) {
      const c = counts[b]!;
      if (!c) continue;
      if (b % ALPHA_LEVELS === 0) {
        at += c; // fully transparent
        continue;
      }
      ctx.fillStyle = fills[b]!;
      ctx.beginPath();
      for (let j = at; j < at + c; j++) {
        const i = order[j]!;
        ctx.moveTo(px[i]! + pr[i]!, py[i]!);
        ctx.arc(px[i]!, py[i]!, pr[i]!, 0, TAU);
      }
      ctx.fill();
      at += c;
    }
    dirty = false;
    schedule();
  };

  const schedule = () => {
    if (scheduled || dead) return;
    scheduled = true;
    handle = env.requestFrame(frame);
  };

  const wake = () => {
    dirty = true;
    schedule();
  };

  build();
  schedule();

  return {
    resize() {
      build();
      schedule();
    },
    drop(x, y, kind) {
      ripples.push(makeRipple(x, y, env.now() / 1000, kind, reduce));
      if (ripples.length > MAX_RIPPLES) ripples.shift();
      wake();
    },
    setCalm(area) {
      calmArea = area;
      placePool();
      wake();
    },
    setReducedMotion(next) {
      reduce = next;
      wake();
    },
    destroy() {
      dead = true;
      if (scheduled) env.cancelFrame(handle);
      scheduled = false;
      ripples.length = 0;
    },
  };
}

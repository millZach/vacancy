// PROTOTYPE variant D: the living dot field behind the scan plate. One canvas, one requestAnimationFrame loop and no
// React state per frame. The field breathes slowly; a click drops a water ripple (a crest that pushes dots outward,
// a damped wake behind it, a bob at the impact point); the Scan button sends a big one; the scan sweep lifts the dots
// it passes. Reduced motion: no ambient movement, and a click is a single soft fade.
import { useEffect, useImperativeHandle, useRef, type Ref } from 'react';

export interface DotFieldHandle {
  /** Drops a ripple at a point in client coordinates. `big` is the Scan button's splash. */
  drop: (clientX: number, clientY: number, big?: boolean) => void;
}

interface Props {
  ref?: Ref<DotFieldHandle>;
  /** performance.now() when the scan started, or null before it. */
  scanStart: number | null;
  scanMs: number;
  /** Fades the field out (end of the scan, the map has covered it). */
  out?: boolean;
}

interface Ripple {
  x: number;
  y: number;
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
  soft?: boolean;
}

/** Clicks on these never ripple: controls, the note and the scan readout. (The map ignores pointers while scanning.) */
const IGNORE = 'button, a, input, [role="button"], .va-agent-note, .va-hud';
const MAX_DOTS = 4200;
const MIN_SPACING = 15;
const MAX_RIPPLES = 12;
const AL = 28; // alpha levels
const NL = 7; // neon levels
const DOT = [196, 205, 219];
const NEON = [255, 79, 179];
const TAU = Math.PI * 2;

const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : v > hi ? hi : v);
const smooth = (a: number, b: number, x: number) => {
  const t = clamp((x - a) / (b - a), 0, 1);
  return t * t * (3 - 2 * t);
};

export function DotField({ ref, scanStart, scanMs, out }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ripples = useRef<Ripple[]>([]);
  const scan = useRef<number | null>(scanStart);
  scan.current = scanStart;
  const wake = useRef<() => void>(() => {});

  const toLocal = (cx: number, cy: number) => {
    const c = canvasRef.current;
    if (!c) return null;
    const b = c.getBoundingClientRect();
    return { x: cx - b.left, y: cy - b.top };
  };

  const add = (x: number, y: number, big: boolean, small = false) => {
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    const t0 = performance.now() / 1000;
    const r: Ripple = reduce
      ? { x, y, t0, amp: big ? 0.9 : 0.75, v: 0, lam: big ? 190 : 120, life: big ? 1.3 : 0.9, push: 0, soft: true }
      : big
        ? { x, y, t0, amp: 1.6, v: 470, lam: 80, life: 2.9, push: 9 }
        : small
          ? { x, y, t0, amp: 0.42, v: 230, lam: 46, life: 1.5, push: 4 }
          : { x, y, t0, amp: 1.35, v: 300, lam: 60, life: 2.3, push: 6.5 };
    const list = ripples.current;
    list.push(r);
    if (list.length > MAX_RIPPLES) list.shift();
    wake.current();
  };

  useImperativeHandle(ref, () => ({
    drop(cx, cy, big) {
      const p = toLocal(cx, cy);
      if (p) add(p.x, p.y, !!big);
    },
  }));

  useEffect(() => {
    const canvas = canvasRef.current!;
    const host = canvas.parentElement!;
    const ctx = canvas.getContext('2d')!;
    const mq = matchMedia('(prefers-reduced-motion: reduce)');
    let reduce = mq.matches;

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
    const counts = new Int32Array(AL * NL);
    const starts = new Int32Array(AL * NL);
    const fills: string[] = [];
    for (let k = 0; k < NL; k++) {
      const m = k / (NL - 1);
      const rgb = DOT.map((c, i) => Math.round(c + (NEON[i] - c) * m));
      for (let a = 0; a < AL; a++) {
        const alpha = Math.pow(a / (AL - 1), 2);
        fills.push(`rgba(${rgb[0]},${rgb[1]},${rgb[2]},${alpha.toFixed(3)})`);
      }
    }
    let pool = { x: 0, y: 0, rx: 300, ry: 190, ok: false };
    let glowN = 0;
    const glow = new Int32Array(1200);

    const build = () => {
      const box = canvas.getBoundingClientRect();
      W = Math.max(1, Math.round(box.width));
      H = Math.max(1, Math.round(box.height));
      dpr = Math.min(3, window.devicePixelRatio || 1);
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
      pool.ok = false;
      dirty = true;
    };

    const measurePool = () => {
      const plate = host.querySelector('.va-plate h1')?.parentElement;
      const cb = canvas.getBoundingClientRect();
      if (plate) {
        // The plate fills the stage and centres its content; pool around the content, not the whole plate.
        const kids = [...plate.children] as HTMLElement[];
        let top = Infinity;
        let bottom = -Infinity;
        for (const k of kids) {
          const r = k.getBoundingClientRect();
          if (r.height) {
            top = Math.min(top, r.top);
            bottom = Math.max(bottom, r.bottom);
          }
        }
        const pb = plate.getBoundingClientRect();
        pool = { x: pb.left + pb.width / 2 - cb.left, y: (top + bottom) / 2 - cb.top, rx: 330, ry: Math.max(150, (bottom - top) * 0.62), ok: true };
      } else pool = { x: W / 2, y: H / 2, rx: 330, ry: 190, ok: true };
    };

    let raf = 0;
    let running = false;
    let dirty = true;
    let lastDrop = 0;

    const frame = (nowMs: number) => {
      raf = 0;
      running = false;
      const t = nowMs / 1000;
      if (Math.min(3, window.devicePixelRatio || 1) !== dpr) build();
      if (!pool.ok) measurePool();

      // Retire calm ripples.
      const list = ripples.current;
      for (let k = list.length - 1; k >= 0; k--) if (t - list[k].t0 > list[k].life) list.splice(k, 1);

      const scanAt = scan.current;
      const sp = scanAt === null ? -1 : (nowMs - scanAt) / scanMs;
      const scanning = sp >= 0 && sp <= 1.05;
      // The scan sweep drops small droplets as it "finds" folders.
      if (scanning && !reduce && sp < 0.92 && nowMs - lastDrop > 260) {
        lastDrop = nowMs;
        const xs = -60 + (W + 120) * smooth(0, 1, sp);
        add(clamp(xs + (Math.random() - 0.3) * 90, 0, W), Math.random() * H, false, true);
      }

      const animate = !reduce || list.length > 0 || dirty;
      if (!animate) return; // reduced motion and nothing to show: stay idle until the next drop

      // Per-ripple values that do not depend on the dot. Each ripple is a wave packet: a few rings under a Gaussian
      // envelope that trails the leading edge, rings spreading apart as it travels (dispersion), crests running
      // outward through the packet the way capillary ripples do, and amplitude falling with distance and age.
      const rs = list.map((r) => {
        const age = t - r.t0;
        const k = age / r.life;
        if (r.soft) {
          const a = r.amp * (1 - smooth(0, 1, k)) * smooth(0, 0.12, k);
          return { r, age, R: 0, lam: r.lam, A: a, Ac: 0, c: 0, w: 1, phase: 0, youth: 0, o2: Infinity, i2: 0 };
        }
        const R = r.v * age * (1 - 0.3 * k);
        const lam = r.lam * (1 + 0.6 * k);
        const A = (r.amp * smooth(0, 0.05, age) * Math.exp(-1.25 * k) * (1 - k * k * k * k)) / Math.sqrt(1 + R / 420);
        const Ac = r.amp * 0.85 * Math.exp(-age * 4) * Math.cos(age * TAU * 2.6);
        const c = 1.25 * lam;
        const w = 1.65 * lam;
        // The annulus the packet occupies, squared, so dots outside it skip the maths.
        const out = R + 0.7 * lam;
        const inn = Math.max(0, R - c - 2.4 * w);
        return { r, age, R, lam, A, Ac, c, w, phase: age * TAU * 0.9, youth: 1 - smooth(0.08, 0.66, k), o2: out * out, i2: inn * inn };
      });

      const poolK = scanAt === null ? 1 : Math.max(0, 1 - (nowMs - scanAt) / 500);
      const xs = scanning ? -60 + (W + 120) * smooth(0, 1, sp) : -1e4;
      const base = Math.max(1.05, Math.min(1.35, W / 1100));
      // Light from the upper left, so each ring has a lit flank and a shaded one.
      const LX = -0.5;
      const LY = -0.86;

      counts.fill(0);
      glowN = 0;
      for (let i = 0; i < n; i++) {
        const x0 = bx[i];
        const y0 = by[i];
        let amb = 0.5;
        let dx = 0;
        let dy = 0;
        if (!reduce) {
          const a1 = Math.sin(x0 * 0.0058 + y0 * 0.0039 - t * 0.5);
          const a2 = Math.sin(-x0 * 0.0031 + y0 * 0.0068 - t * 0.29 + 1.7);
          amb = 0.5 + 0.25 * (a1 + a2);
          const sd = seed[i] * 6.28;
          dx = Math.sin(y0 * 0.011 + t * 0.42 + sd) * 0.45;
          dy = Math.cos(x0 * 0.010 + t * 0.35 + sd) * 0.45;
        }

        let h = 0;
        let shade = 0;
        let f = 0;
        let lift = 0;
        for (let k = 0; k < rs.length; k++) {
          const q = rs[k];
          if (q.A === 0 && q.Ac === 0) continue;
          const ex = x0 - q.r.x;
          const ey = y0 - q.r.y;
          const d2 = ex * ex + ey * ey;
          if (q.r.soft) {
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
            sk = q.A * e * ((-2 * z * lam) / (q.w * TAU) * cs - sn);
            const fu = (u - 0.22 * lam) / (0.32 * lam);
            f += q.A * q.youth * Math.exp(-fu * fu) * (0.55 + 0.45 * cs);
          }
          if (d < 70) hk += q.Ac * Math.exp(-d2 / 500);
          if (hk !== 0 || sk !== 0) {
            h += hk;
            const lr = (ex * LX + ey * LY) / d;
            shade += sk * lr;
            // Seen through moving water the grid bends with the slope; crests also push dots outward.
            const push = ((0.55 * sk + 0.45 * hk) * q.r.push) / d;
            dx += ex * push;
            dy += ey * push;
          }
        }

        let sweep = 0;
        if (scanning) {
          const off = x0 - xs - 22 * Math.sin(y0 * 0.011 + t * 1.6);
          sweep = Math.exp(-(off * off) / 2600) * (1 - smooth(0.85, 1.05, sp));
          if (off < 0) sweep += 0.035;
        }

        const pd = poolK * Math.exp(-(((x0 - pool.x) / pool.rx) ** 2) - ((y0 - pool.y) / pool.ry) ** 2);
        const quiet = 1 - 0.55 * pd;
        const crest = clamp(h, -1, 1.3);
        const lit = clamp(shade, -1, 1);
        const neon = clamp(f * 1.2, 0, 0.72);
        let alpha =
          (0.08 + 0.1 * amb) * quiet * Math.max(0, 1 + 1.1 * Math.min(0, crest) + 0.5 * Math.min(0, lit)) +
          0.62 * Math.max(0, crest) +
          0.42 * Math.max(0, lit) +
          0.3 * sweep +
          0.5 * lift +
          0.3 * neon;
        alpha = clamp(alpha, 0, 1);
        const rad = base * (0.78 + 0.42 * amb) * (1 + 0.85 * crest + 0.25 * lit + 0.5 * sweep + 0.6 * lift) + 0.4 * neon;

        px[i] = x0 + dx;
        py[i] = y0 + dy;
        pr[i] = Math.max(0.3, rad);
        const bi = Math.round(neon * (NL - 1)) * AL + Math.round(Math.sqrt(alpha) * (AL - 1));
        bucket[i] = bi;
        counts[bi]++;
        if (neon > 0.4 && alpha > 0.3 && glowN < glow.length) glow[glowN++] = i;
      }

      let acc = 0;
      for (let b = 0; b < counts.length; b++) {
        starts[b] = acc;
        acc += counts[b];
      }
      for (let i = 0; i < n; i++) order[starts[bucket[i]]++] = i;

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);

      // A soft neon bloom under the wavefront dots.
      if (glowN) {
        ctx.fillStyle = 'rgba(255,79,179,0.045)';
        ctx.beginPath();
        for (let g = 0; g < glowN; g++) {
          const i = glow[g];
          const r = pr[i] * 2.4;
          ctx.moveTo(px[i] + r, py[i]);
          ctx.arc(px[i], py[i], r, 0, TAU);
        }
        ctx.fill();
      }

      let at = 0;
      for (let b = 0; b < counts.length; b++) {
        const c = counts[b];
        if (!c) continue;
        if (b % AL === 0) {
          at += c;
          continue; // fully transparent
        }
        ctx.fillStyle = fills[b];
        ctx.beginPath();
        for (let j = at; j < at + c; j++) {
          const i = order[j];
          ctx.moveTo(px[i] + pr[i], py[i]);
          ctx.arc(px[i], py[i], pr[i], 0, TAU);
        }
        ctx.fill();
        at += c;
      }
      dirty = false;
      schedule();
    };

    const schedule = () => {
      if (running) return;
      running = true;
      raf = requestAnimationFrame(frame);
    };
    wake.current = () => {
      dirty = true;
      schedule();
    };

    const ro = new ResizeObserver(() => {
      build();
      schedule();
    });
    ro.observe(canvas);
    build();
    schedule();

    const onDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      const target = e.target as Element | null;
      if (target?.closest(IGNORE)) return;
      const p = toLocal(e.clientX, e.clientY);
      if (p) add(p.x, p.y, false);
    };
    host.addEventListener('pointerdown', onDown);
    const onMq = () => {
      reduce = mq.matches;
      wake.current();
    };
    mq.addEventListener('change', onMq);

    return () => {
      cancelAnimationFrame(raf);
      running = false;
      wake.current = () => {};
      ro.disconnect();
      host.removeEventListener('pointerdown', onDown);
      mq.removeEventListener('change', onMq);
    };
  }, [scanMs]);

  // A scan in reduced motion still needs one redraw so the pool around the plate clears.
  useEffect(() => wake.current(), [scanStart]);

  return <canvas ref={canvasRef} className={`vd-field ${out ? 'is-out' : ''}`} aria-hidden />;
}

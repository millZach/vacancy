// PROTOTYPE feed primitives for variant B: concierge messages, your replies, toggles, hold-to-confirm.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { motion, useReducedMotion } from 'motion/react';
import { ConciergeBell, Check } from 'lucide-react';

/** A line from the concierge. `avatar` hangs the bell mark in the left margin to open a new turn. */
export function Say({ avatar, children, className = '', delay = 0, animate = false }: {
  avatar?: boolean; children: ReactNode; className?: string; delay?: number; animate?: boolean;
}) {
  const reduced = useReducedMotion();
  const body = (
    <>
      {avatar && (
        <span className="vb-avatar" aria-hidden>
          <ConciergeBell size={16} strokeWidth={1.75} />
        </span>
      )}
      {children}
    </>
  );
  const cls = `vb-say ${avatar ? 'vb-say-turn' : ''} ${className}`;
  if (!animate || reduced) return <div className={cls}>{body}</div>;
  return (
    <motion.div
      className={cls}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.38, delay, ease: [0.2, 0.7, 0.2, 1] }}
    >
      {body}
    </motion.div>
  );
}

/** Your side of the conversation: what you asked the concierge to do. */
export function You({ children }: { children: ReactNode }) {
  return (
    <div className="vb-you">
      <span>{children}</span>
    </div>
  );
}

/** Wraps a card so findings can arrive one after another. */
export function Arrive({ children, delay = 0, animate }: { children: ReactNode; delay?: number; animate: boolean }) {
  const reduced = useReducedMotion();
  if (!animate || reduced) return <div className="vb-arrive">{children}</div>;
  return (
    <motion.div
      className="vb-arrive"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.42, delay, ease: [0.2, 0.7, 0.2, 1] }}
    >
      {children}
    </motion.div>
  );
}

/** Two-way choice, e.g. Keep | Clean. `on` means the second option is chosen. */
export function Choice({ on, off: offLabel, onLabel, onChange, label }: {
  on: boolean; off: string; onLabel: string; onChange: (next: boolean) => void; label: string;
}) {
  return (
    <div className="vb-choice" role="radiogroup" aria-label={label}>
      <button type="button" role="radio" aria-checked={!on} className={!on ? 'vb-choice-on' : ''} onClick={() => onChange(false)}>
        {offLabel}
      </button>
      <button type="button" role="radio" aria-checked={on} className={on ? 'vb-choice-on vb-choice-go' : ''} onClick={() => onChange(true)}>
        {on && <Check size={13} strokeWidth={2.5} />}
        {onLabel}
      </button>
    </div>
  );
}

/** Press and hold to confirm. Releasing early cancels. Works with pointer and with Space/Enter held down. */
export function HoldButton({ label, holdingLabel, ms = 1300, onDone, disabled }: {
  label: string; holdingLabel: string; ms?: number; onDone: () => void; disabled?: boolean;
}) {
  const [holding, setHolding] = useState(false);
  const timer = useRef<number | null>(null);
  const done = useRef(false);

  const start = () => {
    if (disabled || timer.current !== null || done.current) return;
    setHolding(true);
    timer.current = window.setTimeout(() => {
      done.current = true;
      timer.current = null;
      onDone();
    }, ms);
  };
  const stop = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
    if (!done.current) setHolding(false);
  };
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);

  return (
    <button
      type="button"
      className={`vb-hold ${holding ? 'vb-hold-active' : ''}`}
      style={{ ['--vb-hold-ms' as string]: `${ms}ms` }}
      disabled={disabled}
      onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); start(); }}
      onPointerUp={stop}
      onPointerCancel={stop}
      onLostPointerCapture={stop}
      onKeyDown={(e) => { if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) { e.preventDefault(); start(); } }}
      onKeyUp={(e) => { if (e.key === ' ' || e.key === 'Enter') stop(); }}
      onBlur={stop}
    >
      <span className="vb-hold-fill" aria-hidden />
      <span className="vb-hold-label">{holding ? holdingLabel : label}</span>
    </button>
  );
}

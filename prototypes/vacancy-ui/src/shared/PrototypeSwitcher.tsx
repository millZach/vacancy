// PROTOTYPE floating switcher. Not part of any design being evaluated.
import { useEffect } from 'react';
import type { OS } from './data';
import { STEPS, type Step } from './types';

interface Props {
  variants: { key: string; name: string }[];
  current: string;
  os: OS;
  step: Step;
  onVariant: (key: string) => void;
  onOs: (os: OS) => void;
  onStep: (s: Step) => void;
}

export function PrototypeSwitcher({ variants, current, os, step, onVariant, onOs, onStep }: Props) {
  const i = Math.max(0, variants.findIndex((v) => v.key === current));
  const go = (d: number) => onVariant(variants[(i + d + variants.length) % variants.length].key);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      if (e.key === 'ArrowLeft') go(-1);
      if (e.key === 'ArrowRight') go(1);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  return (
    <div className="ps-bar" role="toolbar" aria-label="Prototype switcher">
      <button className="ps-arrow" onClick={() => go(-1)} aria-label="Previous variant">&#x2190;</button>
      <span className="ps-label">{variants[i].key} &mdash; {variants[i].name}</span>
      <button className="ps-arrow" onClick={() => go(1)} aria-label="Next variant">&#x2192;</button>
      <span className="ps-sep" />
      {(['win', 'mac'] as OS[]).map((o) => (
        <button key={o} className={`ps-pill ${o === os ? 'on' : ''}`} onClick={() => onOs(o)}>{o === 'win' ? 'Windows' : 'Mac'}</button>
      ))}
      <span className="ps-sep" />
      {STEPS.map((s) => (
        <button key={s} className={`ps-pill ${s === step ? 'on' : ''}`} onClick={() => onStep(s)}>{s}</button>
      ))}
    </div>
  );
}

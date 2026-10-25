// PROTOTYPE (throwaway): the floating variant switcher. ←/→ cycle A, B and C (not while typing); the toggle picks
// model=pick|show|none. Hidden in production builds and without ?claude=A|B|C.
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { useEffect } from 'react';
import { PROTO_LABELS, PROTO_MODES, PROTO_VARIANTS, proto, useProto, type ProtoVariant } from './protoState';
import './proto.css';

function step(v: ProtoVariant, by: number): ProtoVariant {
  const i = PROTO_VARIANTS.indexOf(v);
  return PROTO_VARIANTS[(i + by + PROTO_VARIANTS.length) % PROTO_VARIANTS.length]!;
}

function typing(el: Element | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  return el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName);
}

export function ProtoSwitcher() {
  const s = useProto();
  const variant = s.variant;
  useEffect(() => {
    if (!variant) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || typing(document.activeElement)) return;
      if (e.key === 'ArrowLeft') proto.setVariant(step(variant, -1));
      else if (e.key === 'ArrowRight') proto.setVariant(step(variant, 1));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [variant]);
  if (import.meta.env.PROD || !variant) return null;
  return (
    <div className="proto-switch" data-no-ripple role="toolbar" aria-label="Prototype variants">
      <button type="button" onClick={() => proto.setVariant(step(variant, -1))} aria-label="Previous variant">
        <ChevronLeft size={16} aria-hidden />
      </button>
      <span className="proto-switch-label">{PROTO_LABELS[variant]}</span>
      <button type="button" onClick={() => proto.setVariant(step(variant, 1))} aria-label="Next variant">
        <ChevronRight size={16} aria-hidden />
      </button>
      <span className="proto-switch-sep" />
      <span className="proto-switch-k">model</span>
      <span className="proto-switch-seg">
        {PROTO_MODES.map((m) => (
          <button key={m} type="button" aria-pressed={s.model === m} className={s.model === m ? 'is-on' : ''} onClick={() => proto.setModelMode(m)}>
            {m}
          </button>
        ))}
      </span>
    </div>
  );
}

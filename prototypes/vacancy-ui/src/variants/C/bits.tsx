// PROTOTYPE variant C: small shared pieces (risk badge, source chip, checkbox, size bar).
import type { CSSProperties } from 'react';
import { Check, Minus, SquareTerminal } from 'lucide-react';
import type { Dataset, Risk } from '../../shared/data';
import { sourceLabel } from './model';

const RISK_TEXT: Record<Risk, string> = { safe: 'Safe', review: 'Review', careful: 'Careful' };

export function RiskBadge({ risk }: { risk: Risk }) {
  return (
    <span className={`vc-risk vc-risk-${risk}`}>
      <i aria-hidden />
      {RISK_TEXT[risk]}
    </span>
  );
}

export function SourceChip({ data, source }: { data: Dataset; source: 'rule' | 'ai' }) {
  if (source === 'rule') return <span className="vc-src">Rule</span>;
  return (
    <span className="vc-src vc-src-ai">
      <SquareTerminal size={12} strokeWidth={2.2} />
      {sourceLabel(data, { source })}
    </span>
  );
}

export function Tick({ on, mixed, onClick, label }: { on: boolean; mixed?: boolean; onClick: () => void; label: string }) {
  return (
    <span
      role="checkbox"
      aria-checked={mixed ? 'mixed' : on}
      aria-label={label}
      className={`vc-tick ${on ? 'on' : ''} ${mixed ? 'mixed' : ''}`}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {mixed ? <Minus size={11} strokeWidth={3} /> : on ? <Check size={11} strokeWidth={3.2} /> : null}
    </span>
  );
}

export function SizeBar({ frac, color, dim, dashed, sub }: { frac: number; color: string; dim?: boolean; dashed?: boolean; sub?: number }) {
  return (
    <span className={`vc-sizebar ${dim ? 'dim' : ''} ${dashed ? 'dashed' : ''}`} style={{ '--c': color } as CSSProperties}>
      <i style={{ width: `${Math.min(1, Math.max(0.012, frac)) * 100}%` }} />
      {sub != null && <b style={{ width: `${Math.min(1, sub) * 100}%` }} />}
    </span>
  );
}

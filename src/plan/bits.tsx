import { Check, Sparkles } from 'lucide-react';
import type { ProposalSource, Risk } from '../bridge';

const RISK_LABEL: Record<Risk, string> = { safe: 'Safe', review: 'Review', careful: 'Careful' };

/** How careful to be with a proposal: Safe, Review or Careful, in its colour. */
export function RiskChip({ risk }: { risk: Risk }) {
  return (
    <span className={`risk risk--${risk}`}>
      <i aria-hidden />
      {RISK_LABEL[risk]}
    </span>
  );
}

/** Who proposed it: a built-in rule, or the AI agent (ticket 07). */
export function SourceTag({ source, agent }: { source: ProposalSource; agent: string | null }) {
  if (source === 'ai')
    return (
      <span className="source source--ai">
        <Sparkles size={11} strokeWidth={2.2} aria-hidden />
        Suggested by {agent ?? 'your AI agent'}
      </span>
    );
  return <span className="source">Built-in rule</span>;
}

/** A tick box drawn the variant D way, with a real checkbox's role and name. */
export function Checkbox({ on, onChange, label }: { on: boolean; onChange: () => void; label: string }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={on}
      aria-label={label}
      className={on ? 'check is-on' : 'check'}
      onClick={(e) => {
        e.stopPropagation();
        onChange();
      }}
    >
      {on && <Check size={11} strokeWidth={3} aria-hidden />}
    </button>
  );
}

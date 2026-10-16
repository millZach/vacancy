// PROTOTYPE variant A "Atlas": small shared pieces (chips, checkboxes, trees, wordmark).
import { Check, File, Folder, Sparkles } from 'lucide-react';
import type { AiAgent, Proposal, Risk, TreeNode } from '../../shared/data';
import { formatCount } from '../../shared/data';

const RISK_LABEL: Record<Risk, string> = { safe: 'Safe', review: 'Review', careful: 'Careful' };

export function RiskChip({ risk }: { risk: Risk }) {
  return (
    <span className={`va-risk va-risk--${risk}`}>
      <i />
      {RISK_LABEL[risk]}
    </span>
  );
}

export function SourceTag({ p, agent }: { p: { source: Proposal['source'] }; agent: AiAgent | null }) {
  if (p.source === 'ai')
    return (
      <span className="va-src va-src--ai">
        <Sparkles size={11} strokeWidth={2.2} />
        Suggested by {agent ?? 'your AI agent'}
      </span>
    );
  return <span className="va-src">Built-in rule</span>;
}

export function AgentQuote({ agent, note }: { agent: AiAgent | null; note: string }) {
  return (
    <blockquote className="va-quote">
      <span className="va-quote-who">
        <Sparkles size={11} strokeWidth={2.2} /> {agent ?? 'AI agent'} says
      </span>
      <p>{note}</p>
    </blockquote>
  );
}

export function Checkbox({ on, onChange, label, disabled }: { on: boolean; onChange: () => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      className={`va-check ${on ? 'is-on' : ''}`}
      onClick={(e) => {
        e.stopPropagation();
        onChange();
      }}
    >
      {on && <Check size={11} strokeWidth={3} />}
    </button>
  );
}

export function Wordmark({ lit }: { lit: boolean }) {
  return (
    <span className={`va-wordmark ${lit ? 'is-lit' : ''}`} aria-label="Vacancy">
      Vacancy
    </span>
  );
}

export function Tree({ node, depth = 0, open = true }: { node: TreeNode; depth?: number; open?: boolean }) {
  const isFolder = !!node.children || (node.count !== undefined && !/^\(|^\.\.\./.test(node.name) && !/\.\w{2,5}$/.test(node.name));
  const muted = /^\(|^\.\.\./.test(node.name);
  return (
    <div className="va-tree-node">
      <div className={`va-tree-row ${muted ? 'is-muted' : ''}`} style={{ paddingLeft: depth * 16 }}>
        {isFolder ? <Folder size={13} strokeWidth={1.8} /> : <File size={13} strokeWidth={1.8} />}
        <span className="va-tree-name">{node.name}</span>
        {node.count !== undefined && <span className="va-tree-count">{formatCount(node.count)}</span>}
      </div>
      {open && node.children?.map((c, i) => <Tree key={i} node={c} depth={depth + 1} />)}
    </div>
  );
}

function hash(s: string) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return () => {
    h = Math.imul(h ^ (h >>> 15), 2246822507);
    h = Math.imul(h ^ (h >>> 13), 3266489909);
    return ((h ^= h >>> 16) >>> 0) / 4294967296;
  };
}

/** A pile of loose files on the left, the sorted folders on the right. */
export function ReorgGlyph({ seed, after }: { seed: string; after: TreeNode }) {
  const rnd = hash(seed);
  const dots = Array.from({ length: 22 }, () => ({ x: 3 + rnd() * 40, y: 4 + rnd() * 22 }));
  const top = (after.children ?? []).filter((c) => !c.name.startsWith('(') && !/unchanged/.test(c.name) && (c.count ?? 0) > 0);
  const groups = (top.length < 3 ? top.flatMap((c) => c.children ?? [c]) : top).slice(0, 6);
  const max = Math.max(1, ...groups.map((g) => g.count ?? 0));
  return (
    <svg className="va-glyph" width="128" height="30" viewBox="0 0 128 30" aria-hidden>
      {dots.map((d, i) => (
        <rect key={i} x={d.x} y={d.y} width="3" height="3" rx="0.6" />
      ))}
      <path d="M54 15h14m-4-4 4 4-4 4" className="va-glyph-arrow" />
      {groups.map((g, i) => {
        const h = 4 + ((g.count ?? 0) / max) * 22;
        return <rect key={g.name} x={78 + i * 8} y={28 - h} width="5" height={h} rx="1" className="va-glyph-bar" />;
      })}
    </svg>
  );
}

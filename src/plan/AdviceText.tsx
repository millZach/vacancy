import type { HintAction } from '../bindings/HintAction';
import guides from '../data/toolGuidance.json';
import { HintActionButton } from './HintActionButton';
import './advice.css';

/** Stored instructions remain text in history. Only a compiled guide kind exposes a reference action;
 * its URL is resolved by the backend from the retained scan, never from this text or saved report. */
export function AdviceText({ why, action, mountPoint, proposalId, compact = false }: {
  why: string; action?: HintAction | null | undefined; mountPoint: string; proposalId: string; compact?: boolean;
}) {
  const guide = guides.find(g => g.action.kind === action?.kind);
  if (!guide) return <span className="advice-why">{why}</span>;
  const lines = why.split('\n');
  const steps = lines.filter(line => /^\d+\. /.test(line)).map(line => line.replace(/^\d+\. /, ''));
  const notes = lines.filter(line => !/^\d+\. /.test(line));
  const content = <div className="guide-text">
    <p>{notes[0]}</p>
    <ol>{steps.map(step => <li key={step}>{step}</li>)}</ol>
    {notes.slice(1).map((note, i) => <p key={i}>{note}</p>)}
    <HintActionButton mountPoint={mountPoint} proposalId={proposalId} label={`${guide.sourceLabel} ↗`} />
  </div>;
  return compact ? <details className="guide-details"><summary>Instructions and reference</summary>{content}</details> : content;
}

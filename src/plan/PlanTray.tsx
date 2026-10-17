import { ChevronDown, PanelRightClose } from 'lucide-react';
import { useId, useState } from 'react';
import type { Proposal } from '../bridge';
import { formatBytes, formatCount } from '../format';
import { Checkbox, RiskChip, SourceTag } from './bits';
import { planTotals, tileOf } from './totals';
import './plan.css';

interface Props {
  /** The drive's proposals, biggest first. */
  proposals: Proposal[];
  /** The ids in the drive's plan. */
  plan: string[];
  /** The AI agent Vacancy found, or null, for AI proposals' source tags. */
  agent: string | null;
  /** The map tile under the pointer, by tile id, so a row and its tile light together. */
  hot: string | null;
  onHot: (tile: string | null) => void;
  onToggle: (id: string) => void;
  onClose: () => void;
}

/** Why the plan's button can't run yet. Running a plan arrives with ticket 04. */
export const NOT_YET = 'Vacancy can’t run plans yet. This version only proposes.';

/**
 * Variant D's plan tray, beside the map: the drive's delete proposals with tick boxes, risk chips, a source tag and
 * an expandable why with sample paths; and the plan's total, with the button that will run it.
 */
export function PlanTray({ proposals, plan, agent, hot, onHot, onToggle, onClose }: Props) {
  const deletes = proposals.filter((p) => p.action === 'delete');
  const on = deletes.filter((p) => plan.includes(p.id)).length;
  const t = planTotals(proposals, plan);
  const later = useId();
  return (
    <aside className="tray" aria-label="Plan" data-no-ripple>
      <div className="tray-head">
        <div>
          <h2>Plan</h2>
          <p>Click glowing areas on the map, or tick them here. Nothing changes until you confirm.</p>
        </div>
        <button type="button" className="icon-btn" onClick={onClose} aria-label="Hide the plan">
          <PanelRightClose size={16} aria-hidden />
        </button>
      </div>

      <div className="tray-scroll">
        <section className="tray-group" aria-label="Delete">
          <h3>
            Delete{' '}
            <span>
              {on} of {deletes.length}
            </span>
          </h3>
          {deletes.length === 0 && <p className="tray-empty">Vacancy found nothing it can clean up on this drive.</p>}
          {deletes.map((p) => (
            <ProposalRow
              key={p.id}
              proposal={p}
              agent={agent}
              on={plan.includes(p.id)}
              hot={hot === tileOf(p.id)}
              onToggle={() => onToggle(p.id)}
              onHot={onHot}
            />
          ))}
        </section>
      </div>

      <div className="tray-foot">
        <div className="tray-total">
          <span className="tray-total-num">{formatBytes(t.bytes)}</span>
          <span className="tray-total-lbl">will be freed</span>
        </div>
        <ul className="tray-notes">
          <li>
            {formatCount(t.items)} {t.items === 1 ? 'item' : 'items'} in {t.places} {t.places === 1 ? 'place' : 'places'},
            deleted permanently
          </li>
        </ul>
        <button type="button" className="cta tray-cta" disabled aria-describedby={later} title={NOT_YET}>
          {t.bytes > 0 ? `Free ${formatBytes(t.bytes)}` : 'Nothing selected yet'}
        </button>
        <p className="tray-later" id={later}>
          {NOT_YET}
        </p>
      </div>
    </aside>
  );
}

interface RowProps {
  proposal: Proposal;
  agent: string | null;
  on: boolean;
  hot: boolean;
  onToggle: () => void;
  onHot: (tile: string | null) => void;
}

/** One proposal: tick box, title (which opens the why and sample paths), summary, risk, source and size. */
function ProposalRow({ proposal: p, agent, on, hot, onToggle, onHot }: RowProps) {
  const [open, setOpen] = useState(false);
  const more = useId();
  const cls = ['tray-row', on && 'is-on', hot && 'is-hot'].filter(Boolean).join(' ');
  return (
    <div className={cls} onMouseEnter={() => onHot(tileOf(p.id))} onMouseLeave={() => onHot(null)}>
      <Checkbox on={on} onChange={onToggle} label={`Include ${p.title}`} />
      <div className="tray-row-main">
        <button
          type="button"
          className="tray-row-title"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          aria-controls={more}
        >
          <span>{p.title}</span>
          <ChevronDown size={13} className={open ? 'is-open' : undefined} aria-hidden />
        </button>
        <p className="tray-row-sum">{p.summary}</p>
        <div className="tray-row-meta">
          <RiskChip risk={p.risk} />
          <SourceTag source={p.source} agent={agent} />
        </div>
        {open && (
          <div className="tray-row-more" id={more}>
            <p>{p.why}</p>
            {p.regenerable && <p className="tray-regen">Comes back on its own if you need it.</p>}
            <ul className="tray-paths" aria-label="Sample paths">
              {p.samplePaths.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ul>
          </div>
        )}
      </div>
      <span className="tray-row-size">{formatBytes(p.bytes)}</span>
    </div>
  );
}

// PROTOTYPE variant A "Atlas": the plan tray (a cart beside the map), the reorganization drawer and the one confirmation.
import { motion } from 'motion/react';
import { ArrowRight, ChevronDown, PanelRightClose, Signpost, X } from 'lucide-react';
import { useState } from 'react';
import { formatCount, type AiAgent, type Dataset, type Proposal, type ReorgProposal } from '../../shared/data';
import { AgentQuote, Checkbox, ReorgGlyph, RiskChip, SourceTag, Tree } from './bits';
import { gb, leafIdFor, sum, toggled } from './model';

export interface PlanTotals {
  delBytes: number;
  delItems: number;
  delCount: number;
  moves: number;
  reorgCount: number;
  reorgFolders: string[];
  hints: Proposal[];
}

export function planTotals(d: Dataset, proposals: Set<string>, reorgs: Set<string>): PlanTotals {
  const dels = d.proposals.filter((p) => proposals.has(p.id) && p.action === 'delete');
  const rs = d.reorgs.filter((r) => reorgs.has(r.id));
  return {
    delBytes: sum(dels, (p) => p.bytes),
    delItems: sum(dels, (p) => p.itemCount),
    delCount: dels.length,
    moves: sum(rs, (r) => r.moveCount),
    reorgCount: rs.length,
    reorgFolders: rs.map((r) => r.folder),
    hints: d.proposals.filter((p) => proposals.has(p.id) && p.action === 'hint'),
  };
}

export function ctaLabel(t: PlanTotals) {
  if (t.delBytes > 0) return `Free ${gb(t.delBytes)}`;
  if (t.moves > 0) return `Reorganize ${t.reorgCount === 1 ? t.reorgFolders[0] : `${t.reorgCount} folders`}`;
  return 'Nothing selected yet';
}

interface TrayProps {
  data: Dataset;
  proposals: Set<string>;
  setProposals: (s: Set<string>) => void;
  reorgs: Set<string>;
  setReorgs: (s: Set<string>) => void;
  hot: string | null;
  onHot: (id: string | null) => void;
  onOpenReorg: (id: string) => void;
  onConfirm: () => void;
  onClose: () => void;
}

export function PlanTray(p: TrayProps) {
  const agent = p.data.scan.aiAgent;
  const dels = p.data.proposals.filter((x) => x.action === 'delete').sort((a, b) => b.bytes - a.bytes);
  const hints = p.data.proposals.filter((x) => x.action === 'hint');
  const t = planTotals(p.data, p.proposals, p.reorgs);
  const nothing = t.delBytes === 0 && t.moves === 0;

  return (
    <aside className="va-side va-tray" aria-label="Plan">
      <header className="va-side-head">
        <div>
          <h2>Plan</h2>
          <p>Click glowing areas on the map, or tick them here. Nothing changes until you confirm.</p>
        </div>
        <button type="button" className="va-icon-btn" onClick={p.onClose} aria-label="Hide the plan">
          <PanelRightClose size={16} />
        </button>
      </header>

      <div className="va-side-scroll">
        <section className="va-group">
          <h3>
            Delete <span>{dels.filter((x) => p.proposals.has(x.id)).length} of {dels.length}</span>
          </h3>
          {dels.map((x) => (
            <ProposalRow
              key={x.id}
              p={x}
              agent={agent}
              on={p.proposals.has(x.id)}
              onToggle={() => p.setProposals(toggled(p.proposals, x.id))}
              hot={p.hot === leafIdFor(x.id)}
              onHot={p.onHot}
            />
          ))}
        </section>

        {p.data.reorgs.length > 0 && (
          <section className="va-group">
            <h3>
              Reorganize <span>{t.reorgCount} of {p.data.reorgs.length}</span>
            </h3>
            {p.data.reorgs.map((r) => (
              <ReorgRow
                key={r.id}
                r={r}
                agent={agent}
                on={p.reorgs.has(r.id)}
                onToggle={() => p.setReorgs(toggled(p.reorgs, r.id))}
                onOpen={() => p.onOpenReorg(r.id)}
              />
            ))}
          </section>
        )}

        {hints.length > 0 && (
          <section className="va-group">
            <h3>Left for you</h3>
            <p className="va-group-note">Vacancy can't do these itself. Tick one to get a reminder when the run finishes. They don't count toward the total.</p>
            {hints.map((x) => (
              <ProposalRow
                key={x.id}
                p={x}
                agent={agent}
                on={p.proposals.has(x.id)}
                onToggle={() => p.setProposals(toggled(p.proposals, x.id))}
                hot={p.hot === leafIdFor(x.id)}
                onHot={p.onHot}
              />
            ))}
          </section>
        )}
      </div>

      <footer className="va-tray-foot">
        <div className="va-total">
          <span className="va-total-num">{gb(t.delBytes)}</span>
          <span className="va-total-lbl">will be freed</span>
        </div>
        <ul className="va-total-notes">
          <li>
            {formatCount(t.delItems)} items in {t.delCount} {t.delCount === 1 ? 'place' : 'places'}, deleted permanently
          </li>
          {t.moves > 0 && <li>{formatCount(t.moves)} files moved, and you can undo that</li>}
          {t.hints.length > 0 && <li>{t.hints.length} left for you, not counted</li>}
        </ul>
        <button type="button" className="va-cta" disabled={nothing} onClick={p.onConfirm}>
          {ctaLabel(t)}
        </button>
      </footer>
    </aside>
  );
}

function ProposalRow({ p, agent, on, onToggle, hot, onHot }: { p: Proposal; agent: AiAgent | null; on: boolean; onToggle: () => void; hot: boolean; onHot: (id: string | null) => void }) {
  const [open, setOpen] = useState(false);
  const id = leafIdFor(p.id);
  const hint = p.action === 'hint';
  return (
    <div
      className={`va-row ${on ? 'is-on' : ''} ${hot ? 'is-hot' : ''} ${hint ? 'va-row--hint' : ''}`}
      onMouseEnter={() => onHot(id)}
      onMouseLeave={() => onHot(null)}
    >
      <Checkbox on={on} onChange={onToggle} label={hint ? `Remind me: ${p.title}` : `Include ${p.title}`} />
      <div className="va-row-main">
        <button type="button" className="va-row-title" onClick={() => setOpen(!open)} aria-expanded={open}>
          <span>{p.title}</span>
          <ChevronDown size={13} className={open ? 'is-open' : ''} />
        </button>
        <p className="va-row-sum">{p.summary}</p>
        <div className="va-row-meta">
          {hint ? (
            <span className="va-src">
              <Signpost size={11} /> You do this one
            </span>
          ) : (
            <RiskChip risk={p.risk} />
          )}
          <SourceTag p={p} agent={agent} />
        </div>
        {p.aiNote && (
          <div className={open ? '' : 'va-clamp'}>
            <AgentQuote agent={agent} note={p.aiNote} />
          </div>
        )}
        {open && (
          <div className="va-row-more">
            <p>{p.why}</p>
            {p.regenerable && !hint && <p className="va-regen">Comes back on its own if you need it.</p>}
            <ul className="va-paths">
              {p.samplePaths.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ul>
          </div>
        )}
      </div>
      <span className={`va-row-size ${hint ? 'is-muted' : ''}`}>{gb(p.bytes)}</span>
    </div>
  );
}

function ReorgRow({ r, agent, on, onToggle, onOpen }: { r: ReorgProposal; agent: AiAgent | null; on: boolean; onToggle: () => void; onOpen: () => void }) {
  return (
    <div className={`va-row va-row--reorg ${on ? 'is-on' : ''}`}>
      <Checkbox on={on} onChange={onToggle} label={`Include ${r.title}`} />
      <div className="va-row-main">
        <button type="button" className="va-row-title" onClick={onOpen}>
          <span>{r.title}</span>
        </button>
        <p className="va-row-sum">{r.summary}</p>
        <button type="button" className="va-reorg-viz" onClick={onOpen} aria-label={`See the folders for ${r.title}`}>
          <ReorgGlyph seed={r.id} after={r.after} />
          <span>Before and after</span>
        </button>
        <div className="va-row-meta">
          <SourceTag p={{ source: 'ai' }} agent={agent} />
          <span className="va-src">Can be undone</span>
        </div>
      </div>
      <span className="va-row-size is-muted">{formatCount(r.moveCount)} files</span>
    </div>
  );
}

export function ReorgDrawer({ r, agent, on, onToggle, onClose }: { r: ReorgProposal; agent: AiAgent | null; on: boolean; onToggle: () => void; onClose: () => void }) {
  return (
    <motion.div
      className="va-drawer"
      role="dialog"
      aria-label={r.title}
      initial={{ opacity: 0, x: 36 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 36 }}
      transition={{ duration: 0.22, ease: [0.2, 0.7, 0.2, 1] }}
    >
      <header className="va-drawer-head">
        <div>
          <h2>{r.title}</h2>
          <p className="va-path">{r.path}</p>
        </div>
        <button type="button" className="va-icon-btn" onClick={onClose} aria-label="Close">
          <X size={16} />
        </button>
      </header>
      <div className="va-drawer-body">
        <AgentQuote agent={agent} note={r.aiNote} />
        <div className="va-trees">
          <section>
            <h3>Before</h3>
            <Tree node={r.before} />
          </section>
          <ArrowRight size={18} className="va-trees-arrow" />
          <section>
            <h3>After</h3>
            <Tree node={r.after} />
          </section>
        </div>
        <section className="va-moves">
          <h3>Sample moves</h3>
          <ul>
            {r.sampleMoves.map((m) => (
              <li key={m.from}>
                <span>{m.from}</span>
                <ArrowRight size={12} />
                <span>{m.to}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
      <footer className="va-drawer-foot">
        <p>
          {formatCount(r.moveCount)} moves. Each one is recorded, so the summary can put everything back.
        </p>
        <button type="button" className={on ? 'va-btn' : 'va-cta va-cta--sm'} onClick={onToggle}>
          {on ? 'Remove from plan' : 'Add to plan'}
        </button>
      </footer>
    </motion.div>
  );
}

export function ConfirmDialog({ data, totals: t, onCancel, onGo }: { data: Dataset; totals: PlanTotals; onCancel: () => void; onGo: () => void }) {
  const bin = data.os === 'mac' ? 'Trash' : 'Recycle Bin';
  const folders = t.reorgFolders.join(' and ');
  return (
    <motion.div className="va-scrim" onClick={onCancel} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.16 }}>
      <motion.div
        className="va-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="va-confirm-title"
        onClick={(e) => e.stopPropagation()}
        initial={{ opacity: 0, y: 10, scale: 0.98 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: 6 }}
        transition={{ duration: 0.2, ease: [0.2, 0.7, 0.2, 1] }}
      >
        <h2 id="va-confirm-title">{t.delBytes > 0 ? `Permanently delete ${gb(t.delBytes)}?` : `Move ${formatCount(t.moves)} files?`}</h2>
        {t.delBytes > 0 && (
          <p className="va-dialog-lead">
            {formatCount(t.delItems)} items in {t.delCount} {t.delCount === 1 ? 'place' : 'places'} will be deleted for good. They skip the {bin}, so they
            can't be brought back.
          </p>
        )}
        {t.moves > 0 && (
          <p>
            {formatCount(t.moves)} files in {folders} will be moved. Every move is recorded, so you can undo it from the summary.
          </p>
        )}
        {t.hints.length > 0 && (
          <p className="va-dialog-quiet">
            {t.hints.map((h) => h.title.replace(/\s*\(.*\)$/, '')).join(', ')} won't be touched. {t.hints.length === 1 ? 'It shows' : 'They show'} up under Left for you.
          </p>
        )}
        <div className="va-dialog-actions">
          <button type="button" className="va-btn" onClick={onCancel} autoFocus>
            Go back
          </button>
          <button type="button" className="va-cta va-cta--danger" onClick={onGo}>
            {t.delBytes > 0 ? `Delete ${gb(t.delBytes)} permanently` : `Move ${formatCount(t.moves)} files`}
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

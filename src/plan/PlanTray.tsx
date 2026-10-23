import { ChevronDown, ChevronRight, LoaderCircle, MessageCircle, PanelRightClose, Signpost } from 'lucide-react';
import { claudeNoteOf } from '../claude/context';
import { useId, useState } from 'react';
import type { Proposal, TidyProposal } from '../bridge';
import { formatBytes, formatCount } from '../format';
import { listOf, tidyTotals } from '../tidy/model';
import { TidyGroup } from '../tidy/TidyGroup';
import { Checkbox, RiskChip, SourceTag } from './bits';
import { DUPLICATES, groupTotals, included } from './duplicates';
import { planTotals, tileOf, type GroupChoice } from './totals';
import { HintActionButton } from './HintActionButton';
import { AdviceText } from './AdviceText';
import './plan.css';

interface Props {
  mountPoint: string;
  /** The drive's proposals, biggest first. */
  proposals: Proposal[];
  /** The folders Tidy up can sort on this drive (none on macOS). */
  tidy?: TidyProposal[];
  /** The ids in the drive's plan: proposals and sorts. */
  plan: string[];
  /** The sort whose drawer is open, if one is, and how to open or close one. */
  drawer?: string | null;
  onDrawer?: (id: string | null) => void;
  /** A sort's folder tile on the map, if it has one. */
  tidyTile?: (t: TidyProposal) => string | null;
  /** The AI agent Vacancy found, or null, for AI proposals' source tags. */
  agent: string | null;
  /** The map tile under the pointer, by tile id, so a row and its tile light together. */
  hot: string | null;
  onHot: (tile: string | null) => void;
  onToggle: (id: string) => void;
  onClose: () => void;
  /** "Free X GB": turn the plan into one to confirm. */
  onFree: () => void;
  /** The plan is being checked before the confirm dialog opens. */
  checking: boolean;
  /** Why the plan couldn't be made, if it couldn't. */
  error: string | null;
  /** The duplicate groups found, and the ones unticked, once the duplicate search is done. */
  duplicates?: GroupChoice | null;
  /** Opens the duplicates panel in place of the tray. */
  onOpenDuplicates?: () => void;
  /** "Ask Claude" about a proposal, in the Claude panel, while Claude is on. */
  onAsk?: ((p: Proposal) => void) | undefined;
}

/**
 * Variant D's plan tray, beside the map: the drive's delete proposals with tick boxes, risk chips, a source tag and
 * an expandable why with sample paths; the folders Tidy up can sort (Windows); and the plan's total, with the
 * button that asks to free it. Moves are said apart from what is freed: they free nothing.
 */
export function PlanTray({
  mountPoint,
  proposals,
  tidy = [],
  plan,
  drawer = null,
  onDrawer = () => {},
  tidyTile = () => null,
  agent,
  hot,
  onHot,
  onToggle,
  onClose,
  onFree,
  checking,
  error,
  duplicates,
  onOpenDuplicates,
  onAsk,
}: Props) {
  const deletes = proposals.filter((p) => p.action === 'delete');
  const hints = proposals.filter((p) => p.action === 'hint');
  const reminders = hints.filter((p) => plan.includes(p.id)).length;
  const on = deletes.filter((p) => plan.includes(p.id)).length;
  const t = planTotals(proposals, plan, duplicates ?? undefined);
  const dupGroups = duplicates && plan.includes(DUPLICATES) ? included(duplicates.groups, duplicates.skipped).length : 0;
  const sorts = tidyTotals(tidy, plan);
  const note = useId();
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
          {deletes.map((p) =>
            p.id === DUPLICATES && duplicates ? (
              <DuplicatesRow
                key={p.id}
                proposal={p}
                choice={duplicates}
                on={plan.includes(p.id)}
                hot={hot === tileOf(p.id)}
                onToggle={() => onToggle(p.id)}
                onHot={onHot}
                onOpen={() => onOpenDuplicates?.()}
              />
            ) : (
            <ProposalRow
              key={p.id}
              proposal={p}
              mountPoint={mountPoint}
              agent={agent}
              on={plan.includes(p.id)}
              hot={hot === tileOf(p.id)}
              onToggle={() => onToggle(p.id)}
              onHot={onHot}
              onAsk={onAsk}
            />
            ),
          )}
        </section>
        <TidyGroup tidy={tidy} plan={plan} open={drawer} onToggle={onToggle} onOpen={onDrawer} tileOf={tidyTile} hot={hot} onHot={onHot} />
        {hints.length > 0 && (
          <section className="tray-group" aria-label="Left for you">
            <h3>Left for you</h3>
            <p className="tray-group-note">Vacancy can’t do these itself. Tick one to get a reminder when the run finishes. They don’t count toward the total.</p>
            {hints.map((p) => (
              <ProposalRow key={p.id} proposal={p} mountPoint={mountPoint} agent={agent} on={plan.includes(p.id)} hot={false}
                onToggle={() => onToggle(p.id)} onHot={onHot} onAsk={onAsk} />
            ))}
          </section>
        )}
      </div>

      <div className="tray-foot">
        <div className="tray-total">
          <span className="tray-total-num">{formatBytes(t.bytes)}</span>
          <span className="tray-total-lbl">will be freed</span>
        </div>
        <ul className="tray-notes">
          {on > 0 ? <li>
            {formatCount(t.items)} {t.items === 1 ? 'item' : 'items'} in {t.places} {t.places === 1 ? 'place' : 'places'},
            deleted permanently
          </li> : <li>No files will be deleted.</li>}
          {dupGroups > 0 && (
            <li>
              One copy of each of {formatCount(dupGroups)} duplicate {dupGroups === 1 ? 'group' : 'groups'} stays
            </li>
          )}
          {sorts.moves > 0 && (
            <li className="note-moves">
              {formatCount(sorts.moves)} {sorts.moves === 1 ? 'file' : 'files'} sorted in {listOf(sorts.folders)}. Moved, not
              deleted: frees 0 B. Undo any time.
            </li>
          )}
          {reminders > 0 && <li>{reminders} left for you, not counted</li>}
        </ul>
        <button
          type="button"
          className="cta tray-cta"
          disabled={(on === 0 && reminders === 0 && sorts.moves === 0) || checking}
          aria-describedby={checking || error ? note : undefined}
          onClick={onFree}
        >
          {on > 0
            ? t.bytes > 0
              ? `Free ${formatBytes(t.bytes)}`
              : 'Delete selected items'
            : sorts.moves > 0
              ? `Move ${formatCount(sorts.moves)} ${sorts.moves === 1 ? 'file' : 'files'}`
              : reminders > 0
                ? 'Review reminders'
                : 'Nothing selected yet'}
        </button>
        {checking && (
          <p className="tray-later" id={note}>
            <LoaderCircle className="tray-spin" size={12} aria-hidden /> Checking every place in the plan again
          </p>
        )}
        {!checking && error && (
          <p className="tray-error" id={note} role="alert">
            {error}
          </p>
        )}
      </div>
    </aside>
  );
}

interface RowProps {
  mountPoint: string;
  proposal: Proposal;
  agent: string | null;
  on: boolean;
  hot: boolean;
  onToggle: () => void;
  onHot: (tile: string | null) => void;
  onAsk?: ((p: Proposal) => void) | undefined;
}

interface DuplicatesRowProps {
  proposal: Proposal;
  choice: GroupChoice;
  on: boolean;
  hot: boolean;
  onToggle: () => void;
  onHot: (tile: string | null) => void;
  onOpen: () => void;
}

/**
 * Duplicate files: tick box, a title that opens the duplicates panel (where the groups are, each with its own tick
 * box), what was found with photos and videos first, Review, and what the ticked groups free.
 */
function DuplicatesRow({ proposal: p, choice, on, hot, onToggle, onHot, onOpen }: DuplicatesRowProps) {
  const tile = tileOf(p.id);
  const kept = included(choice.groups, choice.skipped);
  const t = groupTotals(kept);
  const all = choice.groups.length;
  const cls = ['tray-row', 'dup-row', on && 'is-on', hot && 'is-hot'].filter(Boolean).join(' ');
  return (
    <div
      className={cls}
      onMouseEnter={() => onHot(tile)}
      onMouseLeave={() => onHot(null)}
      onFocus={() => onHot(tile)}
      onBlur={() => onHot(null)}
    >
      <Checkbox on={on} onChange={onToggle} label={`Include ${p.title}`} />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={onOpen}>
          <span>{p.title}</span>
          <ChevronRight size={13} aria-hidden />
        </button>
        <p className="tray-row-sum">
          {on && kept.length < all
            ? `${formatCount(kept.length)} of ${formatCount(all)} groups. One copy of each stays.`
            : p.summary}
        </p>
        <div className="tray-row-meta">
          <RiskChip risk={p.risk} />
          <button type="button" className="link-button dup-review-link" onClick={onOpen}>
            See them on the map
          </button>
        </div>
      </div>
      <span className="tray-row-size">{formatBytes(on ? t.bytes : p.bytes)}</span>
    </div>
  );
}

/** One proposal: tick box, title (which opens the why and sample paths), summary, risk, source and size. */
function ProposalRow({ proposal: p, mountPoint, agent, on, hot, onToggle, onHot, onAsk }: RowProps) {
  const [open, setOpen] = useState(false);
  const more = useId();
  const hint = p.action === 'hint';
  const tile = hint ? null : tileOf(p.id);
  const claude = p.source === 'claude';
  const note = claude ? claudeNoteOf(p.claudeNote) : null;
  const cls = ['tray-row', on && 'is-on', hot && 'is-hot', claude && 'is-ai'].filter(Boolean).join(' ');
  return (
    <div
      data-row={p.id}
      className={cls}
      onMouseEnter={() => onHot(tile)}
      onMouseLeave={() => onHot(null)}
      onFocus={() => onHot(tile)}
      onBlur={() => onHot(null)}
    >
      <Checkbox on={on} onChange={onToggle} label={hint ? `Remind me: ${p.title}` : `Include ${p.title}`} />
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
        {/* Claude's own item says in its note why it suggests it; its source tag says whose it is. */}
        {note ? <p className="tray-row-sum cl-note">{note}</p> : <p className="tray-row-sum">{p.summary}</p>}
        <div className="tray-row-meta">
          {hint ? <span className="source"><Signpost size={11} aria-hidden /> You do this one</span> : <RiskChip risk={p.risk} />}
          <SourceTag source={p.source} agent={agent} />
          {onAsk && (
            <button type="button" className="link-button cl-ask" onClick={() => onAsk(p)}>
              <MessageCircle size={11} aria-hidden /> Ask Claude
            </button>
          )}
        </div>
        {open && (
          <div className="tray-row-more" id={more}>
            {hint ? <AdviceText why={p.why} action={p.hintAction} mountPoint={mountPoint} proposalId={p.id} /> : !note && <p>{p.why}</p>}
            {hint && p.hintAction?.kind === 'steamUninstall' && <HintActionButton mountPoint={mountPoint} proposalId={p.id} />}
            {p.regenerable && !hint && <p className="tray-regen">Comes back on its own if you need it.</p>}
            <ul className="tray-paths" aria-label="Sample paths">
              {p.samplePaths.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ul>
          </div>
        )}
      </div>
      <span className="tray-row-size">{hint ? 'Advice' : formatBytes(p.bytes)}</span>
    </div>
  );
}

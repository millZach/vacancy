import { ArrowLeft, ArrowRight, Check, Info } from 'lucide-react';
import { useState } from 'react';
import type { DuplicateGroup, Duplicates, Platform } from '../bridge';
import { formatBytes, formatCount } from '../format';
import { Checkbox } from './bits';
import { BUCKETS, bucketOf, groupTotals, included, type Bucket } from './duplicates';
import { CopyTile, KindIcon } from './KindIcon';
import './plan.css';

interface Props {
  found: Duplicates;
  /** Groups the user unticked. */
  skipped: readonly string[];
  /** Duplicate files are in the plan. */
  inPlan: boolean;
  platform: Platform;
  /** The group whose links the map draws, by id. */
  picked: string | null;
  onPick: (id: string) => void;
  /** The group under the pointer, for the map's links; null when the pointer leaves. */
  onHover: (id: string | null) => void;
  onToggleGroup: (id: string) => void;
  /** Back to the plan tray. */
  onClose: () => void;
  /** Puts duplicate files in the plan, then goes back to the plan tray. */
  onAdd: () => void;
}

type Filter = 'All' | Bucket;

/**
 * The duplicates panel, in place of the plan tray (variant B, "Map links"): every group of identical files, biggest
 * saving first, filtered by photos, videos or other files. Each shows the copy Vacancy keeps and the extra copies it
 * deletes, as file-type icons with their folders (never thumbnails: Vacancy doesn't read files to show them), why
 * that copy stays, and a tick box: an unticked group keeps every copy. Picking or hovering a group draws its links on
 * the map, from the folder where the kept copy lives to each folder holding an extra copy.
 */
export function DuplicatesPanel(p: Props) {
  const [filter, setFilter] = useState<Filter>('All');
  const groups = p.found.groups;
  const shown = groups.filter((g) => filter === 'All' || bucketOf(g.kind) === filter);
  const on = included(groups, p.skipped);
  const t = groupTotals(on);
  const count = (b: Filter) => (b === 'All' ? groups.length : groups.filter((g) => bucketOf(g.kind) === b).length);
  return (
    <aside className="tray dup-panel" aria-label="Duplicate files" data-no-ripple>
      <div className="tray-head">
        <div>
          <h2>Duplicate files</h2>
          <p>
            Linked tiles show where the kept copy lives and where the extra copies go from. Untick a group to keep all
            of its copies.
          </p>
        </div>
        <button type="button" className="icon-btn" onClick={p.onClose} aria-label="Back to the plan">
          <ArrowLeft size={16} aria-hidden />
        </button>
      </div>
      <div className="dup-filters">
        <div className="seg" role="group" aria-label="Show">
          {(['All', ...BUCKETS] as const).map((b) => (
            <button
              key={b}
              type="button"
              className={filter === b ? 'is-on' : undefined}
              aria-pressed={filter === b}
              onClick={() => setFilter(b)}
            >
              {b} <span>{count(b)}</span>
            </button>
          ))}
        </div>
      </div>
      <div className="tray-scroll dup-list">
        <ul aria-label="Groups">
          {shown.map((g) => (
            <GroupCard
              key={g.id}
              group={g}
              on={!p.skipped.includes(g.id)}
              picked={p.picked === g.id}
              onPick={() => p.onPick(g.id)}
              onHover={p.onHover}
              onToggle={() => p.onToggleGroup(g.id)}
            />
          ))}
        </ul>
        <Notes found={p.found} platform={p.platform} />
      </div>
      <div className="tray-foot">
        <div className="tray-total">
          <span className="tray-total-num">{formatBytes(t.bytes)}</span>
          <span className="tray-total-lbl">
            in {formatCount(on.length)} of {formatCount(groups.length)} {groups.length === 1 ? 'group' : 'groups'}
          </span>
        </div>
        <ul className="tray-notes">
          <li>
            {formatCount(t.copies)} extra {t.copies === 1 ? 'copy goes' : 'copies go'}; one copy of each group stays
          </li>
        </ul>
        {p.inPlan ? (
          <button type="button" className="cta tray-cta" onClick={p.onClose}>
            Back to the plan
          </button>
        ) : (
          <button type="button" className="cta tray-cta" disabled={on.length === 0} onClick={p.onAdd}>
            Add {formatCount(on.length)} {on.length === 1 ? 'group' : 'groups'} to the plan
          </button>
        )}
      </div>
    </aside>
  );
}

interface CardProps {
  group: DuplicateGroup;
  on: boolean;
  picked: boolean;
  onPick: () => void;
  onHover: (id: string | null) => void;
  onToggle: () => void;
}

/** Extra copies a card draws; the rest are counted. */
const SHOWN_EXTRAS = 2;

function GroupCard({ group: g, on, picked, onPick, onHover, onToggle }: CardProps) {
  const cls = ['dup-card', !on && 'is-left', picked && 'is-picked'].filter(Boolean).join(' ');
  const n = g.extras.length;
  return (
    <li
      className={cls}
      onMouseEnter={() => onHover(g.id)}
      onMouseLeave={() => onHover(null)}
      onFocus={() => onHover(g.id)}
      onBlur={() => onHover(null)}
      onClick={onPick}
    >
      <Checkbox on={on} onChange={onToggle} label={`Remove extra copies of ${g.name}`} />
      <span className="dup-copies" aria-hidden>
        <CopyTile kind={g.kind} tag="Kept" folder={g.keep.folder} />
        <ArrowRight size={12} className="dup-arrow" />
        {g.extras.slice(0, SHOWN_EXTRAS).map((e) => (
          <CopyTile key={e.path} kind={g.kind} tag={on ? 'Goes' : 'Stays'} folder={e.folder} gone={on} />
        ))}
        {n > SHOWN_EXTRAS && <span className="dup-more">+{n - SHOWN_EXTRAS}</span>}
      </span>
      <button type="button" className="dup-text" aria-pressed={picked} aria-label={`Show ${g.name} on the map`}>
        <b title={g.keep.path}>
          <KindIcon kind={g.kind} size={12} />
          {g.name}
        </b>
        <span className="keep-reason">
          <Check size={11} strokeWidth={2.6} aria-hidden />
          {g.reason}
        </span>
        <span className="dup-meta">
          {formatBytes(g.bytes)} each · {n} extra {n === 1 ? 'copy' : 'copies'}
        </span>
        <span className="dup-paths">
          <span>
            <em>Keeps</em> <code title={g.keep.path}>{g.keep.dir}</code>
          </span>
          {g.extras.map((e) => (
            <span key={e.path}>
              <em className="is-gone">{on ? 'Deletes' : 'Stays'}</em>{' '}
              <code title={e.path}>{e.dir ? `${e.dir}${sepOf(e.path)}${e.name}` : e.name}</code>
            </span>
          ))}
        </span>
      </button>
      <span className="dup-size">{on ? formatBytes(g.freed) : 'kept'}</span>
    </li>
  );
}

const sepOf = (path: string) => (path.includes('\\') ? '\\' : '/');

/** What the search left alone, and what it never does. */
function Notes({ found, platform }: { found: Duplicates; platform: Platform }) {
  const folders = found.folders.length > 0 ? found.folders : ['your personal folders'];
  const scope =
    folders.length > 1 ? `${folders.slice(0, -1).join(', ')} and ${folders.at(-1)}` : (folders[0] ?? '');
  return (
    <ul className="dup-notes">
      {found.hardLinked > 0 && (
        <li>
          <Info size={12} aria-hidden />
          {formatCount(found.hardLinked)} hard-linked {found.hardLinked === 1 ? 'file shares' : 'files share'} one copy
          on disk, so {found.hardLinked === 1 ? 'it isn’t a duplicate and stays' : 'they aren’t duplicates and stay'}.
        </li>
      )}
      <li>
        <Info size={12} aria-hidden />
        {platform === 'macos'
          ? 'Your Photos library and iCloud Drive are left alone, so nothing disappears from your other devices.'
          : 'Copies in OneDrive, Dropbox and Google Drive are left alone, so nothing disappears from your other devices.'}
      </li>
      <li>
        <Info size={12} aria-hidden />
        Only exact copies over 1 MB in {scope}. Files another proposal already deletes aren’t counted twice.
      </li>
      <li>
        <Info size={12} aria-hidden />
        Vacancy reads files only to compare them. It never shows or sends what is in them.
      </li>
    </ul>
  );
}

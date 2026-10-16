// PROTOTYPE variant C: right-hand inspector for whatever ledger row has focus.
import { FolderInput, Lock, RefreshCcw, SquareTerminal } from 'lucide-react';
import type { Category, Dataset } from '../../shared/data';
import type { Row } from './desk';
import { RiskBadge, SizeBar, SourceChip } from './bits';
import { agentName, catColor, formatAgo, gb, lastUsedFor, n } from './model';

interface Props {
  data: Dataset;
  row: Row | undefined;
  mode: 'where' | 'plan';
  proposals: Set<string>;
  reorgs: Set<string>;
  byCat: Record<string, number>;
  onToggleP: (id: string) => void;
  onToggleR: (id: string) => void;
  onOpenReorg: (id: string) => void;
  onPlan: () => void;
}

export function Inspector(p: Props) {
  const { row, data } = p;
  if (!row) return <aside className="vc-insp" />;
  let body: React.ReactNode;
  if (row.kind === 'cat' || row.kind === 'item') body = <CatPanel {...p} cat={row.cat} itemName={row.kind === 'item' ? row.item.name : undefined} />;
  else if (row.kind === 'prop') {
    const x = row.p;
    const on = p.proposals.has(x.id);
    const hint = x.action === 'hint';
    const last = lastUsedFor(data, x);
    body = (
      <>
        <div className="vc-insp-kicker">
          <i className="vc-swatch" style={{ background: catColor(row.cat, 'dot') }} />
          {row.cat.name}
        </div>
        <h2 className="vc-insp-title">{x.title}</h2>
        <div className="vc-insp-chips">
          <RiskBadge risk={x.risk} />
          <SourceChip data={data} source={x.source} />
          {x.regenerable && (
            <span className="vc-chip">
              <RefreshCcw size={11} strokeWidth={2.2} /> Comes back on its own
            </span>
          )}
        </div>
        <div className="vc-insp-figure">
          <b className={hint ? 'vc-mute' : on ? 'pink' : ''}>{gb(x.bytes)}</b>
          <span>
            {n(x.itemCount)} item{x.itemCount === 1 ? '' : 's'}
            {last != null && <>, last used {formatAgo(last)}</>}
          </span>
        </div>
        {hint ? (
          <div className="vc-insp-hint">
            <b>Vacancy can't do this one for you.</b> It's on your list in the summary and never counts toward freed space.
          </div>
        ) : null}
        <p className="vc-insp-why">{x.why}</p>
        {x.source === 'ai' && x.aiNote && (
          <figure className="vc-ai-note">
            <figcaption>
              <SquareTerminal size={13} strokeWidth={2.2} /> {agentName(data)} explained this folder
            </figcaption>
            <blockquote>{x.aiNote}</blockquote>
          </figure>
        )}
        <div className="vc-insp-paths">
          <h3>{hint ? 'Where it is' : 'Includes'}</h3>
          <ul>
            {x.samplePaths.map((s) => (
              <li key={s} title={s}>
                {s}
              </li>
            ))}
          </ul>
        </div>
        {p.mode === 'plan' && (
          <button className={`vc-btn ${on ? 'vc-btn-ghost' : 'vc-btn-neon'} vc-insp-cta`} onClick={() => p.onToggleP(x.id)}>
            {hint ? (on ? 'Remove from your list' : 'Add to your list') : on ? 'Remove from plan' : `Add ${gb(x.bytes)} to plan`}
          </button>
        )}
      </>
    );
  } else if (row.kind === 'reorgGroup') {
    body = (
      <>
        <div className="vc-insp-kicker">
          <FolderInput size={13} /> Reorganize
        </div>
        <h2 className="vc-insp-title">{data.reorgs.length} folders could be tidier</h2>
        <p className="vc-insp-why">
          {agentName(data)} read file names and contents on this computer and suggested where loose files belong. Nothing is deleted. Every move is journaled, so the
          summary can put each folder back exactly as it was.
        </p>
        <ul className="vc-insp-list">
          {data.reorgs.map((r) => (
            <li key={r.id}>
              <span>{r.folder}</span>
              <em>{n(r.moveCount)} moves</em>
            </li>
          ))}
        </ul>
      </>
    );
  } else {
    const r = row.r;
    const on = p.reorgs.has(r.id);
    body = (
      <>
        <div className="vc-insp-kicker">
          <FolderInput size={13} /> {r.path}
        </div>
        <h2 className="vc-insp-title">{r.title}</h2>
        <div className="vc-insp-chips">
          <SourceChip data={data} source="ai" />
          <span className="vc-chip">Undo from the summary</span>
        </div>
        <div className="vc-insp-figure">
          <b className={on ? '' : 'vc-mute'}>{n(r.moveCount)}</b>
          <span>moves out of {n(r.itemCount)} items</span>
        </div>
        <figure className="vc-ai-note">
          <figcaption>
            <SquareTerminal size={13} strokeWidth={2.2} /> {agentName(data)}'s reasoning
          </figcaption>
          <blockquote>{r.aiNote}</blockquote>
        </figure>
        <div className="vc-insp-paths">
          <h3>New folders</h3>
          <ul>
            {(r.after.children ?? []).map((c) => (
              <li key={c.name} className={c.name.startsWith('(') ? 'vc-mute' : ''}>
                {c.name}
                {c.count != null && <em>{n(c.count)}</em>}
              </li>
            ))}
          </ul>
        </div>
        <div className="vc-insp-row">
          <button className="vc-btn vc-btn-ghost" onClick={() => p.onOpenReorg(r.id)}>
            Compare before and after
          </button>
          {p.mode === 'plan' && (
            <button className={`vc-btn ${on ? 'vc-btn-ghost' : 'vc-btn-neon'}`} onClick={() => p.onToggleR(r.id)}>
              {on ? 'Remove' : 'Add to plan'}
            </button>
          )}
        </div>
      </>
    );
  }
  return (
    <aside className="vc-insp" aria-live="polite">
      <div className="vc-insp-in" key={row.key}>
        {body}
      </div>
    </aside>
  );
}

function CatPanel({ data, cat, itemName, mode, byCat, onPlan }: Props & { cat: Category; itemName?: string }) {
  const ps = data.proposals.filter((x) => x.categoryId === cat.id && x.action === 'delete');
  const hints = data.proposals.filter((x) => x.categoryId === cat.id && x.action === 'hint');
  const canFree = ps.reduce((s, x) => s + x.bytes, 0);
  const max = Math.max(...cat.items.map((i) => i.bytes));
  return (
    <>
      <div className="vc-insp-kicker">
        <i className="vc-swatch" style={{ background: catColor(cat, 'dot') }} />
        {((cat.bytes / data.disk.totalBytes) * 100).toFixed(1)}% of {data.disk.name}
      </div>
      <h2 className="vc-insp-title">{cat.name}</h2>
      <div className="vc-insp-figure">
        <b>{gb(cat.bytes)}</b>
        <span>{cat.blurb}</span>
      </div>
      {cat.protected ? (
        <div className="vc-insp-lock">
          <Lock size={16} strokeWidth={2} />
          <p>
            <b>Locked.</b> Vacancy shows this so the numbers add up, but it never proposes changes here.
          </p>
        </div>
      ) : (
        <div className="vc-insp-can">
          {ps.length ? (
            <>
              <p>
                <b className="pink">{gb(canFree)}</b> can be freed here with {ps.length} proposal{ps.length === 1 ? '' : 's'}.{' '}
                {byCat[cat.id] ? <>{gb(byCat[cat.id])} is in your plan.</> : <>None are in your plan yet.</>}
              </p>
              {mode === 'where' && (
                <button className="vc-link" onClick={onPlan}>
                  See them in the plan
                </button>
              )}
            </>
          ) : hints.length ? null : (
            <p className="vc-mute">Nothing here looks safe to remove. These are your files.</p>
          )}
          {hints.map((h) => (
            <div key={h.id} className="vc-insp-hint">
              <b>Tip, {gb(h.bytes)}:</b> {h.summary} Vacancy can't do this one for you.
            </div>
          ))}
        </div>
      )}
      <div className="vc-insp-items">
        {cat.items.map((it) => (
          <div key={it.name} className={`vc-insp-item ${it.name === itemName ? 'on' : ''}`}>
            <div>
              <span>{it.name}</span>
              <b className="num">{gb(it.bytes)}</b>
            </div>
            <SizeBar frac={it.bytes / max} color={catColor(cat)} dim={!!cat.protected} />
            <small title={it.path}>
              {it.lastUsedDays != null ? `Last used ${formatAgo(it.lastUsedDays)}. ` : ''}
              {it.path}
            </small>
          </div>
        ))}
      </div>
    </>
  );
}

// PROTOTYPE variant C: overview ("Where it goes") and plan, as one dense keyboard-driven ledger.
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { ChevronRight, FolderInput, Layers, ListChecks, Lock, ShieldCheck, SquareTerminal } from 'lucide-react';
import type { Category, CategoryItem, Dataset, Proposal, ReorgProposal } from '../../shared/data';
import type { VariantProps } from '../../shared/types';
import { CapacityBar, Header, Sign, segsFor } from './chrome';
import { RiskBadge, SizeBar, SourceChip, Tick } from './bits';
import { Inspector } from './inspect';
import { ReorgView } from './reorg';
import { Confirm } from './confirm';
import { catColor, formatAgo, gb, lastUsedFor, litFor, maxPlanBytes, n, plannedByCategory, planTotals, type Filter } from './model';

export type Row =
  | { key: string; kind: 'cat'; cat: Category; open: boolean; props: Proposal[] }
  | { key: string; kind: 'item'; cat: Category; item: CategoryItem; last: boolean }
  | { key: string; kind: 'prop'; cat: Category; p: Proposal; last: boolean }
  | { key: string; kind: 'reorgGroup' }
  | { key: string; kind: 'reorg'; r: ReorgProposal; last: boolean };

const matches = (f: Filter, p: Proposal) =>
  f === 'all' || (f === 'safe' && p.risk === 'safe') || (f === 'review' && p.risk !== 'safe') || (f === 'ai' && p.source === 'ai');

function buildRows(d: Dataset, mode: 'where' | 'plan', filter: Filter, expanded: Set<string>, collapsed: Set<string>): Row[] {
  const rows: Row[] = [];
  const pushItems = (c: Category) =>
    c.items.forEach((item, i) => rows.push({ key: `i:${c.id}:${i}`, kind: 'item', cat: c, item, last: i === c.items.length - 1 }));
  const propsOf = (c: Category) =>
    d.proposals.filter((p) => p.categoryId === c.id && matches(filter, p)).sort((a, b) => Number(a.action === 'hint') - Number(b.action === 'hint') || b.bytes - a.bytes);

  if (mode === 'where' || filter === 'locked') {
    const cats = [...d.categories].filter((c) => filter !== 'locked' || c.protected).sort((a, b) => b.bytes - a.bytes);
    for (const c of cats) {
      const open = expanded.has(c.id);
      rows.push({ key: `c:${c.id}`, kind: 'cat', cat: c, open, props: d.proposals.filter((p) => p.categoryId === c.id) });
      if (open) pushItems(c);
    }
    return rows;
  }

  const groups = d.categories
    .filter((c) => !c.protected)
    .map((c) => ({ c, ps: propsOf(c) }))
    .filter((g) => g.ps.length > 0)
    .sort((a, b) => b.ps.filter((p) => p.action === 'delete').reduce((s, p) => s + p.bytes, 0) - a.ps.filter((p) => p.action === 'delete').reduce((s, p) => s + p.bytes, 0));
  for (const { c, ps } of groups) {
    const open = !collapsed.has(c.id);
    rows.push({ key: `c:${c.id}`, kind: 'cat', cat: c, open, props: ps });
    if (open) ps.forEach((p, i) => rows.push({ key: `p:${p.id}`, kind: 'prop', cat: c, p, last: i === ps.length - 1 }));
  }
  if ((filter === 'all' || filter === 'ai') && d.reorgs.length) {
    rows.push({ key: 'rg', kind: 'reorgGroup' });
    if (!collapsed.has('rg')) d.reorgs.forEach((r, i) => rows.push({ key: `r:${r.id}`, kind: 'reorg', r, last: i === d.reorgs.length - 1 }));
  }
  if (filter === 'all') {
    for (const c of d.categories.filter((x) => x.protected)) {
      const open = expanded.has(c.id);
      rows.push({ key: `c:${c.id}`, kind: 'cat', cat: c, open, props: [] });
      if (open) pushItems(c);
    }
  }
  return rows;
}

export function Desk(props: VariantProps & { mode: 'where' | 'plan' }) {
  const { data, mode, onStep, proposals, setProposals, reorgs, setReorgs } = props;
  const [filter, setFilter] = useState<Filter>('all');
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [focus, setFocus] = useState<string | null>(null);
  const [reorgFocus, setReorgFocus] = useState(data.reorgs[0]?.id ?? '');
  const [confirm, setConfirm] = useState(false);
  const [hover, setHover] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);

  const rows = useMemo(() => buildRows(data, mode, filter, expanded, collapsed), [data, mode, filter, expanded, collapsed]);
  const fi = Math.max(0, rows.findIndex((r) => r.key === focus));
  const cur = rows[fi] as Row | undefined;

  // Keyboard-first: the ledger takes focus when it appears or the view changes.
  useEffect(() => {
    if (!confirm) scroller.current?.focus({ preventScroll: true });
  }, [mode, filter === 'reorg', confirm]);

  useEffect(() => {
    const el = scroller.current?.querySelector(`[data-key="${cur?.key}"]`);
    (el as HTMLElement | null)?.scrollIntoView({ block: 'nearest' });
  }, [cur?.key]);

  const totals = planTotals(data, proposals, reorgs);
  const byCat = plannedByCategory(data, proposals);
  const { totalBytes: total, usedBytes: used } = data.disk;
  const free = total - used;
  const maxPlan = maxPlanBytes(data);
  const frac = totals.delBytes / maxPlan;

  const toggleP = (id: string) => {
    const s = new Set(proposals);
    s.has(id) ? s.delete(id) : s.add(id);
    setProposals(s);
  };
  const toggleR = (id: string) => {
    const s = new Set(reorgs);
    s.has(id) ? s.delete(id) : s.add(id);
    setReorgs(s);
  };
  const toggleGroup = (ps: Proposal[]) => {
    const del = ps.filter((p) => p.action === 'delete');
    const allOn = del.every((p) => proposals.has(p.id));
    const s = new Set(proposals);
    del.forEach((p) => (allOn ? s.delete(p.id) : s.add(p.id)));
    setProposals(s);
  };
  const toggleAllReorgs = () => setReorgs(reorgs.size === data.reorgs.length ? new Set() : new Set(data.reorgs.map((r) => r.id)));
  const flip = (set: Set<string>, id: string, apply: (s: Set<string>) => void) => {
    const s = new Set(set);
    s.has(id) ? s.delete(id) : s.add(id);
    apply(s);
  };
  const openRow = (r: Row) => {
    if (r.kind === 'cat') {
      if (mode === 'plan' && !r.cat.protected && filter !== 'locked') flip(collapsed, r.cat.id, setCollapsed);
      else flip(expanded, r.cat.id, setExpanded);
    } else if (r.kind === 'reorgGroup') flip(collapsed, 'rg', setCollapsed);
    else if (r.kind === 'reorg') {
      setReorgFocus(r.r.id);
      setFilter('reorg');
    }
  };
  const toggleRow = (r: Row) => {
    if (mode !== 'plan') return openRow(r);
    if (r.kind === 'prop') toggleP(r.p.id);
    else if (r.kind === 'reorg') toggleR(r.r.id);
    else if (r.kind === 'reorgGroup') toggleAllReorgs();
    else if (r.kind === 'cat' && !r.cat.protected && filter !== 'locked') toggleGroup(r.props);
    else openRow(r);
  };

  const onKey = (e: KeyboardEvent) => {
    if (!rows.length) return;
    const go = (i: number) => {
      e.preventDefault();
      setFocus(rows[Math.max(0, Math.min(rows.length - 1, i))].key);
    };
    if (e.key === 'ArrowDown' || e.key === 'j') go(fi + 1);
    else if (e.key === 'ArrowUp' || e.key === 'k') go(fi - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(rows.length - 1);
    else if (e.key === ' ' && cur) {
      e.preventDefault();
      toggleRow(cur);
    } else if (e.key === 'Enter' && cur) {
      e.preventDefault();
      openRow(cur);
    }
  };

  const pickFilter = (f: Filter) => {
    setFilter(f);
    setFocus(null);
    if (mode === 'where' && f !== 'all' && f !== 'locked') onStep('plan');
    scroller.current?.focus();
  };

  // Counts for the rail.
  const counts: Record<Filter, number> = {
    all: mode === 'where' ? data.categories.length : data.proposals.length + data.reorgs.length,
    safe: data.proposals.filter((p) => p.risk === 'safe').length,
    review: data.proposals.filter((p) => p.risk !== 'safe').length,
    ai: data.proposals.filter((p) => p.source === 'ai').length + data.reorgs.length,
    reorg: data.reorgs.length,
    locked: data.categories.filter((c) => c.protected).length,
  };
  const FILTERS: { f: Filter; label: string; icon: React.ReactNode }[] = [
    { f: 'all', label: mode === 'where' ? 'All categories' : 'Everything', icon: <Layers size={14} /> },
    { f: 'safe', label: 'Safe', icon: <i className="vc-dot vc-dot-safe" /> },
    { f: 'review', label: 'Needs review', icon: <i className="vc-dot vc-dot-review" /> },
    { f: 'ai', label: 'AI suggestions', icon: <SquareTerminal size={14} /> },
    { f: 'reorg', label: 'Reorganize', icon: <FolderInput size={14} /> },
    { f: 'locked', label: 'Locked', icon: <Lock size={13} /> },
  ];

  const hotCat = hover ?? (cur && 'cat' in cur ? cur.cat.id : null);
  const propMax = Math.max(...data.proposals.filter((p) => p.action === 'delete').map((p) => p.bytes));
  const catMax = Math.max(...data.categories.map((c) => c.bytes));
  const reorgView = mode === 'plan' && filter === 'reorg';

  return (
    <>
      <Header
        data={data}
        sign={<Sign lit={litFor(data, totals.delBytes)} glow={0.4 + 0.6 * frac} />}
        stats={[
          { label: 'Used', value: gb(used) },
          { label: 'Free now', value: gb(free) },
          { label: 'Free after plan', value: gb(free + totals.delBytes), tone: 'pink' },
        ]}
        bar={
          <CapacityBar
            segs={segsFor(data, byCat)}
            ghost={totals.delBytes}
            total={total}
            free={free}
            marker={totals.delBytes > 0 ? `${gb(free + totals.delBytes)} free after this plan` : undefined}
            hot={hotCat}
            onHot={setHover}
          />
        }
      />
      <div className={`vc-desk ${reorgView ? 'vc-desk-reorg' : ''}`}>
        <nav className="vc-rail" aria-label="Views and filters">
          <div className="vc-rail-views">
            <button className={`vc-rail-btn ${mode === 'where' ? 'on' : ''}`} onClick={() => onStep('overview')}>
              <Layers size={15} />
              <span>Where it goes</span>
            </button>
            <button className={`vc-rail-btn ${mode === 'plan' ? 'on' : ''}`} onClick={() => onStep('plan')}>
              <ListChecks size={15} />
              <span>Plan</span>
              <em className="pink">{gb(totals.delBytes)}</em>
            </button>
          </div>
          <div className="vc-rail-filters">
            {FILTERS.map(({ f, label, icon }) => (
              <button key={f} className={`vc-rail-f ${filter === f ? 'on' : ''} ${mode === 'where' && f !== 'all' && f !== 'locked' ? 'jump' : ''}`} onClick={() => pickFilter(f)}>
                <span className="vc-rail-ic">{icon}</span>
                <span>{label}</span>
                <em>{counts[f]}</em>
              </button>
            ))}
          </div>
          <div className="vc-rail-keys" aria-hidden>
            <div><kbd>↑</kbd><kbd>↓</kbd><span>Move</span></div>
            <div><kbd>Space</kbd><span>{mode === 'plan' ? 'Select' : 'Expand'}</span></div>
            <div><kbd>Enter</kbd><span>Open</span></div>
          </div>
        </nav>

        {reorgView ? (
          <ReorgView data={data} focusId={reorgFocus} setFocusId={setReorgFocus} reorgs={reorgs} toggle={toggleR} />
        ) : (
          <>
            <section className="vc-ledger" aria-label={mode === 'where' ? 'Where your space goes' : 'Cleanup plan'}>
              <div className={`vc-thead vc-cols-${mode}`} role="row">
                {mode === 'where' ? (
                  <>
                    <span />
                    <span>Category</span>
                    <span>Share of disk</span>
                    <span className="r">Size</span>
                    <span className="r c-pct">Of disk</span>
                    <span className="r">Can free</span>
                  </>
                ) : filter === 'locked' ? (
                  <>
                    <span />
                    <span>Locked category</span>
                    <span className="vc-span3" />
                    <span />
                    <span className="r">Size</span>
                  </>
                ) : (
                  <>
                    <span />
                    <span>Proposal</span>
                    <span>Risk</span>
                    <span>Source</span>
                    <span className="c-last">Last used</span>
                    <span />
                    <span className="r">Size</span>
                  </>
                )}
              </div>
              <div className="vc-tbody" ref={scroller} tabIndex={0} onKeyDown={onKey} role="treegrid" aria-activedescendant={cur ? `vc-row-${cur.key}` : undefined} autoFocus>
                {rows.map((r, i) => (
                  <LedgerRow
                    key={r.key}
                    r={r}
                    data={data}
                    mode={mode}
                    filter={filter}
                    focused={i === fi}
                    hot={'cat' in r && r.cat.id === hover}
                    proposals={proposals}
                    reorgs={reorgs}
                    byCat={byCat}
                    propMax={propMax}
                    catMax={catMax}
                    total={total}
                    onFocus={() => setFocus(r.key)}
                    onOpen={() => openRow(r)}
                    onToggle={() => toggleRow(r)}
                  />
                ))}
                {!rows.length && <div className="vc-empty">Nothing matches this filter.</div>}
              </div>
            </section>
            <Inspector
              data={data}
              row={cur}
              mode={mode}
              proposals={proposals}
              reorgs={reorgs}
              byCat={byCat}
              onToggleP={toggleP}
              onToggleR={toggleR}
              onOpenReorg={(id) => {
                setReorgFocus(id);
                setFilter('reorg');
                onStep('plan');
              }}
              onPlan={() => onStep('plan')}
            />
          </>
        )}

        <footer className="vc-foot">
          {mode === 'where' ? (
            <>
              <p className="vc-foot-say">
                Vacancy can free up to <b className="pink">{gb(maxPlan)}</b> with {data.proposals.filter((p) => p.action === 'delete').length} proposals and sort{' '}
                {data.reorgs.length} folder{data.reorgs.length === 1 ? '' : 's'}. {data.categories.filter((c) => c.protected).length} categories are locked.
              </p>
              <button className="vc-btn vc-btn-neon" onClick={() => onStep('plan')}>
                Review the plan
              </button>
            </>
          ) : (
            <>
              <div className="vc-foot-totals">
                <div>
                  <span>Delete</span>
                  <b className="pink">{gb(totals.delBytes)}</b>
                  <em>{n(totals.delItems)} items in {totals.delCount} places</em>
                </div>
                <div>
                  <span>Move</span>
                  <b>{n(totals.moveCount)}</b>
                  <em>{totals.reorgCount ? `files in ${totals.folders.join(' and ')}` : 'no folders sorted'}</em>
                </div>
                <div>
                  <span>Left for you</span>
                  <b>{totals.hints.length}</b>
                  <em>{totals.hints.length ? `tip${totals.hints.length === 1 ? '' : 's'}, not counted` : 'no tips added'}</em>
                </div>
              </div>
              <button className="vc-btn vc-btn-neon" disabled={!totals.delCount && !totals.reorgCount} onClick={() => setConfirm(true)}>
                Review and run
              </button>
            </>
          )}
        </footer>
      </div>
      {confirm && <Confirm data={data} totals={totals} onCancel={() => setConfirm(false)} onRun={() => onStep('run')} />}
    </>
  );
}

function LedgerRow(p: {
  r: Row;
  data: Dataset;
  mode: 'where' | 'plan';
  filter: Filter;
  focused: boolean;
  hot: boolean;
  proposals: Set<string>;
  reorgs: Set<string>;
  byCat: Record<string, number>;
  propMax: number;
  catMax: number;
  total: number;
  onFocus: () => void;
  onOpen: () => void;
  onToggle: () => void;
}) {
  const { r, data, mode, focused, proposals, reorgs } = p;
  const cls = `vc-row vc-row-${r.kind} vc-cols-${mode} ${focused ? 'focus' : ''} ${p.hot ? 'hot' : ''}`;
  const common = { id: `vc-row-${r.key}`, 'data-key': r.key, className: cls, onMouseDown: p.onFocus, role: 'row' } as const;

  if (r.kind === 'cat') {
    const c = r.cat;
    const locked = !!c.protected;
    const canFree = data.proposals.filter((x) => x.categoryId === c.id && x.action === 'delete').reduce((s, x) => s + x.bytes, 0);
    const caret = (
      <span className={`vc-caret ${r.open ? 'open' : ''}`} onClick={(e) => (e.stopPropagation(), p.onOpen())}>
        <ChevronRight size={14} strokeWidth={2.2} />
      </span>
    );
    const name = (
      <span className="vc-cat-name">
        <i className="vc-swatch" style={{ background: catColor(c, 'dot') }} />
        <b>{c.name}</b>
        {locked && (
          <span className="vc-locked">
            <Lock size={11} strokeWidth={2.4} /> Locked
          </span>
        )}
      </span>
    );
    if (mode === 'where' || locked || p.filter === 'locked') {
      return (
        <div {...common} className={`${cls} ${locked ? 'is-locked' : ''}`} onDoubleClick={p.onOpen}>
          {caret}
          <span className="vc-cell-name">
            {name}
            <small>{c.blurb}</small>
          </span>
          {mode === 'plan' && <span className="vc-span3 vc-mute vc-never">{locked ? 'Vacancy never touches this' : ''}</span>}
          {mode === 'where' && <SizeBar frac={c.bytes / p.catMax} color={catColor(c)} dim={locked} />}
          {mode === 'where' && <span className="r num">{gb(c.bytes)}</span>}
          {mode === 'where' && <span className="r num vc-mute c-pct">{((c.bytes / p.total) * 100).toFixed(1)}%</span>}
          {mode === 'where' ? (
            <span className="r num">
              {locked ? (
                <span className="vc-mute">Locked</span>
              ) : canFree ? (
                <span className="pink">{gb(canFree)}</span>
              ) : r.props.some((x) => x.action === 'hint') ? (
                <span className="amber" title="Advice Vacancy can't carry out itself">1 tip</span>
              ) : (
                <span className="vc-mute">None</span>
              )}
            </span>
          ) : (
            <>
              <SizeBar frac={c.bytes / p.catMax} color={catColor(c)} dim />
              <span className="r num vc-mute">{gb(c.bytes)}</span>
            </>
          )}
        </div>
      );
    }
    const del = r.props.filter((x) => x.action === 'delete');
    const on = del.filter((x) => proposals.has(x.id));
    const onBytes = on.reduce((s, x) => s + x.bytes, 0);
    const allBytes = del.reduce((s, x) => s + x.bytes, 0);
    return (
      <div {...common}>
        <Tick on={on.length > 0 && on.length === del.length} mixed={on.length > 0 && on.length < del.length} onClick={p.onToggle} label={`Select all in ${c.name}`} />
        <span className="vc-cell-name vc-span-group">
          <span className="vc-cat-name">
            {caret}
            <i className="vc-swatch" style={{ background: catColor(c, 'dot') }} />
            <b>{c.name}</b>
            <span className="vc-mute">
              {on.length} of {del.length} selected
            </span>
          </span>
        </span>
        <SizeBar frac={allBytes / p.catMax} color={catColor(c)} dim sub={onBytes / p.catMax} />
        <span className="r num">
          <b className={onBytes ? 'pink' : 'vc-mute'}>{gb(onBytes)}</b>
        </span>
      </div>
    );
  }

  if (r.kind === 'item') {
    const it = r.item;
    return (
      <div {...common} className={`${cls} ${r.last ? 'last' : ''}`}>
        <span className="vc-tree-guide" />
        <span className="vc-cell-name">
          <span className="vc-item-name">{it.name}</span>
          <small title={it.path}>
            {it.lastUsedDays != null && <span className="vc-item-used">Last used {formatAgo(it.lastUsedDays)}</span>}
            {it.path}
          </small>
        </span>
        {mode === 'where' ? (
          <>
            <SizeBar frac={it.bytes / p.catMax} color={catColor(r.cat)} dim={!!r.cat.protected} />
            <span className="r num">{gb(it.bytes)}</span>
            <span className="r num vc-mute c-pct">{((it.bytes / p.total) * 100).toFixed(1)}%</span>
            <span />
          </>
        ) : (
          <>
            <span className="vc-span3" />
            <SizeBar frac={it.bytes / p.catMax} color={catColor(r.cat)} dim />
            <span className="r num vc-mute">{gb(it.bytes)}</span>
          </>
        )}
      </div>
    );
  }

  if (r.kind === 'prop') {
    const x = r.p;
    const on = proposals.has(x.id);
    const hint = x.action === 'hint';
    const last = lastUsedFor(data, x);
    return (
      <div {...common} className={`${cls} ${on ? 'on' : ''} ${hint ? 'is-hint' : ''} ${r.last ? 'last' : ''}`} onDoubleClick={p.onToggle}>
        <Tick on={on} onClick={p.onToggle} label={hint ? `Add ${x.title} to your list` : `Select ${x.title}`} />
        <span className="vc-cell-name">
          <span className="vc-prop-title">
            <span>{x.title}</span>
            {hint && <span className="vc-hint-tag">You do this</span>}
          </span>
          <small>{x.source === 'ai' && x.aiNote ? <span className="vc-ai-line">{x.aiNote}</span> : x.summary}</small>
        </span>
        <span>
          <RiskBadge risk={x.risk} />
        </span>
        <span>
          <SourceChip data={data} source={x.source} />
        </span>
        <span className="num vc-mute c-last">{formatAgo(last) || <span className="vc-dash">n/a</span>}</span>
        <SizeBar frac={x.bytes / p.propMax} color={on ? 'var(--vc-pink)' : catColor(r.cat)} dim={!on} dashed={hint} />
        <span className="r num">{hint ? <span className="vc-mute" title="Not counted: Vacancy can't do this itself">{gb(x.bytes)}</span> : <b className={on ? '' : 'vc-mute'}>{gb(x.bytes)}</b>}</span>
      </div>
    );
  }

  if (r.kind === 'reorgGroup') {
    const on = data.reorgs.filter((x) => reorgs.has(x.id)).length;
    const moves = data.reorgs.filter((x) => reorgs.has(x.id)).reduce((s, x) => s + x.moveCount, 0);
    return (
      <div {...common}>
        <Tick on={on > 0 && on === data.reorgs.length} mixed={on > 0 && on < data.reorgs.length} onClick={p.onToggle} label="Select all reorganizations" />
        <span className="vc-cell-name vc-span-group">
          <span className="vc-cat-name">
            <span className={`vc-caret open`} onClick={(e) => (e.stopPropagation(), p.onOpen())}>
              <ChevronRight size={14} strokeWidth={2.2} />
            </span>
            <FolderInput size={14} className="vc-reorg-ic" />
            <b>Reorganize folders</b>
            <span className="vc-mute">
              {on} of {data.reorgs.length} selected. Every move can be undone.
            </span>
          </span>
        </span>
        <span />
        <span className="r num">
          <b>{n(moves)}</b> <span className="vc-mute">moves</span>
        </span>
      </div>
    );
  }

  const x = r.r;
  const on = reorgs.has(x.id);
  return (
    <div {...common} className={`${cls} ${on ? 'on' : ''} ${r.last ? 'last' : ''}`} onDoubleClick={p.onOpen}>
      <Tick on={on} onClick={p.onToggle} label={`Select ${x.title}`} />
      <span className="vc-cell-name">
        <span className="vc-prop-title">
          <span>{x.title}</span>
        </span>
        <small>{x.summary}</small>
      </span>
      <span>
        <span className="vc-risk vc-risk-undo">
          <ShieldCheck size={11} strokeWidth={2.4} />
          Undoable
        </span>
      </span>
      <span>
        <SourceChip data={data} source="ai" />
      </span>
      <span className="vc-mute c-last">{x.folder}</span>
      <span className="vc-open-diff">
        <button className="vc-link" onClick={(e) => (e.stopPropagation(), p.onOpen())}>
          Before and after
        </button>
      </span>
      <span className="r num">
        <b className={on ? '' : 'vc-mute'}>{n(x.moveCount)}</b>
      </span>
    </div>
  );
}


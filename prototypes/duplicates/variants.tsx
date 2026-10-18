// THROWAWAY: three structurally different duplicate finders on the Atlas map.
//   A  Plan row: progress and the group list live in the Duplicate files row of the plan tray.
//   B  Map links: a quiet toolbar pill; copies are tiles on the map, linked to where the kept copy lives, with a
//      thumbnail panel in place of the tray.
//   C  Compare sheet: a floating HUD while hashing, then a full sheet that pages through groups side by side.
import { ArrowLeft, ArrowRight, ChevronDown, ChevronLeft, ChevronRight, Copy as CopyIcon, Info, LoaderCircle, Undo2, X } from 'lucide-react';
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { formatBytes, formatCount } from '../../src/format';
import { layoutAtlas, type Leaf, type Rect } from '../../src/map/model';
import { Checkbox, RiskChip, SourceTag } from '../../src/plan/bits';
import { countLine, freedOf, keptOf, KIND_LABEL, removedOf, type Group, type Kind } from './data';
import { ago, DupMap, hashLine, included, KindIcon, OtherRows, Reason, Thumb, TrayFoot, TrayHead, whyExtra, type Ctx } from './shared';

const bucket = (k: Kind) => (k === 'photo' ? 'Photos' : k === 'video' ? 'Videos' : 'Other files');
const BUCKETS = ['Photos', 'Videos', 'Other files'] as const;

function Notes({ c }: { c: Ctx }) {
  return (
    <ul className="dup-notes">
      <li><Info size={12} aria-hidden />{c.d.hardLinked} hard-linked files share one copy on disk, so they aren’t duplicates and stay.</li>
      {c.d.notes.map((n) => <li key={n}><Info size={12} aria-hidden />{n}</li>)}
      <li><Info size={12} aria-hidden />Only exact copies over 1 MB in Pictures, {c.platform === 'macos' ? 'Movies' : 'Videos'}, Documents, Downloads and Desktop. Files another proposal already deletes, like old installers, aren’t counted twice.</li>
    </ul>
  );
}

function summaryOf(c: Ctx): string {
  return `${countLine(c.d.groups)} have an exact copy elsewhere. Vacancy keeps one copy of each.`;
}

// =====================================================================================================
// A · Plan row
// =====================================================================================================

export function VariantA({ c }: { c: Ctx }) {
  const n = c.d.proposals.length + 1;
  const onCount = c.on.length + (c.dupsOn ? 1 : 0);
  return (
    <>
      <DupMap c={c} />
      <aside className="tray" aria-label="Plan">
        <TrayHead />
        <div className="tray-scroll">
          <section className="tray-group" aria-label="Delete">
            <h3>Delete <span>{onCount} of {n}</span></h3>
            <DupRowA c={c} />
            <OtherRows c={c} />
          </section>
        </div>
        <TrayFoot c={c} />
      </aside>
    </>
  );
}

function DupRowA({ c }: { c: Ctx }) {
  const [open, setOpen] = useState(false);
  const h = hashLine(c.d, c.hash.t);
  const all = c.d.groups.reduce((s, g) => s + freedOf(g), 0);
  if (!c.hash.done)
    return (
      <div className="tray-row dup-row is-hashing">
        <span className="check is-wait" aria-hidden><LoaderCircle size={11} className="tray-spin" /></span>
        <div className="tray-row-main">
          <span className="tray-row-title"><span>Duplicate files</span></span>
          <p className="dup-progress" role="status" aria-label="Duplicate check progress">{h.label}</p>
          <span className="dup-bar" aria-hidden><i style={{ width: `${h.pct}%` }} /></span>
          <p className="tray-row-sum">{h.stage} Keep using the map and plan meanwhile.</p>
        </div>
        <span className="tray-row-size is-wait">…</span>
      </div>
    );
  return (
    <div className={['tray-row dup-row', c.dupsOn && 'is-on'].filter(Boolean).join(' ')} onMouseEnter={() => c.setHot('dups')} onMouseLeave={() => c.setHot(null)}>
      <Checkbox on={c.dupsOn} onChange={() => c.setDupsOn(!c.dupsOn)} label="Include Duplicate files" />
      <div className="tray-row-main">
        <button type="button" className="tray-row-title" onClick={() => setOpen(!open)} aria-expanded={open}>
          <span>Duplicate files</span>
          <ChevronDown size={13} className={open ? 'is-open' : undefined} aria-hidden />
        </button>
        <p className="tray-row-sum">{summaryOf(c)}</p>
        <div className="tray-row-meta">
          <RiskChip risk="review" />
          <SourceTag source="rule" agent={null} />
          <button type="button" className="link-button dup-review-link" onClick={() => setOpen(!open)} aria-expanded={open}>
            {open ? 'Hide groups' : `Review ${c.d.groups.length} groups`}
          </button>
        </div>
        {open && <GroupsA c={c} />}
      </div>
      <span className="tray-row-size">{formatBytes(c.dupsOn ? c.totals.dupBytes : all)}</span>
    </div>
  );
}

function GroupsA({ c }: { c: Ctx }) {
  const [more, setMore] = useState<string[]>([]);
  return (
    <div className={c.dupsOn ? 'dupA-groups' : 'dupA-groups is-off'}>
      <p className="dupA-lead">
        {c.dupsOn ? 'Vacancy keeps one copy of each group and deletes the rest. Untick a group to keep all of its copies.' : 'Tick Duplicate files to include these. Untick a group to keep all of its copies.'}
      </p>
      {BUCKETS.map((b) => {
        const gs = c.d.groups.filter((g) => bucket(g.kind) === b);
        const shown = more.includes(b) ? gs : gs.slice(0, 3);
        return (
          <section key={b} className="dupA-bucket" aria-label={b}>
            <h4>{b} <span>{gs.length} groups · {formatBytes(gs.reduce((s, g) => s + freedOf(g), 0))}</span></h4>
            <ul>
              {shown.map((g) => <GroupA key={g.id} g={g} c={c} />)}
            </ul>
            {gs.length > 3 && (
              <button type="button" className="replay dupA-more" onClick={() => setMore(more.includes(b) ? more.filter((x) => x !== b) : [...more, b])}>
                {more.includes(b) ? `Show fewer ${b.toLowerCase()}` : `Show ${gs.length - 3} more ${b.toLowerCase()}`}
              </button>
            )}
          </section>
        );
      })}
      <Notes c={c} />
    </div>
  );
}

function GroupA({ g, c }: { g: Group; c: Ctx }) {
  const on = !c.excluded.includes(g.id);
  const k = keptOf(g);
  return (
    <li className={on ? 'dupA-g' : 'dupA-g is-left'}>
      <Checkbox on={on} onChange={() => c.toggleGroup(g.id)} label={`Remove extra copies of ${g.name}`} />
      <div className="dupA-main">
        <div className="dupA-name"><KindIcon kind={g.kind} size={13} /><b title={g.name}>{g.name}</b><span>{on ? formatBytes(freedOf(g)) : 'kept'}</span></div>
        <Reason g={g} />
        <div className="dupA-line is-keep"><em>Keeps</em><code title={k.path}>{k.dir}</code></div>
        {removedOf(g).map((r) => (
          <div key={r.id} className="dupA-line is-gone"><em>{on ? 'Deletes' : 'Stays'}</em><code title={r.path}>{r.dir}{c.d.sep}{r.file}</code></div>
        ))}
      </div>
    </li>
  );
}

// =====================================================================================================
// B · Map links
// =====================================================================================================

export function VariantB({ c }: { c: Ctx }) {
  const [panel, setPanel] = useState(false);
  const [filter, setFilter] = useState<'All' | (typeof BUCKETS)[number]>('All');
  const [picked, setPicked] = useState<string | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const h = hashLine(c.d, c.hash.t);
  const lit = c.d.groups.find((g) => g.id === (hover ?? picked)) ?? null;
  const all = c.d.groups.reduce((s, g) => s + freedOf(g), 0);
  const openPanel = () => {
    setPanel(true);
    setPicked((p) => p ?? c.d.groups[0]?.id ?? null);
  };
  const keeperTiles = new Set(lit ? [c.d.folderTiles[keptOf(lit).folder]] : []);
  const goneTiles = new Set(lit ? removedOf(lit).map((r) => c.d.tiles[r.folder]) : []);

  const pill = !c.hash.done ? (
    <span className="dupB-pill" role="status" aria-label="Duplicate check progress">
      <LoaderCircle size={12} className="tray-spin" aria-hidden />
      Finding duplicates <b>{h.pct}%</b>
      <span className="dupB-pill-bar" aria-hidden><i style={{ width: `${h.pct}%` }} /></span>
    </span>
  ) : (
    <button type="button" className={panel ? 'dupB-pill is-ready is-open' : 'dupB-pill is-ready'} onClick={() => (panel ? setPanel(false) : openPanel())} aria-pressed={panel}>
      <CopyIcon size={12} aria-hidden />
      {c.d.groups.length} duplicate groups · <b>{formatBytes(all)}</b>
    </button>
  );

  const lines = (size: { w: number; h: number }, model: Parameters<typeof layoutAtlas>[0]): ReactNode => {
    if (!panel || !lit || !size.w) return null;
    const rects = layoutAtlas(model, size.w, size.h, null);
    const from = rects.get(c.d.folderTiles[keptOf(lit).folder] ?? '');
    if (!from) return null;
    const fx = from.x + from.w / 2;
    const fy = from.y + from.h / 2;
    return (
      <svg className="dupB-links" width={size.w} height={size.h} aria-hidden>
        {removedOf(lit).map((r) => {
          const to = rects.get(c.d.tiles[r.folder] ?? '');
          if (!to) return null;
          const tx = to.x + to.w / 2;
          const ty = to.y + to.h / 2;
          const mx = (fx + tx) / 2;
          const my = Math.min(fy, ty) - 40;
          return (
            <g key={r.id}>
              <path d={`M${fx},${fy} Q${mx},${my} ${tx},${ty}`} />
              <circle cx={tx} cy={ty} r={4} />
            </g>
          );
        })}
        <circle className="is-keep" cx={fx} cy={fy} r={5} />
      </svg>
    );
  };

  const extraLook = (leaf: Leaf, rect: Rect) => {
    if (leaf.proposal === 'dups') return panel && goneTiles.has(leaf.id) ? { cls: 'is-linked' } : null;
    // The tag only where it clears the tile's name; small tiles keep the white edge and the link's white end.
    if (panel && keeperTiles.has(leaf.id)) return rect.w >= 150 && rect.h >= 50 ? { cls: 'is-keeper', badge: <span className="dupB-kept-tag">Kept here</span> } : { cls: 'is-keeper' };
    return null;
  };
  const status = (leaf: Leaf): ReactNode =>
    leaf.proposal === 'dups' ? (
      <>
        <div className="map-tip-meta"><RiskChip risk="review" /><SourceTag source="rule" agent={null} /></div>
        <p className="map-tip-sum">Extra copies of files kept elsewhere. One copy of each stays.</p>
        <p className="map-tip-act">Click to see the groups.</p>
      </>
    ) : null;

  return (
    <>
      <DupMap
        c={c}
        tools={pill}
        extraLook={extraLook}
        overlay={lines}
        status={status}
        onLeaf={(leaf) => (leaf.proposal === 'dups' ? openPanel() : leaf.proposal && c.toggle(leaf.proposal))}
      />
      {panel ? (
        <PanelB c={c} filter={filter} setFilter={setFilter} picked={lit?.id ?? null} pick={setPicked} hover={setHover} close={() => setPanel(false)} />
      ) : (
        <aside className="tray" aria-label="Plan">
          <TrayHead />
          <div className="tray-scroll">
            <section className="tray-group" aria-label="Delete">
              <h3>Delete <span>{c.on.length + (c.dupsOn ? 1 : 0)} of {c.d.proposals.length + (c.hash.done ? 1 : 0)}</span></h3>
              {c.hash.done && (
                <div className={['tray-row dup-row', c.dupsOn && 'is-on'].filter(Boolean).join(' ')}>
                  <Checkbox on={c.dupsOn} onChange={() => c.setDupsOn(!c.dupsOn)} label="Include Duplicate files" />
                  <div className="tray-row-main">
                    <button type="button" className="tray-row-title" onClick={openPanel}><span>Duplicate files</span><ChevronRight size={13} aria-hidden /></button>
                    <p className="tray-row-sum">{summaryOf(c)}</p>
                    <div className="tray-row-meta">
                      <RiskChip risk="review" />
                      <button type="button" className="link-button dup-review-link" onClick={openPanel}>See them on the map</button>
                    </div>
                  </div>
                  <span className="tray-row-size">{formatBytes(c.dupsOn ? c.totals.dupBytes : all)}</span>
                </div>
              )}
              <OtherRows c={c} />
            </section>
          </div>
          <TrayFoot c={c} />
        </aside>
      )}
    </>
  );
}

function PanelB({ c, filter, setFilter, picked, pick, hover, close }: {
  c: Ctx; filter: 'All' | (typeof BUCKETS)[number]; setFilter: (f: 'All' | (typeof BUCKETS)[number]) => void;
  picked: string | null; pick: (id: string) => void; hover: (id: string | null) => void; close: () => void;
}) {
  const gs = c.d.groups.filter((g) => filter === 'All' || bucket(g.kind) === filter);
  const inc = included(c.d, c.excluded);
  const bytes = inc.reduce((s, g) => s + freedOf(g), 0);
  return (
    <aside className="tray dupB-panel" aria-label="Duplicate files">
      <TrayHead
        title="Duplicate files"
        sub="Linked tiles show where the kept copy lives and where the extra copies go from. Untick a group to keep all of its copies."
        aside={<button type="button" className="icon-btn" onClick={close} aria-label="Back to the plan"><ArrowLeft size={16} aria-hidden /></button>}
      />
      <div className="dupB-filters">
        <div className="seg" role="group" aria-label="Show">
          {(['All', ...BUCKETS] as const).map((b) => (
            <button key={b} type="button" className={filter === b ? 'is-on' : undefined} onClick={() => setFilter(b)}>
              {b} <span>{b === 'All' ? c.d.groups.length : c.d.groups.filter((g) => bucket(g.kind) === b).length}</span>
            </button>
          ))}
        </div>
      </div>
      <div className="tray-scroll dupB-list">
        {gs.map((g) => {
          const on = !c.excluded.includes(g.id);
          const k = keptOf(g);
          const gone = removedOf(g);
          return (
            <div
              key={g.id}
              className={['dupB-card', !on && 'is-left', picked === g.id && 'is-picked'].filter(Boolean).join(' ')}
              onMouseEnter={() => hover(g.id)}
              onMouseLeave={() => hover(null)}
              onClick={() => pick(g.id)}
            >
              <Checkbox on={on} onChange={() => c.toggleGroup(g.id)} label={`Remove extra copies of ${g.name}`} />
              <div className="dupB-thumbs">
                <span className="dupB-th"><Thumb group={g} label="Kept" /><small>{k.folderName}</small></span>
                <ArrowRight size={12} className="dupB-arrow" aria-hidden />
                {gone.slice(0, 2).map((r) => (
                  <span key={r.id} className="dupB-th"><Thumb group={g} gone={on} label={on ? 'Goes' : 'Stays'} /><small>{r.folderName}</small></span>
                ))}
              </div>
              <div className="dupB-text">
                <b title={g.name}>{g.name}</b>
                <Reason g={g} />
                <span className="dupB-meta">{formatBytes(g.bytes)} each · {gone.length} extra {gone.length === 1 ? 'copy' : 'copies'}</span>
              </div>
              <span className="dupB-size">{on ? formatBytes(freedOf(g)) : 'kept'}</span>
            </div>
          );
        })}
        <div className="dupB-notes"><Notes c={c} /></div>
      </div>
      <div className="tray-foot">
        <div className="tray-total">
          <span className="tray-total-num">{formatBytes(bytes)}</span>
          <span className="tray-total-lbl">in {inc.length} of {c.d.groups.length} groups</span>
        </div>
        <ul className="tray-notes"><li>{formatCount(inc.reduce((n, g) => n + g.copies.length - 1, 0))} extra copies go; one copy of each group stays</li></ul>
        {c.dupsOn ? (
          <button type="button" className="cta tray-cta" onClick={close}>Back to the plan</button>
        ) : (
          <button type="button" className="cta tray-cta" disabled={inc.length === 0} onClick={() => { c.setDupsOn(true); close(); }}>
            Add {inc.length} groups to the plan
          </button>
        )}
      </div>
    </aside>
  );
}

// =====================================================================================================
// C · Compare sheet
// =====================================================================================================

export function VariantC({ c }: { c: Ctx }) {
  const [sheet, setSheet] = useState(false);
  const [later, setLater] = useState(false);
  const all = c.d.groups.reduce((s, g) => s + freedOf(g), 0);
  const h = hashLine(c.d, c.hash.t);
  const hud = (): ReactNode => {
    if (!c.hash.done)
      return (
        <div className="hud dupC-hud" role="status" aria-label="Duplicate check progress">
          <div className="hud-nums">
            <span className="dupC-hud-title"><CopyIcon size={14} aria-hidden /> Looking for duplicate files</span>
            <span><b>{h.pct}%</b></span>
          </div>
          <p className="dupC-hud-stage">{h.stage}</p>
          <div className="hud-path" title={h.path}>{h.path}</div>
          <div className="hud-bar"><i style={{ width: `${h.pct}%` }} /></div>
          <p className="dupC-hud-foot">Keep exploring the map and building your plan. Nothing is deleted.</p>
        </div>
      );
    if (later || c.dupsOn) return null;
    return (
      <div className="hud dupC-hud is-ready" role="status">
        <div className="dupC-ready">
          <CopyIcon size={18} aria-hidden />
          <div>
            <b>{c.d.groups.length} groups of duplicate files · {formatBytes(all)}</b>
            <span>{countLine(c.d.groups)} have an exact copy elsewhere.</span>
          </div>
          <button type="button" className="cta cta--sm" onClick={() => setSheet(true)}>Compare copies</button>
          <button type="button" className="hud-cancel" onClick={() => setLater(true)}>Later</button>
        </div>
      </div>
    );
  };
  return (
    <>
      <DupMap c={c} overlay={hud} onLeaf={(leaf) => (leaf.proposal === 'dups' ? setSheet(true) : leaf.proposal && c.toggle(leaf.proposal))} />
      <aside className="tray" aria-label="Plan">
        <TrayHead />
        <div className="tray-scroll">
          <section className="tray-group" aria-label="Delete">
            <h3>Delete <span>{c.on.length + (c.dupsOn ? 1 : 0)} of {c.d.proposals.length + (c.hash.done ? 1 : 0)}</span></h3>
            {c.hash.done && (
              <div className={['tray-row dup-row', c.dupsOn && 'is-on'].filter(Boolean).join(' ')}>
                <Checkbox on={c.dupsOn} onChange={() => c.setDupsOn(!c.dupsOn)} label="Include Duplicate files" />
                <div className="tray-row-main">
                  <button type="button" className="tray-row-title" onClick={() => setSheet(true)}><span>Duplicate files</span><ChevronRight size={13} aria-hidden /></button>
                  <p className="tray-row-sum">
                    {c.dupsOn ? `${c.totals.dupGroups} of ${c.d.groups.length} groups. One copy of each stays.` : summaryOf(c)}
                  </p>
                  <div className="tray-row-meta">
                    <RiskChip risk="review" />
                    <button type="button" className="link-button dup-review-link" onClick={() => setSheet(true)}>Compare copies</button>
                  </div>
                </div>
                <span className="tray-row-size">{formatBytes(c.dupsOn ? c.totals.dupBytes : all)}</span>
              </div>
            )}
            <OtherRows c={c} />
          </section>
        </div>
        <TrayFoot c={c} />
      </aside>
      {sheet && <SheetC c={c} close={() => setSheet(false)} />}
    </>
  );
}

function SheetC({ c, close }: { c: Ctx; close: () => void }) {
  const [at, setAt] = useState(0);
  const gs = c.d.groups;
  const g = gs[at]!;
  const on = !c.excluded.includes(g.id);
  const inc = included(c.d, c.excluded);
  const bytes = inc.reduce((s, x) => s + freedOf(x), 0);
  const k = keptOf(g);
  const go = (d: number) => setAt((i) => Math.max(0, Math.min(gs.length - 1, i + d)));
  const skip = () => {
    c.toggleGroup(g.id);
    if (on) go(1);
  };
  const add = () => {
    c.setDupsOn(inc.length > 0);
    close();
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'ArrowRight') go(1);
      else if (e.key === 'ArrowLeft') go(-1);
      else if (e.key === 'Escape') close();
      else return;
      e.preventDefault();
    };
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  });
  const strip = useMemo(() => gs.map((x, i) => ({ x, i })), [gs]);
  return (
    <section className="dupC-sheet" role="dialog" aria-modal="true" aria-labelledby="dupC-title">
      <header className="dupC-head">
        <div>
          <h2 id="dupC-title">Duplicate files</h2>
          <p>Vacancy keeps one copy of each group and deletes the rest. Skip a group to keep every copy.</p>
        </div>
        <div className="dupC-count" aria-live="polite">
          <span><b>{inc.length}</b> to clean</span>
          <span><b>{gs.length - inc.length}</b> skipped</span>
          <span><b>{formatBytes(bytes)}</b> to free</span>
        </div>
        <button type="button" className="icon-btn" onClick={close} aria-label="Close"><X size={16} aria-hidden /></button>
      </header>
      <div className="dupC-strip" role="tablist" aria-label="Groups">
        {strip.map(({ x, i }) => (
          <button
            key={x.id}
            type="button"
            role="tab"
            aria-selected={i === at}
            aria-label={`Group ${i + 1}: ${x.name}${c.excluded.includes(x.id) ? ', skipped' : ''}`}
            className={['dupC-dot', i === at && 'is-at', c.excluded.includes(x.id) && 'is-skip', `is-${x.kind}`].filter(Boolean).join(' ')}
            onClick={() => setAt(i)}
          />
        ))}
      </div>
      <div className={on ? 'dupC-body' : 'dupC-body is-skip'}>
        <div className="dupC-col dupC-keep">
          <span className="dupC-label">Keeps</span>
          <div className="dupC-card is-keep">
            <Thumb group={g} size="lg" />
            <div className="dupC-info">
              <b>{k.file}</b>
              <code>{k.path}</code>
              <span>{formatBytes(g.bytes)} · modified {ago(k.modifiedDays)}</span>
              <Reason g={g} />
            </div>
          </div>
        </div>
        <ChevronRight size={22} className="dupC-arrow" aria-hidden />
        <div className="dupC-col dupC-gone">
          <span className="dupC-label">{on ? `Deletes ${removedOf(g).length === 1 ? 'this copy' : `these ${removedOf(g).length} copies`}` : 'Skipped: every copy stays'}</span>
          {removedOf(g).map((r) => (
            <div key={r.id} className="dupC-card is-gone">
              <Thumb group={g} gone={on} />
              <div className="dupC-info">
                <b>{r.file}</b>
                <code>{r.path}</code>
                <span>{formatBytes(g.bytes)} · modified {ago(r.modifiedDays)} · {whyExtra(r, g)}</span>
              </div>
            </div>
          ))}
          <p className="dupC-same">Same size and same contents, byte for byte.</p>
        </div>
      </div>
      <footer className="dupC-foot">
        <span className="dupC-where"><KindIcon kind={g.kind} size={13} /> Group {at + 1} of {gs.length} · {KIND_LABEL[g.kind]}</span>
        <button type="button" className="btn" onClick={skip}>{on ? 'Skip group' : <><Undo2 size={13} aria-hidden /> Include group</>}</button>
        <span className="dupC-nav">
          <button type="button" className="btn" onClick={() => go(-1)} disabled={at === 0} aria-label="Previous group"><ChevronLeft size={15} aria-hidden /></button>
          <button type="button" className="btn" onClick={() => go(1)} disabled={at === gs.length - 1} aria-label="Next group"><ChevronRight size={15} aria-hidden /></button>
        </span>
        <button type="button" className="cta" onClick={add} disabled={inc.length === 0}>
          {c.dupsOn ? 'Done' : `Add ${inc.length} groups to the plan`}
        </button>
      </footer>
    </section>
  );
}

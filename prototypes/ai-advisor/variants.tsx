// THROWAWAY: three places for Claude's jobs. A puts them where each thing already is (the tray, its rows, the map's
// hover cards, the Tidy drawer). B docks one Claude panel with a running conversation that links into the map and
// the tray. C makes Claude a step of its own between the plan and the confirm, with badges on the map.
import { useEffect, useRef, type ReactNode } from 'react';
import { ArrowLeft, Check, ChevronRight, CircleCheck, MessageCircle, PanelLeftClose, PanelLeftOpen, ShieldX, Sparkles, TriangleAlert, X } from 'lucide-react';
import type { Proposal } from '../../src/bridge';
import { formatBytes, formatCount } from '../../src/format';
import { Checkbox, RiskChip, SourceTag } from '../../src/plan/bits';
import { planTotals, tileOf } from '../../src/plan/totals';
import { activeMoves, afterTree, kindFolders } from '../../src/tidy/model';
import {
  AGENT, Basis, ChatThread, Composer, Drawer, HiddenFold, JobLine, LayoutSwitch, MapStage, Mark, PickDiff, Privacy, PrivacyChip, PrivacyRow,
  TipSection, Tray, Tree, pickRows, useAdvisorCopy, useExplained,
} from './common';
import { AI_ITEMS, BASIS_ON, EXPLAIN, PICK_REFUSED, REFUSED, REVIEW_OK, RULE_TIDY, TIDY_NOTES, claudeTidy, fmtGb, tileByPath } from './data';
import { folderSubject, proposalSubject, useS } from './store';

const N_EXPLAIN = EXPLAIN.length;

/** The privacy mode a finished job used, as a basis line. */
function JobBasis({ k }: { k: string }) {
  const st = useS();
  const pv = st.s.answered[k];
  if (!pv || st.jobOf(k).phase !== 'done') return null;
  const again = k === 'review' ? st.review : k === 'pick' ? st.pick : k === 'explain' ? st.explain : k === 'tidy' ? st.tidyLayouts : null;
  return (
    <>
      <Basis privacy={pv} text={pv === 'on' ? BASIS_ON : 'Claude also read files on this PC, except the hidden areas.'} />
      {again && pv !== st.s.privacy && (
        <button type="button" className="link-button cl-again" onClick={again}>Ask again with privacy mode {st.s.privacy}</button>
      )}
    </>
  );
}

/** Puts the map's hover and the tray's row on one item, and scrolls the row into view. */
function useShow() {
  const st = useS();
  return (id: string) => {
    st.set({ planOpen: true, hot: tileOf(id), focus: null });
    requestAnimationFrame(() => document.querySelector(`[data-row="${id}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }));
  };
}

// =====================================================================================================================
// A · In the tray: every job sits on the thing it's about.
// =====================================================================================================================

export function VariantA() {
  const st = useS();
  const { s } = st;
  const explainedOf = useExplained();
  const flags = st.flags();
  const flagOf = (id: string) => flags.find((f) => f.id === id);
  const review = st.jobOf('review');
  const preview = s.pickOpen;
  const next = preview ? st.pickPlan(s.plan) : null;
  const changes = preview ? pickRows(s.plan) : [];
  const changeOf = (id: string) => changes.find((c) => c.id === id);
  const ghosts = changes.filter((c) => c.change === 'add' && !s.extra.includes(c.id)).map((c) => AI_ITEMS[c.id]!);
  const ruleDeletes = st.proposals.filter((p) => p.action === 'delete' && s.plan.includes(p.id)).length;

  const bar = (
    <div className={st.on ? 'cl-bar' : 'cl-bar is-off'}>
      {!st.on ? (
        <p className="cl-bar-off"><Mark /> <span>Claude can review this plan or build one.</span> <button type="button" className="link-button" onClick={st.openDialog}>Turn on Claude</button></p>
      ) : (
        <>
          <div className="cl-bar-btns">
            <button type="button" className="btn btn--sm cl-btn" onClick={st.review} disabled={preview}><Mark /> Review my plan</button>
            <button type="button" className="btn btn--sm cl-btn" onClick={st.pick} disabled={preview}><Sparkles size={12} aria-hidden /> Let Claude pick</button>
            <PrivacyChip />
          </div>
          <JobLine job={review} working="Claude is reviewing your plan…" onCancel={() => st.cancel('review')} onRetry={st.review} />
          <JobLine job={st.jobOf('pick')} working="Claude is building a plan…" onCancel={() => st.cancel('pick')} onRetry={st.pick} />
          {review.phase === 'done' && !preview && (
            <p className="cl-bar-sum">{flags.length > 0 ? <><TriangleAlert size={12} aria-hidden /> Claude flagged {flags.length} of {ruleDeletes}. The rest look fine to it.</> : <><CircleCheck size={12} aria-hidden /> Nothing left that Claude flags.</>} <span>It only flags: it can’t add anything.</span></p>
          )}
          {review.phase === 'done' && !preview && <JobBasis k="review" />}
          {preview && (
            <div className="cl-bar-pick" role="status">
              <b>Claude’s plan, shown below.</b> {changes.filter((c) => c.change === 'untick').length} unticked, {changes.filter((c) => c.change === 'tick').length} ticked, {changes.filter((c) => c.change === 'add').length} of its own added unticked.
              <span className="cl-bar-ref"><ShieldX size={11} aria-hidden /> The Guard refused {PICK_REFUSED.length}: {PICK_REFUSED.map((k) => REFUSED[k]!.title).join(', ')}.</span>
              <JobBasis k="pick" />
            </div>
          )}
        </>
      )}
    </div>
  );

  const rowExtra = (p: Proposal): ReactNode => {
    const c = changeOf(p.id);
    if (c) return <p className={`cl-change is-${c.change}`}><Mark size={11} /> {c.change === 'untick' ? 'Claude unticks this' : c.change === 'tick' ? 'Claude ticks this' : 'Claude adds this, unticked'}: {c.why}</p>;
    if (preview) return null;
    const f = flagOf(p.id);
    if (f)
      return (
        <div className="cl-flag" role="note">
          <p><TriangleAlert size={12} aria-hidden /> <span><b>Claude flags this.</b> {f.reason}</span></p>
          <div className="cl-flag-acts">
            <button type="button" className="link-button" onClick={() => st.togglePlan(p.id)}>Untick it</button>
            <button type="button" className="link-button" onClick={() => st.keepFlag(p.id)}>Keep it</button>
            <button type="button" className="link-button" onClick={() => st.openChat(proposalSubject(p))}>Why?</button>
          </div>
        </div>
      );
    if (review.phase === 'done' && s.plan.includes(p.id) && REVIEW_OK[p.id]) return <p className="cl-ok"><Check size={11} strokeWidth={3} aria-hidden /> Claude: {REVIEW_OK[p.id]}</p>;
    return null;
  };

  const chat = s.chatOpen ? s.chats[s.chatOpen] : null;
  return (
    <>
      <MapStage
        badges
        toolbar={
          <>
            {st.on && !s.planOpen && <PrivacyChip />}
            <JobLine job={st.jobOf('explain')} working="Claude is looking at the map…" onCancel={() => st.cancel('explain')} onRetry={st.explain} />
            {st.jobOf('explain').phase !== 'working' && (
              <button type="button" className="btn btn--sm cl-btn" onClick={st.explain} disabled={st.explained()}>
                <Mark /> {st.explained() ? `Claude explained ${N_EXPLAIN} folders` : 'Explain big folders'}
              </button>
            )}
          </>
        }
        status={(leaf) => { const e = explainedOf(leaf); return e ? <TipSection e={e} act="Click to ask Claude about it." /> : null; }}
        onFolder={(leaf) => { const e = explainedOf(leaf); const sub = e && folderSubject(e.path); if (!sub) return false; st.set({ drawer: null }); st.openChat(sub); return true; }}
      >
        {s.drawer && <Drawer id={s.drawer} top={<LayoutSwitch id={s.drawer} />} />}
        {chat && !s.drawer && (
          <section className="reorg-drawer cl-drawer cl-chat-drawer" role="dialog" aria-label={`Ask Claude about ${chat.subject.name}`} data-no-ripple>
            <header className="rd-head">
              <div><h2><Mark size={15} /> {chat.subject.name}</h2><p className="rd-path">Ask Claude · {chat.subject.kind === 'proposal' ? 'a plan item' : 'a folder on the map'}</p></div>
              <button type="button" className="icon-btn" onClick={() => st.set({ chatOpen: null })} aria-label="Close"><X size={16} aria-hidden /></button>
            </header>
            <div className="rd-body"><ChatThread id={chat.subject.id} /></div>
          </section>
        )}
      </MapStage>
      {s.planOpen && (
        <Tray
          head={bar}
          rowExtra={rowExtra}
          rowCls={(p) => { const c = changeOf(p.id); return c ? `is-pick-${c.change}` : flagOf(p.id) && !preview ? 'is-flagged' : undefined; }}
          {...(next ? { checked: (id: string) => next.includes(id), plan: next } : {})}
          ghosts={ghosts}
          onAsk={st.on ? (p) => { st.set({ drawer: null }); st.openChat(proposalSubject(p)); } : undefined}
          {...(preview ? { cta: <div className="cl-diff-actions is-tray"><button type="button" className="btn" onClick={st.discardPick}>Keep mine</button><button type="button" className="cta" onClick={st.acceptPick}>Use Claude’s plan</button></div> } : {})}
        />
      )}
    </>
  );
}

// =====================================================================================================================
// B · Claude panel: one docked place for every job, as a running conversation that links into the map and the tray.
// =====================================================================================================================

export function VariantB() {
  const st = useS();
  const { s } = st;
  const explainedOf = useExplained();
  return (
    <>
      <ClaudePanel />
      <MapStage
        badges
        status={(leaf) => { const e = explainedOf(leaf); return e ? <TipSection e={e} act="Click to ask Claude about it in the panel." /> : null; }}
        onFolder={(leaf) => { const e = explainedOf(leaf); const sub = e && folderSubject(e.path); if (!sub) return false; st.set({ folded: false }); st.openChat(sub, false); return true; }}
      >
        {s.drawer && <Drawer id={s.drawer} top={st.on ? <p className="cl-drawer-note"><Mark size={11} /> Choose the layout in the Claude panel.</p> : undefined} />}
      </MapStage>
      {s.planOpen && <Tray onAsk={st.on ? (p) => { st.set({ folded: false }); st.openChat(proposalSubject(p), false); } : undefined} />}
    </>
  );
}

function ClaudePanel() {
  const st = useS();
  const { s } = st;
  const log = useRef<HTMLDivElement>(null);
  const last = s.log.at(-1);
  const lastJob = last ? st.jobOf(last).phase : null;
  const n = last ? (s.chats[last.slice(5)]?.msgs.length ?? 0) : 0;
  useEffect(() => { log.current?.scrollTo({ top: log.current.scrollHeight, behavior: 'smooth' }); }, [s.log.length, last, lastJob, n]);
  if (s.folded)
    return (
      <aside className="cp is-folded" aria-label="Claude" data-no-ripple>
        <button type="button" className="cp-rail" onClick={() => st.set({ folded: false })} aria-label="Show Claude"><PanelLeftOpen size={16} aria-hidden /><Mark size={16} /><span>Claude</span></button>
      </aside>
    );
  const about = s.about ? s.chats[s.about] : null;
  return (
    <aside className="cp" aria-label="Claude" data-no-ripple>
      <header className="cp-head">
        <div>
          <h2><Mark size={15} /> Claude</h2>
          <p>{st.on ? (s.ai === 'slow' ? 'On · replies are slow' : s.ai === 'error' ? 'On · the last request failed' : 'On · through your Claude Code') : 'Off'}</p>
        </div>
        <button type="button" className="icon-btn" onClick={() => st.set({ folded: true })} aria-label="Fold the Claude panel"><PanelLeftClose size={16} aria-hidden /></button>
      </header>
      {!st.on ? <PanelOff /> : (
        <>
          <div className="cp-status"><PrivacyRow compact /><button type="button" className="link-button cp-off" onClick={st.turnOff}>Turn off Claude</button></div>
          <div className="cp-log" ref={log}>
            <div className="cp-msg is-claude">
              <p><Mark size={12} /> {s.privacy === 'on' ? 'I can see the names, sizes, counts and dates from this scan. Nothing inside your files.' : 'I can see this scan, and I can open files on this PC, except the hidden areas.'} I only propose: nothing changes until you confirm.</p>
            </div>
            {s.log.map((k) => <Entry key={k} k={k} />)}
          </div>
          <div className="cp-foot">
            <div className="cp-acts">
              <button type="button" className="cl-chip" onClick={st.review}>Review my plan</button>
              <button type="button" className="cl-chip" onClick={st.pick}>Let Claude pick</button>
              <button type="button" className="cl-chip" onClick={st.explain}>Explain big folders</button>
              <button type="button" className="cl-chip" onClick={st.tidyLayouts}>Suggest Tidy up layouts</button>
            </div>
            <Composer onSend={(q) => about && st.ask(about.subject.id, q)} disabled={!about?.first} placeholder={about ? `Ask about ${about.subject.name}` : 'Ask Claude about a row or a folder first'}>
              {about && <span className="cp-about"><MessageCircle size={11} aria-hidden /> <span className="cp-about-n">{about.subject.name}</span><button type="button" aria-label="Stop asking about it" onClick={() => st.set({ about: null })}><X size={11} aria-hidden /></button></span>}
            </Composer>
          </div>
        </>
      )}
    </aside>
  );
}

/** B, off: the privacy notice and the turn-on step live in the panel itself. */
function PanelOff() {
  const { s, st, checking, title, intro, row, recheck } = useAdvisorCopy();
  return (
    <div className="cp-off-body">
      <h3>{title}</h3>
      <p>{intro}</p>
      <div className="advisor-provider cp-provider"><span className="advisor-provider-icon" aria-hidden>✳</span><div><b>Claude Code</b><span>{row}</span></div></div>
      <Privacy compact />
      <HiddenFold />
      {s.ai === 'off' && (s.dialog === 'checking'
        ? <p className="cl-job" role="status"><span>Checking that Claude Code answers…</span><button type="button" className="link-button" onClick={() => st.cancelTurnOn('closed')}>Cancel</button></p>
        : <button type="button" className="cta cp-on" onClick={() => st.turnOn('closed')}><Mark size={13} /> Turn on Claude</button>)}
      {(s.ai === 'notfound' || s.ai === 'signedout') && <button type="button" className="btn cp-on" onClick={recheck} disabled={checking}>{checking ? 'Checking…' : 'Check again'}</button>}
    </div>
  );
}

const ASKED: Record<string, string> = { review: 'Review my plan', pick: 'Let Claude pick', explain: 'Explain big folders', tidy: 'Suggest Tidy up layouts' };

function Entry({ k }: { k: string }) {
  const st = useS();
  const { s } = st;
  const show = useShow();
  const job = st.jobOf(k);
  const retry = () => (k === 'review' ? st.review() : k === 'pick' ? st.pick() : k === 'explain' ? st.explain() : k === 'tidy' ? st.tidyLayouts() : undefined);
  const line = <JobLine job={job} working="Claude is working…" onCancel={() => st.cancel(k)} onRetry={retry} />;
  if (k.startsWith('chat:')) {
    const chat = s.chats[k.slice(5)];
    if (!chat) return null;
    return (
      <div className={s.about === chat.subject.id ? 'cp-entry is-about' : 'cp-entry'}>
        <p className="cp-you">About <ItemLink id={chat.subject.id} name={chat.subject.name} /></p>
        <div className="cp-msg is-claude"><ChatThread id={chat.subject.id} compose={false} /></div>
      </div>
    );
  }
  return (
    <div className="cp-entry">
      <p className="cp-you">{ASKED[k]}</p>
      {line}
      {job.phase === 'done' && (
        <div className="cp-msg is-claude">
          {k === 'review' && <ReviewReply show={show} />}
          {k === 'pick' && (s.pickOpen ? <PickDiff /> : <p>{s.pickAccepted ? 'You’re using my plan. My own items are in the tray, unticked.' : 'You kept your plan.'}</p>)}
          {k === 'explain' && <ExplainReply />}
          {k === 'tidy' && <TidyReply />}
          <JobBasis k={k} />
        </div>
      )}
    </div>
  );
}

function ItemLink({ id, name }: { id: string; name: string }) {
  const show = useShow();
  const st = useS();
  const isProposal = st.proposals.some((p) => p.id === id);
  return <button type="button" className="cp-link" onClick={() => (isProposal ? show(id) : st.set({ hot: id, focus: null }))}>{name}<ChevronRight size={11} aria-hidden /></button>;
}

function ReviewReply({ show }: { show: (id: string) => void }) {
  const st = useS();
  const flags = st.flags();
  const p = (id: string) => st.proposals.find((x) => x.id === id)!;
  return (
    <>
      <p><Mark size={12} /> {flags.length ? `I’d look at ${flags.length} of these before deleting:` : 'Nothing here worries me now.'}</p>
      {flags.map((f) => (
        <div key={f.id} className="cl-flag">
          <p><TriangleAlert size={12} aria-hidden /> <span><button type="button" className="cp-link" onClick={() => show(f.id)}>{p(f.id).title}<ChevronRight size={11} aria-hidden /></button> {f.reason}</span></p>
          <div className="cl-flag-acts"><button type="button" className="link-button" onClick={() => st.togglePlan(f.id)}>Untick it</button><button type="button" className="link-button" onClick={() => st.keepFlag(f.id)}>Keep it</button></div>
        </div>
      ))}
      <p className="cp-quiet">The rest look fine to me. A review only flags; to add things, use Let Claude pick.</p>
    </>
  );
}

function ExplainReply() {
  const st = useS();
  const open = st.s.answered.explain === 'off';
  return (
    <>
      <p><Mark size={12} /> {EXPLAIN.length} big folders the rules don’t recognise:</p>
      <ul className="cp-list">
        {EXPLAIN.map((e) => {
          const t = tileByPath(e.path);
          const sub = folderSubject(e.path);
          return (
            <li key={e.path}>
              <span className="cp-list-h">{t ? <ItemLink id={t.id} name={t.name} /> : e.path} <span>{t ? fmtGb(t.bytes) : ''}</span></span>
              <span>{open ? e.deep : e.short}</span>
              {open && e.hidden && <span className="cl-tip-hid">{e.hidden}</span>}
              {e.suggests && <span className="cp-sug"><Sparkles size={11} aria-hidden /> Suggests {AI_ITEMS[e.suggests]!.title}, in the tray unticked</span>}
              {e.refused && <span className="cp-ref"><ShieldX size={11} aria-hidden /> Suggestion refused by the Guard</span>}
              {sub && <button type="button" className="link-button cl-ask" onClick={() => st.openChat(sub, false)}><MessageCircle size={11} aria-hidden /> Ask</button>}
            </li>
          );
        })}
      </ul>
    </>
  );
}

function TidyReply() {
  const st = useS();
  return (
    <>
      <p><Mark size={12} /> Here’s how I’d sort them instead. Pick a layout per folder; the drawer shows the trees.</p>
      {RULE_TIDY.map((t) => {
        const mine = claudeTidy(t);
        const claude = st.s.layout[t.id] === 'claude';
        return (
          <div key={t.id} className="cp-tidy">
            <div className="cp-tidy-h"><b>{t.name}</b><button type="button" className="link-button" onClick={() => st.set({ drawer: t.id, planOpen: true })}>See the trees</button></div>
            <p className="cp-quiet">{TIDY_NOTES[t.id]}</p>
            <div className="cp-tidy-cols">
              <Pick on={!claude} onClick={() => st.setLayout(t.id, 'rule')} label="Built-in" folders={kindFolders(activeMoves(t, st.s.plan))} />
              <Pick on={claude} onClick={() => st.setLayout(t.id, 'claude')} label="Claude’s" folders={kindFolders(activeMoves(mine, st.s.plan))} />
            </div>
          </div>
        );
      })}
    </>
  );
}

function Pick({ on, onClick, label, folders }: { on: boolean; onClick: () => void; label: string; folders: [string, number][] }) {
  return (
    <button type="button" role="radio" aria-checked={on} className={on ? 'cp-pick is-on' : 'cp-pick'} onClick={onClick}>
      <span className="cp-pick-h">{on ? <CircleCheck size={12} aria-hidden /> : <span className="cp-radio" />} {label}</span>
      {folders.slice(0, 4).map(([n, c]) => <span key={n} className="cp-pick-row"><span>{n}</span><b>{c}</b></span>)}
      {folders.length > 4 && <span className="cp-pick-more">+{folders.length - 4} more</span>}
    </button>
  );
}

// =====================================================================================================================
// C · Review step: Claude is a pass of its own between the plan and the confirm, with badges on the map.
// =====================================================================================================================

export function VariantC() {
  const st = useS();
  const { s } = st;
  const explainedOf = useExplained();
  if (s.step === 'review') return <ReviewStep />;
  const t = planTotals(st.proposals, s.plan);
  const card = s.card ? EXPLAIN.find((e) => tileByPath(e.path)?.id === s.card) : null;
  const sub = card ? folderSubject(card.path) : null;
  return (
    <>
      <MapStage
        badges
        toolbar={
          <>
            <PrivacyChip />
            <JobLine job={st.jobOf('explain')} working="Claude is looking at the map…" onCancel={() => st.cancel('explain')} onRetry={st.explain} />
            {!st.explained() && st.jobOf('explain').phase !== 'working' && (
              <button type="button" className="cl-badge-btn" onClick={st.explain}><Mark size={12} /> {N_EXPLAIN} folders the rules don’t know</button>
            )}
            {st.explained() && <span className="cl-badge-btn is-done"><Mark size={12} /> Click a ✳ folder</span>}
          </>
        }
        status={(leaf) => { const e = explainedOf(leaf); return e ? <><p className="cl-tip-one"><Mark size={11} /> {e.short}</p><p className="map-tip-act">Click for Claude’s card.</p></> : null; }}
        onFolder={(leaf) => { const e = explainedOf(leaf); if (!e) return false; st.set({ card: leaf.id }); return true; }}
      >
        {s.drawer && <Drawer id={s.drawer} top={st.on ? <p className="cl-drawer-note"><Mark size={11} /> Claude’s layout is offered in the review step.</p> : undefined} />}
        {card && sub && (
          <aside className="cl-card" aria-label={`Claude on ${sub.name}`} data-no-ripple>
            <header><b><Mark size={13} /> {sub.name}</b><span>{fmtGb(sub.bytes)}</span><button type="button" className="icon-btn" aria-label="Close" onClick={() => st.set({ card: null })}><X size={14} aria-hidden /></button></header>
            <TipSection e={card} />
            {s.chats[sub.id] ? <ChatThread id={sub.id} noWhat /> : <button type="button" className="btn btn--sm" onClick={() => st.openChat(sub, false)}><MessageCircle size={12} aria-hidden /> Ask a follow-up</button>}
          </aside>
        )}
      </MapStage>
      {s.planOpen && (
        <Tray
          {...(st.on ? {
            cta: <button type="button" className="cta tray-cta" onClick={() => { st.set({ step: 'review', drawer: null, card: null }); if (st.jobOf('review').phase !== 'done') st.review(); }}><Mark size={14} /> Review with Claude</button>,
            below: <p className="cl-skip"><button type="button" className="link-button" onClick={() => st.set({ confirm: true })}>Skip the review: free {formatBytes(t.bytes)} now</button></p>,
          } : { below: <p className="cl-skip"><button type="button" className="link-button" onClick={st.openDialog}><Mark size={11} /> Turn on Claude to check the plan first</button></p> })}
        />
      )}
    </>
  );
}

function ReviewStep() {
  const st = useS();
  const { s } = st;
  const review = st.jobOf('review');
  const pickJob = st.jobOf('pick');
  const preview = s.pickOpen;
  const plan = preview ? st.pickPlan(s.plan) : s.plan;
  const changes = preview ? pickRows(s.plan) : [];
  const ghosts = changes.filter((c) => c.change === 'add' && !s.extra.includes(c.id)).map((c) => AI_ITEMS[c.id]!);
  const all = [...st.proposals, ...ghosts];
  const deletes = all.filter((p) => p.action === 'delete');
  const hints = all.filter((p) => p.action === 'hint' && (plan.includes(p.id) || changes.some((c) => c.id === p.id)));
  const t = planTotals(all, plan);
  const flags = st.flags();
  const moves = st.tidy.filter((x) => plan.includes(x.id)).reduce((n, x) => n + activeMoves(x, plan).length, 0);
  const mode = preview ? 'claude' : 'mine';
  return (
    <div className="rv" data-no-ripple>
      <header className="rv-head">
        <nav className="rv-steps" aria-label="Steps">
          <button type="button" className="rv-step" onClick={() => st.set({ step: 'map', planOpen: true })}><ArrowLeft size={13} aria-hidden /> Plan</button>
          <ChevronRight size={13} aria-hidden />
          <span className="rv-step is-here" aria-current="step"><Mark size={12} /> Check with Claude</span>
          <ChevronRight size={13} aria-hidden />
          <span className="rv-step">Confirm</span>
        </nav>
        <div className="mode-seg cl-seg" role="radiogroup" aria-label="Whose plan">
          <button type="button" role="radio" aria-checked={mode === 'mine'} className={mode === 'mine' ? 'is-on' : ''} onClick={() => { if (preview) st.discardPick(); }}>Your plan</button>
          <button type="button" role="radio" aria-checked={mode === 'claude'} className={mode === 'claude' ? 'is-on' : ''} onClick={() => { if (!preview) st.pick(); }}><Sparkles size={12} aria-hidden /> Claude’s picks</button>
        </div>
        <PrivacyChip />
      </header>
      <div className="rv-status">
        <JobLine job={review} working="Claude is checking every item…" onCancel={() => st.cancel('review')} onRetry={st.review} />
        <JobLine job={pickJob} working="Claude is building its own plan…" onCancel={() => st.cancel('pick')} onRetry={st.pick} />
        {!preview && review.phase === 'done' && <p className="rv-sum">{flags.length ? <><TriangleAlert size={13} aria-hidden /> Claude flags {flags.length}. Everything else has a note saying why it’s fine.</> : <><CircleCheck size={13} aria-hidden /> Claude flags nothing now.</>} <JobBasis k="review" /></p>}
        {preview && (
          <p className="rv-sum is-pick">
            <Sparkles size={13} aria-hidden /> Claude’s picks: {changes.filter((c) => c.change === 'untick').length} unticked, {changes.filter((c) => c.change === 'tick').length} ticked, {changes.filter((c) => c.change === 'add').length} of its own added unticked. {formatBytes(planTotals(st.proposals, s.plan).bytes)} → {formatBytes(t.bytes)}.
            <span className="rv-sum-acts"><button type="button" className="btn btn--sm" onClick={st.discardPick}>Keep mine</button><button type="button" className="cta cta--sm" onClick={st.acceptPick}>Use Claude’s picks</button></span>
          </p>
        )}
      </div>
      <div className="rv-body">
        <section className="rv-col" aria-label="Delete">
          <h3>Delete <span>{deletes.filter((p) => plan.includes(p.id)).length} of {deletes.length}</span></h3>
          {deletes.map((p) => <RvRow key={p.id} p={p} on={plan.includes(p.id)} change={changes.find((c) => c.id === p.id)} ghost={ghosts.includes(p)} />)}
          {preview && PICK_REFUSED.map((k) => (
            <div key={k} className="rv-row is-refused"><span className="rv-x"><ShieldX size={13} aria-hidden /></span><div><b>{REFUSED[k]!.title}</b> <span className="rv-size">{formatBytes(REFUSED[k]!.bytes)}</span><p className="rv-note is-refused">Claude suggested this; the Guard refused it. {REFUSED[k]!.reason}</p></div></div>
          ))}
          {hints.length > 0 && <h3 className="rv-h2">Left for you</h3>}
          {hints.map((p) => <RvRow key={p.id} p={p} on={plan.includes(p.id)} change={changes.find((c) => c.id === p.id)} ghost={false} />)}
        </section>
        <section className="rv-col rv-tidy" aria-label="Tidy up">
          <h3>Tidy up <span>pick a layout per folder</span></h3>
          {RULE_TIDY.map((x) => <RvTidy key={x.id} id={x.id} />)}
        </section>
      </div>
      <footer className="rv-foot">
        <div className="tray-total"><span className="tray-total-num">{formatBytes(t.bytes)}</span><span className="tray-total-lbl">will be freed{moves > 0 ? ` · ${formatCount(moves)} files sorted, 0 B` : ''}</span></div>
        <button type="button" className="btn" onClick={() => st.set({ step: 'map', planOpen: true })}>Back to the map</button>
        <button type="button" className="cta" disabled={preview} title={preview ? 'Use Claude’s picks or keep yours first' : undefined} onClick={() => st.set({ confirm: true })}>Continue: free {formatBytes(t.bytes)}</button>
      </footer>
    </div>
  );
}

function RvRow({ p, on, change, ghost }: { p: Proposal; on: boolean; change: ReturnType<typeof pickRows>[number] | undefined; ghost: boolean }) {
  const st = useS();
  const { s } = st;
  const review = st.jobOf('review');
  const flag = st.flags().find((f) => f.id === p.id);
  const open = s.chatOpen === p.id;
  const note = change ? (
    <p className={`rv-note is-${change.change}`}><Mark size={11} /> {change.change === 'untick' ? 'Claude unticks this' : change.change === 'tick' ? 'Claude ticks this' : 'Claude adds this, unticked'}: {change.why}</p>
  ) : s.pickOpen ? null : flag ? (
    <div className="rv-note is-flag"><p><TriangleAlert size={12} aria-hidden /> <span><b>Claude flags this.</b> {flag.reason}</span></p><div className="cl-flag-acts"><button type="button" className="link-button" onClick={() => st.togglePlan(p.id)}>Untick it</button><button type="button" className="link-button" onClick={() => st.keepFlag(p.id)}>Keep it</button></div></div>
  ) : review.phase === 'working' && on ? <p className="rv-note is-wait">Claude is checking…</p>
    : review.phase === 'done' && on && REVIEW_OK[p.id] ? <p className="rv-note is-ok"><Check size={11} strokeWidth={3} aria-hidden /> {REVIEW_OK[p.id]}</p>
    : p.source === 'ai' ? <p className="rv-note is-add"><Mark size={11} /> {p.aiNote}</p> : null;
  return (
    <div className={['rv-row', on && 'is-on', change && `is-pick-${change.change}`, flag && !s.pickOpen && 'is-flagged', ghost && 'is-ghost'].filter(Boolean).join(' ')} data-row={p.id}>
      <Checkbox on={on} onChange={() => { if (!ghost && !s.pickOpen) st.togglePlan(p.id); }} label={`Include ${p.title}`} />
      <div className="rv-main">
        <div className="rv-line"><b>{p.title}</b><span className="rv-size">{p.action === 'hint' ? 'Advice' : formatBytes(p.bytes)}</span></div>
        <div className="tray-row-meta">
          {p.action === 'delete' && <RiskChip risk={p.risk} />}
          <SourceTag source={p.source} agent={AGENT} />
          {!ghost && <button type="button" className="link-button cl-ask" onClick={() => (open ? st.set({ chatOpen: null }) : st.openChat(proposalSubject(p)))}><MessageCircle size={11} aria-hidden /> {open ? 'Hide' : 'Ask'}</button>}
        </div>
        {note}
        {open && <div className="rv-chat"><ChatThread id={p.id} /></div>}
      </div>
    </div>
  );
}

function RvTidy({ id }: { id: string }) {
  const st = useS();
  const rule = RULE_TIDY.find((x) => x.id === id)!;
  const claude = st.s.layout[id] === 'claude';
  const on = st.s.plan.includes(id);
  const tidyJob = st.jobOf('tidy');
  const have = tidyJob.phase === 'done';
  return (
    <div className={on ? 'rv-tidy-f is-on' : 'rv-tidy-f'}>
      <div className="rv-line">
        <Checkbox on={on} onChange={() => st.togglePlan(id)} label={`Include Sort ${rule.name}`} />
        <b>Sort {rule.name}</b><span className="rv-size">{formatCount(rule.loose)} loose</span>
        <button type="button" className="link-button" onClick={() => st.set({ step: 'map', planOpen: true, drawer: id })}>Drawer</button>
      </div>
      <div className="rv-cmp" role="radiogroup" aria-label={`Layout for ${rule.name}`}>
        <button type="button" role="radio" aria-checked={!claude} className={!claude ? 'rv-lay is-on' : 'rv-lay'} onClick={() => st.setLayout(id, 'rule')}>
          <span className="rv-lay-h">{!claude ? <CircleCheck size={12} aria-hidden /> : <span className="cp-radio" />} Built-in rules</span>
          <div className="rv-lay-tree"><Tree node={afterTree(rule, st.s.plan)} /></div>
        </button>
        <button type="button" role="radio" aria-checked={claude} className={claude ? 'rv-lay is-on is-ai' : 'rv-lay is-ai'} onClick={() => st.setLayout(id, 'claude')}>
          <span className="rv-lay-h">{claude ? <CircleCheck size={12} aria-hidden /> : <span className="cp-radio" />} <Mark size={11} /> Claude’s layout</span>
          {have ? <div className="rv-lay-tree"><Tree node={afterTree(claudeTidy(rule), st.s.plan)} /></div> : <span className="rv-lay-ask">{tidyJob.phase === 'working' ? 'Claude is sorting the names…' : 'Ask Claude for a layout'}</span>}
        </button>
      </div>
      {claude && <p className="rv-note is-add"><Mark size={11} /> {TIDY_NOTES[id]}</p>}
    </div>
  );
}

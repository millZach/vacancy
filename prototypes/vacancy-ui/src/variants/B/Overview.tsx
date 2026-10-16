// PROTOTYPE overview step: where the space went, then findings arriving as message cards with Keep / Clean.
import { useState, type ReactNode } from 'react';
import { ChevronDown, Lock, SquareTerminal } from 'lucide-react';
import { formatBytes, formatCount, type Dataset, type Proposal } from '../../shared/data';
import { Arrive, Choice, Say } from './Feed';
import { RISK_LABEL, WHY_LABEL, catColor, formatDuration } from './util';

export function Overview({ data, proposals, setProposals, fresh: freshProp, showNext, onNext }: {
  data: Dataset;
  proposals: Set<string>;
  setProposals: (s: Set<string>) => void;
  fresh: boolean;
  showNext: boolean;
  onNext: () => void;
}) {
  const [open, setOpen] = useState<Set<string>>(new Set());
  // Animate only if the overview opened as the current step, and never again on later re-renders.
  const [fresh] = useState(freshProp);
  const agent = data.scan.aiAgent;
  const deletable = data.proposals.filter((p) => p.action === 'delete');
  const safe = deletable.filter((p) => p.source === 'rule' && p.risk === 'safe');
  const review = deletable.filter((p) => p.source === 'rule' && p.risk !== 'safe');
  const ai = data.proposals.filter((p) => p.source === 'ai');
  const hints = data.proposals.filter((p) => p.action === 'hint' && p.source !== 'ai');
  const total = deletable.reduce((s, p) => s + p.bytes, 0);
  const suggested = deletable.filter((p) => p.defaultOn).reduce((s, p) => s + p.bytes, 0);

  const toggle = (id: string, on: boolean) => {
    const next = new Set(proposals);
    if (on) next.add(id);
    else next.delete(id);
    setProposals(next);
  };
  const flip = (id: string) => {
    const next = new Set(open);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setOpen(next);
  };

  // Findings arrive one after another the first time the overview opens.
  let n = 0;
  const delay = () => 0.55 + n++ * 0.11;
  const card = (p: Proposal) => (
    <Arrive key={p.id} animate={fresh} delay={delay()}>
      <FindingCard data={data} p={p} on={proposals.has(p.id)} open={open.has(p.id)} onToggle={(v) => toggle(p.id, v)} onOpen={() => flip(p.id)} />
    </Arrive>
  );
  const say = (key: string, text: ReactNode, avatar = false) => (
    <Say key={key} avatar={avatar} animate={fresh} delay={delay()}>
      <p className="vb-voice">{text}</p>
    </Say>
  );

  return (
    <section className="vb-section" data-vb-anchor="overview">
      <Say avatar animate={fresh}>
        <h2 className="vb-display">Here's where your {formatBytes(data.disk.usedBytes, 0)} went.</h2>
        <p className="vb-meta">
          {formatCount(data.scan.files)} files in {formatCount(data.scan.folders)} folders, scanned in {formatDuration(data.scan.durationSec)}.
        </p>
      </Say>
      <Arrive animate={fresh} delay={0.15}>
        <SpaceCard data={data} />
      </Arrive>

      {say('intro', <>I found <b>{formatBytes(total)}</b> I could clean, and set the {formatBytes(suggested)} I'd recommend to Clean. Switch anything to Keep and I'll leave it alone.</>, true)}
      {safe.length > 0 && say('safe', 'These rebuild themselves, or were meant to be thrown away.')}
      {safe.map(card)}
      {review.length > 0 && say('review', 'These are probably fine, but have a look first.', true)}
      {review.map(card)}
      {ai.length > 0 && say('ai', <>I didn't recognize {ai.length === 1 ? 'one big folder' : `${ai.length} big folders`}, so I asked {agent ?? 'the AI helper'} about {ai.length === 1 ? 'it' : 'them'}.</>, true)}
      {ai.map(card)}
      {hints.length > 0 && say('hint', <>And {hints.length === 1 ? 'one thing' : 'a few things'} I can't do for you, but you can.</>, true)}
      {hints.map(card)}

      {showNext && data.reorgs.length > 0 && (
        <Say avatar animate={fresh} delay={delay()}>
          <p className="vb-voice">
            That's everything I'd clean. {agent ? `${agent} also has` : 'I also have'} {data.reorgs.length === 1 ? 'an idea' : `${data.reorgs.length} ideas`} for tidying your folders.
          </p>
          <button type="button" className="vb-secondary" onClick={onNext}>Show me the folder ideas</button>
        </Say>
      )}
    </section>
  );
}

function SpaceCard({ data }: { data: Dataset }) {
  const { totalBytes, usedBytes } = data.disk;
  const free = totalBytes - usedBytes;
  const cleanable = (id: string) =>
    data.proposals.filter((p) => p.categoryId === id && p.action === 'delete').reduce((s, p) => s + p.bytes, 0);
  const cats = [...data.categories].sort((a, b) => b.bytes - a.bytes);
  return (
    <div className="vb-card vb-space">
      <div className="vb-stack" role="img" aria-label="Disk usage by category">
        {cats.map((c) => (
          <i
            key={c.id}
            className={c.protected ? 'vb-stack-locked' : ''}
            style={{ flexGrow: c.bytes, backgroundColor: catColor(c.hue, c.protected) }}
            title={`${c.name}: ${formatBytes(c.bytes)}`}
          />
        ))}
        <i className="vb-stack-free" style={{ flexGrow: free }} title={`Free: ${formatBytes(free)}`} />
      </div>
      <div className="vb-stack-scale">
        <span>{formatBytes(usedBytes, 0)} used</span>
        <span>{formatBytes(free)} free</span>
      </div>
      <ul className="vb-cats">
        {cats.map((c) => {
          const can = cleanable(c.id);
          return (
            <li key={c.id} className={c.protected ? 'vb-cat-locked' : ''}>
              <span className={`vb-swatch ${c.protected ? 'vb-stack-locked' : ''}`} style={{ backgroundColor: catColor(c.hue, c.protected) }} />
              <span className="vb-cat-name">
                {c.name}
                {c.protected && <Lock size={12} strokeWidth={2} aria-label="Protected" />}
              </span>
              <span className="vb-cat-blurb">{c.blurb}</span>
              <span className="vb-cat-can">
                {c.protected ? 'Never touched' : can > 0 ? `${formatBytes(can)} cleanable` : ''}
              </span>
              <span className="vb-cat-size">{formatBytes(c.bytes, 0)}</span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export function FindingCard({ data, p, on, open, onToggle, onOpen }: {
  data: Dataset; p: Proposal; on: boolean; open: boolean; onToggle: (v: boolean) => void; onOpen: () => void;
}) {
  const cat = data.categories.find((c) => c.id === p.categoryId);
  const hint = p.action === 'hint';
  const agent = data.scan.aiAgent ?? 'AI helper';
  return (
    <article className={`vb-card vb-find ${on ? 'vb-find-on' : 'vb-find-off'} ${hint ? 'vb-find-hint' : ''}`}>
      <div className="vb-find-meta">
        <span className={`vb-risk vb-risk-${p.risk}`}><i aria-hidden />{RISK_LABEL[p.risk]}</span>
        {p.source === 'ai' && (
          <span className="vb-agent-chip"><SquareTerminal size={12} strokeWidth={2} aria-hidden />Suggested by {agent}</span>
        )}
        {p.regenerable && !hint && <span className="vb-tag">Comes back on its own</span>}
        {hint && <span className="vb-tag">You do this one</span>}
        <span className="vb-find-cat">{cat?.name}</span>
      </div>
      <div className="vb-find-main">
        <div className="vb-find-text">
          <h3>{p.title}</h3>
          <p>{p.summary}</p>
        </div>
        <div className="vb-find-size">
          <span>{formatBytes(p.bytes)}</span>
          <small>{hint ? 'not counted' : `${formatCount(p.itemCount)} ${p.itemCount === 1 ? 'item' : 'items'}`}</small>
        </div>
      </div>
      {p.aiNote && (
        <blockquote className="vb-aside">
          <cite>{agent} says</cite>
          <span>{p.aiNote}</span>
        </blockquote>
      )}
      <p className="vb-why"><b>{WHY_LABEL[p.risk]}.</b> {p.why}</p>
      <div className="vb-find-foot">
        <button type="button" className="vb-link" aria-expanded={open} onClick={onOpen}>
          <ChevronDown size={14} strokeWidth={2} className={open ? 'vb-rot' : ''} aria-hidden />
          {open ? 'Hide' : 'Show'} {p.samplePaths.length === 1 ? 'the path' : `${p.samplePaths.length} sample paths`}
        </button>
        {hint ? (
          <Choice label={`${p.title}: add to your list?`} on={on} off="Not now" onLabel="Add to my list" onChange={onToggle} />
        ) : (
          <Choice label={`${p.title}: keep or clean?`} on={on} off="Keep" onLabel="Clean" onChange={onToggle} />
        )}
      </div>
      {open && (
        <ul className="vb-paths">
          {p.samplePaths.map((s) => <li key={s}>{s}</li>)}
        </ul>
      )}
    </article>
  );
}

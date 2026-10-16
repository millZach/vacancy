// PROTOTYPE plan step: folder reorganizations (before and after trees, sample moves) and the inline confirm card.
import { useState } from 'react';
import { ArrowRight, File, Folder, FolderOpen, SquareTerminal } from 'lucide-react';
import { formatBytes, formatCount, type Dataset, type ReorgProposal, type TreeNode } from '../../shared/data';
import { Arrive, Choice, HoldButton, Say } from './Feed';
import { binName, joinWords, plural, splitMove, type PlanTotals } from './util';

export function PlanSection({ data, reorgs, setReorgs, fresh: freshProp }: {
  data: Dataset; reorgs: Set<string>; setReorgs: (s: Set<string>) => void; fresh: boolean;
}) {
  const [fresh] = useState(freshProp);
  const agent = data.scan.aiAgent;
  const toggle = (id: string, on: boolean) => {
    const next = new Set(reorgs);
    if (on) next.add(id);
    else next.delete(id);
    setReorgs(next);
  };
  return (
    <section className="vb-section" data-vb-anchor="plan">
      <Say avatar animate={fresh}>
        <h2 className="vb-display vb-display-sm">Now, your folders.</h2>
        <p className="vb-voice">
          {agent ? `${agent} looked at` : 'I looked at'} how your files are laid out and suggested {data.reorgs.length === 1 ? 'one tidy-up' : `${data.reorgs.length} tidy-ups`}.
          Nothing here is deleted. Every move is written to a journal, so you can undo it from your receipt.
        </p>
      </Say>
      {data.reorgs.map((r, i) => (
        <Arrive key={r.id} animate={fresh} delay={0.25 + i * 0.18}>
          <ReorgCard data={data} r={r} on={reorgs.has(r.id)} onToggle={(v) => toggle(r.id, v)} />
        </Arrive>
      ))}
      <Say animate={fresh} delay={0.25 + data.reorgs.length * 0.18}>
        <p className="vb-voice">When the plan below looks right, choose Review & run. I'll ask once before deleting anything.</p>
      </Say>
    </section>
  );
}

function ReorgCard({ data, r, on, onToggle }: { data: Dataset; r: ReorgProposal; on: boolean; onToggle: (v: boolean) => void }) {
  const agent = data.scan.aiAgent;
  return (
    <article className={`vb-card vb-reorg ${on ? 'vb-find-on' : 'vb-find-off'}`}>
      <div className="vb-find-meta">
        {agent && <span className="vb-agent-chip"><SquareTerminal size={12} strokeWidth={2} aria-hidden />Suggested by {agent}</span>}
        <span className="vb-tag">Undo any time</span>
        <span className="vb-find-cat vb-mono">{r.path}</span>
      </div>
      <div className="vb-find-main">
        <div className="vb-find-text">
          <h3>{r.title}</h3>
          <p>{r.summary}</p>
        </div>
        <div className="vb-find-size">
          <span>{formatCount(r.moveCount)}</span>
          <small>of {formatCount(r.itemCount)} files move</small>
        </div>
      </div>
      <blockquote className="vb-aside">
        <cite>{agent ?? 'Vacancy'} says</cite>
        <span>{r.aiNote}</span>
      </blockquote>
      <div className="vb-trees">
        <div className="vb-tree-col">
          <div className="vb-tree-cap">Now</div>
          <Tree node={r.before} root />
        </div>
        <span className="vb-tree-arrow" aria-hidden><ArrowRight size={16} strokeWidth={1.75} /></span>
        <div className="vb-tree-col vb-tree-after">
          <div className="vb-tree-cap">After</div>
          <Tree node={r.after} root />
        </div>
      </div>
      <div className="vb-moves">
        <div className="vb-moves-cap">Sample moves</div>
        <ul>
          {r.sampleMoves.map((m) => {
            const [pre, mid, file] = splitMove(m.from, m.to);
            return (
              <li key={m.from}>
                <span className="vb-move-file">{file}</span>
                <span className="vb-move-to">
                  <ArrowRight size={12} strokeWidth={2} aria-hidden />
                  <span className="vb-move-pre">{pre}</span>
                  <span className="vb-move-mid">{mid}</span>
                </span>
              </li>
            );
          })}
        </ul>
      </div>
      <div className="vb-find-foot">
        <span className="vb-foot-note">{on ? `${plural(r.moveCount, 'file')} will move inside ${r.folder}.` : `${r.folder} stays as it is.`}</span>
        <Choice label={`${r.title}: reorganize?`} on={on} off="Leave as is" onLabel="Reorganize" onChange={onToggle} />
      </div>
    </article>
  );
}

function isNote(n: TreeNode) {
  return n.name.startsWith('...') || n.name.startsWith('(') || /\(unchanged\)/.test(n.name);
}

function Tree({ node, root, depth = 0 }: { node: TreeNode; root?: boolean; depth?: number }) {
  const folder = !!node.children || (node.count !== undefined && !isNote(node));
  const note = isNote(node);
  return (
    <ul className={root ? 'vb-tree' : 'vb-tree-sub'}>
      <li>
        <div className={`vb-tree-row ${note ? 'vb-tree-note' : ''} ${root ? 'vb-tree-root' : ''}`} style={{ paddingLeft: depth * 14 }}>
          {!note && (root ? <FolderOpen size={13} strokeWidth={1.75} aria-hidden /> : folder ? <Folder size={13} strokeWidth={1.75} aria-hidden /> : <File size={13} strokeWidth={1.5} aria-hidden />)}
          <span className="vb-tree-name">{note ? node.name.replace(/^\.\.\./, '…') : node.name}</span>
          {node.count !== undefined && !note && <span className="vb-tree-count">{formatCount(node.count)}</span>}
        </div>
        {node.children?.map((c) => <Tree key={c.name} node={c} depth={depth + 1} />)}
      </li>
    </ul>
  );
}

export function ConfirmCard({ data, totals, onRun, onCancel }: {
  data: Dataset; totals: PlanTotals; onRun: () => void; onCancel: () => void;
}) {
  const hasDelete = totals.bytes > 0;
  const hasMoves = totals.moves > 0;
  const nothing = !hasDelete && !hasMoves;
  const runLabel = hasDelete ? `Hold to delete ${formatBytes(totals.bytes)}` : `Hold to move ${plural(totals.moves, 'file')}`;
  return (
    <section className="vb-section" data-vb-anchor="confirm">
      <Say avatar animate>
        <p className="vb-voice">Before I start, here's exactly what happens.</p>
      </Say>
      <Arrive animate delay={0.12}>
        <article className="vb-card vb-confirm">
          {nothing ? (
            <p className="vb-confirm-big">There's nothing selected yet. Switch something to Clean or Reorganize first.</p>
          ) : (
            <>
              {hasDelete ? (
                <p className="vb-confirm-big">
                  This permanently deletes <b>{formatBytes(totals.bytes)}</b> ({plural(totals.items, 'item')} across {plural(totals.cleanups.length, 'cleanup')}).
                  They don't go to the {binName(data.os)}, so they can't be restored.
                </p>
              ) : (
                <p className="vb-confirm-big">Nothing is deleted this time.</p>
              )}
              <ul className="vb-confirm-list">
                {hasMoves ? (
                  <li>
                    <b>Folder moves can be undone.</b> {plural(totals.moves, 'file')} in {joinWords(totals.reorgFolders)} move, and each move is journaled so your receipt can put it back.
                  </li>
                ) : (
                  <li>No folders are reorganized this time.</li>
                )}
                {totals.hints.length > 0 && (
                  <li>
                    <b>Left for you:</b> {joinWords(totals.hints.map((h) => h.title))}. I can't do {totals.hints.length === 1 ? 'that one' : 'those'}, so {totals.hints.length === 1 ? "it isn't" : "they aren't"} counted.
                  </li>
                )}
                <li>Anything in use when I get to it is skipped and listed on your receipt.</li>
              </ul>
              <div className="vb-confirm-act">
                <HoldButton label={runLabel} holdingLabel="Keep holding…" onDone={onRun} />
                <button type="button" className="vb-link" onClick={onCancel}>Change the plan</button>
              </div>
            </>
          )}
        </article>
      </Arrive>
    </section>
  );
}

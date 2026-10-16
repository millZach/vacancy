// PROTOTYPE variant C: two-pane before/after tree diff for a folder reorganization, with sample moves.
import { CornerDownRight, File, Folder, FolderPlus, SquareTerminal } from 'lucide-react';
import type { Dataset, Move, TreeNode } from '../../shared/data';
import { Tick } from './bits';
import { agentName, n } from './model';

const isFolder = (t: TreeNode) => !!t.children || (t.count != null && !t.name.startsWith('...') && !t.name.startsWith('('));
const isNote = (t: TreeNode) => t.name.startsWith('...') || t.name.startsWith('(');

function Tree({ node, side, depth = 0 }: { node: TreeNode; side: 'before' | 'after'; depth?: number }) {
  const folder = isFolder(node);
  const note = isNote(node);
  const fresh = side === 'after' && depth > 0 && folder && !note && !node.name.includes('unchanged');
  return (
    <>
      <div className={`vc-tree-row ${note ? 'note' : ''} ${fresh ? 'fresh' : ''} ${depth === 0 ? 'root' : ''}`} style={{ paddingLeft: 12 + depth * 18 }}>
        {Array.from({ length: depth }, (_, i) => (
          <i key={i} className="vc-tree-line" style={{ left: 19 + i * 18 }} />
        ))}
        <span className="vc-tree-ic">{note ? null : fresh ? <FolderPlus size={14} /> : folder ? <Folder size={14} /> : <File size={13} />}</span>
        <span className="vc-tree-name">{node.name}</span>
        {node.count != null && <em className="num">{n(node.count)}</em>}
      </div>
      {node.children?.map((c) => <Tree key={c.name} node={c} side={side} depth={depth + 1} />)}
    </>
  );
}

function splitPath(s: string) {
  return s.split(/([\\/])/);
}

function MoveLine({ m }: { m: Move }) {
  const a = splitPath(m.from);
  const b = splitPath(m.to);
  let k = 0;
  while (k < a.length && k < b.length && a[k] === b[k]) k++;
  // Keep the shared prefix muted, light up only what changes.
  const file = b[b.length - 1];
  const middle = b.slice(k, b.length - 1).join('');
  return (
    <li className="vc-move">
      <span className="vc-move-from" title={m.from}>
        {m.from}
      </span>
      <span className="vc-move-to" title={m.to}>
        <CornerDownRight size={13} />
        <span className="vc-mute">{b.slice(0, k).join('')}</span>
        <b>{middle}</b>
        <span>{file}</span>
      </span>
    </li>
  );
}

export function ReorgView({
  data,
  focusId,
  setFocusId,
  reorgs,
  toggle,
}: {
  data: Dataset;
  focusId: string;
  setFocusId: (id: string) => void;
  reorgs: Set<string>;
  toggle: (id: string) => void;
}) {
  const r = data.reorgs.find((x) => x.id === focusId) ?? data.reorgs[0];
  if (!r) return <section className="vc-reorg"><div className="vc-empty">{agentName(data)} had no folders to sort.</div></section>;
  const idx = data.reorgs.indexOf(r);
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown' || e.key === 'j') {
      e.preventDefault();
      setFocusId(data.reorgs[Math.min(data.reorgs.length - 1, idx + 1)].id);
    } else if (e.key === 'ArrowUp' || e.key === 'k') {
      e.preventDefault();
      setFocusId(data.reorgs[Math.max(0, idx - 1)].id);
    } else if (e.key === ' ') {
      e.preventDefault();
      toggle(r.id);
    }
  };
  return (
    <section className="vc-reorg" aria-label="Reorganize folders">
      <div className="vc-reorg-list" tabIndex={0} onKeyDown={onKey} role="listbox" aria-activedescendant={`vc-ro-${r.id}`} autoFocus>
        {data.reorgs.map((x) => {
          const on = reorgs.has(x.id);
          return (
            <div
              key={x.id}
              id={`vc-ro-${x.id}`}
              role="option"
              aria-selected={x.id === r.id}
              className={`vc-reorg-tab ${x.id === r.id ? 'focus' : ''} ${on ? 'on' : ''}`}
              onMouseDown={() => setFocusId(x.id)}
            >
              <Tick on={on} onClick={() => toggle(x.id)} label={`Select ${x.title}`} />
              <div>
                <b>{x.title}</b>
                <small>
                  {x.path}, {n(x.moveCount)} moves
                </small>
              </div>
            </div>
          );
        })}
      </div>
      <div className="vc-diff">
        <div className="vc-diff-pane">
          <h3>
            Before <em>{n(r.itemCount)} items, mostly loose</em>
          </h3>
          <div className="vc-tree">
            <Tree node={r.before} side="before" />
          </div>
        </div>
        <div className="vc-diff-pane after">
          <h3>
            After <em>{n(r.moveCount)} moved into new folders</em>
          </h3>
          <div className="vc-tree">
            <Tree node={r.after} side="after" />
          </div>
        </div>
      </div>
      <div className="vc-reorg-foot">
        <figure className="vc-ai-note">
          <figcaption>
            <SquareTerminal size={13} strokeWidth={2.2} /> Suggested by {agentName(data)}
          </figcaption>
          <blockquote>{r.aiNote}</blockquote>
        </figure>
        <div className="vc-moves">
          <h3>Sample moves</h3>
          <ul>
            {r.sampleMoves.map((m) => (
              <MoveLine key={m.from} m={m} />
            ))}
          </ul>
        </div>
      </div>
    </section>
  );
}

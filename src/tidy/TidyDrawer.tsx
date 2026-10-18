import { ArrowRight, CornerDownRight, File, Folder, FolderTree, Trash2, TriangleAlert, Undo2, X } from 'lucide-react';
import { useEffect } from 'react';
import type { Proposal, TidyProposal } from '../bridge';
import { formatCount } from '../format';
import {
  activeMoves,
  afterTree,
  beforeTree,
  foldersMade,
  goingInPlan,
  listOf,
  sampleMoves,
  staysByReason,
  type TreeNode,
} from './model';
import './tidy.css';

interface Props {
  tidy: TidyProposal;
  /** The ids in the drive's plan. */
  plan: string[];
  /** The drive's proposals, to name the delete proposals that take some of the folder's files. */
  proposals: Proposal[];
  onToggle: () => void;
  onClose: () => void;
}

/** How many files a drawer lists by name in one "stays" line. */
const NAMES_SHOWN = 3;

/**
 * A Tidy up proposal's drawer, over the map beside the tray: the folder before and after as two trees, a few
 * sample moves (with any numbered rename), what stays where it is and why (including files this plan's deletes
 * take instead), and what the sort promises. Its foot says how many moves, how many folders it makes, that it frees
 * 0 B, and puts it in the plan or takes it out. Escape closes it.
 */
export function TidyDrawer({ tidy: t, plan, proposals, onToggle, onClose }: Props) {
  const on = plan.includes(t.id);
  const moves = activeMoves(t, plan);
  const going = goingInPlan(t, plan);
  const made = foldersMade(t, moves).length;
  const samples = sampleMoves(moves);
  const stays = staysByReason(t.stays);
  const deleters = [...new Set(going.map((m) => m.deletedBy))]
    .map((id) => proposals.find((p) => p.id === id)?.title ?? id)
    .filter((x): x is string => !!x);

  // Escape closes the drawer before the map hears it (it would zoom out).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      onClose();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  return (
    <section className="reorg-drawer" role="dialog" aria-label={`Sort ${t.name}`} data-no-ripple>
      <header className="rd-head">
        <div>
          <h2>Sort {t.name}</h2>
          <p className="rd-path">{t.path}</p>
        </div>
        <button type="button" className="icon-btn" onClick={onClose} aria-label="Close">
          <X size={16} aria-hidden />
        </button>
      </header>
      <div className="rd-body">
        <p className="rd-rule">
          <FolderTree size={14} aria-hidden /> Built-in rule: sorted by kind and year.
        </p>
        <div className="trees">
          <section aria-label="Before">
            <h3>Before</h3>
            <Tree node={beforeTree(t)} />
          </section>
          <ArrowRight size={16} className="trees-arrow" aria-hidden />
          <section aria-label="After">
            <h3>After</h3>
            <Tree node={afterTree(t, plan)} />
          </section>
        </div>
        {t.later > 0 && (
          <p className="rd-later">
            Vacancy sorts up to {formatCount(t.loose)} files of a folder in one run. {formatCount(t.later)} more loose{' '}
            {t.later === 1 ? 'file waits' : 'files wait'} for a later run.
          </p>
        )}
        {samples.length > 0 && (
          <>
            <h3 className="rd-h">Sample moves</h3>
            <ul className="sample-moves" aria-label="Sample moves">
              {samples.map((m) => (
                <li key={`${m.name}:${m.dest}`} className={m.renamed ? 'is-renamed' : undefined}>
                  <span className="sm-from">{m.name}</span>
                  <ArrowRight size={12} aria-hidden />
                  <span className="sm-to">
                    {m.dest}\{m.to}
                  </span>
                  {m.renamed && <em>A file there already has the name, so this one gets a number. Nothing is overwritten.</em>}
                </li>
              ))}
            </ul>
          </>
        )}
        {(stays.length > 0 || going.length > 0) && (
          <>
            <h3 className="rd-h">Stays where it is</h3>
            <ul className="stays" aria-label="Stays where it is">
              {going.length > 0 && (
                <li>
                  <Trash2 size={13} aria-hidden />
                  <span>
                    <b>{namesOf(going.map((m) => m.name))}</b> {going.length === 1 ? 'is' : 'are'} going in this plan:{' '}
                    {listOf(deleters)} {going.length === 1 ? 'deletes it' : 'deletes them'}, so {going.length === 1 ? 'it isn’t' : 'they aren’t'} sorted.
                  </span>
                </li>
              )}
              {stays.map((g) => (
                <li key={`${g.dropped}:${g.reason}`}>
                  {g.dropped ? <TriangleAlert size={13} aria-hidden /> : <CornerDownRight size={13} aria-hidden />}
                  <span>
                    <b>{namesOf(g.names)}</b> {g.reason}
                  </span>
                </li>
              ))}
            </ul>
          </>
        )}
        <ul className="facts">
          <li>Built-in rule: sorted by kind and year. No AI, offline.</li>
          <li>Only loose files at the top of {t.name}. Subfolders stay as they are.</li>
          <li>Moves only. Nothing is deleted, so this frees 0 B.</li>
          <li>Stays on this drive. Files in your Delete list aren’t moved.</li>
          <li>A file that’s open or changed when its turn comes is skipped and reported.</li>
          <li className="is-undo">
            <Undo2 size={13} aria-hidden /> Undo any time, from the recap or Past runs.
          </li>
        </ul>
      </div>
      <footer className="rd-foot">
        <p>
          {formatCount(moves.length)} {moves.length === 1 ? 'move' : 'moves'} · makes {formatCount(made)}{' '}
          {made === 1 ? 'folder' : 'folders'} · frees 0 B
        </p>
        <button type="button" className={on ? 'btn' : 'cta'} onClick={onToggle} disabled={!on && moves.length === 0}>
          {on ? 'Remove from plan' : 'Add to plan'}
        </button>
      </footer>
    </section>
  );
}

/** "a.pdf", "a.pdf and b.pdf", "a.pdf, b.pdf, c.pdf and 11 more". */
function namesOf(names: readonly string[]): string {
  if (names.length <= NAMES_SHOWN) return listOf(names);
  return `${names.slice(0, NAMES_SHOWN).join(', ')} and ${formatCount(names.length - NAMES_SHOWN)} more`;
}

/** A before or after tree: folders with their counts, new ones tagged, the user's own marked "yours". */
function Tree({ node, depth = 0 }: { node: TreeNode; depth?: number }) {
  return (
    <div className="tree-node">
      <div className={`tree-row${node.tag ? ` is-${node.tag}` : ''}`} style={{ paddingLeft: depth * 15 }}>
        {node.file ? <File size={13} strokeWidth={1.8} aria-hidden /> : <Folder size={13} strokeWidth={1.8} aria-hidden />}
        <span className="tree-name">{node.name}</span>
        {node.tag === 'new' && depth > 0 && <span className="tree-tag">new</span>}
        {node.tag === 'existing' && <span className="tree-tag is-existing">yours</span>}
        {node.count !== undefined && <span className="tree-count">{formatCount(node.count)}</span>}
      </div>
      {node.children?.map((c) => <Tree key={c.name} node={c} depth={depth + 1} />)}
    </div>
  );
}

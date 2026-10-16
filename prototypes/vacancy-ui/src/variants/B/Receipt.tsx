// PROTOTYPE summary step: a printed checkout receipt with the VACANCY stamp. The payoff screen.
import { useMemo, useState } from 'react';
import { motion, useReducedMotion } from 'motion/react';
import { formatBytes, formatCount, type Dataset } from '../../shared/data';
import type { RunResult } from '../../shared/run';
import { formatDuration, receiptDate } from './util';

export type UndoState = Record<string, 'undoing' | 'undone'>;

export function Receipt({ data, result, when, undo, onUndo }: {
  data: Dataset; result: RunResult; when: Date; undo: UndoState; onUndo: (reorgId: string) => void;
}) {
  const reduced = useReducedMotion();
  const d = receiptDate(when);
  const free0 = result.totalBytes - result.usedBefore;
  const free1 = result.totalBytes - result.usedAfter;
  const groups = useMemo(() => groupSteps(data, result), [data, result]);
  const maxCat = Math.max(1, ...groups.map((g) => g.freed));
  const hints = data.proposals.filter((p) => p.action === 'hint');
  const big = result.biggestWin;

  const paper = (
    <div className="vb-receipt">
      <svg className="vb-defs" aria-hidden>
        <filter id="vb-stamp-rough" x="-5%" y="-5%" width="110%" height="110%" colorInterpolationFilters="sRGB">
          <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves="2" seed="3" result="noise" />
          <feColorMatrix in="noise" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  -3.4 0 0 0 2.65" result="speck" />
          <feComposite in="SourceGraphic" in2="speck" operator="in" result="inked" />
          <feDisplacementMap in="inked" in2="noise" scale="2.5" xChannelSelector="R" yChannelSelector="G" />
        </filter>
        <filter id="vb-grain" x="0" y="0" width="100%" height="100%" colorInterpolationFilters="sRGB">
          <feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" stitchTiles="stitch" />
          <feColorMatrix type="matrix" values="0 0 0 0 0.42  0 0 0 0 0.36  0 0 0 0 0.3  0.22 0 0 0 -0.04" />
        </filter>
      </svg>
      <svg className="vb-r-grain" aria-hidden><rect width="100%" height="100%" filter="url(#vb-grain)" /></svg>
      <Letterhead />
      <div className="vb-r-center vb-r-title">Disk cleanup receipt</div>
      <div className="vb-r-center vb-r-meta">
        <div>{data.machineName}</div>
        <div>{data.disk.name}</div>
        <div>{d.date}, {d.time}</div>
      </div>

      <Rule />
      <div className="vb-r-hero">
        <div className="vb-r-label">Space freed</div>
        <div className="vb-r-big">{formatBytes(result.freedBytes)}</div>
        <div className="vb-r-small">
          of {formatBytes(result.plannedBytes)} planned
          {result.plannedBytes - result.freedBytes > 1e6 ? `, ${formatBytes(result.plannedBytes - result.freedBytes)} skipped` : ''}
        </div>
        <Stamp date={d.stamp} reduced={!!reduced} />
      </div>

      <Rule />
      <div className="vb-r-disk">
        <div className="vb-r-grid3 vb-r-head"><span /><span>Before</span><span>After</span></div>
        <div className="vb-r-grid3"><span>Used</span><span>{formatBytes(result.usedBefore)}</span><span>{formatBytes(result.usedAfter)}</span></div>
        <div className="vb-r-grid3"><span>Free</span><span>{formatBytes(free0)}</span><span className="vb-r-strong">{formatBytes(free1)}</span></div>
        <div className="vb-r-bars">
          <DiskBar label="Before" used={result.usedBefore} total={result.totalBytes} />
          <DiskBar label="After" used={result.usedAfter} total={result.totalBytes} />
        </div>
      </div>

      <Rule />
      <div className="vb-r-label">Freed by category</div>
      <ul className="vb-r-cats">
        {[...groups].sort((a, b) => b.freed - a.freed).filter((g) => g.freed > 0).map((g) => (
          <li key={g.id}>
            <span className="vb-r-cat-name">{g.name}</span>
            <span className="vb-r-cat-bar"><i style={{ width: `${(g.freed / maxCat) * 100}%` }} /></span>
            <span className="vb-r-num">{formatBytes(g.freed)}</span>
          </li>
        ))}
      </ul>
      {big && (
        <div className="vb-r-win">
          <span className="vb-r-label">Biggest win</span>
          <div className="vb-r-row"><span>★ {big.title}</span><span /><span className="vb-r-num">{formatBytes(big.freedBytes)}</span></div>
        </div>
      )}

      <Rule double />
      <div className="vb-r-label">Itemized</div>
      {groups.filter((g) => g.steps.length).map((g) => (
        <div key={g.id} className="vb-r-group">
          <div className="vb-r-group-name">{g.name}</div>
          {g.steps.map((s) => (
            <div key={s.id} className="vb-r-item">
              <div className="vb-r-row">
                <span>{big?.id === s.id ? '★ ' : ''}{s.title}</span>
                <span className="vb-r-count">{formatCount(s.items)}</span>
                <span className="vb-r-num">{formatBytes(s.freedBytes)}</span>
              </div>
              {s.skipped.length > 0 && <div className="vb-r-note">partly skipped, see below</div>}
            </div>
          ))}
          <div className="vb-r-row vb-r-sub"><span>Subtotal</span><span /><span className="vb-r-num">{formatBytes(g.freed)}</span></div>
        </div>
      ))}

      {result.undoable.length > 0 && (
        <div className="vb-r-group">
          <div className="vb-r-group-name">Folders tidied (journaled)</div>
          {result.undoable.map((u) => {
            const r = data.reorgs.find((x) => x.id === u.reorgId);
            const st = undo[u.reorgId];
            return (
              <div key={u.reorgId} className="vb-r-item vb-r-undo-row">
                <div className="vb-r-row">
                  <span>{r?.title ?? u.folder}</span>
                  <span className="vb-r-count">{formatCount(u.moves)}</span>
                  <span className="vb-r-num">moved</span>
                </div>
                <div className="vb-r-undo-line">
                  <span className="vb-r-note">{st === 'undone' ? `Put back: ${u.folder} is as it was.` : `${u.folder}, undo any time`}</span>
                  <button type="button" className="vb-r-undo" disabled={!!st} onClick={() => onUndo(u.reorgId)}>
                    {st === 'undone' ? 'Undone' : st === 'undoing' ? 'Undoing…' : `Undo ${u.folder}`}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <Rule double />
      <div className="vb-r-total">
        <div className="vb-r-row vb-r-total-line"><span>Total freed</span><span /><span className="vb-r-num">{formatBytes(result.freedBytes)}</span></div>
        <div className="vb-r-row"><span>Items deleted</span><span /><span className="vb-r-num">{formatCount(result.deletedItems)}</span></div>
        <div className="vb-r-row"><span>Files moved</span><span /><span className="vb-r-num">{formatCount(result.movedItems)}</span></div>
        <div className="vb-r-row"><span>Time taken</span><span /><span className="vb-r-num">{formatDuration(result.durationSec)}</span></div>
      </div>

      <Rule />
      <div className="vb-r-label">Skipped, left in place</div>
      {result.skipped.length === 0 ? (
        <div className="vb-r-note">Nothing was skipped.</div>
      ) : (
        result.skipped.map((k) => (
          <div key={k.path} className="vb-r-item">
            <div className="vb-r-row"><span className="vb-r-path">{k.path}</span><span /><span className="vb-r-num">{formatBytes(k.bytes)}</span></div>
            <div className="vb-r-note">{k.reason}. Run again later to clear it.</div>
          </div>
        ))
      )}

      <Rule />
      <div className="vb-r-label">Left for you (not counted)</div>
      {result.leftForYou.length > 0 ? (
        result.leftForYou.map((h) => (
          <div key={h.id} className="vb-r-item">
            <div className="vb-r-row"><span>☐ {h.title}</span><span /><span className="vb-r-num">{formatBytes(h.bytes)}</span></div>
            <div className="vb-r-note">{h.summary}</div>
          </div>
        ))
      ) : hints.length > 0 ? (
        <>
          <div className="vb-r-note">Nothing on your list. If you need more room later:</div>
          {hints.map((h) => (
            <div key={h.id} className="vb-r-item">
              <div className="vb-r-row"><span>{h.title}</span><span /><span className="vb-r-num">{formatBytes(h.bytes)}</span></div>
            </div>
          ))}
        </>
      ) : (
        <div className="vb-r-note">Nothing on your list.</div>
      )}

      <Rule />
      <div className="vb-r-center vb-r-thanks">Thank you for staying.</div>
      <div className="vb-r-center vb-r-small">Now {formatBytes(free1)} free on {data.disk.name}</div>
      <Barcode seed={`${data.machineName}${result.freedBytes}`} />
      <div className="vb-r-center vb-r-code">VCY-{when.getFullYear()}{String(when.getMonth() + 1).padStart(2, '0')}{String(when.getDate()).padStart(2, '0')}-{Math.round(result.freedBytes / 1e8)}</div>
    </div>
  );

  return (
    <div className="vb-receipt-wrap">
      {reduced ? paper : (
        <motion.div
          initial={{ clipPath: 'inset(0 0 100% 0)', y: -14 }}
          animate={{ clipPath: 'inset(0 0 0% 0)', y: 0 }}
          transition={{ duration: 1.0, ease: [0.25, 0.7, 0.3, 1] }}
        >
          {paper}
        </motion.div>
      )}
    </div>
  );
}

function groupSteps(data: Dataset, result: RunResult) {
  return data.categories
    .map((c) => {
      const steps = result.steps.filter((s) => s.kind === 'delete' && s.categoryId === c.id);
      return { id: c.id, name: c.name, steps, freed: steps.reduce((s, x) => s + x.freedBytes, 0) };
    });
}

function Rule({ double }: { double?: boolean }) {
  return <div className={double ? 'vb-r-rule vb-r-rule-double' : 'vb-r-rule'} aria-hidden />;
}

function DiskBar({ label, used, total }: { label: string; used: number; total: number }) {
  return (
    <div className="vb-r-diskbar">
      <span>{label}</span>
      <span className="vb-r-track"><i style={{ width: `${(used / total) * 100}%` }} /></span>
      <span className="vb-r-num">{Math.round((used / total) * 100)}%</span>
    </div>
  );
}

function Stamp({ date, reduced }: { date: string; reduced: boolean }) {
  const inner = (
    <div className="vb-stamp-ink">
      <span className="vb-stamp-top">Rooms free again</span>
      <span className="vb-stamp-word">VACANCY</span>
      <span className="vb-stamp-bot">{date}</span>
    </div>
  );
  if (reduced) return <div className="vb-stamp" style={{ transform: 'rotate(-9deg)' }}>{inner}</div>;
  return (
    <motion.div
      className="vb-stamp"
      initial={{ opacity: 0, scale: 1.7, rotate: -15 }}
      animate={{ opacity: 1, scale: 1, rotate: -9 }}
      transition={{ delay: 0.75, duration: 0.26, ease: [0.5, 0, 0.9, 0.4] }}
    >
      {inner}
    </motion.div>
  );
}

/** The motel sign, printed in ink at the top of the receipt. */
function Letterhead() {
  return (
    <svg className="vb-r-sign" viewBox="0 0 240 80" role="img" aria-label="Vacancy">
      <g fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round">
        <path d="M16 4 h208 a10 10 0 0 1 10 10 v34 a10 10 0 0 1 -10 10 H60 l-16 18 v-18 H16 a10 10 0 0 1 -10 -10 V14 a10 10 0 0 1 10 -10 z" />
        <rect x="12" y="10" width="216" height="42" rx="6" strokeDasharray="0.1 5" strokeLinecap="round" strokeWidth="2.6" />
      </g>
      <text x="120" y="32" textAnchor="middle" dominantBaseline="central" className="vb-r-sign-word">VACANCY</text>
    </svg>
  );
}

function Barcode({ seed }: { seed: string }) {
  const bars: { x: number; w: number }[] = [];
  let x = 0;
  let h = 2166136261;
  for (let i = 0; i < 46; i++) {
    h ^= seed.charCodeAt(i % seed.length) + i;
    h = Math.imul(h, 16777619) >>> 0;
    const w = 1 + (h % 3);
    if (i % 2 === 0) bars.push({ x, w });
    x += w + 1;
  }
  return (
    <svg className="vb-r-barcode" viewBox={`0 0 ${x} 30`} preserveAspectRatio="none" aria-hidden>
      {bars.map((b) => <rect key={b.x} x={b.x} y={0} width={b.w} height={30} fill="currentColor" />)}
    </svg>
  );
}

/** Plain-text copy of the receipt for Save receipt. */
export function receiptText(data: Dataset, result: RunResult, when: Date): string {
  const d = receiptDate(when);
  const W = 48;
  const line = (l: string, r = '') => l + ' '.repeat(Math.max(1, W - l.length - r.length)) + r;
  const rule = '-'.repeat(W);
  const out: string[] = [
    'VACANCY', 'Disk cleanup receipt', data.machineName, data.disk.name, `${d.date}, ${d.time}`, rule,
    line('Space freed', formatBytes(result.freedBytes)),
    line('Used before', formatBytes(result.usedBefore)), line('Used after', formatBytes(result.usedAfter)), rule,
  ];
  for (const g of groupSteps(data, result).filter((x) => x.steps.length)) {
    out.push(g.name);
    for (const s of g.steps) out.push(line(`  ${s.title}`, formatBytes(s.freedBytes)));
    out.push(line('  Subtotal', formatBytes(g.freed)));
  }
  for (const u of result.undoable) out.push(line(`Moved in ${u.folder}`, `${formatCount(u.moves)} files`));
  out.push(rule, line('Total freed', formatBytes(result.freedBytes)), line('Items deleted', formatCount(result.deletedItems)),
    line('Files moved', formatCount(result.movedItems)), line('Time taken', formatDuration(result.durationSec)), rule);
  for (const k of result.skipped) out.push(`Skipped ${k.path}: ${k.reason} (${formatBytes(k.bytes)})`);
  for (const h of result.leftForYou) out.push(`Left for you: ${h.title} (${formatBytes(h.bytes)}, not counted)`);
  out.push('', 'Thank you for staying.');
  return out.join('\n');
}

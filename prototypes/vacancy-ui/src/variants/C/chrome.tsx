// PROTOTYPE variant C: the neon sign, the segmented capacity bar and the header that holds them.
import type { CSSProperties, ReactNode } from 'react';
import { Lock, SquareTerminal } from 'lucide-react';
import type { Dataset } from '../../shared/data';
import { agentName, catColor, gb } from './model';

const WORD = 'VACANCY'.split('');

export function Sign({ lit, glow = 1, no = false, size = 'head' }: { lit: number; glow?: number; no?: boolean; size?: 'head' | 'hero' }) {
  const style = { '--vc-g': glow, '--vc-lit': lit / 7 } as CSSProperties;
  return (
    <div className={`vc-sign vc-sign-${size}`} style={style} role="img" aria-label={no ? 'No vacancy' : `Vacancy, ${lit} of 7 letters lit`}>
      <span className={`vc-sign-no ${no ? 'on' : ''}`} aria-hidden>
        <span className="vc-sign-no-off">NO</span>
        <span className="vc-sign-no-lit">NO</span>
      </span>
      <span className="vc-sign-word" aria-hidden>
        {WORD.map((ch, i) => (
          <span key={i} className={`vc-tube ${i < lit ? 'on' : ''}`} style={{ animationDelay: `${i * 70}ms` }}>
            <span className="vc-tube-off">{ch}</span>
            <span className="vc-tube-lit" style={{ animationDelay: `${i * 70}ms` }}>{ch}</span>
          </span>
        ))}
      </span>
    </div>
  );
}

export interface Seg {
  id: string;
  name: string;
  bytes: number;
  color: string;
  locked?: boolean;
}

export function segsFor(d: Dataset, minus: Record<string, number> = {}): Seg[] {
  return [...d.categories]
    .sort((a, b) => Number(!!a.protected) - Number(!!b.protected) || b.bytes - a.bytes)
    .map((c) => ({ id: c.id, name: c.name, bytes: Math.max(0, c.bytes - (minus[c.id] ?? 0)), color: catColor(c, 'seg'), locked: c.protected }));
}

export function CapacityBar({
  segs,
  total,
  ghost = 0,
  unknown = 0,
  free,
  live = false,
  marker,
  hot,
  onHot,
}: {
  segs: Seg[];
  total: number;
  ghost?: number;
  unknown?: number;
  free: number;
  live?: boolean;
  marker?: string;
  hot?: string | null;
  onHot?: (id: string | null) => void;
}) {
  const pct = (b: number) => `${(b / total) * 100}%`;
  const usedKept = segs.reduce((s, x) => s + x.bytes, 0) + unknown;
  const markerAt = usedKept / total;
  return (
    <div className={`vc-cap ${live ? 'live' : ''}`}>
      <div className="vc-cap-track" onMouseLeave={() => onHot?.(null)}>
        {segs.map((s) => {
          const w = s.bytes / total;
          return (
            <div
              key={s.id}
              className={`vc-cap-seg ${s.locked ? 'locked' : ''} ${hot === s.id ? 'hot' : ''}`}
              style={{ width: pct(s.bytes), '--c': s.color } as CSSProperties}
              title={`${s.name}: ${gb(s.bytes)}${s.locked ? ' (locked)' : ''}`}
              onMouseEnter={() => onHot?.(s.id)}
            >
              {w > 0.055 && (
                <span className="vc-cap-label">
                  {s.locked && <Lock size={10} strokeWidth={2.4} />}
                  <span>{s.name}</span>
                  {w > 0.12 && <em>{gb(s.bytes)}</em>}
                </span>
              )}
            </div>
          );
        })}
        {unknown > 0 && (
          <div className="vc-cap-unknown" style={{ width: pct(unknown) }}>
            {unknown / total > 0.25 && <span className="vc-cap-label"><span>{gb(unknown)} used, not sorted yet</span></span>}
          </div>
        )}
        {ghost > 0 && <div className="vc-cap-ghost" style={{ width: pct(ghost) }} title={`${gb(ghost)} this plan frees`} />}
        <div className="vc-cap-free" style={{ width: pct(free) }}>
          {free / total > 0.06 && <span className="vc-cap-label"><em>{gb(free)} free</em></span>}
        </div>
      </div>
      {marker && (
        <div className={`vc-cap-marker ${markerAt > 0.8 ? 'flip' : ''}`} style={{ left: `${markerAt * 100}%` }}>
          <span>{marker}</span>
        </div>
      )}
    </div>
  );
}

export function AgentBadge({ data }: { data: Dataset }) {
  const on = !!data.scan.aiAgent;
  return (
    <div className={`vc-agent ${on ? 'on' : ''}`} title={on ? `${agentName(data)} is installed. Vacancy asks it about unknown folders and how to sort them.` : 'No AI agent found. Rules only.'}>
      <SquareTerminal size={14} strokeWidth={2} />
      <span>{on ? agentName(data) : 'Rules only'}</span>
      <i aria-hidden />
    </div>
  );
}

export interface Stat {
  label: string;
  value: ReactNode;
  tone?: 'pink' | 'mute';
}

export function Header({ data, sign, bar, stats }: { data: Dataset; sign: ReactNode; bar: ReactNode; stats: Stat[] }) {
  return (
    <header className="vc-head">
      <div className="vc-head-top">
        {sign}
        <div className="vc-head-disk">
          <div className="vc-disk-name">{data.disk.name}</div>
          <div className="vc-disk-sub">
            {gb(data.disk.totalBytes)} on {data.machineName}
          </div>
        </div>
        <dl className="vc-stats">
          {stats.map((s) => (
            <div key={s.label} className={s.tone ? `vc-stat-${s.tone}` : undefined}>
              <dt>{s.label}</dt>
              <dd>{s.value}</dd>
            </div>
          ))}
        </dl>
        <AgentBadge data={data} />
      </div>
      {bar}
    </header>
  );
}

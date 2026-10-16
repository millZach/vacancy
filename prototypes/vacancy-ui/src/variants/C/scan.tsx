// PROTOTYPE variant C: scan step. Pre-scan "front desk", then a ~4 s power-up of the sign, counters, path ticker and capacity bar.
import { useEffect, useMemo, useState } from 'react';
import { ScanLine, SquareTerminal } from 'lucide-react';
import type { Dataset } from '../../shared/data';
import type { VariantProps } from '../../shared/types';
import { CapacityBar, Header, Sign, segsFor } from './chrome';
import { agentName, gb, litFor, n, planTotals } from './model';

const SCAN_MS = 4200;
const easeOut = (x: number) => 1 - Math.pow(1 - x, 2.4);

function tickerPaths(d: Dataset): string[] {
  const out: string[] = [];
  const sep = d.os === 'mac' ? '/' : '\\';
  for (const c of d.categories) for (const it of c.items) if (!it.path.includes('*')) out.push(it.path);
  for (const p of d.proposals) for (const s of p.samplePaths) if (s.includes(sep) && !s.includes(' = ')) out.push(s);
  for (const r of d.reorgs) for (const ch of r.before.children ?? []) if (!ch.name.startsWith('...')) out.push(`${r.path}${sep}${ch.name}`);
  // Interleave so neighbouring lines come from different places.
  const a: string[] = [];
  const step = 7;
  for (let i = 0; i < step; i++) for (let j = i; j < out.length; j += step) a.push(out[j]);
  return a;
}

export function ScanStage({ data, onStep, proposals, reorgs }: VariantProps) {
  const [phase, setPhase] = useState<'idle' | 'scanning'>('idle');
  const [p, setP] = useState(0);
  const paths = useMemo(() => tickerPaths(data), [data]);
  const target = litFor(data, planTotals(data, proposals, reorgs).delBytes);

  useEffect(() => {
    if (phase !== 'scanning') return;
    let raf = 0;
    let done = 0;
    const t0 = performance.now();
    const tick = (now: number) => {
      const x = Math.min(1, (now - t0) / SCAN_MS);
      setP(x);
      if (x < 1) raf = requestAnimationFrame(tick);
      else done = window.setTimeout(() => onStep('overview'), 500);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      clearTimeout(done);
    };
  }, [phase]);

  const { totalBytes: total, usedBytes: used } = data.disk;
  const free = total - used;
  const e = easeOut(p);
  const scanning = phase === 'scanning';

  // Categories appear left to right as the scan "finds" them.
  let left = e * used;
  const segs = scanning
    ? segsFor(data).map((s) => {
        const b = Math.max(0, Math.min(s.bytes, left));
        left -= s.bytes;
        return { ...s, bytes: b };
      })
    : [];
  const found = segs.reduce((s, x) => s + x.bytes, 0);
  const lit = scanning ? Math.min(target, Math.floor(p * 1.25 * (target + 0.6))) : 0;
  const tick = Math.floor((p * SCAN_MS) / 55);
  const recent = scanning ? Array.from({ length: 8 }, (_, i) => paths[(tick - i + paths.length * 50) % paths.length]) : [];
  const pctFull = Math.round((used / total) * 100);

  return (
    <>
      <Header
        data={data}
        sign={<Sign lit={lit} glow={0.35 + 0.5 * e} no={!scanning || p < 0.82} />}
        stats={[
          { label: 'Used', value: gb(used) },
          { label: 'Free', value: gb(free) },
          { label: 'Scanned', value: scanning ? `${n(Math.round(e * data.scan.files))} files` : 'Not yet', tone: 'mute' },
        ]}
        bar={<CapacityBar segs={segs} unknown={used - found} total={total} free={free} live={scanning} />}
      />
      <main className="vc-scan">
        {!scanning ? (
          <div className="vc-desk-card">
            <h1 className="vc-scan-title">
              {data.disk.name} is {pctFull}% full.
            </h1>
            <p className="vc-scan-sub">
              {gb(free)} left of {gb(total)}. Scan to see where the rest went and what can go.
            </p>
            <div className={`vc-scan-agent ${data.scan.aiAgent ? '' : 'none'}`}>
              <SquareTerminal size={18} strokeWidth={1.8} />
              {data.scan.aiAgent ? (
                <p>
                  <b>{agentName(data)} is installed.</b> Vacancy asks it to explain big folders no rule recognises and to suggest how to sort messy ones. Every
                  suggestion is labelled with its name.
                </p>
              ) : (
                <p>
                  <b>No AI agent found.</b> Vacancy will use its built-in rules only.
                </p>
              )}
            </div>
            <button className="vc-btn vc-btn-neon vc-scan-btn" onClick={() => setPhase('scanning')} autoFocus>
              <ScanLine size={18} strokeWidth={2} />
              Scan {data.disk.name}
            </button>
            <p className="vc-fine">Scanning only reads sizes. Nothing changes until you approve a plan.</p>
          </div>
        ) : (
          <div className="vc-desk-card vc-scanning">
            <h1 className="vc-scan-title">Scanning {data.disk.name}</h1>
            <div className="vc-counters">
              <div>
                <b>{n(Math.round(e * data.scan.files))}</b>
                <span>files</span>
              </div>
              <div>
                <b>{n(Math.round(e * data.scan.folders))}</b>
                <span>folders</span>
              </div>
              <div>
                <b>{gb(found)}</b>
                <span>sorted into categories</span>
              </div>
            </div>
            <div className="vc-scan-progress">
              <i style={{ width: `${p * 100}%` }} />
            </div>
            <ol className="vc-ticker" aria-live="off">
              {recent.map((path, i) => (
                <li key={`${tick}-${i}`} style={{ opacity: 1 - i * 0.12 }}>
                  {path}
                </li>
              ))}
            </ol>
            {data.scan.aiAgent && p > 0.55 && (
              <p className="vc-scan-asking">
                <SquareTerminal size={14} strokeWidth={2} /> Asking {agentName(data)} about {data.reorgs.length + data.proposals.filter((x) => x.source === 'ai').length} folders
              </p>
            )}
          </div>
        )}
      </main>
    </>
  );
}

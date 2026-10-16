// PROTOTYPE variant A "Atlas": the pre-scan plate and the scanning readout that floats over the map as it fills in.
import { HardDrive, ScanLine, Sparkles } from 'lucide-react';
import { useMemo } from 'react';
import { formatCount, type Dataset } from '../../shared/data';
import { gb } from './model';

export function ScanPlate({ data, onScan }: { data: Dataset; onScan: () => void }) {
  const agent = data.scan.aiAgent;
  return (
    <div className="va-plate">
      <HardDrive size={26} strokeWidth={1.4} className="va-plate-icon" />
      <h1>{data.disk.name}</h1>
      <p className="va-plate-cap">
        {gb(data.disk.totalBytes, 0)} on {data.machineName}, {gb(data.disk.usedBytes, 0)} used
      </p>
      <button type="button" className="va-cta va-cta--scan" onClick={onScan}>
        <ScanLine size={16} /> Scan {data.disk.name}
      </button>
      <div className={`va-agent-note ${agent ? '' : 'is-none'}`}>
        <Sparkles size={14} strokeWidth={2} />
        {agent ? (
          <span>
            <b>{agent} is installed.</b> Vacancy will ask it about big folders the built-in rules don't recognize, and for tidier layouts of messy
            ones.
          </span>
        ) : (
          <span>
            <b>No AI agent found.</b> Vacancy will use its built-in rules only.
          </span>
        )}
      </div>
      <p className="va-plate-fine">Scanning only reads. Nothing changes until you approve a plan.</p>
    </div>
  );
}

const ease = (t: number) => 1 - Math.pow(1 - t, 2.4);

export function ScanHud({ data, t }: { data: Dataset; t: number }) {
  const paths = useMemo(() => {
    const all = [
      ...data.categories.flatMap((c) => c.items.map((i) => i.path)),
      ...data.proposals.flatMap((p) => p.samplePaths.filter((s) => /[\\/]/.test(s) && !s.includes(' = '))),
    ];
    // Interleave so the ticker jumps around the disk the way a real walk does.
    return all.map((p, i) => all[(i * 7) % all.length] ?? p);
  }, [data]);
  const path = paths[Math.floor(t * 70) % paths.length];
  return (
    <div className="va-hud" role="status">
      <div className="va-hud-nums">
        <span>
          <b>{formatCount(Math.round(ease(t) * data.scan.files))}</b> files
        </span>
        <span>
          <b>{formatCount(Math.round(ease(t) * data.scan.folders))}</b> folders
        </span>
      </div>
      <div className="va-hud-path">{path}</div>
      <div className="va-hud-bar">
        <i style={{ width: `${t * 100}%` }} />
      </div>
    </div>
  );
}

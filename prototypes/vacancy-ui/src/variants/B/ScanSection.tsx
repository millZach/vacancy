// PROTOTYPE scan step: greeting and disk before the scan, narrated counters during it, a one-line recap after.
import { useEffect, useMemo, useRef, useState } from 'react';
import { HardDrive, SquareTerminal, Check } from 'lucide-react';
import { formatBytes, formatCount, type Dataset } from '../../shared/data';
import { Say, You } from './Feed';
import { formatDuration } from './util';

export const SCAN_MS = 4200;

export function ScanIntro({ data, scanning, onScan, onDone, ensureVisible }: {
  data: Dataset; scanning: boolean; onScan: () => void; onDone: () => void; ensureVisible: (el: HTMLElement) => void;
}) {
  const { disk, scan, userName, machineName } = data;
  const free = disk.totalBytes - disk.usedBytes;
  const pct = Math.round((disk.usedBytes / disk.totalBytes) * 100);
  return (
    <section className="vb-section" data-vb-anchor="scan">
      <Say avatar>
        <h1 className="vb-display">Hello, {userName}.</h1>
        <p className="vb-voice">
          I'll look through {disk.name}, show you where the space went, and suggest what to clean up.
          Nothing changes until you say so.
        </p>
      </Say>

      <div className="vb-card vb-disk">
        <div className="vb-disk-id">
          <HardDrive size={20} strokeWidth={1.5} aria-hidden />
          <div>
            <div className="vb-disk-name">{disk.name}</div>
            <div className="vb-disk-machine">{machineName}</div>
          </div>
        </div>
        <div className="vb-disk-free">
          <span className="vb-disk-num">{formatBytes(free)}</span>
          <span className="vb-disk-label">free of {formatBytes(disk.totalBytes, 0)}</span>
        </div>
        <div className="vb-disk-bar" aria-hidden>
          <i style={{ width: `${pct}%` }} />
        </div>
        <div className="vb-disk-foot">
          <span>{formatBytes(disk.usedBytes, 0)} used, {pct}% full</span>
          {pct > 85 && <span className="vb-disk-warn">Getting tight</span>}
        </div>
      </div>

      <div className="vb-agent">
        <span className="vb-agent-icon"><SquareTerminal size={17} strokeWidth={1.75} aria-hidden /></span>
        {scan.aiAgent ? (
          <p>
            <b>{scan.aiAgent} is set up on this computer.</b> I'll ask it about big folders I don't recognize and for ideas
            on tidying folders. Anything it suggests is labeled with its name.
          </p>
        ) : (
          <p><b>No AI helper found.</b> I'll use Vacancy's built-in rules only.</p>
        )}
      </div>

      {!scanning && (
        <div className="vb-scan-go">
          <button type="button" className="vb-primary vb-primary-lg" onClick={onScan}>Scan {disk.name}</button>
          <span className="vb-hint">Reads file sizes only. Usually under a minute.</span>
        </div>
      )}

      {scanning && <Scanning data={data} onDone={onDone} ensureVisible={ensureVisible} />}
    </section>
  );
}

function Scanning({ data, onDone, ensureVisible }: { data: Dataset; onDone: () => void; ensureVisible: (el: HTMLElement) => void }) {
  const cardRef = useRef<HTMLDivElement>(null);
  const lines = useMemo(() => {
    const l = data.categories.map((c) =>
      c.protected ? `Measuring ${c.name}. I won't touch it.` : `Looking through ${c.name}…`,
    );
    if (data.scan.aiAgent) l.push(`Asking ${data.scan.aiAgent} about folders I don't recognize…`);
    l.push('Adding it all up…');
    return l;
  }, [data]);
  const ticker = useMemo(
    () => [...data.categories.flatMap((c) => c.items.map((i) => i.path)), ...data.proposals.flatMap((p) => p.samplePaths)],
    [data],
  );

  const [t, setT] = useState(0);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  useEffect(() => {
    let raf = 0;
    const t0 = performance.now();
    let fired = false;
    const tick = (now: number) => {
      const e = Math.min(SCAN_MS, now - t0);
      setT(e);
      if (e < SCAN_MS) raf = requestAnimationFrame(tick);
      else if (!fired) {
        fired = true;
        window.setTimeout(() => doneRef.current(), 350);
      }
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []);

  const p = t / SCAN_MS;
  const eased = 1 - Math.pow(1 - p, 2.2);
  const lineAt = Math.min(lines.length - 1, Math.floor(p * lines.length));
  const path = ticker[Math.floor(t / 90) % ticker.length];

  useEffect(() => {
    if (cardRef.current) ensureVisible(cardRef.current);
  }, [lineAt]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <>
      <You>Scan {data.disk.name}</You>
      <Say avatar className="vb-narrate">
        <ul className="vb-narration" aria-live="polite">
          {lines.slice(0, lineAt + 1).map((l, i) => {
            const done = i < lineAt || p >= 1;
            if (i < lineAt - 3) return null;
            return (
              <li key={l} className={done ? 'vb-narr-done' : 'vb-narr-now'}>
                <span className="vb-narr-mark" aria-hidden>{done ? <Check size={13} strokeWidth={2.4} /> : <i />}</span>
                {l}
              </li>
            );
          })}
        </ul>
      </Say>
      <div className="vb-card vb-counting" ref={cardRef}>
        <div className="vb-count">
          <span className="vb-count-num">{formatCount(Math.round(eased * data.scan.files))}</span>
          <span className="vb-count-label">files</span>
        </div>
        <div className="vb-count">
          <span className="vb-count-num">{formatCount(Math.round(eased * data.scan.folders))}</span>
          <span className="vb-count-label">folders</span>
        </div>
        <div className="vb-count vb-count-small">
          <span className="vb-count-num">{formatBytes(eased * data.disk.usedBytes, 0)}</span>
          <span className="vb-count-label">measured</span>
        </div>
        <div className="vb-ticker" aria-hidden>{path}</div>
        <div className="vb-scanline" aria-hidden><i style={{ transform: `scaleX(${p})` }} /></div>
      </div>
    </>
  );
}

export function ScanRecap({ data }: { data: Dataset }) {
  return (
    <div className="vb-recap">
      <Check size={14} strokeWidth={2.2} aria-hidden />
      <span>
        Scanned {data.disk.name}: {formatCount(data.scan.files)} files in {formatCount(data.scan.folders)} folders,{' '}
        {formatDuration(data.scan.durationSec)}.
      </span>
    </div>
  );
}

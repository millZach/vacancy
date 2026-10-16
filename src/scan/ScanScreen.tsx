import { HardDrive, ScanLine, Sparkles } from 'lucide-react';
import { useRef, useState } from 'react';
import { DotField, type DotFieldHandle } from '../dotfield/DotField';
import type { DriveState } from '../drive';
import { formatBytes } from '../format';
import './scan.css';

interface Props {
  drive: DriveState;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
}

/**
 * Before a scan: the drive plate floating on the living dot field. Clicking the water ripples it; the Scan button
 * sends a big splash. (Scanning itself arrives in a later ticket.)
 */
export function ScanScreen({ drive, agent }: Props) {
  const field = useRef<DotFieldHandle>(null);
  const [calm, setCalm] = useState<HTMLDivElement | null>(null);

  const splash = (button: HTMLButtonElement) => {
    const b = button.getBoundingClientRect();
    field.current?.drop(b.left + b.width / 2, b.top + b.height / 2, 'splash');
  };

  return (
    <main className="scan">
      <DotField ref={field} calm={calm} />
      <div className="scan-toolbar">
        {drive.status === 'ready' && (
          <nav className="crumbs" aria-label="Map location">
            <span className="crumb is-here" aria-current="location">
              {drive.volume.name}
            </span>
          </nav>
        )}
      </div>
      <div className="scan-stage">
        <div className="plate">
          <div className="plate-content" ref={setCalm} aria-busy={drive.status === 'loading'}>
            <HardDrive size={26} strokeWidth={1.4} className="plate-icon" aria-hidden />
            {drive.status === 'ready' && (
              <>
                <h1>{drive.volume.name}</h1>
                <p className="plate-cap">
                  {formatBytes(drive.volume.totalBytes, 0)} on {drive.machineName},{' '}
                  {formatBytes(drive.volume.usedBytes, 0)} used
                </p>
                <button type="button" className="cta cta--lg" onClick={(e) => splash(e.currentTarget)}>
                  <ScanLine size={16} aria-hidden /> Scan {drive.volume.name}
                </button>
                <AgentNote agent={agent} />
              </>
            )}
            {drive.status === 'error' && (
              <>
                <h1>Can’t read your system drive</h1>
                <p className="plate-cap plate-error" role="alert">
                  {drive.message}
                </p>
              </>
            )}
            <p className="plate-fine">Scanning only reads. Nothing changes until you approve a plan.</p>
          </div>
        </div>
      </div>
    </main>
  );
}

function AgentNote({ agent }: { agent: string | null }) {
  return (
    <div className={agent ? 'agent-note' : 'agent-note is-none'} data-no-ripple>
      <Sparkles size={14} strokeWidth={2} aria-hidden />
      {agent ? (
        <span>
          <b>{agent} is installed.</b> Vacancy will ask it about big folders the built-in rules don’t recognize, and
          for tidier layouts of messy ones.
        </span>
      ) : (
        <span>
          <b>No AI agent found.</b> Vacancy will use its built-in rules only.
        </span>
      )}
    </div>
  );
}

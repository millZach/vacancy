import { HardDrive, ScanLine, ShieldAlert, Sparkles } from 'lucide-react';
import type { Ref } from 'react';
import type { FullDiskAccess, VolumeInfo } from '../bridge';
import { formatBytes } from '../format';
import './scan.css';

interface Props {
  /** The drives a scan can be pointed at, the system volume first. */
  volumes: VolumeInfo[];
  selected: VolumeInfo;
  onSelect: (volume: VolumeInfo) => void;
  machineName: string;
  fullDiskAccess: FullDiskAccess;
  onOpenFullDiskAccess: () => void;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
  /** Why the last scan failed, if it did. */
  failure: string | null;
  onScan: (button: HTMLButtonElement) => void;
  /** The plate's content, which the dot field keeps calm around. */
  contentRef: Ref<HTMLDivElement>;
}

/**
 * Before a scan: the drive plate floating on the dot field. It names the drive, offers the other local drives,
 * and starts the scan. On a Mac without Full Disk Access it says what will be missing and opens the setting.
 */
export function ScanPlate({
  volumes,
  selected,
  onSelect,
  machineName,
  fullDiskAccess,
  onOpenFullDiskAccess,
  agent,
  failure,
  onScan,
  contentRef,
}: Props) {
  return (
    <div className="plate">
      <div className="plate-content" ref={contentRef}>
        <HardDrive size={26} strokeWidth={1.4} className="plate-icon" aria-hidden />
        <h1>{selected.name}</h1>
        <p className="plate-cap">
          {formatBytes(selected.totalBytes, 0)} on {machineName}, {formatBytes(selected.usedBytes, 0)} used
        </p>
        {volumes.length > 1 && (
          <div className="drive-picker" role="radiogroup" aria-label="Drive to scan">
            {volumes.map((v) => (
              <button
                key={v.mountPoint}
                type="button"
                role="radio"
                aria-checked={v.mountPoint === selected.mountPoint}
                className={v.mountPoint === selected.mountPoint ? 'drive-chip is-on' : 'drive-chip'}
                onClick={() => onSelect(v)}
              >
                <HardDrive size={12} strokeWidth={2} aria-hidden />
                {v.name}
              </button>
            ))}
          </div>
        )}
        <button type="button" className="cta cta--lg" onClick={(e) => onScan(e.currentTarget)}>
          <ScanLine size={16} aria-hidden /> Scan {selected.name}
        </button>
        {failure !== null && (
          <p className="plate-cap plate-error" role="alert">
            The scan stopped: {failure}
          </p>
        )}
        {fullDiskAccess === 'missing' && <AccessNote onOpen={onOpenFullDiskAccess} />}
        <AgentNote agent={agent} />
        <p className="plate-fine">Scanning only reads. Nothing changes until you approve a plan.</p>
      </div>
    </div>
  );
}

/** macOS without Full Disk Access: the scan still runs, and this says what it will miss. */
export function AccessNote({ onOpen }: { onOpen: () => void }) {
  return (
    <div className="agent-note access-note" data-no-ripple>
      <ShieldAlert size={14} strokeWidth={2} aria-hidden />
      <span>
        <b>Vacancy can’t see everything yet.</b> Without Full Disk Access, macOS hides Mail, Messages, Safari and
        other apps’ data, so their space shows as space Vacancy can’t see. Turn it on, then reopen Vacancy.{' '}
        <button type="button" className="link-button" onClick={onOpen}>
          Open System Settings
        </button>
      </span>
    </div>
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

import { Check, Map as MapIcon, ScanLine, ShieldAlert } from 'lucide-react';
import { useId, useRef, type KeyboardEvent, type ReactNode, type Ref } from 'react';
import { DriveIcon, DriveStatus, DriveTags, UsageBar } from '../drives/bits';
import type { DriveItem, ReadyDrives } from '../drives/context';
import { reclaimableOf } from '../drives/session';
import { useNow } from '../drives/useNow';
import { formatBytes } from '../format';
import './scan.css';

interface Props {
  drives: ReadyDrives;
  onOpenFullDiskAccess: () => void;
  /** Read-only advisor status note. */
  advisorNote: ReactNode;
  /** Why the last scan of this drive failed, if it did. */
  failure: string | null;
  onScan: (button: HTMLButtonElement) => void;
  /** The plate's content, which the dot field keeps calm around. */
  contentRef: Ref<HTMLDivElement>;
}

/**
 * Before a scan: the plate floating on the dot field. With several drives it asks which one to scan, with a card per
 * drive, the drive on screen lit; with one, it is that drive's plate. It starts the scan, or for a drive scanned
 * already, goes back to its map or scans again. On a Mac without Full Disk Access it says what will be missing and
 * opens the setting.
 */
export function ScanPlate({ drives, onOpenFullDiskAccess, advisorNote, failure, onScan, contentRef }: Props) {
  const { list, current, machineName } = drives;
  const v = current.volume;
  const several = list.length > 1;
  return (
    <div className="plate">
      <div className={several ? 'plate-content has-cards' : 'plate-content'} ref={contentRef}>
        {several ? (
          <>
            <h1 className="plate-ask">Which drive should Vacancy scan?</h1>
            <p className="plate-cap">
              {list.length} drives on {machineName}
            </p>
            <DriveCards drives={drives} />
          </>
        ) : (
          <>
            <DriveIcon kind={v.kind} size={26} className="plate-icon" />
            <h1>{v.name}</h1>
            <p className="plate-cap">
              {formatBytes(v.totalBytes, 0)} on {machineName}, {formatBytes(v.usedBytes, 0)} used
            </p>
          </>
        )}
        <PlateActions drive={current} onScan={onScan} onOpenMap={drives.openMap} />
        {failure !== null && (
          <p className="plate-cap plate-error" role="alert">
            The scan stopped: {failure}
          </p>
        )}
        {drives.fullDiskAccess === 'missing' && <AccessNote onOpen={onOpenFullDiskAccess} />}
        {advisorNote}
        <p className="plate-fine">
          Scanning only reads. Nothing changes until you approve a plan.
          {v.kind !== 'internal' && <> Keep {v.name} plugged in until the scan finishes.</>}
        </p>
      </div>
    </div>
  );
}

/** One card per drive. The arrow keys move between them, as in any group of radio buttons. */
function DriveCards({ drives }: { drives: ReadyDrives }) {
  const now = useNow();
  const ids = useId();
  const cards = useRef<(HTMLButtonElement | null)[]>([]);
  const { list } = drives;

  const onKey = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    const n = list.length;
    const keys: Record<string, number> = {
      ArrowRight: i + 1,
      ArrowDown: i + 1,
      ArrowLeft: i - 1,
      ArrowUp: i - 1,
      Home: 0,
      End: n - 1,
    };
    const to = keys[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const j = (to + n) % n;
    const next = list[j];
    if (!next) return;
    drives.pick(next.volume.mountPoint);
    cards.current[j]?.focus();
  };

  return (
    <div className="drive-cards" role="radiogroup" aria-label="Drives" data-no-ripple>
      {list.map((d, i) => (
        <button
          key={d.volume.mountPoint}
          ref={(el) => {
            cards.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={d.current}
          aria-label={d.volume.name}
          aria-describedby={`${ids}-${i}-tags ${ids}-${i}-nums ${ids}-${i}-status`}
          tabIndex={d.current ? 0 : -1}
          className={d.current ? 'drive-card is-on' : 'drive-card'}
          onClick={() => drives.pick(d.volume.mountPoint)}
          onKeyDown={(e) => onKey(e, i)}
        >
          <span className="drive-card-top">
            <DriveIcon kind={d.volume.kind} size={20} className="drive-card-icon" />
            <DriveTags volume={d.volume} id={`${ids}-${i}-tags`} />
            {d.current && (
              <span className="drive-card-tick" aria-hidden>
                <Check size={11} strokeWidth={3} />
              </span>
            )}
          </span>
          <span className="drive-card-name">{d.volume.name}</span>
          <UsageBar volume={d.volume} reclaim={reclaimableOf(d.scan)} />
          <span className="drive-card-nums" id={`${ids}-${i}-nums`}>
            <b>{formatBytes(d.volume.freeBytes, 0)} free</b> of {formatBytes(d.volume.totalBytes, 0)}
          </span>
          <DriveStatus scan={d.scan} now={now} id={`${ids}-${i}-status`} />
        </button>
      ))}
    </div>
  );
}

/** Scan the drive; or, for a drive scanned already, go back to its map without scanning again, or scan again. */
function PlateActions({
  drive,
  onScan,
  onOpenMap,
}: {
  drive: DriveItem;
  onScan: (button: HTMLButtonElement) => void;
  onOpenMap: () => void;
}) {
  if (drive.scan.phase !== 'done')
    return (
      <button type="button" className="cta cta--lg" onClick={(e) => onScan(e.currentTarget)}>
        <ScanLine size={16} aria-hidden /> Scan {drive.volume.name}
      </button>
    );
  return (
    <div className="plate-actions">
      <button type="button" className="cta cta--lg" onClick={onOpenMap}>
        <MapIcon size={16} aria-hidden /> Open its map
      </button>
      <button type="button" className="btn btn--lg" onClick={(e) => onScan(e.currentTarget)}>
        <ScanLine size={14} aria-hidden /> Scan again
      </button>
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

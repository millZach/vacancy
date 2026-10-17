import { History, LoaderCircle, Lock } from 'lucide-react';
import { useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { useBridge, type RunHistory, type RunListing, type RunReport } from '../bridge';
import { errorMessage } from '../errors';
import { formatBytes } from '../format';
import { formatWhen } from '../summary/when';
import './history.css';

interface Props {
  /** A plan is running: the history waits, like the drive menu. */
  locked: boolean;
  /** A run was opened: show its summary. */
  onOpen: (report: RunReport) => void;
}

type Listing = { status: 'loading' } | { status: 'ready'; history: RunHistory } | { status: 'error'; message: string };

/** The runs grouped by drive, the drive with the newest run first; each drive's runs newest first. */
function byDrive(runs: RunListing[]): { key: string; name: string; runs: RunListing[] }[] {
  const groups: { key: string; name: string; runs: RunListing[] }[] = [];
  for (const run of runs) {
    // A mount point can belong to a different USB disk next time. With no identity, keep a legacy run on its own
    // rather than guessing which disk it belongs to from a name or drive letter.
    const key = run.volumeId ? `volume:${run.volumeId}` : `run:${run.id}`;
    const group = groups.find((g) => g.key === key);
    if (group) group.runs.push(run);
    else groups.push({ key, name: run.volumeName, runs: [run] });
  }
  return groups;
}

/**
 * The header's "Past runs": every kept run, per drive, with when it ran and what it freed. Choosing one opens its
 * summary, from which its highlight slides can be replayed. Kept runs that can't be read are counted, not listed.
 */
export function HistoryMenu({ locked, onOpen }: Props) {
  const bridge = useBridge();
  const [open, setOpen] = useState(false);
  const [listing, setListing] = useState<Listing>({ status: 'loading' });
  const [opening, setOpening] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const shown = open && !locked;
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const rows = useRef<(HTMLButtonElement | null)[]>([]);
  const asked = useRef(0);

  const close = (refocus: boolean) => {
    asked.current++;
    setOpen(false);
    setOpening(null);
    if (refocus) button.current?.focus();
  };

  const toggle = () => {
    if (locked) return;
    if (open) return close(false);
    // Read afresh each time: a run may have ended since.
    const id = ++asked.current;
    setOpen(true);
    setOpenError(null);
    setListing({ status: 'loading' });
    bridge.listRuns().then(
      (history) => id === asked.current && setListing({ status: 'ready', history }),
      (error: unknown) => id === asked.current && setListing({ status: 'error', message: errorMessage(error) }),
    );
  };

  // A press anywhere outside closes it.
  useEffect(() => {
    if (!shown) return;
    const down = (e: PointerEvent) => {
      if (e.target instanceof Node && root.current?.contains(e.target)) return;
      asked.current++;
      setOpen(false);
      setOpening(null);
    };
    document.addEventListener('pointerdown', down, true);
    return () => document.removeEventListener('pointerdown', down, true);
  }, [shown]);

  // Once the list is in, focus goes to the newest run, so the arrow keys start from there.
  const ready = shown && listing.status === 'ready';
  useEffect(() => {
    if (ready) rows.current[0]?.focus();
  }, [ready]);

  const choose = (run: RunListing) => {
    const id = ++asked.current;
    setOpening(run.id);
    setOpenError(null);
    bridge.openRun(run.id).then(
      (report) => {
        if (id !== asked.current) return;
        close(false);
        onOpen(report);
      },
      (error: unknown) => {
        if (id !== asked.current) return;
        setOpening(null);
        setOpenError(errorMessage(error));
      },
    );
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!shown) return;
    const items = rows.current.filter((r): r is HTMLButtonElement => r?.isConnected === true);
    const i = items.findIndex((r) => r === document.activeElement);
    const go = (to: number) => {
      e.preventDefault();
      items[(to + items.length) % items.length]?.focus();
    };
    if (e.key === 'ArrowDown') go(i + 1);
    else if (e.key === 'ArrowUp') go(i - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(items.length - 1);
    else if (e.key === 'Tab') close(false);
    else if (e.key === 'Escape') {
      // Escape closes the list only; the map behind it doesn't zoom out too.
      e.preventDefault();
      e.stopPropagation();
      close(true);
    }
  };

  const groups = listing.status === 'ready' ? byDrive(listing.history.runs) : [];
  let row = 0;

  return (
    <div className="history" ref={root} onKeyDown={onKey} data-no-ripple>
      <button
        ref={button}
        type="button"
        className={shown ? 'history-button is-open' : 'history-button'}
        aria-haspopup="menu"
        aria-expanded={shown}
        aria-disabled={locked || undefined}
        title={locked ? 'Past runs open once this run has finished.' : 'Past runs'}
        onClick={toggle}
      >
        {locked ? <Lock size={13} aria-hidden /> : <History size={14} aria-hidden />}
        Past runs
      </button>
      {shown && (
        <div className="history-pop" role="menu" aria-label="Past runs" aria-busy={listing.status === 'loading'}>
          <div className="history-head" aria-hidden>
            Past runs
          </div>
          {listing.status === 'loading' && (
            <p className="history-note">
              <LoaderCircle size={13} className="history-spin" aria-hidden /> Reading the history…
            </p>
          )}
          {listing.status === 'error' && (
            <p className="history-note is-error" role="alert">
              The history can’t be read: {listing.message}
            </p>
          )}
          {listing.status === 'ready' && listing.history.runs.length === 0 && (
            <p className="history-note">No runs yet. Each plan you run is kept here, with its summary.</p>
          )}
          {groups.map((g) => (
            <div key={g.key} className="history-drive" role="group" aria-label={g.name}>
              <div className="history-drive-name" aria-hidden>
                {g.name}
              </div>
              {g.runs.map((run) => {
                const at = row++;
                const when = formatWhen(run.startedAt);
                return (
                  <button
                    key={run.id}
                    ref={(el) => {
                      rows.current[at] = el;
                    }}
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    className="history-row"
                    aria-label={`${when} on ${run.volumeName}: ${formatBytes(run.freedBytes)} freed${run.cancelled ? ', stopped part way' : ''}`}
                    onClick={() => choose(run)}
                  >
                    <span className="history-when">{when}</span>
                    {run.cancelled && <span className="history-tag">Stopped</span>}
                    <span className="history-freed">
                      {opening === run.id ? (
                        <LoaderCircle size={13} className="history-spin" aria-label="Opening" />
                      ) : (
                        <>
                          <b>{formatBytes(run.freedBytes)}</b> freed
                        </>
                      )}
                    </span>
                  </button>
                );
              })}
            </div>
          ))}
          {openError && (
            <p className="history-note is-error" role="alert">
              {openError}
            </p>
          )}
          {listing.status === 'ready' && listing.history.unreadable > 0 && (
            <p className="history-foot">
              {listing.history.unreadable === 1
                ? '1 kept run couldn’t be read, so it isn’t listed.'
                : `${listing.history.unreadable} kept runs couldn’t be read, so they aren’t listed.`}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

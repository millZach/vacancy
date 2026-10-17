import { Check, ChevronDown, Lock } from 'lucide-react';
import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { formatBytes } from '../format';
import { DriveIcon, DriveStatus, DriveTags, LOCK_NOTE, UsageBar } from './bits';
import type { ReadyDrives } from './context';
import { useNow } from './useNow';

/**
 * The header's drive plate. It names the drive on screen with how full it is, and opens a menu of every drive with
 * its usage and scanned state, from any screen. Choosing a scanned drive opens its map; choosing an unscanned one
 * opens its scan screen. While a plan runs it is shut, with a lock.
 */
export function DriveMenu({ drives }: { drives: ReadyDrives }) {
  const { current, locked, list } = drives;
  const [open, setOpen] = useState(false);
  const shown = open && !locked;
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const rows = useRef<(HTMLButtonElement | null)[]>([]);
  const ids = useId();
  const now = useNow();
  const v = current.volume;
  const at = list.findIndex((d) => d.current);

  // A press anywhere outside closes it.
  useEffect(() => {
    if (!shown) return;
    const down = (e: PointerEvent) => {
      if (!(e.target instanceof Node) || !root.current?.contains(e.target)) setOpen(false);
    };
    document.addEventListener('pointerdown', down, true);
    return () => document.removeEventListener('pointerdown', down, true);
  }, [shown]);

  // Opening puts focus on the drive on screen, so the arrow keys start from there.
  useEffect(() => {
    if (shown) rows.current[at]?.focus();
  }, [shown, at]);

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!shown) return;
    const items = rows.current.filter((r): r is HTMLButtonElement => r !== null);
    const i = items.findIndex((r) => r === document.activeElement);
    const go = (to: number) => {
      e.preventDefault();
      items[(to + items.length) % items.length]?.focus();
    };
    if (e.key === 'ArrowDown') go(i + 1);
    else if (e.key === 'ArrowUp') go(i - 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(items.length - 1);
    else if (e.key === 'Tab') setOpen(false);
    else if (e.key === 'Escape') {
      // Escape closes the menu only; the map behind it doesn't zoom out too.
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      button.current?.focus();
    }
  };

  return (
    <div className="drive-menu" ref={root} onKeyDown={onKey} data-no-ripple>
      <button
        ref={button}
        type="button"
        className={shown ? 'head-disk drive-menu-button is-open' : 'head-disk drive-menu-button'}
        aria-haspopup="menu"
        aria-expanded={shown}
        disabled={locked}
        title={locked ? LOCK_NOTE : 'Switch drive'}
        onClick={() => setOpen((o) => !o)}
      >
        <DriveIcon kind={v.kind} />
        <span className="head-disk-name">{v.name}</span>
        <UsageBar volume={v} className="usage--head" />
        <span className="head-disk-nums">
          {formatBytes(v.usedBytes, 0)} of {formatBytes(v.totalBytes, 0)} used
        </span>
        <span className="head-disk-free">{formatBytes(v.freeBytes, 0)} free</span>
        {locked ? (
          <Lock size={13} className="drive-menu-mark" aria-hidden />
        ) : (
          <ChevronDown size={14} className="drive-menu-mark drive-menu-chev" aria-hidden />
        )}
      </button>
      {shown && (
        <div className="drive-menu-pop" role="menu" aria-label="Drives" data-tauri-drag-region="false">
          <div className="drive-menu-head" aria-hidden>
            Drives on {drives.machineName}
          </div>
          {list.map((d, i) => (
            <button
              key={d.volume.mountPoint}
              ref={(el) => {
                rows.current[i] = el;
              }}
              type="button"
              role="menuitemradio"
              aria-checked={d.current}
              aria-label={d.volume.name}
              aria-describedby={`${ids}-${i}`}
              tabIndex={-1}
              className={d.current ? 'drive-row is-on' : 'drive-row'}
              onClick={() => {
                setOpen(false);
                drives.switchTo(d.volume.mountPoint);
              }}
            >
              <DriveIcon kind={d.volume.kind} size={18} className="drive-row-icon" />
              <span className="drive-row-main" id={`${ids}-${i}`}>
                <span className="drive-row-name">
                  <b>{d.volume.name}</b>
                  <DriveTags volume={d.volume} system={false} />
                </span>
                <UsageBar volume={d.volume} />
                <span className="drive-row-nums">
                  {formatBytes(d.volume.usedBytes, 0)} of {formatBytes(d.volume.totalBytes, 0)} used,{' '}
                  <b>{formatBytes(d.volume.freeBytes, 0)} free</b>
                </span>
                <DriveStatus scan={d.scan} now={now} />
              </span>
              <span className="drive-row-go" aria-hidden>
                {d.current ? <Check size={15} strokeWidth={2.4} /> : d.scan.phase === 'done' ? 'Open map' : 'Scan'}
              </span>
            </button>
          ))}
          <p className="drive-menu-foot">
            {current.scan.phase === 'scanning' && (
              <>
                Switching drives stops the scan of {v.name}.
                <br />
              </>
            )}
            Internal, USB and external drives. Network and optical drives aren’t listed.
          </p>
        </div>
      )}
    </div>
  );
}

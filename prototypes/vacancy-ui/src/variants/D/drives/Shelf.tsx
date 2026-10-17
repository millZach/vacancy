// PROTOTYPE drive picker, design 3 "Drives as a shelf": a rail left of the map lists every drive at all times, most
// full first, each with a fill gauge, and points at the drive that needs cleaning most. The scan screen's plate shows
// whatever is picked on the shelf. The header's drive plate becomes a "drive ▸ folder" breadcrumb whose drive part
// opens a drive menu.
import { ArrowDownWideNarrow, Check, ChevronDown, ChevronRight, CornerRightDown, Lock } from 'lucide-react';
import { useRef, useState } from 'react';
import type { HeadCtx } from '../Atlas';
import {
  AgentNote,
  DriveIcon,
  KindTag,
  LOCK_NOTE,
  PlateActions,
  PlateFine,
  Status,
  UsageBar,
  goText,
  pct,
  sz,
  useDismiss,
  type PickerApi,
} from './common';

export function DriveShelf({ api }: { api: PickerApi }) {
  const sorted = [...api.views].sort((a, b) => b.used / b.drive.totalBytes - a.used / a.drive.totalBytes);
  return (
    <aside className="vd-shelf" aria-label="Drives">
      <div className="vd-shelf-head">
        <h2>Drives</h2>
        <span className="vd-shelf-sort">
          <ArrowDownWideNarrow size={13} strokeWidth={2} /> Most full first
        </span>
      </div>
      <ol className="vd-shelf-list">
        {sorted.map((x, i) => {
          const p = pct(x);
          const t = x.drive.totalBytes;
          const reclaim = x.scanned && !x.cleaned ? Math.min(x.reclaim, x.used) : 0;
          return (
            <li key={x.drive.id}>
              {i === 0 && (
                <div className="vd-shelf-pointer">
                  Needs cleaning most <CornerRightDown size={14} strokeWidth={2} />
                </div>
              )}
              <button
                type="button"
                className={`vd-shelf-item ${x.current ? 'is-on' : ''} ${i === 0 ? 'is-fullest' : ''} ${p >= 90 ? 'is-full' : ''}`}
                aria-current={x.current ? 'true' : undefined}
                disabled={api.locked && !x.current}
                onClick={() => api.switchTo(x.drive.id)}
              >
                <span className="vd-tank" aria-hidden>
                  <i style={{ height: `${(x.used / t) * 100}%` }} />
                  {reclaim > 0 && <b style={{ bottom: `${((x.used - reclaim) / t) * 100}%`, height: `${(reclaim / t) * 100}%` }} />}
                </span>
                <span className="vd-shelf-body">
                  <span className="vd-shelf-pct">
                    <b>{p}%</b> full
                  </span>
                  <span className="vd-shelf-name">
                    <DriveIcon kind={x.drive.kind} size={13} />
                    {x.drive.label}
                  </span>
                  <span className="vd-shelf-meta">
                    <KindTag v={x} />
                    {sz(x.free)} free of {sz(t)}
                  </span>
                  <Status v={x} now={api.now} />
                </span>
              </button>
            </li>
          );
        })}
      </ol>
      <p className="vd-shelf-foot">
        {api.locked ? (
          <>
            <Lock size={11} /> {LOCK_NOTE}
          </>
        ) : (
          'Plug in a USB or external drive and it shows up here. Network and optical drives are not listed.'
        )}
      </p>
    </aside>
  );
}

/** The scan screen's plate: D's single plate, showing the drive picked on the shelf. */
export function ShelfPlate({ api, scan }: { api: PickerApi; scan: (b: HTMLButtonElement) => void }) {
  const v = api.cur;
  const d = v.drive;
  return (
    <div className="va-plate vd-splate">
      <DriveIcon kind={d.kind} size={26} className="va-plate-icon" />
      <h1>{d.label}</h1>
      <p className="va-plate-cap">
        {sz(d.totalBytes)} {d.media} on {api.machine}, <b className={pct(v) >= 90 ? 'vd-warm' : ''}>{pct(v)}% full</b>, {sz(v.free)} free
      </p>
      {v.scanned && (
        <p className="vd-flip-status">
          <Status v={v} now={api.now} />
        </p>
      )}
      <PlateActions api={api} scan={scan} />
      <AgentNote agent={api.agent} />
      <PlateFine v={v} />
    </div>
  );
}

export function DriveCrumb({ api, ctx }: { api: PickerApi; ctx: HeadCtx }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(ref, open, () => setOpen(false));
  const v = api.cur;
  const here =
    ctx.mode === 'scan' ? (ctx.scanning ? 'Mapping as it scans' : v.scanned ? 'Scan screen' : 'Not scanned yet') : ctx.mode === 'summary' ? 'What changed' : 'All folders';
  return (
    <nav className="vd-crumb" aria-label="Location">
      <div className="vd-crumb-drive-wrap" ref={ref}>
        <button
          type="button"
          className={`vd-crumb-drive ${open ? 'is-open' : ''}`}
          aria-haspopup="menu"
          aria-expanded={open}
          disabled={api.locked}
          title={api.locked ? LOCK_NOTE : 'Switch drive'}
          onClick={() => setOpen(!open)}
        >
          <DriveIcon kind={v.drive.kind} />
          <b>{v.drive.label}</b>
          {api.locked ? <Lock size={12} className="vd-lock" /> : <ChevronDown size={13} className="vd-chev" />}
        </button>
        {open && (
          <div className="vd-pop vd-crumb-pop" role="menu" aria-label="Drives">
            {api.views.map((x) => (
              <button
                key={x.drive.id}
                type="button"
                role="menuitemradio"
                aria-checked={x.current}
                className={`vd-crumb-row ${x.current ? 'is-on' : ''}`}
                onClick={() => {
                  setOpen(false);
                  api.switchTo(x.drive.id);
                }}
              >
                <DriveIcon kind={x.drive.kind} size={15} />
                <span className="vd-crumb-row-main">
                  <b>{x.drive.label}</b>
                  <Status v={x} now={api.now} />
                </span>
                <span className="vd-crumb-row-pct">{pct(x)}%</span>
                <UsageBar v={x} className="vd-use--chip" />
                <span className="vd-dm-go">{x.current ? <Check size={14} strokeWidth={2.4} /> : goText(x)}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      <ChevronRight size={14} className="vd-crumb-sep" />
      {ctx.focusRegion ? (
        <>
          <button type="button" className="vd-crumb-up" onClick={ctx.clearFocus}>
            All folders
          </button>
          <ChevronRight size={14} className="vd-crumb-sep" />
          <span className="vd-crumb-here">{ctx.focusRegion.name}</span>
          <span className="vd-crumb-blurb">{ctx.focusRegion.blurb}</span>
        </>
      ) : (
        <span className={`vd-crumb-here ${ctx.mode === 'scan' ? 'is-muted' : ''}`}>{here}</span>
      )}
    </nav>
  );
}

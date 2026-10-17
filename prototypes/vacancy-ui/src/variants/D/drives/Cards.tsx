// PROTOTYPE drive picker, design 1 "Drive cards": the scan screen shows every drive as a card and the lit card is
// the one the big Scan button scans. The header's drive plate opens a dropdown of drives with usage and scanned state.
import { Check, ChevronDown, Lock } from 'lucide-react';
import { useRef, useState } from 'react';
import type { HeadCtx } from '../Atlas';
import { HeadMeter } from '../Atlas';
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
  sz,
  useDismiss,
  type DriveView,
  type PickerApi,
} from './common';

export function CardsPlate({ api, scan }: { api: PickerApi; scan: (b: HTMLButtonElement) => void }) {
  return (
    <div className="va-plate vd-cards-plate">
      <h1 className="vd-ask">Which drive should Vacancy scan?</h1>
      <p className="va-plate-cap">
        {api.views.length} drives on {api.machine}
      </p>
      <div className="vd-cards" role="radiogroup" aria-label="Drives" data-pick>
        {api.views.map((v) => (
          <DriveCard key={v.drive.id} v={v} now={api.now} onPick={() => api.pick(v.drive.id)} />
        ))}
      </div>
      <PlateActions api={api} scan={scan} />
      <AgentNote agent={api.agent} />
      <PlateFine v={api.cur} />
    </div>
  );
}

function DriveCard({ v, now, onPick }: { v: DriveView; now: number; onPick: () => void }) {
  const d = v.drive;
  return (
    <button type="button" role="radio" aria-checked={v.current} className={`vd-card ${v.current ? 'is-on' : ''}`} onClick={onPick}>
      <span className="vd-card-top">
        <DriveIcon kind={d.kind} size={20} className="vd-card-icon" />
        <span className="vd-card-tags">
          <KindTag v={v} />
          {d.system && <span className="vd-kind">System</span>}
        </span>
        {v.current && (
          <span className="vd-card-tick">
            <Check size={11} strokeWidth={3} />
          </span>
        )}
      </span>
      <span className="vd-card-name">{d.label}</span>
      <UsageBar v={v} />
      <span className="vd-card-nums">
        <b>{sz(v.free)} free</b> of {sz(d.totalBytes)}
      </span>
      <Status v={v} now={now} />
    </button>
  );
}

export function DriveMenu({ api, ctx }: { api: PickerApi; ctx: HeadCtx }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(ref, open, () => setOpen(false));
  const v = api.cur;
  return (
    <div className="vd-dm" ref={ref}>
      <button
        type="button"
        className={`va-head-disk vd-dm-btn ${open ? 'is-open' : ''}`}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={api.locked}
        title={api.locked ? LOCK_NOTE : 'Switch drive'}
        onClick={() => setOpen(!open)}
      >
        <DriveIcon kind={v.drive.kind} />
        <span className="va-head-name">{v.drive.label}</span>
        <HeadMeter model={ctx.model} mode={ctx.mode} used={v.used} />
        <span className="va-head-nums">
          {sz(v.used)} of {sz(v.drive.totalBytes)} used
        </span>
        {api.locked ? <Lock size={13} className="vd-lock" /> : <ChevronDown size={14} className="vd-chev" />}
      </button>
      {open && (
        <div className="vd-pop vd-dm-pop" role="menu" aria-label="Drives">
          <div className="vd-pop-head">Drives on {api.machine}</div>
          {api.views.map((x) => (
            <button
              key={x.drive.id}
              type="button"
              role="menuitemradio"
              aria-checked={x.current}
              className={`vd-dm-row ${x.current ? 'is-on' : ''}`}
              onClick={() => {
                setOpen(false);
                api.switchTo(x.drive.id);
              }}
            >
              <DriveIcon kind={x.drive.kind} size={18} className="vd-dm-icon" />
              <span className="vd-dm-main">
                <span className="vd-dm-name">
                  <b>{x.drive.label}</b>
                  <KindTag v={x} />
                </span>
                <UsageBar v={x} />
                <span className="vd-dm-nums">
                  {sz(x.used)} of {sz(x.drive.totalBytes)} used, <b>{sz(x.free)} free</b>
                </span>
                <Status v={x} now={api.now} />
              </span>
              <span className="vd-dm-go">{x.current ? <Check size={15} strokeWidth={2.4} /> : goText(x)}</span>
            </button>
          ))}
          <p className="vd-pop-foot">Internal, USB and external drives. Network and optical drives aren't listed.</p>
        </div>
      )}
    </div>
  );
}

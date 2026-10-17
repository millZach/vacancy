// PROTOTYPE drive picker, design 2 "The plate is the picker": the scan screen keeps D's single big plate and you flip
// it between drives with letter tabs under it (the arrow keys belong to the prototype switcher). The header's drive
// plate becomes a segmented control of compact drive chips, always visible; hovering one shows its details.
import { useState } from 'react';
import {
  AgentNote,
  DriveIcon,
  KindTag,
  LOCK_NOTE,
  PlateActions,
  PlateFine,
  Status,
  UsageBar,
  pct,
  statusText,
  sz,
  type PickerApi,
} from './common';

export function FlipPlate({ api, scan }: { api: PickerApi; scan: (b: HTMLButtonElement) => void }) {
  const v = api.cur;
  const d = v.drive;
  const idx = api.views.findIndex((x) => x.current);
  const [seen, setSeen] = useState({ idx, dir: 0 });
  if (seen.idx !== idx) setSeen({ idx, dir: idx > seen.idx ? 1 : -1 });
  const dir = seen.idx !== idx ? (idx > seen.idx ? 1 : -1) : seen.dir;
  // Keyed per drive so each flip re-runs the slide-in, from the side the new drive sits on.
  const k = (s: string) => `${s}-${d.id}`;
  const flip = dir ? `vd-flip-in ${dir > 0 ? 'from-right' : 'from-left'}` : '';
  return (
    <div className="va-plate vd-flip">
      <DriveIcon key={k('i')} kind={d.kind} size={26} className={`va-plate-icon ${flip}`} />
      <h1 key={k('h')} className={flip}>
        {d.label}
      </h1>
      <p key={k('c')} className={`va-plate-cap ${flip}`}>
        {sz(d.totalBytes)} {d.media} on {api.machine}, {sz(v.used)} used, <b className={v.used / d.totalBytes >= 0.9 ? 'vd-warm' : ''}>{sz(v.free)} free</b>
      </p>
      <UsageBar key={k('u')} v={v} className={`vd-flip-bar ${flip}`} />
      {v.scanned && (
        <p key={k('s')} className={`vd-flip-status ${flip}`}>
          <Status v={v} now={api.now} />
        </p>
      )}
      <div className="vd-flip-tabs" role="tablist" aria-label="Drives" data-pick>
        {api.views.map((x) => (
          <button
            key={x.drive.id}
            type="button"
            role="tab"
            aria-selected={x.current}
            className={`vd-flip-tab ${x.current ? 'is-on' : ''} ${x.scanned ? 'is-scanned' : ''}`}
            title={`${x.drive.label}: ${statusText(x, api.now)}`}
            onClick={() => api.pick(x.drive.id)}
          >
            <span className="vd-flip-letter">{x.drive.short}</span>
            <UsageBar v={x} className="vd-use--mini" />
            <i className="vd-flip-dot" aria-hidden />
          </button>
        ))}
      </div>
      <PlateActions api={api} scan={scan} />
      <AgentNote agent={api.agent} />
      <PlateFine v={v} />
    </div>
  );
}

export function DriveChips({ api }: { api: PickerApi }) {
  return (
    <div className="vd-chips" role="tablist" aria-label="Drives">
      {api.views.map((x) => (
        <div key={x.drive.id} className="vd-chip-wrap">
          <button
            type="button"
            role="tab"
            aria-selected={x.current}
            className={`vd-chip ${x.current ? 'is-on' : ''} ${x.scanned ? 'is-scanned' : ''}`}
            disabled={api.locked && !x.current}
            onClick={() => api.switchTo(x.drive.id)}
          >
            <DriveIcon kind={x.drive.kind} size={13} />
            <b>{x.drive.short}</b>
            <UsageBar v={x} className="vd-use--chip" />
            <span className="vd-chip-free">{sz(x.free)} free</span>
            <i className="vd-chip-dot" aria-label={x.scanned ? 'Scanned' : 'Not scanned'} />
          </button>
          <div className="vd-pop vd-chip-card" role="tooltip">
            <div className="vd-chip-card-name">
              <DriveIcon kind={x.drive.kind} size={16} />
              <b>{x.drive.label}</b>
              <KindTag v={x} />
            </div>
            <UsageBar v={x} />
            <dl className="vd-chip-card-nums">
              <div>
                <dt>Capacity</dt>
                <dd>{sz(x.drive.totalBytes)}</dd>
              </div>
              <div>
                <dt>Used</dt>
                <dd>
                  {sz(x.used)} <em>{pct(x)}%</em>
                </dd>
              </div>
              <div>
                <dt>Free</dt>
                <dd>{sz(x.free)}</dd>
              </div>
            </dl>
            <Status v={x} now={api.now} />
            <p className="vd-chip-card-go">
              {api.locked && !x.current
                ? LOCK_NOTE
                : x.current
                  ? 'You are here.'
                  : x.scanned
                    ? 'Click to go back to its map. No rescan.'
                    : 'Click to open its scan screen.'}
            </p>
          </div>
        </div>
      ))}
    </div>
  );
}

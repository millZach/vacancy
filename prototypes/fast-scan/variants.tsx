// THROWAWAY: three ways to offer "Fast scan (asks for admin)" on Windows. Each draws its own plate and HUD; the map
// with its Locked folders block, the prompt stand-in and the flow are shared (common.tsx).
import { Info, Map as MapIcon, ScanLine, ShieldCheck, Zap } from 'lucide-react';
import type { ReactNode } from 'react';
import type { VolumeInfo } from '../../src/bridge';
import { fixtures } from '../../src/bridge/fixtures';
import { ScanPlate } from '../../src/scan/ScanPlate';
import { AdvisorNote } from '../../src/advisor/Advisor';
import { ADMIN_LINE, DriveCards, Hud, HudNote, noteText, PlateFrame, readyDrives, ScanScreen, type FlowApi, type PlateProps } from './common';
import { ESTIMATE, FILE_SYSTEM, isNtfs } from './data';

export function Advisor({ api }: { api: FlowApi }) {
  return <AdvisorNote state={{ phase: 'ready', status: fixtures[api.flow.platform].advisorStatus }} onOpen={() => {}} />;
}

/** The plate's buttons when the drive has a map already: open it, or scan again. */
function OpenMap({ api }: { api: FlowApi }) {
  return (
    <button type="button" className="cta cta--lg" onClick={api.openMap}>
      <MapIcon size={16} aria-hidden /> Open its map
    </button>
  );
}

// ---------- A: a second button beside Scan ----------

function PlateA({ api, calmRef, onScan }: PlateProps) {
  const v = api.volume;
  const est = ESTIMATE[v.mountPoint]!;
  const done = !!api.flow.done[v.mountPoint];
  return (
    <PlateFrame api={api} calmRef={calmRef} after={<Advisor api={api} />}>
      <div className="plate-actions">
        {done && <OpenMap api={api} />}
        <button type="button" className={done ? 'btn btn--lg' : 'cta cta--lg'} onClick={(e) => onScan(e.currentTarget, 'normal')}>
          <ScanLine size={done ? 14 : 16} aria-hidden /> {done ? 'Scan again' : `Scan ${v.name}`}
        </button>
        <button type="button" className="btn btn--lg fast-btn" onClick={(e) => onScan(e.currentTarget, 'fast')}>
          <Zap size={14} aria-hidden /> Fast scan <span className="fast-btn-ask">(asks for admin)</span>
        </button>
      </div>
      <p className="fast-fine">
        {est.fast ? <>Fast scan reads the drive’s file index directly: {est.fast} instead of {est.normal}. </> : <>Fast scan reads an NTFS drive’s file index directly. </>}
        {ADMIN_LINE}
      </p>
    </PlateFrame>
  );
}

function HudA({ api }: { api: FlowApi }) {
  const note = noteText(api.flow.note, api.volume);
  return <Hud api={api}>{note && <HudNote>{note}</HudNote>}</Hud>;
}

export function VariantA({ api }: { api: FlowApi }) {
  return <ScanScreen api={api} plate={(p) => <PlateA {...p} />} hud={<HudA api={api} />} />;
}

// ---------- B: the scan mode lives in the drive picker, with each drive's time ----------

function Estimates({ v, choice }: { v: VolumeInfo; choice: 'normal' | 'fast' }) {
  const est = ESTIMATE[v.mountPoint]!;
  return (
    <span className="est" aria-label={`Normal scan ${est.normal}. Fast scan ${est.fast ?? `not on ${FILE_SYSTEM[v.mountPoint]}`}.`}>
      <span className={choice === 'normal' ? 'est-row is-on' : 'est-row'}>
        <ScanLine size={11} aria-hidden /> Normal <b>{est.normal}</b>
      </span>
      <span className={['est-row', choice === 'fast' && 'is-on', !est.fast && 'is-off'].filter(Boolean).join(' ')}>
        <Zap size={11} aria-hidden /> Fast {est.fast ? <b>{est.fast}</b> : <em>not on {FILE_SYSTEM[v.mountPoint]}</em>}
      </span>
    </span>
  );
}

function PlateB({ api, calmRef, onScan }: PlateProps) {
  const v = api.volume;
  const { choice } = api.flow;
  const done = !!api.flow.done[v.mountPoint];
  const fast = choice === 'fast' && isNtfs(v.mountPoint);
  const label = fast ? `Fast scan ${v.name}` : `Scan ${v.name}`;
  const mode = (
    <div className="mode-seg" role="radiogroup" aria-label="How to scan" data-no-ripple>
      <button type="button" role="radio" aria-checked={choice === 'normal'} className={choice === 'normal' ? 'is-on' : ''} onClick={() => api.set({ choice: 'normal' })}>
        <ScanLine size={13} aria-hidden /> Normal scan
      </button>
      <button type="button" role="radio" aria-checked={choice === 'fast'} className={choice === 'fast' ? 'is-on' : ''} onClick={() => api.set({ choice: 'fast' })}>
        <Zap size={13} aria-hidden /> Fast scan <span className="mode-ask">asks for admin</span>
      </button>
    </div>
  );
  return (
    <PlateFrame api={api} calmRef={calmRef} above={mode} cards={<DriveCards api={api} foot={(d) => <Estimates v={d} choice={choice} />} />}
      after={choice === 'normal' ? <Advisor api={api} /> : null}>
      <div className="plate-actions">
        {done && <OpenMap api={api} />}
        <button type="button" className={done ? 'btn btn--lg' : 'cta cta--lg'} onClick={(e) => onScan(e.currentTarget, choice)}>
          {fast ? <Zap size={done ? 14 : 16} aria-hidden /> : <ScanLine size={done ? 14 : 16} aria-hidden />} {done ? (fast ? 'Fast scan again' : 'Scan again') : label}
        </button>
      </div>
      {choice === 'fast' && (
        <div className={fast ? "agent-note fast-note" : "agent-note fast-note is-plain"} data-no-ripple>
          {fast ? <ShieldCheck size={14} aria-hidden /> : <Info size={14} aria-hidden />}
          {fast ? (
            <span>
              <b>Windows will ask for admin, for this scan only.</b> Vacancy’s scan helper uses it to read the drive’s file
              index and nothing else. Deleting still happens normally, with the same checks. Folders your account can’t
              open show as totals only.
            </span>
          ) : (
            <span>
              <b>{v.name} gets the normal scan.</b> It’s {FILE_SYSTEM[v.mountPoint]}, and fast scan only reads NTFS
              drives. Nothing to approve.
            </span>
          )}
        </div>
      )}
    </PlateFrame>
  );
}

export function VariantB({ api }: { api: FlowApi }) {
  return <ScanScreen api={api} plate={(p) => <PlateB {...p} />} hud={<HudA api={api} />} />;
}

// ---------- C: scan as today; the HUD offers to switch ----------

function HudC({ api }: { api: FlowApi }) {
  const { flow, volume } = api;
  const est = ESTIMATE[volume.mountPoint]!;
  let row: ReactNode;
  if (!isNtfs(volume.mountPoint)) row = <HudNote tone="quiet">{noteText('notNtfs', volume)}</HudNote>;
  else if (flow.offer === 'switched') row = <HudNote tone="safe"><b>Started over as a fast scan.</b> Admin is used only to read the file index, and is given back when it’s read.</HudNote>;
  else if (flow.offer === 'declined')
    row = <HudNote><b>The normal scan carries on.</b> Windows wasn’t given admin, so Vacancy can’t read the file index. Nothing scanned so far was lost.</HudNote>;
  else
    row = (
      <div className="hud-offer">
        <Zap size={14} aria-hidden />
        <p>
          <b>This drive takes {est.normal}.</b> Fast scan reads its file index instead: {est.fast}.
          <span>{ADMIN_LINE}</span>
        </p>
        <button type="button" className="btn btn--sm" disabled={flow.uac !== null} onClick={api.switchToFast}>Switch to fast scan</button>
      </div>
    );
  return <Hud api={api}>{row}</Hud>;
}

export function VariantC({ api }: { api: FlowApi }) {
  const drives = readyDrives(api);
  return (
    <ScanScreen
      api={api}
      plate={({ calmRef, onScan }) => (
        <ScanPlate drives={drives} onOpenFullDiskAccess={() => {}} advisorNote={<Advisor api={api} />} failure={null}
          onScan={(b) => onScan(b, 'normal')} contentRef={calmRef} />
      )}
      hud={<HudC api={api} />}
    />
  );
}

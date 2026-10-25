// PROTOTYPE (throwaway): Claude before a scan. Variant A docks the panel beside the scan screen; variant C shows a
// ghosted rail where the panel will be; each variant has its own scan plate note.
import { ArrowLeft, PanelLeftClose, PanelLeftOpen, ScanLine, Send, TriangleAlert } from 'lucide-react';
import { useBridge } from '../../bridge';
import { ClaudeNote, Mark, PrivacyRow } from '../bits';
import { PanelOff } from '../ClaudePanel';
import { checkShown, isOn, readsFiles, useClaude, type Claude } from '../context';
import type { ClaudeStatus } from '../../bridge';
import { ProtoHeadLine, ProtoModelPicker } from './ProtoModel';
import { proto, useProto } from './protoState';
import './proto.css';

const CHIPS = ['Review my plan', 'Let Claude pick', 'Explain big folders'];

/** Variant A: the docked Claude panel beside the scan screen, before there's a map. */
export function ProtoPrescanPanel() {
  const claude = useClaude();
  const s = useProto();
  if (!claude || s.variant !== 'A') return null;
  const status = claude.status.phase === 'ready' ? claude.status.status : null;
  if (claude.folded)
    return (
      <aside className="cp is-folded" aria-label="Claude" data-no-ripple>
        <button
          type="button"
          className="cp-rail"
          onClick={() => claude.setFolded(false)}
          aria-label="Show Claude"
          title={status?.state === 'on' ? 'Show Claude' : 'Claude is off. Show Claude to turn it on.'}
        >
          <PanelLeftOpen size={16} aria-hidden />
          <Mark size={16} />
          <span>{status?.state === 'on' ? 'Claude' : 'Claude · off'}</span>
        </button>
      </aside>
    );
  return (
    <aside className="cp" aria-label="Claude" data-no-ripple>
      <div className="cp-head">
        <div>
          <h2>
            <Mark size={15} /> Claude
          </h2>
          <p>
            <ProtoHeadLine line={!status ? 'Checking' : status.state === 'on' ? 'On · through your Claude Code' : status.state === 'off' ? 'Off' : 'Not ready'} />
          </p>
        </div>
        <button type="button" className="icon-btn" onClick={() => claude.setFolded(true)} aria-label="Fold the Claude panel">
          <PanelLeftClose size={16} aria-hidden />
        </button>
      </div>
      {claude.status.phase === 'loading' && <p className="cp-wait">Looking for Claude Code…</p>}
      {status && status.state !== 'on' && <PanelOff claude={claude} status={status} />}
      {status && status.state === 'on' && <PrescanOn claude={claude} status={status} />}
    </aside>
  );
}

/** Claude on, no scan yet: its settings work; its jobs wait for a scan. */
function PrescanOn({ claude, status }: { claude: Claude; status: ClaudeStatus }) {
  const bridge = useBridge();
  const chips = bridge.platform === 'windows' ? [...CHIPS, 'Suggest Tidy up layouts'] : CHIPS;
  return (
    <>
      <div className="cp-status">
        <PrivacyRow
          claude={claude}
          privacy={status.privacy}
          fixture={status.fixture}
          unavailable={status.privacy ? null : status.readsUnavailable}
          check={checkShown(status, claude.verifying)}
        />
        <ProtoModelPicker />
        <button type="button" className="link-button cp-off" onClick={claude.turnOff}>
          Turn off Claude
        </button>
        {claude.settingError && (
          <p className="cl-job is-error" role="alert">
            <TriangleAlert size={13} aria-hidden /> <span>{claude.settingError}</span>
          </p>
        )}
      </div>
      <div className="cp-log" aria-label="Claude’s answers">
        <div className="cp-msg is-claude">
          <p>
            <Mark size={12} />{' '}
            {readsFiles(status)
              ? 'Once you scan, I can see the scan, and open files on the drive you scanned, except the places that always stay hidden.'
              : 'Once you scan, I can see the names, sizes, counts and dates from the scan. Nothing inside your files.'}{' '}
            I only propose: nothing changes until you confirm.
          </p>
        </div>
        <div className="proto-wait">
          <ScanLine size={16} aria-hidden />
          <span>
            <b>Scan a drive first.</b> Claude works from the scan, so its jobs start once the map is up.
          </span>
        </div>
      </div>
      <div className="cp-foot">
        <div className="cp-acts">
          {chips.map((c) => (
            <button key={c} type="button" className="cl-chip" disabled title="Scan a drive first">
              {c}
            </button>
          ))}
        </div>
        <p className="proto-foot-note">Scan a drive first — Claude works from the scan.</p>
        <form className="cl-compose" onSubmit={(e) => e.preventDefault()}>
          <input disabled placeholder="Ask Claude, after the scan" aria-label="Ask Claude" />
          <button type="submit" className="icon-btn" aria-label="Send" disabled>
            <Send size={14} aria-hidden />
          </button>
        </form>
      </div>
    </>
  );
}

/** Variant C: a ghosted rail on the left edge, where Claude's panel will be once there's a map. */
export function ProtoGhostRail() {
  const s = useProto();
  if (s.variant !== 'C') return null;
  return (
    <aside key={s.hint} className={s.hint > 0 ? 'cp is-folded proto-ghost is-flash' : 'cp is-folded proto-ghost'} data-no-ripple aria-label="Where Claude will be">
      <button type="button" className="cp-rail" onClick={() => proto.flashHint()}title="Claude’s panel opens here once a drive is scanned.">
        <Mark size={16} />
        <span>Claude · opens here after the scan</span>
      </button>
    </aside>
  );
}

/** The scan plate's Claude row, per variant. Variants fall back to today's note for the states they don't change. */
export function ProtoNote() {
  const claude = useClaude();
  const s = useProto();
  const status = claude?.status.phase === 'ready' ? claude.status.status : null;
  const on = isOn(claude);
  if (!status || (status.state !== 'off' && status.state !== 'on')) return <ClaudeNote />;
  const privacy = status.privacy
    ? 'Privacy mode is on: Claude gets names, sizes, counts and dates only.'
    : 'Privacy mode is off: Claude can read files on the drive you scan, except hidden places.';
  if (s.variant === 'A')
    return (
      <div className="advisor-note" data-no-ripple>
        {on ? <Mark size={15} /> : <ArrowLeft size={15} aria-hidden />}
        <span>
          <b>{on ? 'Built-in rules, and Claude in the panel on the left.' : 'Built-in rules. Claude is off.'}</b>
          <span>{on ? `${privacy} Its jobs start once the scan is done.` : 'Turn it on in the Claude panel on the left, now or after the scan.'}</span>
        </span>
        {!on && claude?.folded && (
          <button type="button" className="btn btn--sm proto-note-act" onClick={() => claude.setFolded(false)}>
            Show Claude
          </button>
        )}
      </div>
    );
  if (s.variant === 'B')
    return (
      <div className="advisor-note" data-no-ripple>
        <Mark size={15} />
        <span>
          <b>{on ? 'Built-in rules, and Claude beside the map.' : 'Built-in rules. Claude is off.'}</b>
          <span>{on ? `${privacy} Its panel opens beside the map after you scan.` : 'Set it up now from the Claude button at the top right. Its panel opens beside the map after you scan.'}</span>
        </span>
        <button type="button" className="btn btn--sm proto-note-act" onClick={() => proto.setCard(true)}>
          {on ? 'Claude settings' : 'Set up Claude'}
        </button>
      </div>
    );
  // C
  return (
    <div key={s.hint} className={s.hint > 0 ? 'advisor-note proto-c-row is-flash' : 'advisor-note proto-c-row'} data-no-ripple>
      <Mark size={15} />
      <span>
        <b>{on ? 'Claude: on. It opens beside the map after the scan.' : 'Claude: scan a drive, then turn it on beside the map.'}</b>
        <span>
          {on
            ? privacy
            : 'Claude works from the scan, so its panel appears on the left once the map is up. Built-in rules work either way.'}
        </span>
      </span>
      <span className="proto-c-where" aria-hidden>
        <ArrowLeft size={13} /> panel goes here
      </span>
    </div>
  );
}


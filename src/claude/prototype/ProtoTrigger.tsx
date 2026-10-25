// PROTOTYPE (throwaway): the header's Claude control before a scan, per variant. On the map it is today's control.
// A: a button that unfolds the docked panel. B: a button that opens a turn-on card under it. C: "Claude · after scan",
// which flashes the plate row and the ghosted rail and says where Claude will be.
import { ChevronDown, LoaderCircle, Sparkles, X } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import { ClaudeTrigger, HiddenFold, Mark, PrivacyRow } from '../bits';
import { PanelOff } from '../ClaudePanel';
import { checkShown, isOn, readsFiles, useClaude, type Claude } from '../context';
import type { ClaudeStatus } from '../../bridge';
import { ProtoModelPicker } from './ProtoModel';
import { proto, useProto } from './protoState';
import './proto.css';

interface Props {
  busy: boolean;
  failed: boolean;
  closed: boolean;
  /** Unfolds the panel beside the map; null before there's one. */
  onOpen: (() => void) | null;
}

export function ProtoTrigger(props: Props) {
  const claude = useClaude();
  const s = useProto();
  const button = useRef<HTMLButtonElement>(null);
  const [bubble, setBubble] = useState(false);
  const onMap = props.onOpen !== null;
  useEffect(() => {
    if (onMap) proto.setCard(false);
  }, [onMap]);
  useEffect(() => {
    if (!bubble) return;
    const t = window.setTimeout(() => setBubble(false), 4000);
    return () => window.clearTimeout(t);
  }, [bubble, s.hint]);
  if (!claude || onMap) return <ClaudeTrigger {...props} />;
  const status = claude.status.phase === 'ready' ? claude.status.status : null;
  const on = isOn(claude);
  const label = !status
    ? 'Claude'
    : s.variant === 'C'
      ? on
        ? 'Claude on · after scan'
        : 'Claude · after scan'
      : status.state === 'notFound'
        ? 'Claude · not found'
        : status.state === 'off'
          ? 'Claude · off'
          : on
            ? readsFiles(status)
              ? 'Claude on · reads files'
              : 'Claude on'
            : 'Claude · needs a look';
  const cls = on ? 'pill advisor-trigger cl-trigger' : 'pill advisor-trigger';
  const icon = claude.turningOn ? <LoaderCircle size={12} className="tray-spin" aria-hidden /> : on ? <Mark size={12} /> : <Sparkles size={12} strokeWidth={2.2} aria-hidden />;
  const click = () => {
    if (s.variant === 'A') claude.setFolded(false);
    else if (s.variant === 'B') proto.setCard(!s.card);
    else {
      proto.flashHint();
      setBubble(true);
    }
  };
  return (
    <>
      <button
        ref={button}
        type="button"
        className={cls}
        onClick={click}
        aria-expanded={s.variant === 'B' ? s.card : undefined}
        aria-label={s.variant === 'C' ? `${label}: where is Claude?` : `${label}: Claude settings`}
      >
        {icon} {label}
        <ChevronDown size={12} aria-hidden />
      </button>
      {s.variant === 'B' && s.card && status && <Floating anchor={button} onClose={() => proto.setCard(false)} claude={claude} wide>
        <Card claude={claude} status={status} />
      </Floating>}
      {s.variant === 'C' && bubble && (
        <Floating anchor={button} onClose={() => setBubble(false)} claude={claude}>
          <div className="proto-bubble">
            <Mark size={13} />
            <span>
              <b>Claude opens beside the map, after the scan.</b> Scan a drive and its panel appears on the left, where it
              can see the scan. Built-in rules work without it.
            </span>
          </div>
        </Floating>
      )}
    </>
  );
}

/** A card portalled under the header button; closes on Escape or a click outside (not while the privacy confirm is up). */
function Floating({
  anchor,
  onClose,
  claude,
  wide = false,
  children,
}: {
  anchor: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
  claude: Claude;
  wide?: boolean;
  children: ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);
  useLayoutEffect(() => {
    const place = () => {
      const r = anchor.current?.getBoundingClientRect();
      if (r) setPos({ top: r.bottom + 8, right: Math.max(12, window.innerWidth - r.right) });
    };
    place();
    window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  }, [anchor]);
  const asking = claude.privacyAsk;
  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (asking) return;
      const t = e.target as Node;
      if (box.current?.contains(t) || anchor.current?.contains(t)) return;
      if ((t as HTMLElement).closest?.('.proto-switch')) return;
      onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (!asking && e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', down);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('mousedown', down);
      window.removeEventListener('keydown', key);
    };
  }, [anchor, onClose, asking]);
  if (!pos) return null;
  return createPortal(
    <div ref={box} className={wide ? 'proto-float is-wide' : 'proto-float'} style={{ top: pos.top, right: pos.right }} data-no-ripple>
      {children}
    </div>,
    document.body,
  );
}

/** Variant B's card: Claude Code found or off, the privacy notice in short, Turn on, privacy mode, and where the panel goes. */
function Card({ claude, status }: { claude: Claude; status: ClaudeStatus }) {
  const on = status.state === 'on';
  return (
    <div className="proto-card" role="dialog" aria-label="Claude">
      <div className="proto-card-head">
        <h2>
          <Mark size={15} /> Claude
        </h2>
        <button type="button" className="icon-btn" onClick={() => proto.setCard(false)} aria-label="Close">
          <X size={15} aria-hidden />
        </button>
      </div>
      {status.state !== 'on' && status.state !== 'off' ? (
        <div className="proto-card-off">
          <PanelOff claude={claude} status={status} />
        </div>
      ) : (
        <div className="proto-card-body">
          <div className="cl-provider proto-card-provider">
            <span className="cl-provider-icon" aria-hidden>
              ✳
            </span>
            <div>
              <b>Claude Code</b>
              <span>
                {claude.turningOn ? 'Checking that Claude Code answers…' : on ? 'Found on this PC · on' : 'Found on this PC · off'}
              </span>
            </div>
          </div>
          <ul className="proto-short">
            <li>
              <b>Sees</b> {status.privacy ? 'names, sizes, counts and dates from the scan. Never what’s inside your files.' : 'the scan, and files on the drive you scan, except places that always stay hidden.'}
            </li>
            <li>
              <b>Goes</b> to Anthropic, through your own Claude Code.
            </li>
            <li>
              <b>Does</b> propose only. Nothing runs until you confirm.
            </li>
          </ul>
          <PrivacyRow
            claude={claude}
            privacy={status.privacy}
            fixture={status.fixture}
            unavailable={status.privacy ? null : status.readsUnavailable}
            check={checkShown(status, claude.verifying)}
          />
          {on && <ProtoModelPicker />}
          {!on &&
            (claude.turningOn ? (
              <p className="cl-job cp-checking" role="status">
                <LoaderCircle size={13} className="tray-spin" aria-hidden />
                <span>Checking that Claude Code answers…</span>
                <button type="button" className="link-button" onClick={claude.cancelTurnOn}>
                  Cancel
                </button>
              </p>
            ) : (
              <button type="button" className="cta cp-on" onClick={claude.turnOn}>
                <Mark size={13} /> Turn on Claude
              </button>
            ))}
          {on && (
            <button type="button" className="link-button cp-off" onClick={claude.turnOff}>
              Turn off Claude
            </button>
          )}
          <HiddenFold hidden={status.hidden} />
        </div>
      )}
      <p className="proto-card-foot">
        <Mark size={11} /> Claude’s panel opens beside the map after you scan.
      </p>
    </div>
  );
}

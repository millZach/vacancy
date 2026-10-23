import { ChevronDown, Eye, EyeOff, LoaderCircle, Lock, ShieldX, Sparkles, TriangleAlert, X } from 'lucide-react';
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { ClaudeBasis, ClaudeDropped, ClaudeFailure, ClaudeFolderNote, ClaudeHidden, Proposal } from '../bridge';
import { formatBytes } from '../format';
import { failureText, isOn, useClaude, type Claude, type Phase } from './context';
import './claude.css';

/** Claude's mark, in its lilac. */
export function Mark({ size = 13 }: { size?: number }) {
  return (
    <span className="cl-mark" style={{ fontSize: size }} aria-hidden>
      ✳
    </span>
  );
}

const PRIVACY_ON: [string, string][] = [
  ['What Claude sees', 'Folder and file names, sizes, counts and dates, from this scan.'],
  ['Where it goes', 'To Anthropic, through your own Claude Code and its account.'],
  ['Never', 'The contents of any file.'],
  ['What it can do', 'Propose only. Its suggestions pass the Guard and your review, and nothing runs until you confirm.'],
];
const PRIVACY_OFF: [string, string][] = [
  ['What Claude sees', 'The scan’s names, sizes, counts and dates, and what’s inside files it opens on this PC.'],
  ['Where it goes', 'To Anthropic, through your own Claude Code and its account, including the contents of files it reads.'],
  ['Never', 'The places that always stay hidden. It can’t write, delete, run commands or go online.'],
  ['What it can do', 'Propose only. Its suggestions pass the Guard and your review, and nothing runs until you confirm.'],
];

/** The privacy notice: what Claude sees, where it goes, what it never gets and what it can do, for a privacy mode. */
export function PrivacyNotice({ privacy, compact = false }: { privacy: boolean; compact?: boolean }) {
  return (
    <dl className={compact ? 'cl-privacy is-compact' : 'cl-privacy'}>
      {(privacy ? PRIVACY_ON : PRIVACY_OFF).map(([k, v]) => (
        <div key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

/** What Claude can never read, even with privacy mode off, as the core lists it. */
export function HiddenList({ hidden }: { hidden: ClaudeHidden[] }) {
  return (
    <ul className="cl-hidden" aria-label="Always hidden from Claude">
      {hidden.map((h) => (
        <li key={h.group}>
          <b>
            <EyeOff size={12} aria-hidden /> {h.title}
          </b>
          <span>{h.items.join(', ')}</span>
        </li>
      ))}
    </ul>
  );
}

/** "What always stays hidden", folded. */
export function HiddenFold({ hidden }: { hidden: ClaudeHidden[] }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <div className="cl-hidden-fold">
      <button type="button" className="cl-why" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <EyeOff size={14} aria-hidden />
        What always stays hidden
        <ChevronDown size={13} aria-hidden />
      </button>
      {open && (
        <div id={id}>
          <p className="cl-explanation">
            Even with privacy mode off, Claude can never read these. Claude Code refuses them, and Vacancy never puts
            them in a request.
          </p>
          <HiddenList hidden={hidden} />
        </div>
      )}
    </div>
  );
}

/** Privacy mode's switch: on is the default; turning it off asks first, and the choice is remembered. */
export function PrivacyRow({ claude, privacy, fixture }: { claude: Claude; privacy: boolean; fixture: boolean }) {
  return (
    <div className="cl-provider cl-privacy-row">
      <span className="cl-provider-icon" aria-hidden>
        {privacy ? <Lock size={14} /> : <Eye size={14} />}
      </span>
      <div>
        <b>Privacy mode</b>
        <span>
          {privacy
            ? 'On: names, sizes, counts and dates only'
            : fixture
              ? 'Off: Claude can read files in the fixture folder, except hidden places. Remembered.'
              : 'Off: Claude can read files, except hidden places. Remembered.'}
        </span>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={privacy}
        aria-label="Privacy mode"
        className={privacy ? 'cl-toggle is-on' : 'cl-toggle'}
        onClick={() => claude.setPrivacy(!privacy)}
      >
        <i />
      </button>
    </div>
  );
}

/** The confirm before privacy mode goes off: file contents would go to Anthropic. "Keep privacy mode on" has the focus. */
export function PrivacyConfirm({ claude }: { claude: Claude }) {
  const keep = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const title = useId();
  const status = claude.status.phase === 'ready' ? claude.status.status : null;
  const cancel = claude.cancelPrivacyOff;
  useLayoutEffect(() => {
    const before = document.activeElement;
    keep.current?.focus();
    return () => {
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  }, []);
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopImmediatePropagation();
      cancel();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [cancel]);
  const trap = (e: KeyboardEvent) => {
    if (e.key !== 'Tab' || !box.current) return;
    const buttons = [...box.current.querySelectorAll<HTMLButtonElement>('button')];
    const first = buttons[0];
    const last = buttons.at(-1);
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  };
  return createPortal(
    <div className="scrim cl-scrim" data-no-ripple onClick={cancel}>
      <div
        ref={box}
        className="dialog cl-pconfirm"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={title}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={trap}
      >
        <h2 id={title}>Let Claude read your files?</h2>
        <p className="dialog-lead">
          With privacy mode off, Claude can open and read files {status?.fixture ? 'in the fixture folder' : 'anywhere on this PC'}.{' '}
          <b>What’s inside them goes to Anthropic through your Claude Code.</b>
        </p>
        <p>It still can’t write, delete, run commands or go online, and it still only proposes. Nothing runs until you confirm.</p>
        <p className="dialog-quiet">These always stay hidden:</p>
        <HiddenList hidden={status?.hidden ?? []} />
        <p className="dialog-quiet">Vacancy remembers this choice. Turn privacy mode back on any time.</p>
        <div className="dialog-actions">
          <button ref={keep} type="button" className="btn" onClick={cancel}>
            Keep privacy mode on
          </button>
          <button type="button" className="cta" onClick={claude.confirmPrivacyOff}>
            Turn privacy mode off
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

/** What an answer was based on: names, sizes and dates only, or the files Claude opened and the reads refused. */
export function Basis({ basis }: { basis: ClaudeBasis }) {
  // The line often names the first file already.
  const opened = basis.opened.filter((f) => !basis.text.includes(f));
  return (
    <div className={basis.privacy ? 'cl-basis' : 'cl-basis is-open'}>
      <p>
        {basis.privacy ? <Lock size={11} aria-hidden /> : <Eye size={11} aria-hidden />} {basis.text}
      </p>
      {!basis.privacy && opened.length > 0 && <p className="cl-basis-list">Opened: {opened.join(', ')}</p>}
      {basis.refused.length > 0 && (
        <p className="cl-basis-list is-refused">
          <EyeOff size={11} aria-hidden /> Not read: {basis.refused.join(', ')}
        </p>
      )}
    </div>
  );
}

/** A request's status line: working (with Cancel), slow, or why it ended without an answer (with Try again). */
export function JobLine({
  phase,
  failure,
  working,
  onCancel,
  onRetry,
}: {
  phase: Phase | null;
  failure: ClaudeFailure | null;
  working: string;
  onCancel: () => void;
  onRetry: () => void;
}) {
  if (phase === 'working' || phase === 'slow')
    return (
      <p className={phase === 'slow' ? 'cl-job is-slow' : 'cl-job'} role="status">
        <LoaderCircle size={13} className="tray-spin" aria-hidden />
        <span>
          {phase === 'slow' ? (
            <>
              <b>Still working.</b> Claude is taking longer than usual.
            </>
          ) : (
            working
          )}
        </span>
        <button type="button" className="link-button" onClick={onCancel}>
          Cancel
        </button>
      </p>
    );
  if (phase === 'failed' && failure)
    return (
      <div className={failure.kind === 'cancelled' ? 'cl-job is-quiet' : 'cl-job is-error'} role={failure.kind === 'cancelled' ? 'status' : 'alert'}>
        {failure.kind === 'cancelled' ? <X size={13} aria-hidden /> : <TriangleAlert size={13} aria-hidden />}
        <span>
          {failureText(failure)}
          {failure.kind === 'error' && <q className="cl-said">{failure.message}</q>}
        </span>
        <button type="button" className="link-button" onClick={onRetry}>
          Try again
        </button>
      </div>
    );
  return null;
}

/** What Vacancy left out of an answer, by name, with why. */
export function Dropped({ items, heading = 'Vacancy left out' }: { items: ClaudeDropped[]; heading?: string }) {
  if (items.length === 0) return null;
  return (
    <ul className="cl-dropped" aria-label={heading}>
      {items.map((d) => (
        <li key={`${d.name}:${d.path}`}>
          <ShieldX size={12} aria-hidden />
          <span>
            <span className="cl-dropped-k">{heading}</span>
            <b>{d.name}</b>
            {d.path && <code>{d.path}</code>}
            <span>{d.reason}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}

/**
 * The header's control: "Rules only" while Claude is off; Claude's state once it's on. A click unfolds the Claude
 * panel beside the map; before there's a map it only says where Claude is.
 */
export function ClaudeTrigger({ busy, failed, onOpen }: { busy: boolean; failed: boolean; onOpen: (() => void) | null }) {
  const claude = useClaude();
  const status = claude?.status.phase === 'ready' ? claude.status.status : null;
  const on = isOn(claude);
  const label = !status || status.state === 'off' || status.state === 'notFound'
    ? 'Rules only'
    : status.state === 'signedOut'
      ? 'Claude · not signed in'
      : status.state === 'checkTimedOut' || status.state === 'checkFailed'
        ? 'Claude · couldn’t check'
        : busy
        ? 'Claude is working'
        : failed
          ? 'Claude · didn’t answer'
          : status.privacy
            ? 'Claude on'
            : 'Claude on · reads files';
  const icon = busy ? <LoaderCircle size={12} className="tray-spin" aria-hidden /> : on ? <Mark size={12} /> : <Sparkles size={12} strokeWidth={2.2} aria-hidden />;
  const cls = on ? 'pill advisor-trigger cl-trigger' : 'pill advisor-trigger';
  if (!onOpen)
    return (
      <span className={cls} title="Claude’s panel is beside the map, once a drive is scanned.">
        {icon} {label}
      </span>
    );
  return (
    <button type="button" className={cls} onClick={onOpen} aria-label={`${label}: show the Claude panel`}>
      {icon} {label}
      {failed && !busy && on && <i className="cl-dot is-error" aria-hidden />}
      <ChevronDown size={12} aria-hidden />
    </button>
  );
}

/** The scan plate's note: built-in rules always; Claude off until it's turned on, beside the map. */
export function ClaudeNote() {
  const claude = useClaude();
  const status = claude?.status.phase === 'ready' ? claude.status.status : null;
  const [title, detail] = !status
    ? ['Built-in rules find cleanups offline.', 'Claude stays off until you turn it on, beside the map.']
    : status.state === 'notFound'
      ? ['Built-in rules find cleanups offline.', 'Claude Code isn’t on this computer, so Claude stays off.']
      : status.state === 'off'
        ? ['Built-in rules. Claude is off.', 'After the scan you can turn on Claude, your own Claude Code, in the panel beside the map.']
        : status.state === 'signedOut'
          ? ['Built-in rules. Claude Code isn’t signed in.', 'Run claude in a terminal and sign in to use Claude beside the map.']
          : status.state === 'checkTimedOut' || status.state === 'checkFailed'
            ? ['Built-in rules. Vacancy couldn’t check Claude Code.', 'Check again in the Claude panel beside the map.']
            : [
              'Built-in rules, and Claude beside the map.',
              status.privacy
                ? 'Privacy mode is on: Claude gets names, sizes, counts and dates only.'
                : 'Privacy mode is off: Claude can read files, except the places that always stay hidden.',
            ];
  return (
    <div className="advisor-note" data-no-ripple>
      {status?.state === 'on' ? <Mark size={15} /> : <Sparkles size={15} aria-hidden />}
      <span>
        <b>{title}</b>
        <span>{detail}</span>
      </span>
    </div>
  );
}

/** "Claude says", in a hover card, for a folder Claude explained. */
export function TipSection({ note, suggested, act }: { note: ClaudeFolderNote; suggested: Proposal[]; act: string }) {
  return (
    <div className="cl-tip">
      <span className="cl-tip-h">
        <Mark size={11} /> Claude says
      </span>
      <p>{note.explanation}</p>
      {suggested.map((p) => (
        <p key={p.id} className="cl-tip-sug">
          <Sparkles size={11} aria-hidden /> Suggests {p.title} ({formatBytes(p.bytes)}), in the tray unticked.
        </p>
      ))}
      <p className="map-tip-act">{act}</p>
    </div>
  );
}

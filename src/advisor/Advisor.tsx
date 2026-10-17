import { Check, ChevronDown, CircleHelp, ShieldCheck, Sparkles, X } from 'lucide-react';
import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import type { AdvisorAvailability } from '../bindings/AdvisorAvailability';
import type { AdvisorProvider } from '../bindings/AdvisorProvider';
import type { AdvisorState } from './useAdvisorStatus';
import './advisor.css';

const PROVIDERS: readonly { id: AdvisorProvider; name: string; mark: string }[] = [
  { id: 'claudeCode', name: 'Claude Code', mark: '✳' },
  { id: 'codex', name: 'Codex', mark: '›_' },
];

export function AdvisorTrigger({ open, onOpen }: { open: boolean; onOpen: () => void }) {
  return <button type="button" className="pill advisor-trigger" aria-haspopup="dialog" aria-expanded={open} onClick={onOpen}>
    <Sparkles size={12} strokeWidth={2.2} aria-hidden /> Rules only <ChevronDown size={12} aria-hidden />
  </button>;
}

export function AdvisorNote({ state, onOpen }: { state: AdvisorState; onOpen: () => void }) {
  const detail = state.phase === 'loading' ? 'Checking app availability. Built-in cleanups still work.'
    : state.phase === 'error' ? 'App availability couldn’t be checked. Built-in cleanups still work.'
    : state.status.discovery === 'fixture' ? 'Preview status only. No apps were checked on this computer.'
    : state.status.providers.some((p) => p.availability.state === 'unsupported') ? 'Detected AI apps can’t be used within Vacancy’s privacy limit.'
    : state.status.providers.some((p) => p.availability.state === 'notChecked') ? 'AI app availability wasn’t checked.'
    : 'No supported AI app was detected in this check.';
  return <div className="advisor-note" data-no-ripple>
    <Sparkles size={15} aria-hidden />
    <span><b>Built-in rules. AI stays off.</b><span>{detail}</span></span>
    <button type="button" onClick={onOpen}>AI options</button>
  </div>;
}

function availabilityText(availability: AdvisorAvailability | undefined, preview: boolean): string {
  if (!availability) return 'Status unavailable';
  switch (availability.state) {
    case 'unsupported': return `${preview ? 'Preview: detected' : 'Detected'} · Not available to Vacancy`;
    case 'notDetected': return preview ? 'Preview: not detected' : 'Not detected in this check';
    case 'notChecked': return preview ? 'Not checked in fixture mode' : 'Not checked';
  }
}

/** Approved focused dialog (prototype/advisor-status, C). It reports status; it cannot change a setting. */
export function AdvisorDialog({ state, onClose, onRetry }: { state: AdvisorState; onClose: () => void; onRetry: () => void }) {
  const id = useId();
  const panel = useRef<HTMLElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const [details, setDetails] = useState(false);
  const preview = state.phase === 'ready' && state.status.discovery === 'fixture';
  const unsupported = state.phase === 'ready' && state.status.providers.some((p) => p.availability.state === 'unsupported');

  useEffect(() => {
    const before = document.activeElement;
    const host = document.querySelector('.shell');
    const wasInert = host?.hasAttribute('inert') ?? false;
    host?.setAttribute('inert', '');
    close.current?.focus();
    return () => {
      if (!wasInert) host?.removeAttribute('inert');
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  }, []);

  useEffect(() => {
    const escape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener('keydown', escape, true);
    return () => window.removeEventListener('keydown', escape, true);
  }, [onClose]);

  const trap = (event: KeyboardEvent) => {
    if (event.key !== 'Tab' || !panel.current) return;
    const buttons = [...panel.current.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    const first = buttons[0], last = buttons.at(-1);
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  };

  return createPortal(<div className="advisor-shade" data-no-ripple onPointerDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section ref={panel} className="advisor-dialog" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} aria-describedby={`${id}-intro`} onKeyDown={trap}>
      <div className="advisor-top"><span className="advisor-eyebrow">AI assistance</span><button ref={close} type="button" className="advisor-close" aria-label="Close AI options" onClick={onClose}><X size={17} aria-hidden /></button></div>
      <div className="advisor-dialog-body">
        <div className="advisor-dialog-symbol"><ShieldCheck size={27} aria-hidden /></div>
        <h2 id={`${id}-title`}>Your scan is ready. <br />AI stays off.</h2>
        <p id={`${id}-intro`} className="advisor-intro" role={state.phase === 'error' ? 'alert' : state.phase === 'loading' ? 'status' : undefined}>
          {state.phase === 'loading' ? <>Checking app availability. <br />Built-in cleanups stay available.</>
            : state.phase === 'error' ? <>App availability couldn’t be checked. <br />Built-in cleanups stay available.</>
            : preview ? <>This preview uses synthetic app status. <br />No apps were checked on this computer.</>
            : unsupported ? <>Vacancy only shares folder details with AI. <br />These apps can’t guarantee that boundary.</>
            : <>Built-in cleanups work offline. <br />No supported AI app is available in this check.</>}
        </p>
        <div className="advisor-providers" aria-label="AI app availability">
          {PROVIDERS.map((provider) => <div className="advisor-provider" key={provider.id}>
            <span className="advisor-provider-icon" aria-hidden>{provider.mark}</span>
            <div><b>{provider.name}</b><span id={`${id}-${provider.id}`}>{state.phase === 'loading' ? 'Checking availability…' : state.phase === 'error' ? 'Status unavailable' : availabilityText(state.status.providers.find((p) => p.provider === provider.id)?.availability, preview)}</span></div>
            <button type="button" className="advisor-toggle" role="switch" aria-checked="false" aria-label={`Use ${provider.name}`} aria-describedby={`${id}-${provider.id}`} disabled><i /></button>
          </div>)}
        </div>
        {state.phase === 'error' && <button type="button" className="link-button advisor-retry" onClick={onRetry}>Try again</button>}
        {state.phase === 'ready' && <p className="advisor-scope">{preview ? 'Fixture preview. No host apps were searched or launched.' : 'Availability is a passive check of standard locations. Custom locations may be missed; versions and sign-in weren’t checked.'}</p>}
        <div className="advisor-dialog-detail"><span>What AI would see</span><b>Names · sizes · counts · dates</b><span>Never the contents of your files.</span></div>
        <button type="button" className="advisor-why" aria-expanded={details} aria-controls={`${id}-why`} onClick={() => setDetails(!details)}><CircleHelp size={14} aria-hidden />Why does AI stay off?<ChevronDown size={13} aria-hidden /></button>
        {details && <p id={`${id}-why`} className="advisor-explanation">These apps may include other files or settings with a request. Vacancy can’t keep them to folder details alone, so it leaves AI off. Built-in cleanups still work offline.</p>}
      </div>
      <button type="button" className="btn advisor-continue" onClick={onClose}><Check size={15} aria-hidden />Continue with built-in rules</button>
    </section>
  </div>, document.body);
}

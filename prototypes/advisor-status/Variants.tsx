// THROWAWAY treatments; shared fixture content, deliberately different hierarchy and placement.
import { Check, ChevronDown, CircleHelp, LockKeyhole, ShieldCheck, X } from 'lucide-react';

interface Props { close: () => void; details: boolean; onDetails: () => void }

export function VariantA(props: Props) {
  return <><Heading close={props.close} /><h1 id="advisor-title">Built-in rules are ready.</h1><p className="advisor-intro">AI is off. You can still scan and clean up.</p><Providers /><p className="advisor-boundary">Vacancy shares folder details, never file contents. These apps can’t guarantee that limit.</p><Why {...props} /><div className="advisor-compact-foot"><LockKeyhole size={13} /> No folder details are being shared.</div></>;
}

export function VariantB(props: Props) {
  return <><Heading close={props.close} /><div className="advisor-panel-body"><h1 id="advisor-title">AI for your folders.</h1><p className="advisor-intro">Explanations for unfamiliar folders.<br />Ideas for a tidier layout.</p>
    <div className="advisor-privacy"><ShieldCheck size={18} /><p><b>Folder details, never file contents.</b><span>AI can receive names, sizes, counts and dates. Your files stay with you.</span></p></div>
    <div className="advisor-mode"><div><b>Use AI assistance</b><span>Unavailable with the installed apps</span></div><button className="advisor-toggle" role="switch" aria-checked="false" aria-label="Use AI assistance" disabled><i /></button></div>
    <h2 className="advisor-section-label">Installed apps</h2><Providers /><Why {...props} /></div>
    <div className="advisor-panel-foot"><p><Check size={14} /> Built-in cleanups stay available.</p><button className="btn" onClick={props.close}>Done</button></div></>;
}

export function VariantC(props: Props) {
  return <><Heading close={props.close} /><div className="advisor-dialog-body"><div className="advisor-dialog-symbol"><ShieldCheck size={27} /></div><h1 id="advisor-title">Your scan is ready.<br />AI stays off.</h1>
    <p className="advisor-intro">Vacancy only shares folder details with AI.<br />The installed apps can’t guarantee that boundary.</p>
    <Providers /><div className="advisor-dialog-detail"><span>What AI would see</span><b>Names · sizes · counts · dates</b><span>Never the contents of your files.</span></div>
    <Why {...props} /></div><button className="btn advisor-continue" onClick={props.close}><Check size={15} /> Continue with built-in rules</button></>;
}

function Heading({ close }: Pick<Props, 'close'>) {
  return <div className="advisor-top"><span className="advisor-eyebrow">AI assistance</span><button className="advisor-close" aria-label="Close AI settings" onClick={close}><X size={17} /></button></div>;
}

function Providers() {
  return <div className="advisor-providers" aria-label="Installed AI apps">{['Claude Code', 'Codex'].map(name => <div className="advisor-provider" key={name}><span className="advisor-provider-icon">{name === 'Codex' ? '›_' : '✳'}</span><div><b>{name}</b><span>Installed <i>·</i> Not available to Vacancy</span></div><button className="advisor-toggle" role="switch" aria-checked="false" aria-label={`Use ${name}`} disabled><i /></button></div>)}</div>;
}

function Why({ details, onDetails }: Props) {
  return <><button className="advisor-why" aria-expanded={details} onClick={onDetails}><CircleHelp size={14} /> Why can’t these apps be used?<ChevronDown size={13} /></button>{details && <p className="advisor-explanation">These versions may include other files or settings with a request. Vacancy can’t keep them to the folder details alone, so it leaves AI off. Built-in cleanups still work offline.</p>}</>;
}

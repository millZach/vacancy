// THROWAWAY: three placements for AI status/settings in the existing Vacancy Scan/Atlas host. ?variant=A/B/C.
// No production entry point is changed. Both providers are synthetic installed-but-unsupported fixtures.
import '../../src/design/fonts';
import '../../src/design/tokens.css';
import '../../src/design/base.css';
import '../../src/design/controls.css';
import '../../src/shell/shell.css';
import './tokens.css';
import './prototype.css';
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { ArrowLeft, ArrowRight, ChevronDown, Sparkles } from 'lucide-react';
import { Atlas } from '../../src/atlas/Atlas';
import { DrivesPending } from '../../src/atlas/DrivesPending';
import { BridgeProvider, createMockBridge, fixtures, useBridge, type RunReport } from '../../src/bridge';
import { Wordmark } from '../../src/design/Wordmark';
import { DrivesProvider } from '../../src/drives/DrivesProvider';
import { useDrives } from '../../src/drives/context';
import { DriveMenu } from '../../src/drives/DriveMenu';
import { reclaimableOf } from '../../src/drives/session';
import { formatBytes } from '../../src/format';
import { HistoryMenu } from '../../src/history/HistoryMenu';
import { Finish } from '../../src/wrapped/Finish';
import { VariantA, VariantB, VariantC } from './Variants';

const keys = ['A', 'B', 'C'] as const;
type Variant = typeof keys[number];
const names: Record<Variant, string> = { A: 'Compact menu', B: 'Side panel', C: 'Focused dialog' };
const initial = new URLSearchParams(location.search);
const initialVariant = keys.find(v => v === initial.get('variant')) ?? 'A';
const fixture = initial.get('fixture') === 'macos' ? fixtures.macos : fixtures.windows;
const bridge = createMockBridge(fixture, { scanMs: 1400, fullDiskAccess: fixture.platform === 'macos' ? 'granted' : 'notApplicable' });

function Prototype() {
  const [variant, setVariant] = useState<Variant>(initialVariant);
  const [open, setOpen] = useState(initial.get('open') !== '0');
  const [details, setDetails] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLElement>(null);
  const shade = useRef<HTMLDivElement>(null);
  const close = () => { setOpen(false); trigger.current?.focus(); };
  const choose = (direction: number) => {
    const next = keys[(keys.indexOf(variant) + direction + keys.length) % keys.length]!;
    const url = new URL(location.href);
    url.searchParams.set('variant', next);
    history.replaceState({}, '', url);
    setVariant(next); setDetails(false); setOpen(true);
  };
  useEffect(() => {
    // Close settings before the host Atlas sees Escape and navigates its breadcrumb.
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && open) { event.preventDefault(); event.stopImmediatePropagation(); close(); }
    };
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented || (event.target instanceof Element && event.target.closest('input,textarea,[contenteditable="true"]'))) return;
      if (event.key === 'Escape' && open) { event.preventDefault(); close(); }
      else if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); choose(event.key === 'ArrowRight' ? 1 : -1); }
      else if (event.key === 'Tab' && open && variant !== 'A' && panel.current) {
        const controls = [...panel.current.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
        const first = controls[0], last = controls.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    };
    window.addEventListener('keydown', escape, true);
    window.addEventListener('keydown', key);
    return () => { window.removeEventListener('keydown', escape, true); window.removeEventListener('keydown', key); };
  });
  useEffect(() => { if (open) panel.current?.querySelector<HTMLButtonElement>('button')?.focus(); }, [open, variant]);
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const badge = trigger.current?.getBoundingClientRect();
      const header = document.querySelector('.shell-head')?.getBoundingClientRect();
      if (!badge || !header) return;
      shade.current?.style.setProperty('--advisor-anchor-right', `${window.innerWidth - badge.right}px`);
      shade.current?.style.setProperty('--advisor-anchor-top', `${header.bottom + 9}px`);
    };
    place(); window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  }, [open, variant]);

  const content = { close, details, onDetails: () => setDetails(!details) };
  return <div className="advisor-prototype" data-variant={variant}>
    <Host trigger={<button ref={trigger} className="pill advisor-trigger" aria-expanded={open} aria-haspopup="dialog" onClick={() => setOpen(!open)}><Sparkles size={12} /> Rules only <ChevronDown size={12} /></button>} onOptions={() => setOpen(true)} />
    {open && <div ref={shade} className={`advisor-shade variant-${variant}`} onPointerDown={e => { if (e.target === e.currentTarget) close(); }}>
      <section ref={panel} className={`advisor-surface variant-${variant}`} role="dialog" aria-modal={variant === 'A' ? undefined : true} aria-labelledby="advisor-title">
        {variant === 'A' ? <VariantA {...content} /> : variant === 'B' ? <VariantB {...content} /> : <VariantC {...content} />}
      </section>
    </div>}
    {import.meta.env.MODE === 'prototype' && <nav className="prototype-switcher" aria-label="Prototype variants"><span className="prototype-label">PROTOTYPE</span><button aria-label="Previous variant" onClick={() => choose(-1)}><ArrowLeft size={17} /></button><span className="prototype-current"><b>{variant}</b> {names[variant]}<small>Fixture: both installed · unsupported · AI off</small></span><button aria-label="Next variant" onClick={() => choose(1)}><ArrowRight size={17} /></button></nav>}
  </div>;
}

/** The existing host, composed from the real read-only UI and an explicit mock bridge, never defaultBridge/Tauri. */
function Host({ trigger, onOptions }: { trigger: ReactNode; onOptions: () => void }) {
  const drives = useDrives();
  const { platform } = useBridge();
  const ready = drives.status === 'ready' ? drives : null;
  const [planOpen, setPlanOpen] = useState(false);
  const [past, setPast] = useState<RunReport | null>(null);
  const [noteTarget, setNoteTarget] = useState<Element | null>(null);
  useLayoutEffect(() => {
    const next = document.querySelector('.plate-content');
    if (next !== noteTarget) setNoteTarget(next);
  });
  const scan = ready?.current.scan;
  const onMap = !past && ready?.screen === 'map' && scan?.phase === 'done';
  return <div className="shell" data-chrome={platform === 'macos' ? 'overlay' : 'native'} data-prototype-map={onMap}>
    <header className="shell-head"><Wordmark />{ready && <DriveMenu drives={ready} />}<div className="head-right">
      {onMap && <span className="head-reclaim"><b>{formatBytes(reclaimableOf(scan))}</b> reclaimable</span>}
      {ready && <HistoryMenu locked={ready.locked} onOpen={setPast} />}
      {trigger}
      {onMap && !planOpen && <button className="cta cta--sm" onClick={() => setPlanOpen(true)}>Build a plan</button>}
    </div></header>
    <div className="shell-body">{past ? <Finish key={past.id} report={past} start="rest" onClose={() => setPast(null)} /> : ready ? <Atlas drives={ready} agent={null} planOpen={planOpen} onPlanOpen={setPlanOpen} /> : <DrivesPending error={null} />}</div>
    {noteTarget && createPortal(<div className="prototype-agent-note" data-no-ripple><Sparkles size={15} /><span><b>Built-in rules. AI stays off.</b><span>Claude Code and Codex are installed, but can’t be used privately here.</span></span><button onClick={onOptions}>AI options</button></div>, noteTarget)}
  </div>;
}

createRoot(document.getElementById('root')!).render(<BridgeProvider bridge={bridge}><DrivesProvider><Prototype /></DrivesProvider></BridgeProvider>);

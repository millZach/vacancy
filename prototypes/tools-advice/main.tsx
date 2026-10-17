// THROWAWAY: three placements for instruction-only tool reminders on the existing Atlas map.
import '../../src/design/fonts';
import '../../src/design/tokens.css';
import '../../src/design/base.css';
import '../../src/design/controls.css';
import '../../src/atlas/atlas.css';
import '../../src/plan/plan.css';
import './prototype.css';
import { StrictMode, useEffect, useState, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowLeft, ArrowRight, BookOpen, ChevronDown, ChevronRight, HardDrive, Signpost, X } from 'lucide-react';
import type { Platform } from '../../src/bridge';
import { fixtures } from '../../src/bridge/fixtures';
import { Shell } from '../../src/shell/Shell';
import { MapView } from '../../src/map/MapView';
import { overviewModel } from '../../src/map/model';
import { useSize } from '../../src/map/useSize';
import { Legend } from '../../src/map/Legend';
import { Checkbox, RiskChip } from '../../src/plan/bits';

type Advice = { id: string; group: string; title: string; summary: string; steps: string[]; caution: string; source: string; vendor: string };
const windows: Advice[] = [
  { id: 'win-storage', group: 'Windows', title: 'Review Windows cleanup', summary: 'Let Windows review update files and temporary system files.', steps: ['Open Settings → System → Storage → Cleanup recommendations.', 'Review the categories and the files Windows offers. Select only what you want to remove.', 'If a previous Windows installation is offered, keep it if you still need the option to roll back.'], caution: 'Available categories vary. Vacancy has not measured or selected any system files.', vendor: 'Microsoft', source: 'https://support.microsoft.com/en-us/windows/experience/storage-filemanagement/free-up-drive-space-in-windows' },
  { id: 'win-delivery', group: 'Windows', title: 'Review Delivery Optimization', summary: 'Use the Windows cleanup interface for its download cache.', steps: ['Open Disk Cleanup and select the Windows drive.', 'If Delivery Optimization Files is offered, review that category.', 'Use the Windows interface to confirm removal if you want it.'], caution: 'This is a review reminder, not a finding that the cache exists or is safe for Vacancy to delete.', vendor: 'Microsoft', source: 'https://support.microsoft.com/en-us/windows/deployment/updates-lifecycle/delivery-optimization-in-windows' },
];
const docker: Advice[] = [
  { id: 'docker-images', group: 'Docker', title: 'Review Docker images', summary: 'Choose images you no longer need in Docker Desktop.', steps: ['Open Docker Desktop → Images → Local.', 'Review your images and their container usage.', 'Remove only images you recognize and no longer need, using Docker Desktop.'], caution: 'Images may need to be pulled or built again. Vacancy does not inspect or modify Docker’s disk image or volumes.', vendor: 'Docker', source: 'https://docs.docker.com/desktop/use-desktop/images/' },
  { id: 'docker-cache', group: 'Docker', title: 'Review Docker build cache', summary: 'Let Docker manage its build cache and retention.', steps: ['Review Docker’s build cache garbage-collection documentation for the builder you use.', 'Check your builder’s cache policy before changing it.', 'Use Docker’s own supported controls if you decide to reclaim cache.'], caution: 'Builds may take longer after cache is removed. No Docker commands run from Vacancy.', vendor: 'Docker', source: 'https://docs.docker.com/build/cache/garbage-collection/' },
];
const mac: Advice[] = [
  { id: 'xcode', group: 'Xcode', title: 'Review Xcode components', summary: 'Choose simulator runtimes you no longer need in Xcode.', steps: ['Open Xcode → Settings → Components.', 'Review installed simulator runtimes against the platforms you still test.', 'Use Xcode’s removal control only for components you choose to remove.'], caution: 'An older runtime may still be needed. Vacancy does not infer that a runtime is unused.', vendor: 'Apple', source: 'https://developer.apple.com/documentation/Xcode/downloading-and-installing-additional-xcode-components' },
  { id: 'brew', group: 'Homebrew', title: 'Preview Homebrew cleanup', summary: 'Review what Homebrew would clean before choosing to proceed.', steps: ['In your own terminal, use brew cleanup --dry-run to preview Homebrew’s cleanup candidates.', 'Read the output and Homebrew’s cleanup documentation.', 'If you decide to proceed, run cleanup yourself through Homebrew.'], caution: 'The preview may include cached downloads and older installed versions. Vacancy never runs the command.', vendor: 'Homebrew', source: 'https://docs.brew.sh/Manpage#cleanup-options-formulacask-' },
];
const names = { A: 'Left for you', B: 'Guidance notebook', C: 'Map drawer' };
type Variant = keyof typeof names;
type Shared = { advice: Advice[]; selected: string[]; toggle: (id: string) => void; review: () => void; children: ReactNode };

function Details({ item }: { item: Advice }) {
  return <div className="advice-details"><ol>{item.steps.map(step => <li key={step}>{step}</li>)}</ol><p>{item.caution}</p><a href={item.source} target="_blank" rel="noreferrer">{item.vendor} instructions ↗</a></div>;
}
function AdviceRow({ item, selected, toggle }: { item: Advice; selected: boolean; toggle: () => void }) {
  const [open, setOpen] = useState(false);
  return <div className={`tray-row ${selected ? 'is-on' : ''}`}><Checkbox on={selected} onChange={toggle} label={`Remind me: ${item.title}`}/><div className="tray-row-main"><button type="button" className="tray-row-title" onClick={() => setOpen(!open)} aria-expanded={open}><span>{item.title}</span><ChevronDown size={13} className={open ? 'is-open' : ''}/></button><p className="tray-row-sum">{item.summary}</p><span className="source"><Signpost size={11}/> You do this one</span>{open && <Details item={item}/>}</div><span className="tray-row-size">Advice</span></div>;
}
function DeleteContext() {
  return <section className="tray-group"><h3>Delete <span>0 of 2</span></h3>{[['App caches', '6.2 GB'], ['Temporary files', '1.8 GB']].map(([title, size]) => <div className="tray-row context-row" key={title}><button className="check" type="button" aria-label={`${title} · not part of this prototype`} disabled/><div><div className="tray-row-title">{title}</div><p className="tray-row-sum">Rebuilds when the app needs it.</p><RiskChip risk="safe"/></div><span className="tray-row-size">{size}</span></div>)}</section>;
}
function Footer({ count, review }: { count: number; review: () => void }) {
  return <div className="tray-foot"><div className="tray-total"><span className="tray-total-num">0 B</span><span className="tray-total-lbl">will be freed</span></div><ul className="tray-notes"><li>No files will be deleted.</li><li>{count} {count === 1 ? 'reminder' : 'reminders'}, excluded from savings</li></ul><button type="button" className="cta tray-cta" disabled={!count} onClick={review}>Review reminders</button></div>;
}
export function VariantA(p: Shared) {
  return <>{p.children}<aside className="tray" aria-label="Plan"><div className="tray-head"><div><h2>Plan</h2><p>Choose what belongs in your plan.</p></div></div><div className="tray-scroll"><DeleteContext/><section className="tray-group"><h3>Left for you <span>{p.selected.length} selected</span></h3><p className="tray-group-note">Use the tool’s own controls. Tick an item to keep its instructions in your recap. These don’t count toward savings.</p>{p.advice.map(item => <AdviceRow key={item.id} item={item} selected={p.selected.includes(item.id)} toggle={() => p.toggle(item.id)}/>)}</section></div><Footer count={p.selected.length} review={p.review}/></aside></>;
}
export function VariantB(p: Shared) {
  const [active, setActive] = useState(p.advice[0]?.id ?? '');
  const item = p.advice.find(x => x.id === active) ?? p.advice[0]!;
  return <>{p.children}<aside className="tray notebook" aria-label="Guidance notebook"><div className="tray-head"><div><h2>Plan</h2><p>Cleanup and guidance, in two sections.</p></div></div><div className="tray-scroll"><DeleteContext/><section className="tray-group"><h3><span className="advice-heading"><BookOpen size={14}/> Tool guidance</span><span>{p.advice.length}</span></h3><p className="tray-group-note">Choose a guide. Add a reminder to keep it after your run.</p><div className="guide-index">{Array.from(new Set(p.advice.map(x => x.group))).map(group => <div key={group}><p>{group}</p>{p.advice.filter(x => x.group === group).map(x => <button type="button" className={x.id === item.id ? 'active' : ''} key={x.id} onClick={() => setActive(x.id)}>{p.selected.includes(x.id) ? '✓ ' : ''}{x.title}<ChevronRight size={12}/></button>)}</div>)}</div><article className="guide-page"><h3>{item.title}</h3><p>{item.summary}</p><Details item={item}/><label className="guide-remind"><Checkbox on={p.selected.includes(item.id)} onChange={() => p.toggle(item.id)} label={`Remind me: ${item.title}`}/> Keep this in my recap</label></article></section></div><Footer count={p.selected.length} review={p.review}/></aside></>;
}
export function VariantC(p: Shared) {
  const [open, setOpen] = useState(true);
  return <><div className="drawer-host">{p.children}<div className={`guidance-drawer ${open ? 'is-open' : ''}`}><button type="button" className="drawer-handle" onClick={() => setOpen(!open)} aria-expanded={open}><Signpost size={15}/><b>Left for you</b><span>{p.advice.length} guides · {p.selected.length} reminders</span><ChevronDown size={15}/></button>{open && <div className="drawer-content">{p.advice.map(item => <AdviceRow key={item.id} item={item} selected={p.selected.includes(item.id)} toggle={() => p.toggle(item.id)}/>)}</div>}</div></div><aside className="tray compact-plan" aria-label="Plan"><div className="tray-head"><div><h2>Plan</h2><p>The map keeps your tool guidance close.</p></div></div><div className="tray-scroll"><DeleteContext/><section className="tray-group"><h3>Reminders <span>{p.selected.length}</span></h3><p className="tray-group-note">Keep guidance from the drawer. Instructions stay in your recap.</p>{p.advice.filter(x => p.selected.includes(x.id)).map(x => <p className="saved-name" key={x.id}><Signpost size={12}/>{x.title}</p>)}</section></div><Footer count={p.selected.length} review={p.review}/></aside></>;
}
function AtlasMap({ platform }: { platform: Platform }) {
  const [ref, size] = useSize<HTMLDivElement>();
  const [hot, setHot] = useState<string | null>(null);
  const fixture = fixtures[platform];
  // Keep the approved map fixture as context without suggesting WER was detected.
  const overview = { ...fixture.overview, categories: fixture.overview.categories.map(category => ({ ...category,
    items: category.items.map(item => item.name === 'WER' ? { ...item, name: 'Other temporary data', path: '', note: 'Synthetic example data.' } : item),
  })) };
  const model = overviewModel(overview);
  return <main className="atlas is-map"><div className="atlas-toolbar"><div className="crumbs"><span className="crumb is-here">{platform === 'windows' ? 'Windows (C:)' : 'Macintosh HD'}</span></div><Legend bytesInView={model.totalBytes} w={size.w} h={size.h} reclaimable/></div><div className="atlas-stage" ref={ref}><MapView model={model} width={size.w} height={size.h} focus={null} interactive hot={hot} onHot={setHot} clickable={() => false} look={leaf => leaf.proposal ? { cls: 'is-reclaim', state: 'reclaimable' } : {}}/></div><p className="map-note">Synthetic scan · tool instructions are reminders, never map cleanup.</p></main>;
}
function Prototype() {
  const params = new URLSearchParams(location.search);
  const initial = params.get('variant');
  const [variant, setVariant] = useState<Variant>(initial === 'B' || initial === 'C' ? initial : 'A');
  const [platform, setPlatform] = useState<Platform>(params.get('platform') === 'macos' ? 'macos' : 'windows');
  const [selected, setSelected] = useState<string[]>([]);
  const [recap, setRecap] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const advice = platform === 'windows' ? [...windows, ...docker] : [...mac, ...docker];
  const visibleSelected = selected.filter(id => advice.some(x => x.id === id));
  function changeVariant(next: Variant) { setVariant(next); const url = new URL(location.href); url.searchParams.set('variant', next); history.replaceState(null, '', url); }
  function changePlatform(next: Platform) { setPlatform(next); setRecap(false); setReviewing(false); const url = new URL(location.href); url.searchParams.set('platform', next); history.replaceState(null, '', url); }
  function cycle(delta: number) { const keys: Variant[] = ['A', 'B', 'C']; changeVariant(keys[(keys.indexOf(variant) + delta + 3) % 3]!); }
  useEffect(() => { const key = (e: KeyboardEvent) => { if ((e.target as HTMLElement).closest('input,textarea,[contenteditable]')) return; if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') { e.preventDefault(); cycle(e.key === 'ArrowLeft' ? -1 : 1); } }; addEventListener('keydown', key); return () => removeEventListener('keydown', key); });
  const props: Shared = { advice, selected: visibleSelected, toggle: id => setSelected(s => s.includes(id) ? s.filter(x => x !== id) : [...s, id]), review: () => setReviewing(true), children: <AtlasMap platform={platform}/> };
  return <><Shell platform={platform} drive={<div className="head-disk"><HardDrive size={15}/><span className="head-disk-name">{platform === 'windows' ? 'Windows (C:)' : 'Macintosh HD'}</span><span className="head-disk-nums">Synthetic drive</span></div>} agent={null} reclaimable={8e9}>{recap ? <><AtlasMap platform={platform}/><aside className="tray recap"><div className="tray-head"><div><h2>Recap</h2><p>Reminders only · simulated result</p></div></div><div className="tray-scroll"><section className="tray-group"><div className="recap-zero">0 B <small>freed</small></div><p className="tray-group-note">0 files deleted · 0 folders moved</p><h3>Left for you <span>{visibleSelected.length}</span></h3><p className="tray-group-note">Your chosen instructions, ready when you are.</p>{advice.filter(x => visibleSelected.includes(x.id)).map(item => <AdviceRow key={item.id} item={item} selected toggle={() => props.toggle(item.id)}/>)}</section></div><div className="tray-foot"><button className="btn" onClick={() => setRecap(false)}>Back to plan</button></div></aside></> : variant === 'A' ? <VariantA {...props}/> : variant === 'B' ? <VariantB key={platform} {...props}/> : <VariantC {...props}/>}</Shell>
  {reviewing && <div className="review-scrim"><section className="review-modal" role="dialog" aria-modal="true" aria-labelledby="review-title"><button className="icon-btn close" aria-label="Close review" onClick={() => setReviewing(false)}><X size={16}/></button><Signpost size={23}/><h2 id="review-title">Keep {visibleSelected.length} {visibleSelected.length === 1 ? 'reminder' : 'reminders'}?</h2><p>No files will change. These instructions will appear in your recap.</p><ul>{advice.filter(x => visibleSelected.includes(x.id)).map(x => <li key={x.id}>{x.title}</li>)}</ul><div className="review-stats">0 B freed · 0 deletions · 0 moves</div><button className="cta" onClick={() => { setReviewing(false); setRecap(true); }}>Simulate recap</button></section></div>}
  {(import.meta.env.DEV || params.get('review') === '1') && <div className="prototype-switcher"><span className="prototype-label">THROWAWAY</span><button aria-label="Previous variant" onClick={() => cycle(-1)}><ArrowLeft size={14}/></button><span className="variant-label">{variant} · {names[variant]}{variant === 'A' && ' · Recommended'}</span><button aria-label="Next variant" onClick={() => cycle(1)}><ArrowRight size={14}/></button><span className="switcher-divider"/><button className={platform === 'windows' ? 'on' : ''} onClick={() => changePlatform('windows')}>Windows</button><button className={platform === 'macos' ? 'on' : ''} onClick={() => changePlatform('macos')}>macOS</button><span className="state-label">{recap ? 'Recap' : 'Plan'} · {visibleSelected.length} {visibleSelected.length === 1 ? 'reminder' : 'reminders'} · 0 B</span></div>}</>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Prototype/></StrictMode>);

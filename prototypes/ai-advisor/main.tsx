// THROWAWAY: three places for live Claude in Vacancy, switchable with ?variant=A|B|C, on the Windows fixture's map.
// &state=off|notfound|signedout|on|slow|error sets the advisor's state; &privacy=on|off sets privacy mode.
// Claude's replies are canned with a short fake delay (&delay=ms). No live calls, no Tauri, no file access.
import '../../src/design/fonts';
import '../../src/design/tokens.css';
import '../../src/design/base.css';
import '../../src/design/controls.css';
import '../../src/shell/shell.css';
import '../../src/atlas/atlas.css';
import '../../src/map/map.css';
import '../../src/plan/plan.css';
import '../../src/tidy/tidy.css';
import '../../src/run/run.css';
import '../../src/advisor/advisor.css';
import './prototype.css';
import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import { DriveMenu } from '../../src/drives/DriveMenu';
import { formatBytes } from '../../src/format';
import { Shell } from '../../src/shell/Shell';
import { planTotals } from '../../src/plan/totals';
import { AdvisorDialog, Confirm, Ended, NoBridge, PrivacyConfirm, Trigger, readyDrives } from './common';
import type { Privacy } from './data';
import { AI_STATES, StoreCtx, fresh, isOn, useStore, type AiState } from './store';
import { VariantA, VariantB, VariantC } from './variants';

const names = { A: 'In the tray', B: 'Claude panel', C: 'Review step' };
type Variant = keyof typeof names;
const RECOMMENDED: Variant = 'A';

function Prototype() {
  const params = new URLSearchParams(location.search);
  const v0 = params.get('variant');
  const a0 = params.get('state') as AiState | null;
  const [variant, setVariant] = useState<Variant>(v0 === 'B' || v0 === 'C' ? v0 : 'A');
  const store = useStore(a0 && AI_STATES.includes(a0) ? a0 : 'off', params.get('privacy') === 'off' ? 'off' : 'on');
  return (
    <StoreCtx.Provider value={store}>
      <NoBridge>
        <App variant={variant} setVariant={setVariant} />
      </NoBridge>
    </StoreCtx.Provider>
  );
}

function App({ variant, setVariant }: { variant: Variant; setVariant: (v: Variant) => void }) {
  const st = useStoreCtx();
  const { s } = st;
  const params = new URLSearchParams(location.search);
  const drives = readyDrives();
  const url = (k: string, val: string) => { const u = new URL(location.href); u.searchParams.set(k, val); history.replaceState(null, '', u); };

  function changeVariant(next: Variant) {
    setVariant(next);
    url('variant', next);
    st.set({ step: 'map', card: null, folded: false, chatOpen: null, drawer: null, dialog: 'closed' });
  }
  const cycle = (d: number) => { const k: Variant[] = ['A', 'B', 'C']; changeVariant(k[(k.indexOf(variant) + d + 3) % 3]!); };
  const setAi = (ai: AiState) => { st.setAi(ai); url('state', ai); };
  const setPrivacy = (p: Privacy) => { st.set({ privacy: p, privacyAsk: false }); url('privacy', p); };

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest('input,textarea,select,[contenteditable],[role="radio"],[role="dialog"],[role="alertdialog"]')) return;
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') { e.preventDefault(); cycle(e.key === 'ArrowLeft' ? -1 : 1); }
    };
    addEventListener('keydown', key);
    return () => removeEventListener('keydown', key);
  });
  // B has no dialog: a Claude action while AI is off opens the panel, which holds the turn-on step.
  useEffect(() => { if (variant === 'B' && s.dialog === 'open') st.set({ dialog: 'closed', folded: false }); }, [variant, s.dialog, st]);

  const switcher = import.meta.env.DEV || params.get('review') === '1';
  useEffect(() => { document.body.classList.toggle('has-switcher', switcher); }, [switcher]);

  const total = planTotals(st.proposals, s.plan).bytes;
  const all = planTotals(st.proposals, st.proposals.map((p) => p.id)).bytes;
  const jobs = Object.entries(s.jobs).filter(([, j]) => j.phase !== 'idle').map(([k, j]) => `${k.replace('chat:', 'chat ')} ${j.phase}${j.slow ? ' (slow)' : ''}`);
  const state = [
    `AI ${s.ai}`, isOn(s.ai) ? `privacy ${s.privacy}` : `privacy ${s.privacy} (when on)`,
    s.step === 'review' ? 'review step' : s.confirm ? 'confirm' : s.planOpen ? 'plan open' : 'map',
    `${s.plan.length} in plan, ${formatBytes(total)}`, s.extra.length ? `${s.extra.length} Claude items` : null,
    ...jobs.slice(-2),
  ].filter(Boolean).join(' · ');

  const body = variant === 'A' ? <VariantA /> : variant === 'B' ? <VariantB /> : <VariantC />;
  return (
    <>
      <Shell
        platform="windows"
        drive={<DriveMenu drives={drives} />}
        advisor={<Trigger onOpen={variant === 'B' ? () => st.set({ folded: false }) : st.openDialog} />}
        reclaimable={all}
        action={!s.planOpen && s.step === 'map' ? <button type="button" className="cta cta--sm" onClick={() => st.set({ planOpen: true })}>Build a plan</button> : null}
      >
        {body}
      </Shell>
      {s.dialog !== 'closed' && variant !== 'B' && <AdvisorDialog />}
      {s.privacyAsk && <PrivacyConfirm />}
      {s.confirm && <Confirm />}
      {s.ended && <Ended />}
      {switcher && (
        <div className="prototype-switcher" data-no-ripple>
          <span className="prototype-label">THROWAWAY</span>
          <button aria-label="Previous variant" onClick={() => cycle(-1)}><ArrowLeft size={14} /></button>
          <span className="variant-label">{variant} · {names[variant]}{variant === RECOMMENDED && ' · Recommended'}</span>
          <button aria-label="Next variant" onClick={() => cycle(1)}><ArrowRight size={14} /></button>
          <span className="switcher-divider" />
          <button className={isOn(s.ai) ? 'on' : ''} onClick={() => setAi(isOn(s.ai) ? 'off' : 'on')} aria-label="AI on or off">AI {isOn(s.ai) ? 'on' : 'off'}</button>
          <select aria-label="Advisor state" value={s.ai} onChange={(e) => setAi(e.target.value as AiState)}>
            {AI_STATES.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
          <button className={s.privacy === 'off' ? 'on' : ''} onClick={() => setPrivacy(s.privacy === 'on' ? 'off' : 'on')} aria-label="Privacy mode">Privacy {s.privacy}</button>
          <span className="switcher-divider" />
          <button onClick={() => { const p = new URLSearchParams(location.search); const a = p.get('state') as AiState | null; st.set(fresh(a && AI_STATES.includes(a) ? a : 'off', p.get('privacy') === 'off' ? 'off' : 'on')); }}>Start over</button>
          <span className="state-label" title={state}>{state}</span>
        </div>
      )}
    </>
  );
}

import { useS as useStoreCtx } from './store';

createRoot(document.getElementById('root')!).render(<StrictMode><Prototype /></StrictMode>);

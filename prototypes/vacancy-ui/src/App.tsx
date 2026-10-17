// PROTOTYPE: variants of the whole Vacancy flow (D is the picked direction), switchable via ?variant=D|A|B|C, ?os=win|mac, ?step=scan|overview|plan|run|summary.
// Inside D, ?picker=1|2|3 picks one of three drive-picker designs; the floating switcher cycles those while D is shown.
import { useEffect, useState } from 'react';
import { datasets, defaultSelection, type OS } from './shared/data';
import { STEPS, type Picker, type Step, type VariantModule } from './shared/types';
import { WindowFrame } from './shared/WindowFrame';
import { PrototypeSwitcher } from './shared/PrototypeSwitcher';
import { variant as A } from './variants/A';
import { variant as B } from './variants/B';
import { variant as C } from './variants/C';
import { variant as D } from './variants/D';

const VARIANTS: VariantModule[] = [D, A, B, C];
const PICKERS: { key: string; name: string }[] = [
  { key: '1', name: 'Drive cards' },
  { key: '2', name: 'The plate is the picker' },
  { key: '3', name: 'Drives as a shelf' },
];

function readParams() {
  const q = new URLSearchParams(location.search);
  const v = q.get('variant') ?? 'D';
  const os = (q.get('os') === 'mac' ? 'mac' : 'win') as OS;
  const s = q.get('step') as Step | null;
  const pk = Number(q.get('picker'));
  const picker = (pk === 2 || pk === 3 ? pk : 1) as Picker;
  return { v: VARIANTS.some((x) => x.key === v) ? v : 'D', os, step: s && STEPS.includes(s) ? s : 'scan', picker };
}

export function App() {
  const [p, setP] = useState(readParams);
  const data = datasets[p.os];
  const [sel, setSel] = useState(() => defaultSelection(data));

  useEffect(() => {
    const q = new URLSearchParams({ variant: p.v, ...(p.v === 'D' ? { picker: String(p.picker) } : {}), os: p.os, step: p.step });
    history.replaceState(null, '', `?${q}`);
  }, [p]);

  const mod = VARIANTS.find((x) => x.key === p.v)!;
  const setOs = (os: OS) => {
    setSel(defaultSelection(datasets[os]));
    setP({ ...p, os });
  };

  return (
    <>
      <div className="pf-stage">
        <WindowFrame os={p.os} theme={mod.frame}>
          <mod.Component
            key={`${p.v}-${p.os}`}
            data={data}
            picker={p.picker}
            step={p.step}
            onStep={(step) => setP((cur) => ({ ...cur, step }))}
            proposals={sel.proposals}
            setProposals={(proposals) => setSel((cur) => ({ ...cur, proposals }))}
            reorgs={sel.reorgs}
            setReorgs={(reorgs) => setSel((cur) => ({ ...cur, reorgs }))}
          />
        </WindowFrame>
      </div>
      <PrototypeSwitcher
        variants={p.v === 'D' ? PICKERS : VARIANTS.map(({ key, name }) => ({ key, name }))}
        current={p.v === 'D' ? String(p.picker) : p.v}
        os={p.os}
        step={p.step}
        onVariant={(k) => (p.v === 'D' ? setP({ ...p, picker: Number(k) as Picker }) : setP({ ...p, v: k }))}
        onOs={setOs}
        onStep={(step) => setP({ ...p, step })}
      />
    </>
  );
}

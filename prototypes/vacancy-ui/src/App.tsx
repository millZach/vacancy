// PROTOTYPE: three variants of the whole Vacancy flow, switchable via ?variant=A|B|C, ?os=win|mac, ?step=scan|overview|plan|run|summary.
import { useEffect, useState } from 'react';
import { datasets, defaultSelection, type OS } from './shared/data';
import { STEPS, type Step, type VariantModule } from './shared/types';
import { WindowFrame } from './shared/WindowFrame';
import { PrototypeSwitcher } from './shared/PrototypeSwitcher';
import { variant as A } from './variants/A';
import { variant as B } from './variants/B';
import { variant as C } from './variants/C';

const VARIANTS: VariantModule[] = [A, B, C];

function readParams() {
  const q = new URLSearchParams(location.search);
  const v = q.get('variant') ?? 'A';
  const os = (q.get('os') === 'mac' ? 'mac' : 'win') as OS;
  const s = q.get('step') as Step | null;
  return { v: VARIANTS.some((x) => x.key === v) ? v : 'A', os, step: s && STEPS.includes(s) ? s : 'scan' };
}

export function App() {
  const [p, setP] = useState(readParams);
  const data = datasets[p.os];
  const [sel, setSel] = useState(() => defaultSelection(data));

  useEffect(() => {
    const q = new URLSearchParams({ variant: p.v, os: p.os, step: p.step });
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
        variants={VARIANTS.map(({ key, name }) => ({ key, name }))}
        current={p.v}
        os={p.os}
        step={p.step}
        onVariant={(v) => setP({ ...p, v })}
        onOs={setOs}
        onStep={(step) => setP({ ...p, step })}
      />
    </>
  );
}

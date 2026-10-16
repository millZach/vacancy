// PROTOTYPE variant C, "Neon Ledger": a dense, keyboard-first ledger under a real neon VACANCY sign.
import type { VariantModule, VariantProps } from '../../shared/types';
import { ScanStage } from './scan';
import { Desk } from './desk';
import { RunRoom } from './run';
import { Wrapped } from './summary';
import './vc.css';

function NeonLedger(props: VariantProps) {
  const { step } = props;
  return (
    <div className={`vc-root vc-step-${step}`}>
      {step === 'scan' && <ScanStage {...props} />}
      {(step === 'overview' || step === 'plan') && <Desk {...props} mode={step === 'overview' ? 'where' : 'plan'} />}
      {step === 'run' && <RunRoom {...props} />}
      {step === 'summary' && <Wrapped {...props} />}
    </div>
  );
}

export const variant: VariantModule = {
  key: 'C',
  name: 'Neon Ledger',
  frame: { bg: '#080b19', fg: '#8c93b8', border: '#1a2141' },
  Component: NeonLedger,
};

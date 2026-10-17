import { useEffect, useState } from 'react';
import { useBridge, type AdvisorStatus } from '../bridge';

export type AdvisorState = { phase: 'loading' } | { phase: 'ready'; status: AdvisorStatus } | { phase: 'error' };

/** Read-only availability. A status failure never blocks rules, and no UI preference can enable a provider. */
export function useAdvisorStatus() {
  const bridge = useBridge();
  const [state, setState] = useState<AdvisorState>({ phase: 'loading' });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let current = true;
    bridge.advisorStatus().then(
      (status) => {
        if (current) setState(status.enabled === false ? { phase: 'ready', status } : { phase: 'error' });
      },
      () => current && setState({ phase: 'error' }),
    );
    return () => { current = false; };
  }, [bridge, attempt]);
  const retry = () => {
    setState({ phase: 'loading' });
    setAttempt((n) => n + 1);
  };
  return { state, retry };
}

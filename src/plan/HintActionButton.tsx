import { ExternalLink } from 'lucide-react';
import { useState } from 'react';
import { useBridge } from '../bridge';

/** The shell resolves scan authority by these ids; the UI never supplies an external URL or command. */
export function HintActionButton({ mountPoint, proposalId }: {
  mountPoint: string;
  proposalId: string;
}) {
  const bridge = useBridge();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const open = () => {
    if (pending) return;
    setPending(true);
    setError(null);
    bridge.openHint(mountPoint, proposalId).then(
      () => setPending(false),
      (reason: unknown) => {
        setPending(false);
        setError(reason instanceof Error ? reason.message : String(reason));
      },
    );
  };
  return <span className="hint-action">
    <button type="button" className="btn" onClick={open} disabled={pending}>
      <ExternalLink size={12} aria-hidden /> {pending ? 'Opening Steam…' : 'Open Steam uninstall'}
    </button>
    {error && <span className="hint-action-error" role="alert">{error}</span>}
  </span>;
}

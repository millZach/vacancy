import { useBridge } from './bridge';
import { useSystemDrive } from './drive';
import { ScanScreen } from './scan/ScanScreen';
import { Shell } from './shell/Shell';

/** Which AI agent Vacancy found. Detection of Claude Code and Codex arrives with ticket 7; until then, rules only. */
const AGENT: string | null = null;

export function App() {
  const bridge = useBridge();
  const drive = useSystemDrive();
  return (
    <Shell platform={bridge.platform} drive={drive} agent={AGENT}>
      <ScanScreen drive={drive} agent={AGENT} />
    </Shell>
  );
}

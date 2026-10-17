import { useBridge } from './bridge';
import { Atlas } from './atlas/Atlas';
import { DrivesPending } from './atlas/DrivesPending';
import { useDrives } from './drives/context';
import { DriveMenu } from './drives/DriveMenu';
import { Shell } from './shell/Shell';

/** Which AI agent Vacancy found. Detection of Claude Code and Codex arrives with ticket 7; until then, rules only. */
const AGENT: string | null = null;

export function App() {
  const bridge = useBridge();
  const drives = useDrives();
  const ready = drives.status === 'ready' ? drives : null;

  return (
    <Shell platform={bridge.platform} drive={ready && <DriveMenu drives={ready} />} agent={AGENT}>
      {ready ? (
        <Atlas drives={ready} agent={AGENT} />
      ) : (
        <DrivesPending error={drives.status === 'error' ? drives.message : null} />
      )}
    </Shell>
  );
}

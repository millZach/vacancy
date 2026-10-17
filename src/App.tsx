import { useState } from 'react';
import { useBridge, type VolumeInfo } from './bridge';
import { Atlas } from './atlas/Atlas';
import { DrivesPending } from './atlas/DrivesPending';
import { useDrives } from './drives';
import { useScan } from './scan/useScan';
import { Shell } from './shell/Shell';

/** Which AI agent Vacancy found. Detection of Claude Code and Codex arrives with ticket 7; until then, rules only. */
const AGENT: string | null = null;

export function App() {
  const bridge = useBridge();
  const drives = useDrives();
  const scan = useScan();
  const [picked, setPicked] = useState<VolumeInfo | null>(null);

  const selected = drives.status === 'ready' ? (picked ?? drives.volumes[0] ?? null) : null;
  const s = scan.state;
  // The header follows the drive being scanned or shown; after a scan, with the OS's figures from its end.
  const shown = s.phase === 'done' ? s.overview.volume : s.phase === 'idle' ? selected : s.volume;

  return (
    <Shell platform={bridge.platform} volume={shown} agent={AGENT}>
      {drives.status === 'ready' && selected ? (
        <Atlas drives={drives} selected={selected} onSelect={setPicked} scan={scan} agent={AGENT} />
      ) : (
        <DrivesPending error={drives.status === 'error' ? drives.message : null} />
      )}
    </Shell>
  );
}

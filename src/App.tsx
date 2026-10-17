import { useState } from 'react';
import { useBridge } from './bridge';
import { Atlas } from './atlas/Atlas';
import { DrivesPending } from './atlas/DrivesPending';
import { useDrives } from './drives/context';
import { DriveMenu } from './drives/DriveMenu';
import { reclaimableOf } from './drives/session';
import { Shell } from './shell/Shell';

/** Which AI agent Vacancy found. Detection of Claude Code and Codex arrives with ticket 7; until then, rules only. */
const AGENT: string | null = null;

export function App() {
  const bridge = useBridge();
  const drives = useDrives();
  const ready = drives.status === 'ready' ? drives : null;
  // The plan tray beside the map. Each drive keeps its own plan; the tray shows the plan of the drive on screen.
  const [planOpen, setPlanOpen] = useState(false);
  const scan = ready?.current.scan;
  const onMap = ready?.screen === 'map' && scan?.phase === 'done';
  const proposals = onMap && scan.phase === 'done' ? scan.proposals : [];

  return (
    <Shell
      platform={bridge.platform}
      drive={ready && <DriveMenu drives={ready} />}
      agent={AGENT}
      reclaimable={onMap && scan ? reclaimableOf(scan) : null}
      action={
        onMap && !planOpen && proposals.length > 0 ? (
          <button type="button" className="cta cta--sm" onClick={() => setPlanOpen(true)}>
            Build a plan
          </button>
        ) : null
      }
    >
      {ready ? (
        <Atlas drives={ready} agent={AGENT} planOpen={planOpen} onPlanOpen={setPlanOpen} />
      ) : (
        <DrivesPending error={drives.status === 'error' ? drives.message : null} />
      )}
    </Shell>
  );
}

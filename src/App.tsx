import { useState } from 'react';
import { useBridge, type RunReport } from './bridge';
import { Atlas } from './atlas/Atlas';
import { DrivesPending } from './atlas/DrivesPending';
import { useDrives } from './drives/context';
import { DriveMenu } from './drives/DriveMenu';
import { reclaimableOf, reportOf } from './drives/session';
import { HistoryMenu } from './history/HistoryMenu';
import { Shell } from './shell/Shell';
import { Finish } from './wrapped/Finish';

/** Which AI agent Vacancy found. Detection of Claude Code and Codex arrives with ticket 7; until then, rules only. */
const AGENT: string | null = null;

export function App() {
  const bridge = useBridge();
  const drives = useDrives();
  const ready = drives.status === 'ready' ? drives : null;
  // The plan tray beside the map. Each drive keeps its own plan; the tray shows the plan of the drive on screen,
  // and closes when another drive comes on screen.
  const [planOpen, setPlanOpen] = useState(false);
  // A run reopened from the history, shown in place of the drive's screen until it is closed.
  const [past, setPast] = useState<RunReport | null>(null);
  const onScreen = ready?.current.volume.mountPoint ?? null;
  const [seenDrive, setSeenDrive] = useState(onScreen);
  if (seenDrive !== onScreen) {
    setSeenDrive(onScreen);
    setPlanOpen(false);
    setPast(null);
  }
  const scan = ready?.current.scan;
  const onMap = !past && ready?.screen === 'map' && scan?.phase === 'done';
  const proposals = onMap && scan.phase === 'done' ? scan.proposals : [];
  // Once the plan runs, its proposals are spent: no reclaimable figure and no new plan until the drive is scanned again.
  const planRun = scan?.phase === 'done' ? scan.planRun : null;
  const planning = !planRun || planRun.phase === 'failed';
  const report = onMap && scan ? reportOf(scan) : null;

  return (
    <Shell
      platform={bridge.platform}
      drive={ready && <DriveMenu drives={ready} />}
      agent={AGENT}
      reclaimable={onMap && scan && planning ? reclaimableOf(scan) : null}
      lit={(!!report && report.freedBytes > 0) || (!!past && past.freedBytes > 0)}
      history={ready && <HistoryMenu locked={ready.locked} onOpen={setPast} />}
      action={
        onMap && !planOpen && !planRun && proposals.length > 0 ? (
          <button type="button" className="cta cta--sm" onClick={() => setPlanOpen(true)}>
            Build a plan
          </button>
        ) : null
      }
    >
      {past ? (
        <Finish key={past.id} report={past} start="rest" onClose={() => setPast(null)} />
      ) : ready ? (
        <Atlas drives={ready} agent={AGENT} planOpen={planOpen} onPlanOpen={setPlanOpen} />
      ) : (
        <DrivesPending error={drives.status === 'error' ? drives.message : null} />
      )}
    </Shell>
  );
}

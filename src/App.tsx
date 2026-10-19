import { useCallback, useState } from 'react';
import { useBridge, type RunReport } from './bridge';
import { Atlas } from './atlas/Atlas';
import { DrivesPending } from './atlas/DrivesPending';
import { useDrives } from './drives/context';
import { DriveMenu } from './drives/DriveMenu';
import { reclaimableOf, reportOf } from './drives/session';
import { HistoryMenu } from './history/HistoryMenu';
import { Shell } from './shell/Shell';
import { Finish } from './wrapped/Finish';
import { AdvisorDialog, AdvisorNote, AdvisorTrigger } from './advisor/Advisor';
import { useAdvisorStatus } from './advisor/useAdvisorStatus';

export function App() {
  const bridge = useBridge();
  const drives = useDrives();
  const advisor = useAdvisorStatus();
  const [advisorOpen, setAdvisorOpen] = useState(false);
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
  // Undo of a folder's Tidy up, from wherever its run shows: the drive's summary, a reopened past run, or Past runs.
  // The core changes the kept report; every view of that run takes the new one.
  const reportUpdated = ready?.reportUpdated;
  const undoTidy = useCallback(
    async (runId: string, tidyId: string) => {
      const updated = await bridge.undoTidy(runId, tidyId);
      reportUpdated?.(updated);
      setPast((p) => (p && p.id === updated.id ? updated : p));
      return updated;
    },
    [bridge, reportUpdated],
  );

  return (
    <>
      <Shell
        platform={bridge.platform}
        drive={ready && <DriveMenu drives={ready} />}
        advisor={<AdvisorTrigger open={advisorOpen} onOpen={() => setAdvisorOpen(true)} />}
        reclaimable={onMap && scan && planning ? reclaimableOf(scan) : null}
        lit={(!!report && report.freedBytes > 0) || (!!past && past.freedBytes > 0)}
        history={ready && <HistoryMenu locked={ready.locked} onOpen={setPast} {...(bridge.platform === 'windows' ? { onUndo: undoTidy } : {})} />}
        action={
          onMap && !planOpen && !planRun && proposals.length > 0 ? (
            <button type="button" className="cta cta--sm" onClick={() => setPlanOpen(true)}>
              Build a plan
            </button>
          ) : null
        }
      >
        {past ? (
          <Finish
            key={past.id}
            report={past}
            start="rest"
            onClose={() => setPast(null)}
            {...(bridge.platform === 'windows' ? { onUndo: (tidyId: string) => undoTidy(past.id, tidyId) } : {})}
          />
        ) : ready ? (
          <Atlas
            drives={ready}
            agent={null}
            advisorNote={<AdvisorNote state={advisor.state} onOpen={() => setAdvisorOpen(true)} />}
            advisorOpen={advisorOpen}
            planOpen={planOpen}
            onPlanOpen={setPlanOpen}
            onUndo={undoTidy}
          />
        ) : (
          <DrivesPending error={drives.status === 'error' ? drives.message : null} />
        )}
      </Shell>
      {advisorOpen && <AdvisorDialog state={advisor.state} onRetry={advisor.retry} onClose={() => setAdvisorOpen(false)} />}
    </>
  );
}

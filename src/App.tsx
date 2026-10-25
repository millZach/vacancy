import { useCallback, useState } from 'react';
import { useBridge, type RunReport } from './bridge';
import { Atlas } from './atlas/Atlas';
import { DrivesPending } from './atlas/DrivesPending';
import { ClaudeNote, ClaudeTrigger, PrivacyConfirm } from './claude/bits';
import { ClaudeProvider } from './claude/ClaudeProvider';
import { scanUnavailable, useClaude } from './claude/context';
import { useDrives } from './drives/context';
import { DriveMenu } from './drives/DriveMenu';
import { reclaimableOf, reportOf } from './drives/session';
import { HistoryMenu } from './history/HistoryMenu';
import { Shell } from './shell/Shell';
import { Finish } from './wrapped/Finish';

export function App() {
  const drives = useDrives();
  return (
    <ClaudeProvider drives={drives.status === 'ready' ? drives : null}>
      <Screens />
    </ClaudeProvider>
  );
}

function Screens() {
  const bridge = useBridge();
  const drives = useDrives();
  const claude = useClaude();
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
  // Claude's panel is beside the scan screen, and beside the map while the plan can still change (not while a plan
  // runs or beside its finish, nor over a past run); the header's control unfolds it.
  const where = onMap && planning && scan.phase === 'done' ? { mountPoint: scan.overview.volume.mountPoint, run: scan.run } : null;
  const panelShown = !past && !!ready && (ready.screen !== 'map' || scan?.phase !== 'done' || planning);
  const entries = where && claude ? claude.conversation(where).entries : [];
  const busy = entries.some((e) => e.phase === 'working' || e.phase === 'slow');
  const last = entries.at(-1);
  const failed = last?.phase === 'failed' && last.failure?.kind !== 'cancelled';
  const closed = where && claude ? scanUnavailable(claude.conversation(where)) !== null : false;

  return (
    <>
      <Shell
        platform={bridge.platform}
        drive={ready && <DriveMenu drives={ready} />}
        advisor={<ClaudeTrigger busy={busy} failed={failed} closed={closed} onOpen={panelShown && claude ? () => claude.setFolded(false) : null} />}
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
            agent="Claude"
            advisorNote={<ClaudeNote />}
            advisorOpen={!!claude?.privacyAsk}
            planOpen={planOpen}
            onPlanOpen={setPlanOpen}
            onUndo={undoTidy}
          />
        ) : (
          <DrivesPending error={drives.status === 'error' ? drives.message : null} />
        )}
      </Shell>
      {claude?.privacyAsk && <PrivacyConfirm claude={claude} />}
    </>
  );
}

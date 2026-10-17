// PROTOTYPE drive picker (variant D, ?picker=1|2|3). Holds one scan, map, plan and summary per drive and renders
// Atlas for the current one, with the chosen picker design plugged into its header, scan plate and (design 3) rail.
// Rules (Zach's decisions): one drive at a time; a scanned drive reopens where you left it without rescanning; an
// unscanned drive opens its scan screen; switching is locked only while a run is deleting.
import { useEffect, useMemo, useState } from 'react';
import { defaultSelection } from '../../../shared/data';
import { drives } from '../../../shared/drives';
import { buildRun } from '../../../shared/run';
import type { Step, VariantProps } from '../../../shared/types';
import { Atlas, type AtlasSlots } from '../Atlas';
import type { DriveView, PickerApi } from './common';
import { CardsPlate, DriveMenu } from './Cards';
import { DriveChips, FlipPlate } from './Flip';
import { DriveCrumb, DriveShelf, ShelfPlate } from './Shelf';
import './drives.css';

interface Rec {
  scannedAt: number | null;
  /** Where to reopen this drive: its last map step. */
  last: Step;
  cleaned: boolean;
  proposals: Set<string>;
  reorgs: Set<string>;
}

const reclaimable = (d: (typeof drives)['win'][number]) => d.data.proposals.filter((p) => p.action === 'delete').reduce((s, p) => s + p.bytes, 0);

export function DriveAtlas(props: VariantProps) {
  const { step } = props;
  const os = props.data.os;
  const list = drives[os];
  const picker = props.picker ?? 1;
  const [cur, setCur] = useState(() => list.find((d) => d.system)!.id);
  // Bumped when a switch should start the new drive fresh (its own map state, focus, summary phase).
  const [gen, setGen] = useState(0);
  const [scanning, setScanning] = useState(false);
  const [recs, setRecs] = useState<Record<string, Rec>>(() => {
    // Opening the prototype past the scan step means the system drive was scanned a few minutes ago.
    const past = step !== 'scan';
    return Object.fromEntries(
      list.map((d) => {
        const sel = defaultSelection(d.data);
        const seeded = d.system && past;
        return [d.id, { scannedAt: seeded ? Date.now() - 4 * 60_000 : null, last: seeded && step !== 'run' ? step : 'overview', cleaned: false, ...sel }];
      }),
    );
  });
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);

  const patch = (id: string, p: Partial<Rec>) => setRecs((r) => ({ ...r, [id]: { ...r[id], ...p } }));

  // Any step past scanning (the scan finishing, or the switcher jumping ahead) means this drive has been scanned.
  useEffect(() => {
    if (step === 'scan') return;
    setRecs((r) => {
      const c = r[cur];
      const last = step === 'run' ? c.last : step;
      const cleaned = c.cleaned || step === 'summary';
      if (c.scannedAt !== null && c.last === last && c.cleaned === cleaned) return r;
      return { ...r, [cur]: { ...c, scannedAt: c.scannedAt ?? Date.now(), last, cleaned } };
    });
  }, [step, cur]);

  const views: DriveView[] = useMemo(
    () =>
      list.map((d) => {
        const r = recs[d.id];
        const freed = r.cleaned ? buildRun(d.data, r.proposals, r.reorgs).freedBytes : 0;
        const used = d.usedBytes - freed;
        return {
          drive: d,
          current: d.id === cur,
          scanned: r.scannedAt !== null,
          scannedAt: r.scannedAt,
          cleaned: r.cleaned,
          freed,
          used,
          free: d.totalBytes - used,
          reclaim: reclaimable(d),
        };
      }),
    [list, recs, cur],
  );
  const curView = views.find((v) => v.current)!;
  const rec = recs[cur];
  const locked = step === 'run';

  const api: PickerApi = {
    views,
    cur: curView,
    machine: curView.drive.data.machineName,
    agent: curView.drive.data.scan.aiAgent,
    locked,
    now,
    switchTo(id) {
      if (locked) return;
      const r = recs[id];
      const target: Step = r.scannedAt !== null ? r.last : 'scan';
      // Moving between two scan screens keeps the screen (and its dot field) in place; anything else starts fresh.
      if (!(step === 'scan' && target === 'scan' && !scanning)) setGen((g) => g + 1);
      if (id !== cur) setCur(id);
      if (target !== step) props.onStep(target);
    },
    pick(id) {
      if (id !== cur) setCur(id);
    },
    openMap() {
      setGen((g) => g + 1);
      props.onStep(rec.last);
    },
  };

  const slots: AtlasSlots =
    picker === 1
      ? { headDrive: (ctx) => <DriveMenu api={api} ctx={ctx} />, plate: (scan) => <CardsPlate api={api} scan={scan} /> }
      : picker === 2
        ? { headDrive: () => <DriveChips api={api} />, plate: (scan) => <FlipPlate api={api} scan={scan} /> }
        : {
            headDrive: (ctx) => <DriveCrumb api={api} ctx={ctx} />,
            plate: (scan) => <ShelfPlate api={api} scan={scan} />,
            rail: <DriveShelf api={api} />,
            hideCrumbs: true,
          };

  return (
    <div className={`vd-picker vd-picker-${picker}`}>
      <Atlas
        key={gen}
        {...props}
        {...slots}
        data={curView.drive.data}
        onStep={(s) => {
          // A scan that just finished (not a switch) is the moment this drive's scan happened.
          if (step === 'scan' && s !== 'scan') patch(cur, { scannedAt: Date.now(), cleaned: false });
          props.onStep(s);
        }}
        proposals={rec.proposals}
        setProposals={(proposals) => patch(cur, { proposals })}
        reorgs={rec.reorgs}
        setReorgs={(reorgs) => patch(cur, { reorgs })}
        restSummary={rec.cleaned}
        onScanning={setScanning}
      />
    </div>
  );
}

import { ChevronRight, LoaderCircle, RotateCcw, ShieldAlert } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useBridge, type CategoryId, type MapFolder, type VolumeInfo } from '../bridge';
import { DotField, type DotFieldHandle } from '../dotfield/DotField';
import type { DrivesState } from '../drives';
import { errorMessage } from '../errors';
import { formatCount } from '../format';
import { Legend } from '../map/Legend';
import { MapView } from '../map/MapView';
import { folderModel, overviewModel, scanModel, type Leaf, type Region } from '../map/model';
import { useSize } from '../map/useSize';
import { ScanHud } from '../scan/ScanHud';
import { ScanPlate } from '../scan/ScanPlate';
import type { Scan } from '../scan/useScan';
import './atlas.css';

type Ready = Extract<DrivesState, { status: 'ready' }>;

interface Props {
  drives: Ready;
  /** The drive the plate offers to scan. */
  selected: VolumeInfo;
  onSelect: (volume: VolumeInfo) => void;
  scan: Scan;
  /** The AI agent Vacancy found, or null for built-in rules only. */
  agent: string | null;
}

/** Where the map is zoomed: a category, then folders opened inside it. */
type Level = { kind: 'category'; id: CategoryId } | { kind: 'folder'; folder: MapFolder };

/** Scan progress shown before the scan says it is done: never quite full. */
const ALMOST = 0.98;

/**
 * Variant D's map screen. Before a scan: the drive plate on the living dot field. While scanning: the HUD over
 * the map as it fills in. After: the overview treemap, where a click zooms into a category, then opens its
 * folders, with a breadcrumb back out; hovering shows what a tile is, how big and when it was last used.
 */
export function Atlas({ drives, selected, onSelect, scan, agent }: Props) {
  const bridge = useBridge();
  const field = useRef<DotFieldHandle>(null);
  const [calm, setCalm] = useState<HTMLDivElement | null>(null);
  const [stageRef, size] = useSize<HTMLDivElement>();
  const [levels, setLevels] = useState<Level[]>([]);
  const [hot, setHot] = useState<string | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [browseError, setBrowseError] = useState<string | null>(null);

  const state = scan.state;
  const overview = state.phase === 'done' ? state.overview : null;
  const scanning = state.phase === 'scanning' ? state : null;
  const [seen, setSeen] = useState(overview);
  if (seen !== overview) {
    // A new map starts at the whole disk.
    setSeen(overview);
    setLevels([]);
    setHot(null);
    setOpening(null);
    setBrowseError(null);
  }
  // Which folder opening is still wanted. Any move on the map, or a new map, makes a pending one stale, so a
  // folder that arrives late never pulls the map back to where the user was.
  const request = useRef(0);
  useEffect(() => {
    request.current++;
  }, [overview]);

  const top = useMemo(() => (overview ? overviewModel(overview) : null), [overview]);
  const level = levels.at(-1);
  const model = useMemo(() => {
    if (scanning) return scanModel(scanning.volume, scanning.preview, scanning.update?.bytes ?? 0);
    if (!overview || !top) return null;
    if (level?.kind === 'folder') {
      const cat = overview.categories.find((c) => c.id === level.folder.category);
      return folderModel(level.folder, { name: cat?.name ?? '', protected: cat?.protected ?? false }, overview.volume);
    }
    return top;
  }, [scanning, overview, top, level]);
  const focus = level?.kind === 'category' ? level.id : null;

  const progress = scanning
    ? Math.min(ALMOST, (scanning.update?.bytes ?? 0) / Math.max(1, scanning.volume.usedBytes))
    : 0;

  const navigate = useCallback((next: Level[] | ((levels: Level[]) => Level[])) => {
    request.current++;
    setOpening(null);
    setBrowseError(null);
    setHot(null);
    setLevels(next);
  }, []);

  const open = useCallback(
    (leaf: Leaf) => {
      const id = ++request.current;
      setOpening(leaf.id);
      setBrowseError(null);
      bridge.browse(leaf.id).then(
        (folder) => {
          if (request.current !== id) return;
          setOpening(null);
          setHot(null);
          setLevels((ls) => [...ls, { kind: 'folder', folder }]);
        },
        (error: unknown) => {
          if (request.current !== id) return;
          setOpening(null);
          setBrowseError(errorMessage(error));
        },
      );
    },
    [bridge],
  );

  const up = useCallback(() => navigate((ls) => ls.slice(0, -1)), [navigate]);
  useEffect(() => {
    if (!overview) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') up();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [overview, up]);

  const onLeaf = (leaf: Leaf) => {
    if (levels.length === 0) navigate([{ kind: 'category', id: leaf.regionId as CategoryId }]);
    else if (leaf.browsable) open(leaf);
  };
  const onRegion = (region: Region) => {
    if (levels.length === 0 && region.category) navigate([{ kind: 'category', id: region.category }]);
    else up();
  };
  const clickable = (leaf: Leaf) => (levels.length === 0 ? leaf.kind !== 'free' : leaf.browsable);
  const leafAction = (leaf: Leaf) =>
    levels.length === 0 ? (leaf.kind === 'free' ? null : 'Click to zoom in.') : leaf.browsable ? 'Click to open.' : null;
  const regionAction = () => (levels.length === 0 ? 'Click to zoom in.' : 'Click to zoom out.');

  const startScan = (button: HTMLButtonElement) => {
    const b = button.getBoundingClientRect();
    field.current?.drop(b.left + b.width / 2, b.top + b.height / 2, 'splash');
    scan.start(selected);
  };

  const onPlate = state.phase === 'idle' || state.phase === 'failed';
  const bytesInView =
    level?.kind === 'folder'
      ? level.folder.item.bytes
      : (focus && overview?.categories.find((c) => c.id === focus)?.bytes) || (overview?.volume.totalBytes ?? 0);

  return (
    <main className={overview ? 'atlas is-map' : 'atlas is-scan'}>
      {!overview && <DotField ref={field} calm={onPlate ? calm : null} scan={scanning ? progress : null} out={progress > 0.85} />}
      <div className="atlas-toolbar">
        <nav className="crumbs" aria-label="Map location">
          {levels.length === 0 || !overview ? (
            <span className="crumb is-here" aria-current="location">
              {(overview?.volume ?? scanning?.volume ?? selected).name}
            </span>
          ) : (
            <button type="button" className="crumb" onClick={() => navigate([])}>
              {overview.volume.name}
            </button>
          )}
          {overview &&
            levels.map((l, i) => {
              const last = i === levels.length - 1;
              const name =
                l.kind === 'category'
                  ? (overview.categories.find((c) => c.id === l.id)?.name ?? l.id)
                  : l.folder.item.name;
              return (
                <span key={i} className="crumb-step">
                  <ChevronRight size={13} aria-hidden />
                  {last ? (
                    <span className="crumb is-here" aria-current="location">
                      {name}
                    </span>
                  ) : (
                    <button type="button" className="crumb" onClick={() => navigate(levels.slice(0, i + 1))}>
                      {name}
                    </button>
                  )}
                </span>
              );
            })}
          {overview && level && (
            <span className="crumb-blurb">
              {level.kind === 'category'
                ? overview.categories.find((c) => c.id === level.id)?.blurb
                : level.folder.item.path}
            </span>
          )}
          {scanning && <span className="crumb-blurb">Mapping as it scans</span>}
          {opening && <LoaderCircle className="crumb-spin" size={13} aria-label="Opening" />}
          {browseError && (
            <span className="crumb-error" role="alert">
              {browseError}
            </span>
          )}
        </nav>
        {overview && (
          <>
            <ScanNotes
              unreadable={overview.scan.unreadable}
              samples={overview.scan.unreadableSamples}
              missingAccess={drives.fullDiskAccess === 'missing'}
              onOpenAccess={() => void bridge.openFullDiskAccessSettings()}
            />
            <Legend bytesInView={bytesInView} w={size.w} h={size.h} />
            <button type="button" className="btn btn--sm" onClick={scan.reset}>
              <RotateCcw size={12} aria-hidden /> Scan again
            </button>
          </>
        )}
      </div>
      <div className={onPlate ? 'atlas-stage is-empty' : 'atlas-stage'} ref={stageRef}>
        {model && (
          <MapView
            model={model}
            width={size.w}
            height={size.h}
            focus={focus}
            interactive={!!overview}
            hot={hot}
            onHot={setHot}
            onLeaf={onLeaf}
            onRegion={onRegion}
            clickable={clickable}
            leafAction={leafAction}
            regionAction={regionAction}
          />
        )}
        {onPlate && (
          <ScanPlate
            volumes={drives.volumes}
            selected={selected}
            onSelect={onSelect}
            machineName={drives.machineName}
            fullDiskAccess={drives.fullDiskAccess}
            onOpenFullDiskAccess={() => void bridge.openFullDiskAccessSettings()}
            agent={agent}
            failure={state.phase === 'failed' ? state.message : null}
            onScan={startScan}
            contentRef={setCalm}
          />
        )}
        {scanning && <ScanHud update={scanning.update} progress={progress} onCancel={scan.cancel} />}
      </div>
    </main>
  );
}

/** After a scan: how many folders could not be read, and on a Mac without Full Disk Access, the way to fix it. */
function ScanNotes(p: { unreadable: number; samples: string[]; missingAccess: boolean; onOpenAccess: () => void }) {
  if (p.unreadable === 0 && !p.missingAccess) return null;
  return (
    <div className="scan-notes">
      {p.unreadable > 0 && (
        <span className="scan-note" title={`Couldn’t read:\n${p.samples.join('\n')}`}>
          {formatCount(p.unreadable)} {p.unreadable === 1 ? 'folder' : 'folders'} couldn’t be read
        </span>
      )}
      {p.missingAccess && (
        <button type="button" className="scan-note is-action" onClick={p.onOpenAccess}>
          <ShieldAlert size={12} aria-hidden /> Allow Full Disk Access
        </button>
      )}
    </div>
  );
}

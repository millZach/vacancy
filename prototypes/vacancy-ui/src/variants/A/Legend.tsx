// PROTOTYPE variant A "Atlas": the map key, with a scale square that shows how much area a few gigabytes take.
export type MapMode = 'scan' | 'overview' | 'plan' | 'run' | 'summary';

const UNITS = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100];

function scale(bytesInView: number, area: number) {
  if (!bytesInView || area <= 0) return null;
  let best: { u: number; side: number } | null = null;
  for (const u of UNITS) {
    const side = Math.sqrt((u * 1e9 * area) / bytesInView);
    if (side >= 9 && side <= 20) best = { u, side };
  }
  return best;
}

const unit = (u: number) => (u < 1 ? `${Math.round(u * 1000)} MB` : `${u} GB`);

export function Legend({ mode, bytesInView, w, h }: { mode: MapMode; bytesInView: number; w: number; h: number }) {
  const s = scale(bytesInView, w * h);
  return (
    <div className="va-legend" aria-label="Map key">
      {(mode === 'overview' || mode === 'plan') && (
        <span className="va-key">
          <i className="va-key-reclaim" />
          Reclaimable
        </span>
      )}
      {(mode === 'overview' || mode === 'plan') && (
        <span className="va-key">
          <i className="va-key-hint" />
          Advice
        </span>
      )}
      {(mode === 'run' || mode === 'summary') && (
        <span className="va-key">
          <i className="va-key-vacant" />
          Vacant, freed by this run
        </span>
      )}
      <span className="va-key">
        <i className="va-key-protected" />
        Protected
      </span>
      <span className="va-key">
        <i className="va-key-free" />
        Free
      </span>
      {s && (
        <span className="va-key va-key-scale" title={`A square this size holds ${unit(s.u)}`}>
          <i style={{ width: s.side, height: s.side }} />
          {unit(s.u)}
        </span>
      )}
    </div>
  );
}

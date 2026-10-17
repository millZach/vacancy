import { scaleSquare } from './scale';

const unit = (gb: number) => (gb < 1 ? `${Math.round(gb * 1000)} MB` : `${gb} GB`);

/**
 * The map key: reclaimable tiles (when the map has proposals), vacant ones (during and after a run), locked and free
 * tiles, and a square showing how much space a small area holds.
 */
export function Legend({
  bytesInView,
  w,
  h,
  reclaimable = false,
  vacant = false,
}: {
  bytesInView: number;
  w: number;
  h: number;
  reclaimable?: boolean;
  vacant?: boolean;
}) {
  const s = scaleSquare(bytesInView, w * h);
  return (
    <div className="legend" aria-label="Map key">
      {reclaimable && (
        <span className="key">
          <i className="key-reclaim" />
          Reclaimable
        </span>
      )}
      {vacant && (
        <span className="key">
          <i className="key-vacant" />
          Vacant, freed by this run
        </span>
      )}
      <span className="key">
        <i className="key-locked" />
        Protected
      </span>
      <span className="key">
        <i className="key-free" />
        Free
      </span>
      {s && (
        <span className="key key-scale" title={`A square this size holds ${unit(s.gb)}`}>
          <i style={{ width: s.side, height: s.side }} />
          {unit(s.gb)}
        </span>
      )}
    </div>
  );
}

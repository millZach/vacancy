import { scaleSquare } from './scale';

const unit = (gb: number) => (gb < 1 ? `${Math.round(gb * 1000)} MB` : `${gb} GB`);

/** The map key: locked and free tiles, and a square showing how much space a small area holds. */
export function Legend({ bytesInView, w, h }: { bytesInView: number; w: number; h: number }) {
  const s = scaleSquare(bytesInView, w * h);
  return (
    <div className="legend" aria-label="Map key">
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

import type { CSSProperties } from 'react';

const WORD = 'Vacancy';

/** The motel sign: the neon script over its unlit tube, each letter catching in turn. */
export function NeonSign() {
  return (
    <div className="sign" role="img" aria-label={WORD}>
      <span className="sign-off" aria-hidden>
        {WORD}
      </span>
      <span className="sign-lit" aria-hidden>
        {WORD.split('').map((ch, k) => (
          <span key={k} style={{ '--k': k } as CSSProperties}>
            {ch}
          </span>
        ))}
      </span>
    </div>
  );
}

/**
 * A motel letterboard: one row of tiles per line, each tile flipping down into place after the one before it.
 * `label` is what it says, for screen readers.
 */
export function Letterboard({ lines, label }: { lines: string[]; label: string }) {
  return (
    <div className="board" role="img" aria-label={label}>
      {lines.map((line, row) => (
        <span key={row} className="board-row" aria-hidden>
          {line.split('').map((ch, k) => {
            if (ch === ' ') return <span key={k} className="board-gap" />;
            return (
              <span key={k} className="board-tile" style={{ '--i': k, '--row': row } as CSSProperties}>
                {ch}
              </span>
            );
          })}
        </span>
      ))}
    </div>
  );
}

/** The biggest win's key fob: a room number, the bytes freed, and its category on the tag. */
export function KeyFob({ num, unit, foot }: { num: string; unit: string; foot: string }) {
  return (
    <div className="fob-wrap" aria-hidden>
      <span className="fob-ring" />
      <div className="fob">
        <span className="fob-hole" />
        <small>Room 1</small>
        <b>{num}</b>
        <em>{unit}</em>
        <span className="fob-foot">{foot}</span>
      </div>
    </div>
  );
}

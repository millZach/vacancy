import { formatBytes, NBSP } from '../format';

/**
 * What a run freed, the way the summary's hero shows it: "26" large, ".9" smaller, "GB" small. The checkout slide
 * uses the same markup, so its number can fly onto the summary's with one uniform scale.
 */
export function HeroNumber({ bytes }: { bytes: number }) {
  const [num = '', unit = ''] = formatBytes(bytes).split(NBSP);
  const [whole, frac] = num.split('.');
  return (
    <>
      {whole}
      {frac !== undefined && <span className="summary-frac">.{frac}</span>}
      <span className="summary-unit">{unit}</span>
    </>
  );
}

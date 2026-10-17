/** Map scale steps, in GB. */
const UNITS = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100];

/** The largest round amount whose square on the map is 9 to 20 px wide. (The prototype's scale square.) */
export function scaleSquare(bytesInView: number, area: number): { gb: number; side: number } | null {
  if (!bytesInView || area <= 0) return null;
  let best: { gb: number; side: number } | null = null;
  for (const gb of UNITS) {
    const side = Math.sqrt((gb * 1e9 * area) / bytesInView);
    if (side >= 9 && side <= 20) best = { gb, side };
  }
  return best;
}

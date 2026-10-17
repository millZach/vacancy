// When a kept run happened, the way the history list and a reopened summary say it, in the user's own locale:
// "Today, 09:12", "Yesterday, 18:40", "3 Oct, 18:00", "3 Oct 2025, 18:00".

const DAY_MS = 86_400_000;

function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const time = (ms: number) => new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(ms);

/** The day and time of `ms`, relative to `now` for today and yesterday. */
export function formatWhen(ms: number, now = Date.now()): string {
  const days = Math.round((startOfDay(now) - startOfDay(ms)) / DAY_MS);
  if (days === 0) return `Today, ${time(ms)}`;
  if (days === 1) return `Yesterday, ${time(ms)}`;
  const sameYear = new Date(ms).getFullYear() === new Date(now).getFullYear();
  const day = new Intl.DateTimeFormat(undefined, {
    day: 'numeric',
    month: 'short',
    ...(sameYear ? {} : { year: 'numeric' }),
  }).format(ms);
  return `${day}, ${time(ms)}`;
}

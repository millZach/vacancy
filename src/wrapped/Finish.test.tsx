import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BridgeProvider, createMockBridge, fixtures, type RunReport } from '../bridge';
import { fullReport, report } from '../test/run';
import { Finish } from './Finish';

const plain = (s: string | null | undefined) => (s ?? '').replace(/\u00a0/g, ' ');
const deck = () => screen.getByRole('region', { name: 'Highlights of this run' });
const slide = () => screen.getByRole('group', { name: /^\d+ of \d+: / });
const dots = () => within(screen.getByRole('navigation', { name: 'Highlight slides' })).getAllByRole('button', { name: /^Slide / });
const panel = () => screen.getByRole('complementary', { name: 'What Vacancy did' });
/** Lets the slide on screen leave and the next arrive (no Web Animations in jsdom: they finish at once). */
const settle = () => act(async () => {});
const key = async (k: string) => {
  fireEvent.keyDown(window, { key: k });
  await settle();
};

function renderFinish(r: RunReport = fullReport, start: 'slides' | 'rest' = 'slides') {
  const onSeen = vi.fn();
  const onScanAgain = vi.fn();
  const view = render(
    <BridgeProvider bridge={createMockBridge(fixtures.windows)}>
      <Finish report={r} start={start} onSeen={onSeen} onScanAgain={onScanAgain} />
    </BridgeProvider>,
  );
  return { ...view, onSeen, onScanAgain };
}

/** Makes the OS ask for reduced motion, or not. */
function reducedMotion(on: boolean) {
  vi.spyOn(window, 'matchMedia').mockImplementation(
    (query: string) =>
      ({
        matches: on && query.includes('reduce'),
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }) as MediaQueryList,
  );
}

/**
 * Web Animations, recorded: every `animate` call with the element and keyframes. Countdowns (linear) never finish
 * on their own; everything else finishes at once.
 */
function recordAnimations() {
  const calls: { el: Element; frames: Keyframe[]; options: KeyframeAnimationOptions }[] = [];
  const animate = vi.fn(function (this: Element, frames: Keyframe[], options: KeyframeAnimationOptions) {
    calls.push({ el: this, frames, options });
    const finished = options.easing === 'linear' ? new Promise<Animation>(() => {}) : Promise.resolve({} as Animation);
    return { finished, play() {}, pause() {}, cancel() {} } as unknown as Animation;
  });
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, writable: true, value: animate });
  return calls;
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});
afterEach(() => {
  // @ts-expect-error: jsdom has no Web Animations; tests that record them add `animate`, and it goes again here.
  delete Element.prototype.animate;
  vi.useRealTimers();
});

describe('the highlight slides', () => {
  it('keeps disclosure Space and Enter native while retaining slide navigation', async () => {
    const guides = fixtures.windows.proposals.filter(p => p.id.startsWith('hint-'));
    renderFinish({ ...fullReport, leftForYou: guides.map(p => ({
      id: p.id, title: p.title, summary: p.summary, why: p.why, bytes: 0, hintAction: p.hintAction ?? null,
    })) });
    await userEvent.click(screen.getByRole('button', { name: /Slide .*Skipped and left for you/ }));
    await settle();
    const summary = within(slide()).getAllByText('Instructions and reference')[0]!;
    summary.focus();
    for (const key of [' ', 'Enter']) {
      const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
      expect(summary.dispatchEvent(event)).toBe(true);
      expect(event.defaultPrevented).toBe(false);
      expect(screen.getByRole('button', { name: 'Pause the slides' })).toBeInTheDocument();
    }
    expect(slide()).toHaveClass('slide--kept');
    expect(slide()).toHaveAttribute('tabindex', '0');
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('7 of 7: Checkout');
  });
  it('keeps actual deletions visible when hard-link names free zero bytes', async () => {
    const r: RunReport = {
      ...report, freedBytes: 0, deletedItems: 1, biggestWin: null, freedByCategory: [],
      steps: [{ ...report.steps[0]!, freedBytes: 0, deletedItems: 1, skipped: [{
        path: 'C:\\cache\\linked', reason: 'Space still in use by another hard link', kind: 'hardLinked', bytes: 4096, items: 1,
      }] }],
    };
    renderFinish(r);
    expect(plain(slide().textContent)).toContain('Vacancy deleted 1 item');
    await key('ArrowRight');
    await key('ArrowRight');
    expect(slide()).toHaveTextContent('Items were deleted without freeing space.');
    expect(slide()).not.toHaveTextContent('Nothing was deleted');
    await key('ArrowRight');
    expect(slide()).toHaveTextContent('No category freed any disk space.');
    await key('ArrowRight');
    expect(slide()).toHaveTextContent('Space still in use by another hard link');
    await key('Escape');
    expect(within(panel()).getByText('items deleted').parentElement).toHaveTextContent('1');
  });

  it('tell the run in seven slides, each named for screen readers, every figure from the report', async () => {
    renderFinish();
    expect(deck()).toHaveAttribute('aria-roledescription', 'carousel');
    expect(dots().map((d) => d.getAttribute('aria-label'))).toEqual([
      'Slide 1 of 7: Space freed',
      'Slide 2 of 7: Disk before and after',
      'Slide 3 of 7: Biggest win',
      'Slide 4 of 7: Freed by category',
      'Slide 5 of 7: Moved folders',
      'Slide 6 of 7: Skipped and left for you',
      'Slide 7 of 7: Checkout',
    ]);

    // 1. The sign, and what was freed on the letterboard.
    expect(slide()).toHaveAccessibleName('1 of 7: Space freed');
    expect(within(slide()).getByRole('img', { name: 'Vacancy' })).toBeInTheDocument();
    expect(within(slide()).getByRole('img', { name: /^26\.9\sGB freed$/ })).toHaveTextContent('26.9GBFREED');
    expect(plain(slide().textContent)).toContain(
      'Local Disk (C:) has room again. Vacancy deleted 460,216 items and sorted 1,028 files in 1 min 12 s.',
    );

    // 2. Free space before and after.
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('2 of 7: Disk before and after');
    expect(plain(within(slide()).getByRole('heading').textContent)).toBe('Free space went from 45.0 GB to 71.6 GB.');
    expect(within(slide()).getByRole('img')).toHaveAccessibleName(/^Before the run: 45\.0\sGB free\. After it: 71\.6\sGB free\.$/);
    expect(plain(slide().textContent)).toContain('That’s 1.6 times the room you had. Local Disk (C:) is 85% full now, down from 91%.');
    // One bar segment per category of the map the run started from, freed space as a vacant lot.
    expect(slide().querySelectorAll('.dbar.is-after .dbar-track > i')).toHaveLength(fixtures.windows.overview.categories.length);
    expect(plain(slide().querySelector('.dbar-vacant')?.textContent)).toBe('+26.9 GB');

    // 3. The biggest win, on the key fob.
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('3 of 7: Biggest win');
    expect(within(slide()).getByRole('heading')).toHaveTextContent('Your biggest win: node_modules in 61 idle projects.');
    expect(plain(slide().textContent)).toContain(
      '21.0 GB from 412,118 items, 78% of everything Vacancy freed. It comes back on its own if you ever need it.',
    );
    expect(slide()).toHaveTextContent('Run npm install in a project to bring them back.');
    expect(plain(slide().querySelector('.fob')?.textContent)).toBe('Room 121.0GBDeveloper');

    // 4. Freed by category, with each category's name from the map.
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('4 of 7: Freed by category');
    expect(plain(within(slide()).getByRole('heading').textContent)).toBe('Where the 26.9 GB came from.');
    const rows = within(within(slide()).getByRole('list', { name: 'Freed space by category' })).getAllByRole('listitem');
    expect(rows.map((r) => plain(r.textContent))).toEqual(['Developer21.0 GB78%', 'Caches & temp5.9 GB22%']);

    // 5. Moved folders.
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('5 of 7: Moved folders');
    expect(within(slide()).getByRole('heading')).toHaveTextContent('1,028 files found a home.');
    expect(within(slide()).getAllByRole('listitem').map((li) => li.textContent)).toEqual([
      'Downloads812 files moved',
      'Desktop216 files moved',
    ]);

    // 6. What stayed and why, the hard-link note kept apart and quiet, and the advice left for the user.
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('6 of 7: Skipped and left for you');
    expect(within(slide()).getByRole('heading', { level: 1 })).toHaveTextContent('A few things stayed put.');
    const skipped = within(slide()).getByRole('region', { name: 'Skipped' });
    const notes = within(skipped).getByRole('list', { name: 'Deleted, space still in use' });
    const warnings = within(skipped).getAllByRole('list')[0]!;
    expect(within(warnings).getAllByRole('listitem').map((li) => plain(li.textContent))).toEqual([
      '212 files are open in Microsoft Edge.C:\\Users\\alex\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache\\Cache_Data410 MB',
    ]);
    expect(plain(notes.textContent)).toBe(
      'Deleted. 38 files are also kept elsewhere under other names (hard links), so their space is still in use.C:\\Users\\alex\\code\\portfolio-2024\\node_modules\\.pnpm',
    );
    expect(skipped).toHaveTextContent('Files in use can go next time, once nothing has them open.');
    const advice = within(slide()).getByRole('region', { name: 'Left for you' });
    expect(plain(advice.textContent)).toContain('Cyberpunk 2077 (not played in 14 months)');
    expect(advice).toHaveTextContent('Tips aren’t counted in freed space.');

    // 7. Checkout.
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('7 of 7: Checkout');
    expect(within(slide()).getByRole('heading')).toHaveTextContent('That’s checkout for Local Disk (C:).');
    expect(plain(slide().querySelector('[data-fly="freed"]')?.textContent)).toBe('26.9GB');
    expect(plain(slide().querySelector('[data-fly="deleted"]')?.textContent)).toBe('460,216');
    expect(plain(slide().querySelector('[data-fly="moved"]')?.textContent)).toBe('1,028');
    expect(plain(slide().querySelector('[data-fly="took"]')?.textContent)).toBe('1 min 12 s');
    expect(screen.queryByRole('button', { name: 'Skip to recap' })).toBeNull();
  });

  it('leaves out Moved folders when nothing moved, and says honestly when nothing was skipped or left', async () => {
    const clean = { ...report, steps: [report.steps[0]!], skippedItems: 0 };
    renderFinish(clean);
    expect(dots()).toHaveLength(6);
    expect(screen.queryByRole('button', { name: /Moved folders/ })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Slide 5 of 6: Skipped and left for you' }));
    await settle();
    expect(within(slide()).getByRole('heading', { level: 1 })).toHaveTextContent('Nothing was skipped.');
    expect(slide()).toHaveTextContent('Every file in the plan was free to go.');
    expect(slide()).toHaveTextContent('Nothing in the plan was left for you to do.');
  });

  it('plays on its own, one slide after another, and holds while paused or pointed at', async () => {
    vi.useFakeTimers();
    renderFinish();
    expect(slide()).toHaveAccessibleName('1 of 7: Space freed');
    await act(() => vi.advanceTimersByTimeAsync(6400));
    expect(slide()).toHaveAccessibleName('1 of 7: Space freed');
    await act(() => vi.advanceTimersByTimeAsync(200));
    expect(slide()).toHaveAccessibleName('2 of 7: Disk before and after');
    // The current dot says which it is.
    expect(dots()[1]).toHaveAttribute('aria-current', 'step');

    // Paused: nothing moves, and the slide's content is read out as it changes.
    fireEvent.click(screen.getByRole('button', { name: 'Pause the slides' }));
    await act(() => vi.advanceTimersByTimeAsync(30_000));
    expect(slide()).toHaveAccessibleName('2 of 7: Disk before and after');
    expect(slide().parentElement).toHaveAttribute('aria-live', 'polite');
    // Space plays again, from where the countdown was.
    await key(' ');
    expect(slide().parentElement).toHaveAttribute('aria-live', 'off');
    await act(() => vi.advanceTimersByTimeAsync(6500));
    expect(slide()).toHaveAccessibleName('3 of 7: Biggest win');

    // Resting the pointer on a slide holds it too.
    fireEvent.pointerEnter(slide());
    await act(() => vi.advanceTimersByTimeAsync(20_000));
    expect(slide()).toHaveAccessibleName('3 of 7: Biggest win');
    fireEvent.pointerLeave(slide());
    await act(() => vi.advanceTimersByTimeAsync(6500));
    expect(slide()).toHaveAccessibleName('4 of 7: Freed by category');
  });

  it('goes back and forth with the arrows and the dots, and Escape skips to the recap', async () => {
    const { onSeen } = renderFinish();
    // Focus starts on the slides, so the keys work at once and screen readers land there.
    expect(deck()).toHaveFocus();
    await key('ArrowLeft');
    expect(slide()).toHaveAccessibleName('1 of 7: Space freed');
    expect(screen.getByRole('button', { name: 'Previous slide' })).toBeDisabled();
    await key('ArrowRight');
    await key('ArrowRight');
    expect(slide()).toHaveAccessibleName('3 of 7: Biggest win');
    await key('ArrowLeft');
    expect(slide()).toHaveAccessibleName('2 of 7: Disk before and after');
    fireEvent.click(dots()[5]!);
    await settle();
    expect(slide()).toHaveAccessibleName('6 of 7: Skipped and left for you');
    fireEvent.click(screen.getByRole('button', { name: 'Next slide' }));
    await settle();
    expect(slide()).toHaveAccessibleName('7 of 7: Checkout');

    expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
    await key('Escape');
    expect(screen.queryByRole('region', { name: 'Highlights of this run' })).toBeNull();
    expect(within(panel()).getByRole('heading', { level: 1 })).toHaveAccessibleName(/^26\.9\sGB freed$/);
    // Focus lands on what was freed; the drive remembers the summary was seen.
    expect(within(panel()).getByRole('heading', { level: 1 })).toHaveFocus();
    expect(onSeen).toHaveBeenCalledTimes(1);
  });

  it('skips to the recap from any slide', async () => {
    renderFinish();
    await userEvent.click(screen.getByRole('button', { name: 'Skip to recap' }));
    await settle();
    expect(panel()).toBeInTheDocument();
    expect(screen.queryByRole('region', { name: 'Highlights of this run' })).toBeNull();
  });
});

describe('the hand-off to the map summary', () => {
  it('flies the checkout’s numbers onto the summary’s own, which read the same', async () => {
    reducedMotion(false);
    const flights = recordAnimations();
    renderFinish();
    fireEvent.click(dots()[6]!);
    await settle();
    const sources = [...slide().querySelectorAll<HTMLElement>('[data-fly]')].map((el) => [el.dataset.fly, plain(el.textContent)]);
    fireEvent.click(screen.getByRole('button', { name: 'See it on the map' }));
    // The summary is laid out under the slides, its numbers hidden until theirs land.
    const flown = flights.filter((f) => f.el instanceof HTMLElement && f.el.dataset.fly);
    await settle();
    expect(flown.map((f) => (f.el as HTMLElement).dataset.fly)).toEqual(['freed', 'deleted', 'moved', 'took']);
    for (const f of flown) {
      expect(f.frames[0]).toEqual({ transform: 'translate(0, 0) scale(1)' });
      expect(String(f.frames[1]!.transform)).toMatch(/^translate\(-?[\d.]+px, -?[\d.]+px\) scale\([\d.]+\)$/);
    }
    // The freed number leaves first; the counts follow.
    expect(flown[0]!.options.delay).toBe(0);
    expect(Number(flown[1]!.options.delay)).toBeGreaterThan(0);

    // At rest the slides are gone and the summary shows the same numbers the checkout did.
    expect(screen.queryByRole('region', { name: 'Highlights of this run' })).toBeNull();
    const targets = [...panel().querySelectorAll<HTMLElement>('[data-fly-target]')].map((el) => [el.dataset.flyTarget, plain(el.textContent)]);
    expect(targets).toEqual(sources);
    expect(screen.getByRole('slider', { name: 'Before and after' })).toBeInTheDocument();
  });

  it('hands off on its own after the checkout’s countdown', async () => {
    vi.useFakeTimers();
    renderFinish(report);
    fireEvent.click(dots()[5]!);
    await settle();
    expect(slide()).toHaveAccessibleName('6 of 6: Checkout');
    await act(() => vi.advanceTimersByTimeAsync(3500));
    expect(panel()).toBeInTheDocument();
  });

  it('with reduced motion, cross-fades instead: slides fade in place and nothing flies', async () => {
    reducedMotion(true);
    const animations = recordAnimations();
    renderFinish();
    await key('ArrowRight');
    // Slides fade where they are: no slide moves sideways.
    const slideMoves = animations.filter((a) => a.el instanceof HTMLElement && a.el.classList.contains('slide'));
    expect(slideMoves.length).toBeGreaterThanOrEqual(2);
    for (const m of slideMoves) {
      for (const frame of m.frames) expect(['translateX(0)', 'translateX(0px)']).toContain(frame.transform);
      expect(m.frames.map((f) => f.opacity)).toContain(0);
    }

    fireEvent.click(dots()[6]!);
    await settle();
    animations.length = 0;
    fireEvent.click(screen.getByRole('button', { name: 'See it on the map' }));
    await settle();
    // No number flew; the slides faded out over the summary.
    expect(animations.filter((a) => a.el instanceof HTMLElement && a.el.dataset.fly)).toEqual([]);
    const fades = animations.filter((a) => a.el instanceof HTMLElement && a.el.classList.contains('wrap'));
    expect(fades.map((a) => a.frames)).toEqual([[{ opacity: 1 }, { opacity: 0 }]]);
    expect(panel()).toBeInTheDocument();
  });

  it('slides move sideways when motion is welcome', async () => {
    reducedMotion(false);
    const animations = recordAnimations();
    renderFinish();
    await key('ArrowRight');
    const moves = animations.filter((a) => a.el instanceof HTMLElement && a.el.classList.contains('slide'));
    expect(moves.flatMap((m) => m.frames.map((f) => f.transform))).toEqual(
      expect.arrayContaining(['translateX(48px)', 'translateX(-48px)', 'translateX(0)']),
    );
  });
});

describe('the summary', () => {
  it('replays the highlights from the first slide', async () => {
    renderFinish(fullReport, 'rest');
    expect(screen.queryByRole('region', { name: 'Highlights of this run' })).toBeNull();
    await userEvent.click(within(panel()).getByRole('button', { name: 'Replay highlights' }));
    await settle();
    expect(slide()).toHaveAccessibleName('1 of 7: Space freed');
    expect(deck()).toHaveFocus();
    expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
    await key('Escape');
    expect(panel()).toBeInTheDocument();
  });

  it('keeps the hard-link note apart from what was skipped, and lists moves and advice', () => {
    renderFinish(fullReport, 'rest');
    const section = (name: string) => within(panel()).getByText(name, { selector: 'h3' }).parentElement!;
    expect(section('Skipped')).not.toHaveTextContent('hard links');
    expect(plain(section('Deleted, space still in use').textContent)).toContain(
      '38 files are also kept elsewhere under other names (hard links), so their space is still in use.',
    );
    expect(plain(section('Reorganized').textContent)).toBe(
      'Reorganized moved, not deleted · 0 B freedDownloads. 812 files moved.Desktop. 216 files moved.',
    );
    const advice = within(section('Left for you'));
    expect(advice.getByText('Cyberpunk 2077 (not played in 14 months)')).toBeInTheDocument();
    expect(advice.getByText('71.0 GB in Steam.')).toBeInTheDocument();
    expect(advice.getByText(fullReport.leftForYou[0]!.why)).toBeInTheDocument();
    expect(plain(section('Left for you').querySelector('.advice-num')?.textContent)).toBe('71 GB');
    const stat = (label: string) => plain(within(panel()).getByText(label).nextElementSibling?.textContent);
    expect(stat('files moved')).toBe('1,028');
  });
});

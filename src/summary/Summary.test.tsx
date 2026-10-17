import { render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fixtures, type RunReport } from '../bridge';
import { overviewModel } from '../map/model';
import { changed, inUse, report, stepReport } from '../test/run';
import { SummaryMap, SummaryPanel } from './Summary';

const plain = (s: string | null | undefined) => (s ?? '').replace(/\u00a0/g, ' ');
const overview = fixtures.windows.overview;
const panel = () => screen.getByRole('complementary', { name: 'What Vacancy did' });

function renderPanel(r: RunReport = report, onScanAgain = () => {}) {
  return render(<SummaryPanel overview={overview} report={r} os="Windows" onHot={() => {}} onScanAgain={onScanAgain} />);
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

describe('the summary panel', () => {
  it('leads with what was freed, on which drive, and free space before and after', () => {
    renderPanel();
    expect(screen.getByRole('heading', { level: 1 })).toHaveAccessibleName(/^26\.9\sGB freed$/);
    expect(plain(screen.getByText(/^back on/).textContent)).toBe(
      'back on Local Disk (C:). Free space went from 45.0 GB to 71.6 GB, 1.6 times the room you started with.',
    );
    const bars = within(panel()).getByLabelText('Disk before and after');
    expect(plain(bars.textContent)).toBe('Before45.0 GB freeAfter71.6 GB free');
  });

  it('sets what Windows reports beside what was freed when they differ', () => {
    renderPanel();
    expect(plain(panel().textContent)).toContain(
      'Windows reports 26.6 GB more free space. Other apps write to the drive too, so the two rarely match.',
    );
  });

  it('says nothing more when the drive agrees, to rounding', () => {
    renderPanel({ ...report, freeChange: report.freedBytes - 2e6 });
    expect(panel()).not.toHaveTextContent('Windows reports');
  });

  it('counts items deleted, files moved and how long it took', () => {
    renderPanel();
    const stat = (label: string) => plain(within(panel()).getByText(label).nextElementSibling?.textContent);
    expect(stat('items deleted')).toBe('460,216');
    expect(stat('files moved')).toBe('0');
    expect(stat('start to finish')).toBe('1 min 12 s');
  });

  it('names the biggest win and what each category got back', () => {
    renderPanel();
    const win = within(panel()).getByText('Biggest win').parentElement!;
    expect(plain(win.textContent)).toBe(
      'Biggest win21.0 GBnode_modules in 61 idle projectsDeveloper. Comes back on its own if you ever need it.',
    );
    const cats = within(panel()).getByText('Freed by category').parentElement!;
    const rows = within(cats).getAllByRole('listitem').map((li) => plain(li.textContent));
    expect(rows[0]).toMatch(/^Developer21\.0 GB\d+ GB to [\d.]+ GB$/);
    expect(rows[1]).toMatch(/^Caches & temp5\.9 GB/);
  });

  it('lists what was skipped, why, where, and what stayed', () => {
    const r = { ...report, steps: [report.steps[0]!, { ...report.steps[1]!, skipped: [inUse, changed] }] };
    renderPanel(r);
    const skips = within(panel()).getByText('Skipped').parentElement!;
    const items = within(skips).getAllByRole('listitem').map((li) => plain(li.textContent));
    expect(items).toEqual([
      `212 files are open in Microsoft Edge. 410 MB stayed. It can go next time, once nothing has it open.${inUse.path}`,
      `It changed since the scan. 620 MB stayed. Scan again to see it as it is now.${changed.path}`,
    ]);
  });

  it('says honestly when the run was stopped, and groups what it never reached', () => {
    const stopped = {
      path: 'C:\\b',
      reason: 'You stopped the run before it got here',
      kind: 'cancelled' as const,
      bytes: 6.3e9,
      items: 48_310,
    };
    const r: RunReport = {
      ...report,
      cancelled: true,
      freedBytes: 21e9,
      steps: [
        report.steps[0]!,
        stepReport({ id: 'w-browser', title: 'Browser caches', category: 'caches', outcome: 'skipped', skipped: [stopped] }),
      ],
    };
    renderPanel(r);
    expect(panel()).toHaveTextContent('You stopped the run. 1 of 2 steps ran, and what it hadn’t reached is still on the drive.');
    const skips = within(panel()).getByText('Skipped').parentElement!;
    expect(plain(skips.textContent)).toContain('You stopped the run. 6.3 GB stayed, in Browser caches.');
  });

  it('leaves out the drive figures when the drive couldn’t report them', () => {
    renderPanel({ ...report, totalBytes: 0, freeBefore: 0, freeAfter: 0, usedBefore: 0, usedAfter: 0, freeChange: 0 });
    expect(plain(screen.getByText(/^back on/).textContent)).toBe('back on Local Disk (C:).');
    expect(within(panel()).queryByLabelText('Disk before and after')).toBeNull();
    expect(panel()).not.toHaveTextContent('Windows reports');
  });

  it('scans again', () => {
    const onScanAgain = vi.fn();
    renderPanel(report, onScanAgain);
    within(panel()).getByRole('button', { name: 'Scan again' }).click();
    expect(onScanAgain).toHaveBeenCalledTimes(1);
  });
});

describe('the summary map', () => {
  it('lays the After map over the Before map, with a Vacant sign and what it freed on each cleared tile', () => {
    const { container } = render(<SummaryMap model={overviewModel(overview)} report={report} hot={null} onHot={() => {}} />);

    const after = container.querySelector('.summary-after')!;
    const node = after.querySelector<HTMLElement>('[data-tile="p:w-node"]')!;
    expect(node).toHaveClass('is-vacant');
    expect(plain(node.textContent)).toBe('Vacant21.0 GBnode_modules in 61 idle projects');
    // Partly freed: what stayed is still filled.
    const browser = after.querySelector<HTMLElement>('[data-tile="p:w-browser"]')!;
    expect(browser.querySelector<HTMLElement>('.map-fill')!.style.height).toMatch(/^6\.5\d*%$/);
    // Tiles the run didn't touch look as they did.
    expect(after.querySelector('[data-tile="p:w-temp"]')).not.toHaveClass('is-vacant');
    expect(container.querySelector('.map [data-tile="p:w-node"]')).not.toHaveClass('is-vacant');
    expect(plain(after.querySelector('.map-tile--free')!.textContent)).toBe('Free space45.0 GB + 26.9 GB vacant');

    expect(screen.getByRole('group', { name: 'Compare' })).toHaveTextContent('BeforeAfter');
    expect(screen.getByRole('slider', { name: 'Before and after' })).toBeInTheDocument();
    expect(screen.getByLabelText('Map key')).toHaveTextContent('Vacant, freed by this run');
  });
});

import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import {
  BridgeProvider,
  createMockBridge,
  fixtures,
  type Bridge,
  type DuplicatesDone,
  type MockOptions,
  type PlanSummary,
} from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { ConfirmDialog } from '../run/ConfirmDialog';
import { activeMoves, goingInPlan } from '../tidy/model';
import { sortable } from './duplicates';
import { SummaryPanel } from '../summary/Summary';
import { plan as basePlan, report as baseReport } from '../test/run';

const plain = (s: string | null | undefined) => (s ?? '').replace(/\u00a0/g, ' ');

function renderApp(bridge: Bridge) {
  return render(
    <BridgeProvider bridge={bridge}>
      <DrivesProvider>
        <App />
      </DrivesProvider>
    </BridgeProvider>,
  );
}

const mock = (fixture: keyof typeof fixtures, options: MockOptions = {}) =>
  createMockBridge(fixtures[fixture], { scanMs: 0, runMs: 0, ...options });

/** Scans the system drive and waits for the map. */
async function scanToMap(name: string) {
  await userEvent.click(await screen.findByRole('button', { name: `Scan ${name}` }));
  await screen.findByLabelText('Map key');
}

/** The duplicate search's ready pill, once it has found its groups. */
const readyPill = async () => screen.findByRole('button', { name: /duplicate groups ·/ }, { timeout: 4000 });

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

describe('the duplicate search after a scan', () => {
  it('shows its progress on a pill while the map stays usable, then what it found', async () => {
    renderApp(mock('windows', { dupsMs: 900 }));
    await scanToMap('Local Disk (C:)');

    const pill = await screen.findByRole('progressbar', { name: 'Finding duplicates' });
    expect(pill).toHaveTextContent(/Finding duplicates \d+%/);
    // The map is usable meanwhile: the plan opens.
    await userEvent.click(screen.getByRole('button', { name: 'Build a plan' }));
    expect(screen.getByRole('complementary', { name: 'Plan' })).toBeInTheDocument();

    const ready = await readyPill();
    expect(plain(ready.textContent)).toMatch(/^\d+ duplicate groups · [\d.]+ GB$/);
    expect(screen.queryByRole('progressbar', { name: 'Finding duplicates' })).toBeNull();
    // Its proposal is in the tray, off by default, Review risk.
    const tray = screen.getByRole('complementary', { name: 'Plan' });
    const include = within(tray).getByRole('checkbox', { name: 'Include Duplicate files' });
    expect(include).toHaveAttribute('aria-checked', 'false');
    const row = include.closest('.tray-row') as HTMLElement;
    expect(within(row).getByText('Review')).toBeInTheDocument();
    expect(plain(row.textContent)).toMatch(/\d+ photos, \d+ videos and \d+ other files have an exact copy elsewhere/);
  });

  it('draws one tile per folder holding extra copies, and links the picked group from where its copy is kept', async () => {
    const { container } = renderApp(mock('windows', { dupsMs: 0 }));
    await scanToMap('Local Disk (C:)');
    await userEvent.click(await readyPill());

    const panel = screen.getByRole('complementary', { name: 'Duplicate files' });
    expect(screen.queryByRole('complementary', { name: 'Plan' })).toBeNull();
    const dupTiles = [...container.querySelectorAll<HTMLElement>('[data-tile^="p:duplicates:"]')];
    expect(dupTiles.length).toBeGreaterThanOrEqual(3);
    const links = container.querySelector('svg.dup-links');
    expect(links).not.toBeNull();
    const to = [...links!.querySelectorAll('[data-to]')].map((g) => g.getAttribute('data-to'));
    expect(to.length).toBeGreaterThan(0);
    for (const id of to) expect(container.querySelector(`[data-tile="${id}"]`)).toHaveClass('is-linked');
    expect(container.querySelectorAll('.map-tile.is-keeper')).toHaveLength(1);
    expect(screen.getByText('Kept here')).toBeInTheDocument();

    // Picking another group links that one instead.
    const cards = within(panel).getAllByRole('listitem').filter((li) => li.classList.contains('dup-card'));
    const second = cards[3]!;
    await userEvent.click(within(second).getByRole('button', { name: /^Show .* on the map$/ }));
    expect(second).toHaveClass('is-picked');
    expect(container.querySelector('svg.dup-links')?.getAttribute('data-group')).toBeTruthy();
  });

  it('lists groups with file-type icons, never thumbnails, and an unticked group keeps every copy', async () => {
    renderApp(mock('windows', { dupsMs: 0 }));
    await scanToMap('Local Disk (C:)');
    await userEvent.click(await readyPill());
    const panel = screen.getByRole('complementary', { name: 'Duplicate files' });

    expect(panel.querySelector('img')).toBeNull();
    expect(panel.querySelector('svg[data-kind="video"]')).not.toBeNull();
    expect(panel.querySelector('svg[data-kind="photo"]')).not.toBeNull();
    expect(panel.querySelector('svg[data-kind="document"]')).not.toBeNull();
    expect(within(panel).getAllByText(/^Kept: /).length).toBeGreaterThan(3);

    const foot = () => plain(panel.querySelector('.tray-foot')?.textContent);
    const all = foot().match(/in (\d+) of (\d+) groups/)!;
    expect(all[1]).toBe(all[2]);
    expect(foot()).toContain('one copy of each group stays');

    const box = within(panel).getByRole('checkbox', { name: 'Remove extra copies of Wedding highlights.mp4' });
    await userEvent.click(box);
    expect(box).toHaveAttribute('aria-checked', 'false');
    const card = box.closest('.dup-card') as HTMLElement;
    expect(card).toHaveClass('is-left');
    expect(within(card).getByText('kept')).toBeInTheDocument();
    expect(within(card).getAllByText('Stays').length).toBeGreaterThan(0);
    expect(foot()).toContain(`in ${Number(all[1]) - 1} of ${all[2]} groups`);

    // The filter shows one kind at a time.
    await userEvent.click(within(panel).getByRole('button', { name: /^Videos/ }));
    for (const c of panel.querySelectorAll('.dup-card')) expect(c.querySelector('svg[data-kind="video"]')).not.toBeNull();
  });

  it('plans only the ticked groups, and the confirm dialog says one copy of each group stays', async () => {
    const bridge = mock('windows', { dupsMs: 0 });
    const build = vi.spyOn(bridge, 'buildPlan');
    renderApp(bridge);
    await scanToMap('Local Disk (C:)');
    await userEvent.click(await readyPill());
    const panel = screen.getByRole('complementary', { name: 'Duplicate files' });
    await userEvent.click(within(panel).getByRole('checkbox', { name: 'Remove extra copies of Wedding highlights.mp4' }));
    await userEvent.click(within(panel).getByRole('button', { name: /^Add \d+ groups to the plan$/ }));

    const tray = screen.getByRole('complementary', { name: 'Plan' });
    expect(within(tray).getByRole('checkbox', { name: 'Include Duplicate files' })).toHaveAttribute('aria-checked', 'true');
    expect(plain(tray.textContent)).toMatch(/One copy of each of \d+ duplicate groups stays/);

    await userEvent.click(within(tray).getByRole('button', { name: /^Free / }));
    const dialog = await screen.findByRole('alertdialog');
    const ids = build.mock.calls.at(-1)!;
    expect(ids[1]).toContain('duplicates');
    expect(ids[2]).toEqual(['g1']);
    expect(plain(dialog.textContent)).toContain('Duplicate files: only the extra copies go.');
    expect(plain(dialog.textContent)).toMatch(/One copy of each of the \d+ groups stays where it is\./);
    expect(plain(dialog.textContent)).toContain('1 group you unticked is left alone, every copy kept.');

    // The run frees it, and the summary says one copy of each group stayed.
    await userEvent.click(within(dialog).getByRole('button', { name: /permanently$/ }));
    await userEvent.click(await screen.findByRole('button', { name: 'See what changed' }, { timeout: 4000 }));
    await userEvent.keyboard('{Escape}');
    const summary = await screen.findByRole('complementary', { name: 'What Vacancy did' });
    const recap = within(summary).getByRole('region', { name: 'Duplicate files' });
    expect(plain(recap.textContent)).toMatch(/One copy of each of the \d+ groups stays where it was\./);
    expect(plain(recap.textContent)).toContain('You left 1 group alone, every copy kept.');
    expect(within(recap).getByRole('list', { name: 'Sample pairs' })).toBeInTheDocument();
  });

  it('finds duplicates on a Mac too', async () => {
    renderApp(mock('macos', { dupsMs: 0 }));
    await scanToMap('Macintosh HD');
    await userEvent.click(await readyPill());
    const panel = screen.getByRole('complementary', { name: 'Duplicate files' });
    expect(plain(panel.textContent)).toContain('Your Photos library and iCloud Drive are left alone');
    expect(within(panel).getAllByText(/^Kept: /).length).toBeGreaterThan(3);
  });

  it('stops when a plan starts, and the pill goes', async () => {
    renderApp(mock('windows', { dupsMs: 60_000, runMs: 2000 }));
    await scanToMap('Local Disk (C:)');
    await screen.findByRole('progressbar', { name: 'Finding duplicates' });
    await userEvent.click(screen.getByRole('button', { name: 'Build a plan' }));
    const tray = screen.getByRole('complementary', { name: 'Plan' });
    await userEvent.click(within(tray).getByRole('button', { name: /^Free / }));
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: /permanently$/ }));
    await waitFor(() => expect(screen.queryByRole('progressbar', { name: 'Finding duplicates' })).toBeNull());
  });
});

describe('duplicates and Tidy up', () => {
  const t = fixtures.windows.tidy[0]!;
  const [a, b] = t.moves as [(typeof t.moves)[number], (typeof t.moves)[number]];
  const tidy = [{ ...t, moves: [{ ...a, deletedBy: ['duplicates'] }, { ...b, deletedBy: ['duplicates'] }] }];
  const copy = (name: string) => ({ path: `${t.path}\\${name}`, name, folder: t.name, dir: t.name, bytes: 1, modified: null, tile: 'x' });
  const groups = [
    { id: 'g0', kind: 'photo' as const, name: 'k0', bytes: 1, freed: 1, keep: copy('k0'), extras: [copy(a.name)], reason: '' },
    { id: 'g1', kind: 'photo' as const, name: 'k1', bytes: 1, freed: 1, keep: copy('k1'), extras: [copy(b.name)], reason: '' },
  ];

  it('leaves the extra copies a plan deletes out of the sort, and sorts an unticked group’s', () => {
    const plan = ['duplicates', t.id];
    const shown = sortable(tidy, groups, ['g1'])[0]!;
    expect(goingInPlan(shown, plan).map((m) => m.name)).toEqual([a.name]);
    expect(activeMoves(shown, plan).map((m) => m.name)).toContain(b.name);
    // Without duplicates in the plan both are sorted.
    expect(activeMoves(shown, [t.id]).map((m) => m.name)).toEqual(expect.arrayContaining([a.name, b.name]));
  });
});

describe('the confirm dialog and the summary, from the core’s figures', () => {
  const GB = 1e9;
  const withDups: PlanSummary = {
    ...basePlan,
    bytes: basePlan.bytes + 2 * GB,
    duplicates: { groups: 12, copies: 15, bytes: 2 * GB, skippedGroups: 0, tiles: [] },
  };

  it('says only the extra copies go', () => {
    render(<ConfirmDialog plan={withDups} platform="windows" onCancel={() => {}} onGo={() => {}} />);
    const dialog = screen.getByRole('alertdialog');
    expect(plain(dialog.textContent)).toContain('15 extra copies (2.0 GB) are deleted.');
    expect(plain(dialog.textContent)).toContain('One copy of each of the 12 groups stays where it is.');
    expect(plain(dialog.textContent)).not.toContain('unticked');
  });

  it('says nothing of duplicates when the plan has none', () => {
    render(<ConfirmDialog plan={basePlan} platform="windows" onCancel={() => {}} onGo={() => {}} />);
    expect(plain(screen.getByRole('alertdialog').textContent)).not.toContain('Duplicate');
  });

  it('tells the groups kept whole and the sample pairs, with file-type icons', () => {
    const duplicates: DuplicatesDone = {
      groups: 4,
      clearedGroups: 3,
      keptWhole: 1,
      skippedGroups: 2,
      copiesDeleted: 5,
      freedBytes: 1.5 * GB,
      samples: [
        { kind: 'photo', name: 'IMG_4471.JPG', kept: 'C:\\Users\\alex\\Pictures\\IMG_4471.JPG', removed: 'C:\\Users\\alex\\Downloads\\IMG_4471.JPG', copies: 2 },
        { kind: 'video', name: 'Wedding.mp4', kept: 'C:\\Users\\alex\\Videos\\Wedding.mp4', removed: 'C:\\Users\\alex\\Downloads\\Wedding.mp4', copies: 1 },
      ],
      tiles: [],
    };
    render(<SummaryPanel report={{ ...baseReport, duplicates }} os="Windows" onHot={() => {}} onReplay={() => {}} />);
    const recap = screen.getByRole('region', { name: 'Duplicate files' });
    expect(plain(recap.textContent)).toContain('5 extra copies deleted');
    expect(plain(recap.textContent)).toContain('One copy of each of the 4 groups stays where it was.');
    expect(plain(recap.textContent)).toContain('1 group kept every copy');
    expect(plain(recap.textContent)).toContain('You left 2 groups alone, every copy kept.');
    expect(recap.querySelector('img')).toBeNull();
    expect(recap.querySelector('svg[data-kind="photo"]')).not.toBeNull();
    expect(recap.querySelector('svg[data-kind="video"]')).not.toBeNull();
  });

  it('has no duplicates section for a run without them, as in older history', () => {
    render(<SummaryPanel report={baseReport} os="Windows" onHot={() => {}} onReplay={() => {}} />);
    expect(screen.queryByRole('region', { name: 'Duplicate files' })).toBeNull();
  });
});

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type PlanSummary, type RunReport } from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { NBSP } from '../format';
import { ConfirmDialog } from '../run/ConfirmDialog';
import { RunPanel } from '../run/RunPanel';
import { foldEvents, settleRun, startRun } from '../run/progress';
import { reorganized } from '../test/run';
import { TidyDrawer } from './TidyDrawer';
import { UndoResult } from './Undo';
import { activeMoves, afterTree, beforeTree, foldersMade, goingInPlan, kindFolders, sampleMoves, tidyTotals } from './model';

const plain = (s: string | null | undefined) => (s ?? '').replaceAll(NBSP, ' ');
const downloads = fixtures.windows.tidy.find((t) => t.id === 'tidy-downloads')!;

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(DOMRect.fromRect({ width: 1200, height: 700 }));
});

describe('the Tidy up model', () => {
  it('leaves files a delete in the plan takes out of the sort, and says they go in the plan', () => {
    const taken = downloads.moves.filter((m) => m.deletedBy.includes('w-installers')).length;
    expect(taken).toBe(3);
    expect(activeMoves(downloads, [])).toHaveLength(downloads.moves.length);
    expect(activeMoves(downloads, ['w-installers'])).toHaveLength(downloads.moves.length - taken);
    expect(goingInPlan(downloads, ['w-installers']).map((m) => m.name)).toContain('SteamSetup.exe');
    const after = afterTree(downloads, ['w-installers']);
    expect(after.children?.at(-1)).toMatchObject({ name: '3 going in this plan', tag: 'going' });
    expect(tidyTotals(fixtures.windows.tidy, ['tidy-downloads', 'w-installers'])).toEqual({ moves: downloads.moves.length - taken, folders: ['Downloads'] });
  });

  it('draws the before and after trees, with the folders it makes and the ones the user has', () => {
    expect(beforeTree(downloads).children?.map((c) => c.name)).toEqual(['Archives', 'Installers', `${downloads.loose} loose files`]);
    const after = afterTree(downloads, []);
    const installers = after.children?.find((c) => c.name === 'Installers');
    expect(installers).toMatchObject({ tag: 'existing', count: 41 });
    const images = after.children?.find((c) => c.name === 'Images');
    expect(images?.tag).toBe('new');
    expect(images?.children?.map((c) => c.name)).toEqual(['2026', '2025']);
    expect(after.children?.find((c) => c.name === 'Sort later')?.tag).toBe('later');
    const made = foldersMade(downloads, activeMoves(downloads, []));
    expect(made).toContain('Images');
    expect(made).toContain('Images\\2026');
    expect(made).not.toContain('Installers');
    expect(kindFolders(activeMoves(downloads, []))[0]).toEqual(['Images', 65]);
    expect(sampleMoves(activeMoves(downloads, [])).filter((m) => m.renamed).map((m) => m.to)).toEqual([
      'Docker Desktop Installer (1).exe',
      'fonts (1).zip',
    ]);
  });
});

async function openPlan(platform: 'windows' | 'macos' = 'windows', runMs = 0) {
  const fixture = fixtures[platform];
  const bridge = createMockBridge(fixture, { scanMs: 0, runMs, fullDiskAccess: 'granted', pastRuns: false });
  render(
    <BridgeProvider bridge={bridge}>
      <DrivesProvider>
        <App />
      </DrivesProvider>
    </BridgeProvider>,
  );
  await userEvent.click(await screen.findByRole('button', { name: `Scan ${fixture.volumes[0]!.name}` }));
  await screen.findByLabelText('Map key');
  await userEvent.click(screen.getByRole('button', { name: 'Build a plan' }));
  return { bridge, tray: screen.getByRole('complementary', { name: 'Plan' }) };
}

describe('Tidy up in the plan tray', () => {
  it('lists each folder below Delete, unticked, and says moves free nothing', async () => {
    const { tray } = await openPlan();
    const group = within(tray).getByRole('region', { name: 'Tidy up' });
    expect(group).toHaveTextContent('Tidy up 0 of 3');
    expect(group).toHaveTextContent('Moves only: nothing is deleted and no space is freed.');
    for (const name of ['Downloads', 'Desktop', 'Documents']) {
      expect(within(group).getByRole('checkbox', { name: `Include Sort ${name}` })).toHaveAttribute('aria-checked', 'false');
    }
    // Old installers is on by default, so three of Downloads' files go in its delete instead.
    expect(group).toHaveTextContent(`${downloads.moves.length - 3} of ${downloads.loose} loose files, sorted by kind and year. 3 go in this plan’s deletes instead.`);
    const before = plain(within(tray).getByText(/will be freed/).parentElement?.textContent);
    await userEvent.click(within(group).getByRole('checkbox', { name: 'Include Sort Downloads' }));
    expect(within(tray).getByText(/files sorted in Downloads/)).toHaveTextContent(
      `${downloads.moves.length - 3} files sorted in Downloads. Moved, not deleted: frees 0 B. Undo any time.`,
    );
    // Moves never add to what is freed.
    expect(plain(within(tray).getByText(/will be freed/).parentElement?.textContent)).toBe(before);
  });

  it('opens a drawer with both trees, sample moves, what stays and why, and adds the folder from there', async () => {
    const { tray } = await openPlan();
    await userEvent.click(within(tray).getByRole('button', { name: 'Sort Downloads' }));
    const drawer = screen.getByRole('dialog', { name: 'Sort Downloads' });
    expect(within(drawer).getByRole('region', { name: 'Before' })).toHaveTextContent(`${downloads.loose} loose files`);
    const after = within(drawer).getByRole('region', { name: 'After' });
    expect(after).toHaveTextContent('Installersyours');
    expect(after).toHaveTextContent('3 going in this plan');
    expect(within(drawer).getByRole('list', { name: 'Sample moves' })).toHaveTextContent('Docker Desktop Installer (1).exe');
    expect(drawer).toHaveTextContent('A file there already has the name, so this one gets a number. Nothing is overwritten.');
    const stays = within(drawer).getByRole('list', { name: 'Stays where it is' });
    expect(stays).toHaveTextContent('going in this plan: Old installers in Downloads deletes them, so they aren’t sorted.');
    expect(stays).toHaveTextContent('invoice-0425.pdf It changed after Vacancy looked.');
    expect(drawer).toHaveTextContent('Undo any time, from the recap or Past runs.');
    expect(drawer).toHaveTextContent(`${downloads.moves.length - 3} moves · makes`);
    expect(drawer).toHaveTextContent('frees 0 B');
    await userEvent.click(within(drawer).getByRole('button', { name: 'Add to plan' }));
    expect(within(tray).getByRole('checkbox', { name: 'Include Sort Downloads' })).toHaveAttribute('aria-checked', 'true');
    // With the installers out of the plan, they are sorted after all.
    await userEvent.click(within(tray).getByRole('checkbox', { name: 'Include Old installers in Downloads' }));
    expect(drawer).toHaveTextContent(`${downloads.moves.length} moves · makes`);
    expect(drawer).not.toHaveTextContent('going in this plan');
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Sort Downloads' })).toBeNull();
  });

  it('shows nothing of Tidy up on a Mac', async () => {
    const { bridge, tray } = await openPlan('macos');
    expect(within(tray).queryByRole('region', { name: 'Tidy up' })).toBeNull();
    expect(within(tray).queryByRole('checkbox', { name: /^Include Sort / })).toBeNull();
    await expect(bridge.undoTidy('run-1', 'tidy-downloads')).rejects.toThrow('Tidy up isn’t available on this computer.');
  });
});

const movePlan: PlanSummary = {
  id: 'plan-moves',
  mountPoint: 'C:\\',
  bytes: 0,
  items: 0,
  places: 0,
  moves: 186,
  steps: [{ id: 'tidy-downloads', title: 'Sort Downloads', category: 'downloads', bytes: 0, items: 0, places: 0, kind: 'move', moves: 186, source: 'rule' }],
  refused: [],
  movesRefused: [],
  leftForYou: [],
};

describe('a plan with moves', () => {
  it('confirms with the move count and says where Undo lives', () => {
    render(<ConfirmDialog plan={movePlan} platform="windows" onCancel={() => {}} onGo={() => {}} />);
    const dialog = screen.getByRole('alertdialog');
    expect(dialog).toHaveAccessibleName('Move 186 files?');
    expect(dialog).toHaveTextContent('186 files will be moved into folders, which frees no space. Every move is recorded, so you can undo it from the recap or Past runs.');
    expect(dialog).not.toHaveTextContent('deleted for good');
    expect(within(dialog).getByRole('button', { name: 'Move 186 files' })).toBeInTheDocument();
  });

  it('says moving, counts files, and names each file that stayed', () => {
    const callbacks = { hot: null, onHot: () => {}, onStop: () => {}, onSeeChanges: () => {}, onBack: () => {}, onScanAgain: () => {} };
    const going = foldEvents(startRun(movePlan), [
      { kind: 'stepStarted', step: 0, id: 'tidy-downloads' },
      { kind: 'moveProgress', step: 0, done: 41, total: 186 },
    ]);
    const { unmount } = render(<RunPanel run={{ phase: 'running', plan: movePlan, view: going, stopping: false }} {...callbacks} />);
    expect(screen.getByRole('heading', { name: 'Moving files' })).toBeInTheDocument();
    expect(screen.getByRole('list', { name: 'Steps' })).toHaveTextContent('Sort Downloads41 of 186');
    expect(screen.getByText(/Every move is recorded/)).toBeInTheDocument();
    unmount();

    const skip = { path: 'C:\\Users\\alex\\Downloads\\Q3-invoice.pdf', reason: 'It was open in another app when its turn came', kind: 'inUse' as const, bytes: 2.1e6, items: 1 };
    const report: RunReport = {
      id: 'run-m',
      planId: movePlan.id,
      mountPoint: 'C:\\',
      volumeName: 'Local Disk (C:)',
      startedAt: 0,
      durationMs: 4000,
      cancelled: false,
      steps: [{
        id: 'tidy-downloads', title: 'Sort Downloads', why: '', category: 'downloads', regenerable: false,
        plannedBytes: 0, freedBytes: 0, plannedItems: 186, deletedItems: 0, kind: 'move', movedItems: 185, places: 0, source: 'rule',
        outcome: 'partial', skipped: [skip],
      }],
      plannedBytes: 0,
      freedBytes: 0,
      deletedItems: 0,
      movedItems: 185,
      skippedItems: 1,
      freedByCategory: [],
      totalBytes: 0,
      usedBefore: 0,
      usedAfter: 0,
      freeBefore: 0,
      freeAfter: 0,
      freeChange: 0,
      biggestWin: null,
      leftForYou: [],
      reorganized: [],
      map: fixtures.windows.overview,
    };
    const ended = foldEvents(going, [{ kind: 'skipped', step: 0, skip }, { kind: 'stepFinished', step: 0, report: report.steps[0]! }]);
    render(<RunPanel run={{ phase: 'ended', plan: movePlan, view: settleRun(ended, report), report }} {...callbacks} />);
    const steps = screen.getByRole('list', { name: 'Steps' });
    expect(steps).toHaveTextContent('185 moved');
    expect(steps).toHaveTextContent('It was open in another app when its turn came: Q3-invoice.pdf');
    expect(steps).not.toHaveTextContent('kept');
    expect(screen.getByRole('log')).toHaveTextContent('Sort Downloads: 185 files moved, nothing deleted, every move recorded, 1 left where it was');
    expect(screen.getByText(/185 files moved\. Moves free no space/)).toBeInTheDocument();
  });
});

describe('Undo', () => {
  it('runs a sort, then undoes it from the summary and Past runs, saying what came back and what couldn’t', async () => {
    const { bridge, tray } = await openPlan();
    const undo = vi.spyOn(bridge, 'undoTidy');
    await userEvent.click(within(tray).getByRole('checkbox', { name: 'Include Sort Downloads' }));
    await userEvent.click(within(tray).getByRole('checkbox', { name: 'Include Sort Desktop' }));
    await userEvent.click(within(tray).getByRole('button', { name: /^Free / }));
    const confirm = await screen.findByRole('alertdialog');
    const moves = downloads.moves.length - 3 + fixtures.windows.tidy[1]!.moves.length;
    expect(confirm).toHaveTextContent(`${moves.toLocaleString('en-US')} files will be moved into folders`);
    await userEvent.click(within(confirm).getByRole('button', { name: /permanently$/ }));
    await userEvent.click(await screen.findByRole('button', { name: 'See what changed' }));
    // The Moved slide: each folder with where its files went, and Undo.
    await userEvent.click(screen.getByRole('button', { name: /Moved folders$/ }));
    const slide = await screen.findByRole('group', { name: /Moved folders$/ });
    expect(slide).toHaveTextContent(`${(moves - 2).toLocaleString('en-US')} files found a home.`);
    expect(slide).toHaveTextContent('Moving deletes nothing, so it freed no space.');
    expect(slide).toHaveTextContent('2 stayed');
    expect(within(slide).getByRole('button', { name: 'Undo Desktop' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Skip to recap' }));

    const panel = await screen.findByRole('complementary', { name: 'What Vacancy did' });
    const recap = within(panel).getByRole('region', { name: 'Reorganized' });
    expect(recap).toHaveTextContent('moved, not deleted · 0 B freed');
    expect(recap).toHaveTextContent('It was open in another app when its turn came. It stayed where it was.');
    expect(panel).toHaveTextContent('The moved files freed nothing');
    await userEvent.click(within(recap).getByRole('button', { name: 'Undo Downloads' }));
    const result = await within(recap).findByRole('status');
    expect(undo).toHaveBeenCalledWith(expect.stringMatching(/^run-/), 'tidy-downloads');
    expect(result).toHaveTextContent(/of [\d,]+ files are back in Downloads\./);
    expect(result).toHaveTextContent('2 files couldn’t go back: a file now has its old name. They’re still in Downloads\\Documents\\2026: statement-2026-09.pdf, statement-2026-10.pdf.');
    expect(result).toHaveTextContent('Kept Downloads\\Documents\\2026 and Downloads\\Documents, which still hold the files that couldn’t go back.');
    expect(result).toHaveTextContent(/Removed the \d+ empty folders Vacancy made: Downloads\\[A-Z]/);
    expect(within(recap).getByText('Moved back · 2 stayed')).toBeInTheDocument();

    // Past runs offers Undo for the folder still sorted, and shows the one moved back.
    await userEvent.click(screen.getByRole('button', { name: 'Past runs' }));
    const menu = await screen.findByRole('menu', { name: 'Past runs' });
    await within(menu).findAllByRole('menuitem');
    expect(menu).toHaveTextContent('Moved back · 2 stayed');
    expect(menu).toHaveTextContent('Undo works from here any time.');
    await userEvent.click(within(menu).getByRole('menuitem', { name: /^Undo Desktop/ }));
    await within(menu).findByText('Moved back');
    expect(undo).toHaveBeenLastCalledWith(expect.stringMatching(/^run-/), 'tidy-desktop');
    // The summary on screen shows the Undo done from Past runs too.
    expect(within(recap).getByText('Moved back')).toBeInTheDocument();
  });
});

describe('the mock bridge’s Tidy up', () => {
  it.each([['windows', 3], ['macos', 0]] as const)('%s scans propose %i folders', async (platform, n) => {
    const fixture = fixtures[platform];
    const bridge = createMockBridge(fixture, { scanMs: 0 });
    const outcome = await bridge.scan(fixture.volumes[0]!.mountPoint, () => {});
    expect(outcome.status === 'done' && outcome.tidy.length).toBe(n);
  });

  it('plans a sort without the files the plan deletes, and says it frees nothing', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
    const c = fixtures.windows.volumes[0]!.mountPoint;
    await bridge.scan(c, () => {});
    const both = await bridge.buildPlan(c, ['w-installers', 'tidy-downloads']);
    const alone = await bridge.buildPlan(c, ['tidy-downloads']);
    expect(alone.moves - both.moves).toBe(3);
    expect(alone).toMatchObject({ bytes: 0, items: 0, steps: [{ id: 'tidy-downloads', kind: 'move', bytes: 0 }] });
  });
});

describe('what Undo and the drawer say', () => {
  it('names the folders Undo removed, and says a kept folder still holds something unless stuck files are in it', () => {
    const record = reorganized({ id: 'tidy-downloads', folder: 'Downloads', movedItems: 3 });
    render(
      <UndoResult
        record={{ ...record, undo: { at: null, back: 3, stuck: [], removed: ['Images\\2026', 'Images'], kept: ['Videos\\2025', 'Videos'] } }}
      />,
    );
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('All 3 files are back in Downloads.');
    expect(status).toHaveTextContent('Removed the 2 empty folders Vacancy made: Downloads\\Images\\2026 and Downloads\\Images.');
    expect(status).toHaveTextContent('Kept Downloads\\Videos\\2025 and Downloads\\Videos: they still hold something, so Vacancy left them.');
    expect(status).not.toHaveTextContent('couldn’t go back');
  });

  it('explains a capped folder even when no count of the rest is known', () => {
    const capped = { ...downloads, capped: true, later: 0 };
    render(<TidyDrawer tidy={capped} plan={[]} proposals={[]} onToggle={() => {}} onClose={() => {}} />);
    expect(screen.getByRole('dialog', { name: 'Sort Downloads' })).toHaveTextContent(
      'Vacancy looks at up to 512 files of a folder in one run, the first ones as Windows lists them. Any more wait for a later run.',
    );
  });
});

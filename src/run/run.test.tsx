import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import {
  BridgeProvider,
  createMockBridge,
  fixtures,
  type MockBridge,
  type PlanSummary,
  type RunEvent,
  type RunReport,
} from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { browserPartial, changed, inUse, nodeDone, plan, report } from '../test/run';

const plain = (s: string | null | undefined) => (s ?? '').replace(/\u00a0/g, ' ');
const header = () => screen.getByRole('banner');
const tray = () => screen.getByRole('complementary', { name: 'Plan' });
const panel = () => screen.getByRole('complementary', { name: 'Run' });
const steps = () => within(panel()).getAllByRole('listitem').map((li) => plain(li.textContent));
const log = () => [...within(panel()).getByRole('log').children].map((l) => plain(l.textContent));
const tile = (container: HTMLElement, id: string) => container.querySelector<HTMLElement>(`.map [data-tile="p:${id}"]`)!;
const freeTile = (container: HTMLElement) => container.querySelector<HTMLElement>('.map .map-tile--free')!;
const area = (el: HTMLElement) => parseFloat(el.style.width) * parseFloat(el.style.height);

/** From the ended run to its summary: the highlight slides come first, and Skip to recap goes past them. */
async function seeWhatChanged() {
  await userEvent.click(within(panel()).getByRole('button', { name: 'See what changed' }));
  expect(screen.getByRole('region', { name: 'Highlights of this run' })).toBeInTheDocument();
  expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Skip to recap' }));
  return screen.findByRole('complementary', { name: 'What Vacancy did' });
}

/** A bridge whose run is played by the test, event by event. */
function scripted(made: PlanSummary = plan) {
  const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
  const build = vi.spyOn(bridge, 'buildPlan').mockResolvedValue(made);
  let onEvent: (e: RunEvent) => void = () => {};
  let finish: (r: RunReport) => void = () => {};
  let fail: (e: unknown) => void = () => {};
  const runPlan = vi.spyOn(bridge, 'runPlan').mockImplementation((_m, _id, cb) => {
    onEvent = cb;
    return new Promise<RunReport>((resolve, reject) => {
      finish = resolve;
      fail = reject;
    });
  });
  const cancel = vi.spyOn(bridge, 'cancelRun');
  return {
    bridge,
    build,
    runPlan,
    cancel,
    /** Sends events, and waits for the run panel to draw them. */
    emit: async (...events: RunEvent[]) => {
      act(() => events.forEach((e) => onEvent(e)));
      await act(() => new Promise((r) => setTimeout(r, 80)));
    },
    finish: async (r: RunReport) => {
      await act(async () => {
        finish(r);
        await Promise.resolve();
      });
    },
    fail: async (e: unknown) => {
      await act(async () => {
        fail(e);
        await Promise.resolve();
      });
    },
  };
}

function renderApp(bridge: MockBridge) {
  return render(
    <BridgeProvider bridge={bridge}>
      <DrivesProvider>
        <App />
      </DrivesProvider>
    </BridgeProvider>,
  );
}

/** Scans C:, opens the plan, presses Free and confirms. */
async function confirmRun() {
  await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
  await screen.findByLabelText('Map key');
  await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
  await userEvent.click(within(tray()).getByRole('button', { name: /^Free / }));
  await userEvent.click(await screen.findByRole('button', { name: /permanently$/ }));
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

describe('freeing space', () => {
  it('checks the plan, then asks once with the plan’s own numbers; Go back changes nothing', async () => {
    const run = scripted();
    renderApp(run.bridge);
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByLabelText('Map key');
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));

    // The tray adds up the proposals; the dialog says what the core planned.
    await userEvent.click(within(tray()).getByRole('button', { name: /^Free 75\.3\sGB$/ }));
    expect(run.build).toHaveBeenCalledWith('C:\\', fixtures.windows.proposals.filter((p) => p.defaultOn).map((p) => p.id));
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveAccessibleName(/^Permanently delete 27\.3\sGB\?$/);
    expect(plain(dialog.textContent)).toContain('460,428 items in 67 places will be deleted for good.');

    await userEvent.click(screen.getByRole('button', { name: 'Go back' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(run.runPlan).not.toHaveBeenCalled();
    expect(tray()).toBeInTheDocument();
  });

  it('closes the dialog on Escape without zooming the map out', async () => {
    const run = scripted();
    const { container } = renderApp(run.bridge);
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByLabelText('Map key');
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
    await userEvent.click(within(tray()).getByRole('button', { name: /^Free / }));
    await screen.findByRole('alertdialog');
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(container.querySelector('.map')).not.toBeNull();
  });

  it('shows in the tray why a plan couldn’t be made', async () => {
    const run = scripted();
    run.build.mockRejectedValue('this scan of C:\\ already ran a plan: scan again first');
    renderApp(run.bridge);
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByLabelText('Map key');
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
    await userEvent.click(within(tray()).getByRole('button', { name: /^Free / }));

    expect(await within(tray()).findByRole('alert')).toHaveTextContent('already ran a plan: scan again first');
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });

  it('turns the tray into the run: steps, a live counter, the log and skips as they happen, the map draining', async () => {
    const run = scripted();
    const { container } = renderApp(run.bridge);
    await confirmRun();

    expect(run.runPlan).toHaveBeenCalledWith('C:\\', 'plan-1', expect.any(Function));
    expect(screen.queryByRole('complementary', { name: 'Plan' })).toBeNull();
    expect(within(panel()).getByRole('heading', { name: 'Freeing space' })).toBeInTheDocument();
    expect(steps()).toEqual(['node_modules in 61 idle projects21.0 GB', 'Browser caches6.3 GB']);
    // Drive switching is shut, and the header has nothing to plan.
    expect(within(header()).getByRole('button', { name: /Local Disk \(C:\)/ })).toHaveAttribute('aria-disabled', 'true');
    expect(within(header()).queryByRole('button', { name: 'Build a plan' })).toBeNull();
    expect(within(header()).queryByText(/reclaimable/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Scan again' })).toBeNull();
    expect(tile(container, 'w-node')).toHaveClass('is-queued');
    expect(tile(container, 'w-temp')).not.toHaveClass('is-queued');
    const node = area(tile(container, 'w-node'));
    const free = area(freeTile(container));
    const temp = area(tile(container, 'w-temp'));

    await run.emit(
      { kind: 'stepStarted', step: 0, id: 'w-node' },
      { kind: 'progress', step: 0, freedBytes: 7e9, deletedItems: 100_000, totalFreedBytes: 7e9, currentPath: 'C:\\x' },
      { kind: 'deleted', step: 0, path: 'C:\\Users\\alex\\code\\web-shop\\node_modules', bytes: 7e9, items: 100_000, whole: true },
    );
    expect(within(panel()).getByText(/^Step 1 of 2: node_modules/)).toBeInTheDocument();
    expect(plain(within(panel()).getByText(/freed of/).previousElementSibling?.textContent)).toBe('7.0 GB');
    expect(plain(within(panel()).getByText(/freed of/).textContent)).toBe('freed of 27.3 GB planned');
    expect(within(panel()).getByRole('progressbar', { name: 'Freed' })).toHaveAttribute('aria-valuenow', '26');
    expect(within(panel()).getAllByRole('listitem')[0]).toHaveClass('is-running');
    expect(log()).toEqual(['deleted C:\\Users\\alex\\code\\web-shop\\node_modules']);
    // The tile shrinks by what its step freed, 14 GB left under a sliver of vacant ground, and the free tile grows by
    // it and says how much free space there is now.
    expect(tile(container, 'w-node')).toHaveClass('is-running');
    expect(tile(container, 'w-node').querySelector<HTMLElement>('.map-fill')!.style.height).toMatch(/^93\.0\d*%$/);
    expect(area(tile(container, 'w-node'))).toBeLessThan(node * 0.8);
    expect(area(freeTile(container))).toBeGreaterThan(free * 1.1);
    // A tile the plan doesn't touch stays about the size it was.
    expect(area(tile(container, 'w-temp')) / temp).toBeCloseTo(1, 1);
    expect(plain(container.querySelector('.map .map-tile--free .free-grow')?.textContent)).toBe('52.0 GB');
    expect(container.querySelector('.map')).toHaveClass('is-freeing');

    await run.emit(
      { kind: 'stepFinished', step: 0, report: nodeDone },
      { kind: 'stepStarted', step: 1, id: 'w-browser' },
      { kind: 'skipped', step: 1, skip: inUse },
    );
    // Cleared, it is a sliver of vacant ground; its 21 GB are in the free tile.
    expect(tile(container, 'w-node')).toHaveClass('is-vacant');
    expect(tile(container, 'w-node').querySelector<HTMLElement>('.map-fill')!.style.height).toBe('0%');
    expect(area(tile(container, 'w-node'))).toBeLessThan(node * 0.25);
    expect(area(freeTile(container))).toBeGreaterThan(free * 1.35);
    expect(plain(container.querySelector('.map .map-tile--free .free-grow')?.textContent)).toBe('66.0 GB');
    expect(log().slice(1)).toEqual([
      'node_modules in 61 idle projects: 21.0 GB freed',
      `skipped ${inUse.path} (212 files are open in Microsoft Edge)`,
    ]);
    // The skip shows in its step's row at once, and in the count below.
    expect(steps()[1]).toContain('212 files are open in Microsoft Edge, 410 MB kept');
    expect(within(panel()).getByText(/1 skipped so far/)).toBeInTheDocument();

    await run.emit({ kind: 'stepFinished', step: 1, report: browserPartial });
    await run.finish(report);
    // The done map is the drive as the run left it.
    expect(plain(container.querySelector('.map .map-tile--free .free-grow')?.textContent)).toBe('71.9 GB');
    expect(tile(container, 'w-browser')).toHaveClass('is-vacant');

    expect(within(panel()).getByRole('heading', { name: 'Done' })).toBeInTheDocument();
    expect(within(panel()).getByText('All 2 steps finished.')).toBeInTheDocument();
    expect(plain(within(panel()).getByText(/freed, 212 files skipped/).textContent)).toBe('26.9 GB freed, 212 files skipped.');
    expect(within(panel()).getAllByRole('listitem')[1]).toHaveClass('has-skip');
    expect(within(header()).getByRole('button', { name: /Local Disk \(C:\)/ })).not.toHaveAttribute('aria-disabled');
    expect(within(header()).getByRole('img', { name: 'Vacancy' })).toHaveClass('is-lit');

    const summary = await seeWhatChanged();
    expect(within(summary).getByRole('heading', { level: 1 })).toHaveAccessibleName(/^26\.9\sGB freed$/);
    expect(screen.getByRole('slider', { name: 'Before and after' })).toBeInTheDocument();
  });

  it('stops when asked, and says so honestly', async () => {
    const run = scripted();
    renderApp(run.bridge);
    await confirmRun();
    await run.emit({ kind: 'stepStarted', step: 0, id: 'w-node' });

    await userEvent.click(within(panel()).getByRole('button', { name: 'Stop' }));
    expect(run.cancel).toHaveBeenCalledTimes(1);
    expect(within(panel()).getByRole('button', { name: 'Stopping' })).toBeDisabled();
    expect(within(panel()).getByText('Stopping after the file being deleted')).toBeInTheDocument();

    // The step running when Stop came keeps what went; the next one never starts deleting, though planning had
    // already left out a place of it that changed.
    const left = { path: 'C:\\n', reason: '212,118 files were left: you stopped the run', kind: 'cancelled' as const, bytes: 9e9, items: 212_118 };
    const partly = { ...nodeDone, outcome: 'partial' as const, freedBytes: 12e9, deletedItems: 200_000, skipped: [left] };
    const stopped = { path: 'C:\\b', reason: 'You stopped the run before it got here', kind: 'cancelled' as const, bytes: 6.3e9, items: 48_310 };
    const notReached = { ...browserPartial, outcome: 'skipped' as const, freedBytes: 0, deletedItems: 0, skipped: [changed, stopped] };
    await run.emit(
      { kind: 'skipped', step: 0, skip: left },
      { kind: 'stepFinished', step: 0, report: partly },
      { kind: 'stepStarted', step: 1, id: 'w-browser' },
      { kind: 'skipped', step: 1, skip: changed },
      { kind: 'skipped', step: 1, skip: stopped },
      { kind: 'stepFinished', step: 1, report: notReached },
    );
    await run.finish({ ...report, cancelled: true, freedBytes: 12e9, skippedItems: 260_429, steps: [partly, notReached] });

    expect(within(panel()).getByRole('heading', { name: 'Stopped' })).toBeInTheDocument();
    expect(within(panel()).getByText('You stopped the run. 1 of 2 steps ran.')).toBeInTheDocument();
    expect(plain(within(panel()).getByText(/files left in place/).textContent)).toBe('12.0 GB freed, 260,429 files left in place.');
    expect(steps()[0]).toBe('node_modules in 61 idle projects12.0 GByou stopped the run, 9.0 GB kept');
    expect(steps()[1]).toBe('Browser caches0 BNot run: you stopped the run first; It changed since the scan, 620 MB kept');
    expect(await seeWhatChanged()).toHaveTextContent('You stopped the run.');
  });

  it('says why a run failed, unlocks, and goes back to the plan', async () => {
    const run = scripted();
    renderApp(run.bridge);
    await confirmRun();
    await run.fail('a plan is already running');

    expect(within(panel()).getByRole('heading', { name: 'The run stopped' })).toBeInTheDocument();
    expect(within(panel()).getByRole('alert')).toHaveTextContent('a plan is already running');
    expect(within(header()).getByRole('button', { name: /Local Disk \(C:\)/ })).not.toHaveAttribute('aria-disabled');
    await userEvent.click(within(panel()).getByRole('button', { name: 'Back to the plan' }));
    expect(tray()).toBeInTheDocument();
    expect(within(tray()).getByRole('button', { name: /^Free / })).toBeEnabled();
  });

  it('keeps the summary on its drive until it is scanned again', async () => {
    const run = scripted({ ...plan, refused: [changed] });
    renderApp(run.bridge);
    await confirmRun();
    await run.finish(report);
    await seeWhatChanged();

    // To D: and back: C: still shows what changed, at the summary now that it has been seen.
    await userEvent.click(within(header()).getByRole('button', { name: /Local Disk \(C:\)/ }));
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Data (D:)' }));
    expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
    await userEvent.click(within(header()).getByRole('button', { name: /Data \(D:\)/ }));
    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Local Disk (C:)' }));
    expect(await screen.findByRole('complementary', { name: 'What Vacancy did' })).toBeInTheDocument();

    // Scan again: a fresh map with its proposals back.
    await userEvent.click(within(screen.getByRole('complementary', { name: 'What Vacancy did' })).getByRole('button', { name: 'Scan again' }));
    // The scan screen can go back to what changed, or scan again.
    await userEvent.click(await screen.findByRole('button', { name: 'Open its map' }));
    expect(screen.getByRole('complementary', { name: 'What Vacancy did' })).toBeInTheDocument();
    await userEvent.click(within(screen.getByRole('complementary', { name: 'What Vacancy did' })).getByRole('button', { name: 'Scan again' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Scan again' }));
    await screen.findByLabelText('Map key');
    await waitFor(() => expect(within(header()).getByRole('button', { name: 'Build a plan' })).toBeInTheDocument());
    expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
  });
});

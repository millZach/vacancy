import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type MockOptions } from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { NBSP } from '../format';
import { formatWhen } from '../summary/when';
import { HistoryMenu } from './HistoryMenu';

const plain = (s: string | null | undefined) => (s ?? '').replaceAll(NBSP, ' ');
const header = () => screen.getByRole('banner');
const menu = () => screen.getByRole('menu', { name: 'Past runs' });
const panel = () => screen.getByRole('complementary', { name: 'What Vacancy did' });

function renderApp(options: MockOptions = {}) {
  const bridge = createMockBridge(fixtures.windows, { scanMs: 0, ...options });
  render(
    <BridgeProvider bridge={bridge}>
      <DrivesProvider>
        <App />
      </DrivesProvider>
    </BridgeProvider>,
  );
  return bridge;
}

async function openHistory() {
  await userEvent.click(await within(header()).findByRole('button', { name: 'Past runs' }));
  await within(menu()).findAllByRole('menuitem');
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

describe('past runs', () => {
  it('keeps swapped disks apart and follows a known disk to another mount point', async () => {
    const bridge = createMockBridge(fixtures.windows);
    const base = (await bridge.listRuns()).runs[0]!;
    vi.spyOn(bridge, 'listRuns').mockResolvedValue({ unreadable: 0, runs: [
      { ...base, id: 'new-disk', volumeId: 'usb-b', volumeName: 'New USB', mountPoint: 'E:\\' },
      { ...base, id: 'old-disk', volumeId: 'usb-a', volumeName: 'Old USB', mountPoint: 'E:\\' },
      { ...base, id: 'old-disk-before', volumeId: 'usb-a', volumeName: 'Old USB', mountPoint: 'F:\\' },
      { ...base, id: 'unknown-1', volumeId: '', volumeName: 'Unknown disk', mountPoint: 'E:\\' },
      { ...base, id: 'unknown-2', volumeId: '', volumeName: 'Unknown disk', mountPoint: 'E:\\' },
    ] });
    render(<BridgeProvider bridge={bridge}><HistoryMenu locked={false} onOpen={() => {}} /></BridgeProvider>);
    await userEvent.click(screen.getByRole('button', { name: 'Past runs' }));
    const groups = within(await screen.findByRole('menu')).getAllByRole('group');
    expect(groups.map((g) => g.getAttribute('aria-label'))).toEqual(['New USB', 'Old USB', 'Unknown disk', 'Unknown disk']);
    expect(groups.map((g) => within(g).getAllByRole('menuitem').length)).toEqual([1, 2, 1, 1]);
  });

  it('lists each drive’s runs, newest first, with when each ran and what it freed', async () => {
    const bridge = renderApp();
    const { runs } = await bridge.listRuns();
    await openHistory();
    const groups = within(menu()).getAllByRole('group');
    expect(groups.map((g) => g.getAttribute('aria-label'))).toEqual(['Local Disk (C:)', 'Data (D:)']);
    const names = within(menu()).getAllByRole('menuitem').map((m) => plain(m.getAttribute('aria-label')));
    expect(names).toEqual(
      runs.map((r) => `${formatWhen(r.startedAt)} on ${r.volumeName}: ${plain(fmt(r.freedBytes))} freed`),
    );
    // Focus starts on the newest run, and the arrows move through them.
    expect(within(menu()).getAllByRole('menuitem')[0]).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(within(menu()).getAllByRole('menuitem')[1]).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(within(header()).getByRole('button', { name: 'Past runs' })).toHaveFocus();
  });

  it('reopens a run’s summary as it was kept, replays its slides, and closes back to the drive', async () => {
    const bridge = renderApp();
    const kept = await bridge.openRun((await bridge.listRuns()).runs[1]!.id);
    await openHistory();
    await userEvent.click(within(menu()).getAllByRole('menuitem')[1]!);

    expect(await screen.findByRole('complementary', { name: 'What Vacancy did' })).toBeInTheDocument();
    expect(panel()).toHaveTextContent(`Run of ${formatWhen(kept.startedAt)}`);
    expect(within(panel()).getByRole('heading', { level: 1 })).toHaveAccessibleName(`${fmt(kept.freedBytes)} freed`);
    expect(plain(panel().textContent)).toContain(`back on ${kept.volumeName}.`);
    expect(screen.getByRole('slider', { name: 'Before and after' })).toBeInTheDocument();

    await userEvent.click(within(panel()).getByRole('button', { name: 'Replay highlights' }));
    await act(async () => {});
    expect(screen.getByRole('group', { name: '1 of 6: Space freed' })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await act(async () => {});

    await userEvent.click(within(panel()).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
    expect(screen.getByRole('button', { name: /^Scan / })).toBeInTheDocument();
  });

  it('counts kept runs it couldn’t read, and lists the rest', async () => {
    renderApp({ unreadableRuns: 2 });
    await openHistory();
    expect(menu()).toHaveTextContent('2 kept runs couldn’t be read, so they aren’t listed.');
    expect(within(menu()).getAllByRole('menuitem')).toHaveLength(2);
  });

  it('says when there are no runs yet', async () => {
    renderApp({ pastRuns: false });
    await userEvent.click(await within(header()).findByRole('button', { name: 'Past runs' }));
    expect(await within(menu()).findByText('No runs yet. Each plan you run is kept here, with its summary.')).toBeInTheDocument();
  });

  it('says why a run can’t be opened, and stays on the list', async () => {
    const bridge = renderApp();
    vi.spyOn(bridge, 'openRun').mockRejectedValue(new Error('This run can’t be opened: it is damaged.'));
    await openHistory();
    await userEvent.click(within(menu()).getAllByRole('menuitem')[0]!);
    expect(await within(menu()).findByRole('alert')).toHaveTextContent('This run can’t be opened: it is damaged.');
    expect(screen.queryByRole('complementary', { name: 'What Vacancy did' })).toBeNull();
  });

  it('waits while a plan runs', async () => {
    const bridge = createMockBridge(fixtures.windows);
    const list = vi.spyOn(bridge, 'listRuns');
    render(
      <BridgeProvider bridge={bridge}>
        <HistoryMenu locked onOpen={() => {}} />
      </BridgeProvider>,
    );
    const button = screen.getByRole('button', { name: 'Past runs' });
    expect(button).toHaveAttribute('aria-disabled', 'true');
    await userEvent.click(button);
    expect(screen.queryByRole('menu')).toBeNull();
    expect(list).not.toHaveBeenCalled();
  });
});

/** What the app shows for bytes, as the core's report figures read. */
function fmt(bytes: number): string {
  const gb = bytes / 1e9;
  return `${gb.toFixed(1)}${NBSP}GB`;
}

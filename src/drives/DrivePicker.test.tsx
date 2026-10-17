import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type MockBridge, type MockOptions } from '../bridge';
import { designTokens, fakeContext, manualFrames, prefersReducedMotion } from '../test/canvas';
import { useDrives } from './context';
import { DrivesProvider } from './DrivesProvider';

const ASK = 'Which drive should Vacancy scan?';

/** Stands in for the run screen of ticket 04, which holds the drive lock while a plan runs. */
function RunControls() {
  const drives = useDrives();
  if (drives.status !== 'ready') return null;
  return (
    <>
      <button type="button" onClick={() => drives.setRunning(true)}>
        Start the run
      </button>
      <button type="button" onClick={() => drives.setRunning(false)}>
        End the run
      </button>
    </>
  );
}

function renderApp(bridge: MockBridge, extra?: ReactNode) {
  return render(
    <BridgeProvider bridge={bridge}>
      <DrivesProvider>
        <App />
        {extra}
      </DrivesProvider>
    </BridgeProvider>,
  );
}

const windows = (options: MockOptions = {}) => createMockBridge(fixtures.windows, { scanMs: 0, ...options });
const header = () => screen.getByRole('banner');
/** The header's drive plate, which opens the drive menu. (On a map the header also has "Build a plan".) */
const plate = () => within(header()).getByRole('button', { name: /used/ });
const crumbs = () => screen.getByRole('navigation', { name: 'Map location' });
const cardNames = () =>
  within(screen.getByRole('radiogroup', { name: 'Drives' }))
    .getAllByRole('radio')
    .map((r) => r.getAttribute('aria-label'));

/** Opens the header's drive menu and chooses a drive. */
async function choose(name: string) {
  await userEvent.click(plate());
  await userEvent.click(within(screen.getByRole('menu', { name: 'Drives' })).getByRole('menuitemradio', { name }));
}

/** Scans the drive on screen and waits for its map. */
async function scanToMap(name: string) {
  await userEvent.click(await screen.findByRole('button', { name: `Scan ${name}` }));
  await screen.findByLabelText('Map key');
}

/** The map tile or region header with this name. */
const tile = (container: HTMLElement, name: string) => {
  const el = [...container.querySelectorAll<HTMLElement>('[data-tile]')].find(
    (t) => t.querySelector('.map-name, .map-region-name')?.textContent === name,
  );
  if (!el) throw new Error(`no tile named ${name}`);
  return el;
};

beforeEach(() => {
  // jsdom has no layout: give every element the app window's map size so the treemap has room.
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

afterEach(() => {
  vi.useRealTimers();
});

describe('choosing a drive on the scan screen', () => {
  it('offers a card per drive, the system drive first and lit, and scans the drive picked', async () => {
    const bridge = windows();
    const scan = vi.spyOn(bridge, 'scan');
    renderApp(bridge);

    const cards = await screen.findByRole('radiogroup', { name: 'Drives' });
    const [c, d, e] = within(cards).getAllByRole('radio') as [HTMLElement, HTMLElement, HTMLElement];
    expect(cardNames()).toEqual(['Local Disk (C:)', 'Data (D:)', 'SanDisk (E:)']);
    expect([c, d, e].map((r) => r.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    expect(within(c).getByText('Internal')).toBeInTheDocument();
    expect(within(c).getByText('System')).toBeInTheDocument();
    expect(c).toHaveTextContent('45 GB free of 476 GB');
    expect(c).toHaveTextContent('Not scanned yet');
    expect(within(d).queryByText('System')).toBeNull();
    expect(d).toHaveTextContent('1.3 TB free of 2 TB');
    expect(within(e).getByText('USB')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeInTheDocument();

    // Picking a card stays on the scan screen; the header follows it.
    await userEvent.click(d);
    expect(d).toHaveAttribute('aria-checked', 'true');
    expect(c).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByRole('heading', { name: ASK })).toBeInTheDocument();
    expect(within(header()).getByText('Data (D:)')).toBeInTheDocument();
    expect(within(header()).getByText('700 GB of 2 TB used')).toBeInTheDocument();

    // The arrow keys move between cards, like any radio group.
    await userEvent.keyboard('{ArrowRight}');
    expect(e).toHaveAttribute('aria-checked', 'true');
    expect(e).toHaveFocus();
    expect(screen.getByText(/Keep SanDisk \(E:\) plugged in until the scan finishes\./)).toBeInTheDocument();
    await userEvent.keyboard('{ArrowLeft}');
    expect(d).toHaveFocus();

    await scanToMap('Data (D:)');
    expect(scan).toHaveBeenCalledWith('D:\\', expect.any(Function));
    expect(within(crumbs()).getByText('Data (D:)')).toBeInTheDocument();
  });
});

describe('the header’s drive menu', () => {
  it('lists every drive with its usage and scanned state, on the scan screen and on the map', async () => {
    renderApp(windows());
    await screen.findByRole('heading', { name: ASK });

    await userEvent.click(plate());
    const menu = screen.getByRole('menu', { name: 'Drives' });
    const items = within(menu).getAllByRole('menuitemradio');
    expect(items.map((i) => i.getAttribute('aria-label'))).toEqual(['Local Disk (C:)', 'Data (D:)', 'SanDisk (E:)']);
    expect(items[0]).toHaveAttribute('aria-checked', 'true');
    expect(items[0]).toHaveFocus();
    expect(items[1]).toHaveAccessibleDescription(/^Internal 700\sGB of 2\sTB used, 1\.3\sTB free Not scanned yet$/);
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveAccessibleDescription(
      /^Internal System 45\sGB free of 476\sGB Not scanned yet$/,
    );
    expect(menu).toHaveTextContent('Drives on DESKTOP-ALEX');
    expect(menu).toHaveTextContent('Network and optical drives aren’t listed.');

    await userEvent.keyboard('{ArrowDown}');
    expect(items[1]).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(plate()).toHaveFocus();

    await scanToMap('Local Disk (C:)');
    await userEvent.click(plate());
    expect(screen.getByRole('menuitemradio', { name: 'Local Disk (C:)' })).toHaveAccessibleDescription(
      /Scanned just now/,
    );
    // A press anywhere else closes it.
    await userEvent.click(crumbs());
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('switches to a scanned drive’s map and back without scanning again, each drive with its own map', async () => {
    const bridge = windows();
    const scan = vi.spyOn(bridge, 'scan');
    const browse = vi.spyOn(bridge, 'browse');
    const { container } = renderApp(bridge);
    await scanToMap('Local Disk (C:)');

    // An unscanned drive opens its scan screen, where the scanned one says so.
    await choose('Data (D:)');
    expect(screen.getByRole('heading', { name: ASK })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Data (D:)' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveTextContent('Scanned just now');
    await scanToMap('Data (D:)');
    expect(within(crumbs()).getByText('Data (D:)')).toBeInTheDocument();

    // A scanned drive opens its map.
    await userEvent.click(plate());
    expect(screen.getByRole('menuitemradio', { name: 'Local Disk (C:)' })).toHaveTextContent('Open map');
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Local Disk (C:)' }));
    expect(await screen.findByLabelText('Map key')).toBeInTheDocument();
    expect(plate()).toHaveFocus();
    expect(within(crumbs()).getByText('Local Disk (C:)')).toBeInTheDocument();
    expect(within(header()).getByText('431 GB of 476 GB used')).toBeInTheDocument();
    expect(scan).toHaveBeenCalledTimes(2);

    // Each map opens its folders from its own drive.
    await userEvent.click(tile(container, 'Apps & games'));
    await userEvent.click(tile(container, 'Adobe'));
    await waitFor(() => expect(within(crumbs()).getByText('C:\\Program Files\\Adobe')).toBeInTheDocument());
    expect(browse).toHaveBeenLastCalledWith('C:\\', 'fx-apps-2');

    await choose('Data (D:)');
    expect(within(crumbs()).getByText('Data (D:)')).toHaveAttribute('aria-current', 'location');
    await userEvent.click(tile(container, 'Apps & games'));
    await userEvent.click(tile(container, 'Adobe'));
    await waitFor(() => expect(within(crumbs()).getByText('D:\\Program Files\\Adobe')).toBeInTheDocument());
    expect(browse).toHaveBeenLastCalledWith('D:\\', 'fx-apps-2');
    expect(scan).toHaveBeenCalledTimes(2);
  });

  it('keeps each drive’s plan apart, and says how much each drive could reclaim', async () => {
    renderApp(windows());
    await scanToMap('Local Disk (C:)');
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
    const tray = () => screen.getByRole('complementary', { name: 'Plan' });
    const node = () => within(tray()).getByRole('checkbox', { name: 'Include node_modules in 61 idle projects' });
    await userEvent.click(node());
    expect(node()).toHaveAttribute('aria-checked', 'false');

    // D: has its own proposals and its own plan, starting from the defaults. The tray closes on the way.
    await choose('Data (D:)');
    expect(screen.queryByRole('complementary', { name: 'Plan' })).toBeNull();
    await scanToMap('Data (D:)');
    expect(screen.queryByRole('complementary', { name: 'Plan' })).toBeNull();
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
    expect(node()).toHaveAttribute('aria-checked', 'true');
    expect(within(header()).getByText(/reclaimable/)).toHaveTextContent(/^122\.3\sGB reclaimable$/);

    await userEvent.click(plate());
    expect(screen.getByRole('menuitemradio', { name: 'Local Disk (C:)' })).toHaveAccessibleDescription(
      /Scanned just now, 75\sGB to reclaim$/,
    );
    expect(screen.getByRole('menuitemradio', { name: 'Data (D:)' })).toHaveAccessibleDescription(
      /Scanned just now, 122\sGB to reclaim$/,
    );
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Local Disk (C:)' }));
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
    expect(node()).toHaveAttribute('aria-checked', 'false');
    expect(within(header()).getByText(/reclaimable/)).toHaveTextContent(/^75\.3\sGB reclaimable$/);

    // The scan screen's cards say the same, with each drive's reclaimable share lit on its bar.
    await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveTextContent(/Scanned just now, 75\sGB to reclaim/);
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' }).querySelector('.usage > b')).not.toBeNull();
    expect(screen.getByRole('radio', { name: 'SanDisk (E:)' }).querySelector('.usage > b')).toBeNull();
  });

  it('stops a scan when switching to another drive part way', async () => {
    const bridge = windows({ scanMs: 60_000 });
    const cancel = vi.spyOn(bridge, 'cancelScan');
    renderApp(bridge);
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByRole('status', { name: 'Scan progress' });

    await userEvent.click(plate());
    const menu = screen.getByRole('menu', { name: 'Drives' });
    expect(menu).toHaveTextContent('Switching drives stops the scan of Local Disk (C:).');
    expect(within(menu).getByRole('menuitemradio', { name: 'Local Disk (C:)' })).toHaveTextContent('Scanning now');
    await userEvent.click(within(menu).getByRole('menuitemradio', { name: 'Data (D:)' }));

    expect(cancel).toHaveBeenCalled();
    expect(screen.queryByRole('status', { name: 'Scan progress' })).toBeNull();
    expect(screen.getByRole('radio', { name: 'Data (D:)' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveTextContent('Not scanned yet');
  });

  it('is locked while a plan runs', async () => {
    renderApp(windows(), <RunControls />);
    await scanToMap('Local Disk (C:)');

    await userEvent.click(screen.getByRole('button', { name: 'Start the run' }));
    expect(plate()).toHaveAttribute('aria-disabled', 'true');
    expect(plate()).toHaveAccessibleDescription('Switch drives after this run finishes.');
    await userEvent.click(plate());
    expect(screen.queryByRole('menu')).toBeNull();
    plate().focus();
    await userEvent.keyboard('{Enter}');
    expect(screen.queryByRole('menu')).toBeNull();

    await userEvent.click(screen.getByRole('button', { name: 'End the run' }));
    expect(plate()).not.toHaveAttribute('aria-disabled');
    await userEvent.click(plate());
    expect(screen.getByRole('menu', { name: 'Drives' })).toBeInTheDocument();
  });
});

describe('a drive that goes away', () => {
  it('leaves the list and takes its map with it, with a short notice when it was on screen', async () => {
    const bridge = windows();
    const forget = vi.spyOn(bridge, 'forgetDrive');
    renderApp(bridge);
    await userEvent.click(await screen.findByRole('radio', { name: 'SanDisk (E:)' }));
    await scanToMap('SanDisk (E:)');

    bridge.unplug('E:\\');
    act(() => {
      fireEvent.focus(window);
    });

    const notice = await screen.findByRole('status');
    expect(notice).toHaveTextContent('SanDisk (E:) is no longer connected, so its map was cleared.');
    expect(forget).toHaveBeenCalledWith('E:\\');
    await expect(bridge.browse('E:\\', 'fx-apps-2')).rejects.toThrow('there is no finished scan');
    // Back to the system drive, which hasn't been scanned: its scan screen, without the stick.
    expect(screen.getByRole('heading', { name: ASK })).toBeInTheDocument();
    expect(cardNames()).toEqual(['Local Disk (C:)', 'Data (D:)']);
    expect(screen.getByText('2 drives on DESKTOP-ALEX')).toBeInTheDocument();

    await userEvent.click(within(notice).getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByText(/no longer connected/)).toBeNull();
  });

  it('leaves quietly when it wasn’t on screen, noticed within a few seconds', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
    const bridge = windows();
    renderApp(bridge);
    await screen.findByRole('radio', { name: 'SanDisk (E:)' });

    bridge.unplug('E:\\');
    act(() => {
      vi.advanceTimersByTime(3000);
    });

    await waitFor(() => expect(screen.queryByRole('radio', { name: 'SanDisk (E:)' })).toBeNull());
    expect(screen.queryByText(/no longer connected/)).toBeNull();
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveAttribute('aria-checked', 'true');
  });
});

describe('the dot field', () => {
  it('ripples on the background, but not on the drive cards, between them, or on the drive menu', async () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(fakeContext().ctx);
    designTokens({ '--dot-rgb': '196, 205, 219', '--neon-rgb': '255, 79, 179' });
    prefersReducedMotion(true);
    const frames = manualFrames();
    const { container } = renderApp(windows());
    const cards = await screen.findByRole('radiogroup', { name: 'Drives' });
    frames.run();
    frames.run();
    expect(frames.waiting).toBe(0);

    fireEvent.pointerDown(within(cards).getByRole('radio', { name: 'Data (D:)' }), { button: 0 });
    fireEvent.pointerDown(within(cards).getByText('Data (D:)'), { button: 0 });
    fireEvent.pointerDown(cards, { button: 0 }); // the gap between two cards
    await userEvent.click(plate());
    fireEvent.pointerDown(screen.getByRole('menu', { name: 'Drives' }), { button: 0 });
    expect(frames.waiting).toBe(0);

    fireEvent.pointerDown(container.querySelector('.atlas')!, { button: 0, clientX: 40, clientY: 400 });
    expect(frames.waiting).toBeGreaterThan(0);
  });
});

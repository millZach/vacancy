import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type Bridge, type MapFolder, type MockOptions } from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';

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

const ASK = 'Which drive should Vacancy scan?';
/** The window's header, where the drive plate is. */
const header = () => screen.getByRole('banner');

const windows = (options: MockOptions = {}) => createMockBridge(fixtures.windows, { scanMs: 0, ...options });

/** Scans the plate's drive and waits for the map. */
async function scanToMap(name = 'Local Disk (C:)') {
  await userEvent.click(await screen.findByRole('button', { name: `Scan ${name}` }));
  await screen.findByLabelText('Map key');
}

/** The tile or header with this text. */
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

describe('the scan plate', () => {
  it('asks which drive to scan, the system drive first, also shown in the header with used and free space', async () => {
    renderApp(windows());

    expect(await screen.findByRole('heading', { name: ASK })).toBeInTheDocument();
    expect(screen.getByText('3 drives on DESKTOP-ALEX')).toBeInTheDocument();
    expect(within(header()).getByText('Local Disk (C:)')).toBeInTheDocument();
    expect(within(header()).getByText('431 GB of 476 GB used')).toBeInTheDocument();
    expect(within(header()).getByText('45 GB free')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
  });

  it('is the single drive plate when there is only one drive', async () => {
    const fixture = { ...fixtures.windows, volumes: fixtures.windows.volumes.slice(0, 1) };
    renderApp(createMockBridge(fixture, { scanMs: 0 }));

    expect(await screen.findByRole('heading', { name: 'Local Disk (C:)' })).toBeInTheDocument();
    expect(screen.getByText('476 GB on DESKTOP-ALEX, 431 GB used')).toBeInTheDocument();
    expect(screen.queryByRole('radiogroup', { name: 'Drives' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
  });

  it('says no AI agent was found, and that scanning only reads', async () => {
    renderApp(windows());

    expect(await screen.findByText('No AI agent found.')).toBeInTheDocument();
    expect(screen.getByText(/Vacancy will use its built-in rules only/)).toBeInTheDocument();
    expect(screen.getByText('Rules only')).toBeInTheDocument();
    expect(screen.getByText('Scanning only reads. Nothing changes until you approve a plan.')).toBeInTheDocument();
  });

  it('shows a Mac’s drives, with the header ready for the traffic lights', async () => {
    const { container } = renderApp(createMockBridge(fixtures.macos));

    const cards = await screen.findByRole('radiogroup', { name: 'Drives' });
    expect(within(cards).getAllByRole('radio').map((r) => r.getAttribute('aria-label'))).toEqual([
      'Macintosh HD',
      'Samsung T7',
    ]);
    expect(screen.getByText('2 drives on Alex’s MacBook Air')).toBeInTheDocument();
    expect(within(header()).getByText('460 GB of 494 GB used')).toBeInTheDocument();
    expect(container.querySelector('.shell')).toHaveAttribute('data-chrome', 'overlay');
    expect(container.querySelector('.shell-head')).toHaveAttribute('data-tauri-drag-region', 'deep');
  });

  it('keeps the native title bar on Windows', async () => {
    const { container } = renderApp(windows());

    await screen.findByRole('heading', { name: ASK });
    expect(container.querySelector('.shell')).toHaveAttribute('data-chrome', 'native');
    expect(container.querySelector('[data-tauri-drag-region]')).toBeNull();
  });

  it('says so when the drive cannot be read', async () => {
    renderApp(windows({ fail: 'Access is denied. (os error 5)' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Access is denied. (os error 5)');
    expect(screen.getByRole('heading', { name: 'Can’t read your drives' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Scan/ })).toBeNull();
  });
});

describe('Full Disk Access on a Mac', () => {
  it('explains what is missing and opens the right System Settings pane', async () => {
    const bridge = createMockBridge(fixtures.macos, { scanMs: 0 });
    const open = vi.spyOn(bridge, 'openFullDiskAccessSettings');
    renderApp(bridge);

    expect(await screen.findByText('Vacancy can’t see everything yet.')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Open System Settings' }));
    expect(open).toHaveBeenCalledTimes(1);

    // The scan still runs without it, and the map keeps the way to fix it.
    await scanToMap('Macintosh HD');
    await userEvent.click(screen.getByRole('button', { name: 'Allow Full Disk Access' }));
    expect(open).toHaveBeenCalledTimes(2);
    expect(screen.getByText('111 folders couldn’t be read')).toBeInTheDocument();
  });

  it('says nothing when access is granted', async () => {
    renderApp(createMockBridge(fixtures.macos, { fullDiskAccess: 'granted' }));
    await screen.findByRole('heading', { name: ASK });
    expect(screen.queryByText('Vacancy can’t see everything yet.')).toBeNull();
  });

  it('is never mentioned on Windows', async () => {
    renderApp(windows());
    await scanToMap();
    expect(screen.queryByText('Vacancy can’t see everything yet.')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Allow Full Disk Access' })).toBeNull();
  });
});

describe('scanning', () => {
  it('floats the HUD over the map as it fills in, then shows the map', async () => {
    const { container } = renderApp(windows({ scanMs: 600 }));
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));

    const hud = await screen.findByRole('status', { name: 'Scan progress' });
    expect(within(hud).getByText('files')).toBeInTheDocument();
    expect(within(hud).getByRole('progressbar', { name: 'Scanned' })).toBeInTheDocument();
    expect(screen.getByText('Mapping as it scans')).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: ASK })).toBeNull();
    await waitFor(() => expect(container.querySelectorAll('.map-region').length).toBeGreaterThan(1));

    await screen.findByLabelText('Map key', undefined, { timeout: 3000 });
    expect(screen.queryByRole('status', { name: 'Scan progress' })).toBeNull();
    expect(plain(within(hud.ownerDocument.body).getByText('111 folders couldn’t be read').textContent)).toBe(
      '111 folders couldn’t be read',
    );
  });

  it('can be cancelled, back to the plate', async () => {
    renderApp(windows({ scanMs: 60_000 }));
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

    expect(await screen.findByRole('heading', { name: ASK })).toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Scan progress' })).toBeNull();
  });

  it('says why a scan stopped, and offers it again', async () => {
    const bridge = windows();
    vi.spyOn(bridge, 'scan').mockRejectedValue('could not scan C:\\: Access is denied. (os error 5)');
    renderApp(bridge);
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('The scan stopped: could not scan C:\\: Access is denied.');
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
  });
});

describe('the map', () => {
  it('shows every category, locked ones hatched, and free space', async () => {
    const { container } = renderApp(windows());
    await scanToMap();

    const heads = [...container.querySelectorAll('.map-region-name')].map((h) => h.textContent);
    expect(heads).toEqual([
      'Apps & games',
      'Developer',
      'Photos, videos & documents',
      'Windows & system',
      'Downloads',
      'Caches & temp',
      'OneDrive & Dropbox',
      'Other',
    ]);
    expect(tile(container, 'Windows & system').closest('.map-region')).toHaveClass('is-locked');
    expect(tile(container, 'OneDrive & Dropbox').closest('.map-region')).toHaveClass('is-locked');
    expect(tile(container, 'Apps & games').closest('.map-region')).not.toHaveClass('is-locked');
    expect(tile(container, 'Windows')).toHaveClass('is-locked');
    const free = tile(container, 'Free space');
    expect(free).toHaveClass('map-tile--free');
    expect(plain(free.textContent)).toBe('Free space45.0 GB');
    expect(screen.getByLabelText('Map key')).toHaveTextContent('Protected');
  });

  it('says so when the scan counted more than the OS reports in use', async () => {
    const fixture = structuredClone(fixtures.windows);
    fixture.overview.scan.overCountBytes = 2e9;
    renderApp(createMockBridge(fixture, { scanMs: 0 }));
    await scanToMap();

    expect(
      screen.getByText((text) => plain(text) === 'Counted 2.0 GB more than Windows says is in use: files changed during the scan'),
    ).toBeInTheDocument();
  });

  it('adds no over-count note when the scan fits in the used space', async () => {
    renderApp(windows());
    await scanToMap();
    expect(screen.queryByText(/more than Windows says is in use/)).toBeNull();
  });

  it('shows a tile’s path, size and last use on hover', async () => {
    const { container } = renderApp(windows());
    await scanToMap();

    fireEvent.pointerMove(tile(container, 'Cyberpunk 2077'), { clientX: 100, clientY: 100 });
    const card = screen.getByRole('tooltip');
    expect(card).toHaveTextContent('Cyberpunk 2077');
    expect(card).toHaveTextContent('C:\\Program Files (x86)\\Steam\\steamapps\\common\\Cyberpunk 2077');
    expect(plain(card.textContent)).toContain('71.0 GB');
    expect(card).toHaveTextContent('15% of the disk');
    expect(card).toHaveTextContent('Last used 14 months ago');
    expect(card).toHaveTextContent('Click to zoom in.');

    fireEvent.pointerMove(tile(container, 'Windows'), { clientX: 100, clientY: 100 });
    expect(screen.getByRole('tooltip')).toHaveTextContent('Protected. Vacancy never changes anything here.');

    fireEvent.pointerMove(tile(container, "Space Vacancy can't see"), { clientX: 9, clientY: 9 });
    expect(screen.getByRole('tooltip')).toHaveTextContent('Windows counts this as used');

    fireEvent.pointerLeave(container.querySelector('.map')!);
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('zooms into a category, opens its folders, and goes back with the breadcrumb', async () => {
    const { container } = renderApp(windows());
    await scanToMap();
    const crumbs = () => screen.getByRole('navigation', { name: 'Map location' });

    await userEvent.click(tile(container, 'Apps & games'));
    expect(within(crumbs()).getByText('Apps & games')).toHaveAttribute('aria-current', 'location');
    expect(within(crumbs()).getByText('Installed programs and games.')).toBeInTheDocument();
    expect(tile(container, 'Developer').closest('.map-region')).toHaveStyle({ opacity: '0' });

    await userEvent.click(tile(container, 'Adobe'));
    await waitFor(() => expect(within(crumbs()).getByText('Adobe')).toHaveAttribute('aria-current', 'location'));
    expect(within(crumbs()).getByText('C:\\Program Files\\Adobe')).toBeInTheDocument();
    for (const name of ['Adobe Photoshop 2026', 'Adobe Premiere Pro 2026', 'Adobe Creative Cloud']) {
      expect(tile(container, name)).toBeInTheDocument();
    }
    expect(() => tile(container, 'Cyberpunk 2077')).toThrow();

    await userEvent.click(within(crumbs()).getByRole('button', { name: 'Apps & games' }));
    expect(tile(container, 'Cyberpunk 2077')).toBeInTheDocument();

    await userEvent.keyboard('{Escape}');
    expect(within(crumbs()).queryByText('Apps & games')).toBeNull();
    expect(tile(container, 'Developer').closest('.map-region')).toHaveStyle({ opacity: '1' });
  });

  it('drops a folder that arrives after the user has moved on', async () => {
    const bridge = windows();
    let arrive: (folder: MapFolder) => void = () => {};
    vi.spyOn(bridge, 'browse').mockImplementation(
      () =>
        new Promise<MapFolder>((resolve) => {
          arrive = resolve;
        }),
    );
    const { container } = renderApp(bridge);
    await scanToMap();
    const crumbs = () => screen.getByRole('navigation', { name: 'Map location' });

    await userEvent.click(tile(container, 'Apps & games'));
    await userEvent.click(tile(container, 'Adobe'));
    expect(screen.getByLabelText('Opening')).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    await act(async () => {
      arrive(fixtures.windows.folders['fx-apps-2']!);
      await Promise.resolve();
    });

    expect(within(crumbs()).queryByText('Adobe')).toBeNull();
    expect(within(crumbs()).queryByText('Apps & games')).toBeNull();
    expect(screen.queryByLabelText('Opening')).toBeNull();
    expect(tile(container, 'Developer').closest('.map-region')).toHaveStyle({ opacity: '1' });
  });

  it('opens folders from the keyboard too', async () => {
    const { container } = renderApp(windows());
    await scanToMap();
    await userEvent.click(tile(container, 'Developer'));

    tile(container, 'code').focus();
    await userEvent.keyboard('{Enter}');
    await waitFor(() => expect(tile(container, 'target')).toBeInTheDocument());
    const crumbs = screen.getByRole('navigation', { name: 'Map location' });
    expect(within(crumbs).getByText('code')).toHaveAttribute('aria-current', 'location');
    // Folders that hold nothing more to show are not buttons.
    expect(tile(container, 'target')).not.toHaveAttribute('role');
  });

  it('starts again from the scan screen, which can also go back to the map', async () => {
    const bridge = windows();
    const scan = vi.spyOn(bridge, 'scan');
    renderApp(bridge);
    await scanToMap();
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    });
    expect(await screen.findByRole('heading', { name: ASK })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveAttribute('aria-checked', 'true');

    await userEvent.click(screen.getByRole('button', { name: 'Open its map' }));
    expect(await screen.findByLabelText('Map key')).toBeInTheDocument();
    expect(scan).toHaveBeenCalledTimes(1);

    await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    expect(await screen.findByLabelText('Map key')).toBeInTheDocument();
    expect(scan).toHaveBeenCalledTimes(2);
  });
});

/** A proposal's tile on the map. */
const proposalTile = (container: HTMLElement, id: string) => {
  const el = container.querySelector<HTMLElement>(`[data-tile="p:${id}"]`);
  if (!el) throw new Error(`no tile for proposal ${id}`);
  return el;
};
const tray = () => screen.getByRole('complementary', { name: 'Plan' });
const row = (title: string) => within(tray()).getByRole('checkbox', { name: `Include ${title}` });
const freed = () => plain(within(tray()).getByText('will be freed').previousElementSibling?.textContent);

describe('proposals and the plan', () => {
  it('lights reclaimable tiles, and shows how much the drive could get back', async () => {
    const { container } = renderApp(windows());
    await scanToMap();

    const node = proposalTile(container, 'w-node');
    expect(node).toHaveClass('is-reclaim');
    expect(node).toHaveTextContent('node_modules in 61 idle projects');
    expect(tile(container, 'Cyberpunk 2077')).not.toHaveClass('is-reclaim');
    expect(plain(within(header()).getByText(/reclaimable/).textContent)).toBe('75.3 GB reclaimable');
    expect(container.querySelector('.usage--head > b')).not.toBeNull();
    expect(screen.getByLabelText('Map key')).toHaveTextContent('Reclaimable');
  });

  it('says what a reclaimable tile is on hover', async () => {
    const { container } = renderApp(windows());
    await scanToMap();

    fireEvent.pointerMove(proposalTile(container, 'w-installers'), { clientX: 50, clientY: 50 });
    const card = screen.getByRole('tooltip');
    expect(card).toHaveTextContent('Old installers in Downloads');
    expect(card).toHaveTextContent('Review');
    expect(card).toHaveTextContent('Built-in rule');
    expect(card).toHaveTextContent('47 .exe and .msi files older than 30 days.');
    expect(card).toHaveTextContent('Click to zoom in.');
  });

  it('opens the plan tray with each proposal’s risk, source, why and sample paths', async () => {
    renderApp(windows());
    await scanToMap();
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));

    expect(within(tray()).getByRole('heading', { name: 'Delete 8 of 8' })).toBeInTheDocument();
    expect(row('Old installers in Downloads')).toHaveAttribute('aria-checked', 'true');
    expect(within(header()).queryByRole('button', { name: 'Build a plan' })).toBeNull();

    const title = within(tray()).getByRole('button', { name: 'Old installers in Downloads' });
    expect(title).toHaveAttribute('aria-expanded', 'false');
    await userEvent.click(title);
    expect(title).toHaveAttribute('aria-expanded', 'true');
    expect(tray()).toHaveTextContent('These programs are already installed or can be downloaded again.');
    const paths = within(tray()).getByRole('list', { name: 'Sample paths' });
    expect(paths).toHaveTextContent('C:\\Users\\alex\\Downloads\\SteamSetup.exe');
    expect(tray()).not.toHaveTextContent('Comes back on its own if you need it.');

    const installers = row('Old installers in Downloads').closest('.tray-row') as HTMLElement;
    expect(within(installers).getByText('Review')).toHaveClass('risk', 'risk--review');
    expect(within(installers).getByText('Built-in rule')).toBeInTheDocument();

    expect(freed()).toBe('75.3 GB');
    expect(tray()).toHaveTextContent('712,567 items in 399 places, deleted permanently');
    const free = within(tray()).getByRole('button', { name: /^Free 75\.3\sGB$/ });
    expect(free).toBeDisabled();
    expect(free).toHaveAccessibleDescription('Vacancy can’t run plans yet. This version only proposes.');

    await userEvent.click(within(tray()).getByRole('button', { name: 'Hide the plan' }));
    expect(screen.queryByRole('complementary', { name: 'Plan' })).toBeNull();
    expect(within(header()).getByRole('button', { name: 'Build a plan' })).toBeInTheDocument();
  });

  it('takes a proposal out and puts it back, from its row or its tile', async () => {
    const { container } = renderApp(windows());
    await scanToMap();
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));

    expect(proposalTile(container, 'w-node')).toHaveClass('is-reclaim', 'is-selected');
    expect(proposalTile(container, 'w-node')).toHaveAttribute('aria-pressed', 'true');
    expect(proposalTile(container, 'w-node')).toHaveAccessibleName(
      /^node_modules in 61 idle projects, 21\.0\sGB, reclaimable, in the plan$/,
    );
    await userEvent.click(row('node_modules in 61 idle projects'));
    expect(proposalTile(container, 'w-node')).toHaveAttribute('aria-pressed', 'false');
    expect(row('node_modules in 61 idle projects')).toHaveAttribute('aria-checked', 'false');
    expect(freed()).toBe('54.3 GB');
    expect(proposalTile(container, 'w-node')).toHaveClass('is-available');
    expect(proposalTile(container, 'w-node')).not.toHaveClass('is-reclaim');

    fireEvent.pointerMove(proposalTile(container, 'w-node'), { clientX: 50, clientY: 50 });
    expect(screen.getByRole('tooltip')).toHaveTextContent('Click to add it to the plan.');
    await userEvent.click(proposalTile(container, 'w-node'));
    expect(row('node_modules in 61 idle projects')).toHaveAttribute('aria-checked', 'true');
    expect(freed()).toBe('75.3 GB');

    // Everything out: nothing to free.
    for (const p of fixtures.windows.proposals) await userEvent.click(row(p.title));
    expect(within(tray()).getByRole('button', { name: 'Nothing selected yet' })).toBeDisabled();
    expect(freed()).toBe('0 B');
  });

  it('lights a proposal’s row and tile together', async () => {
    const { container } = renderApp(windows());
    await scanToMap();
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));

    const temp = row('Temp folders').closest('.tray-row') as HTMLElement;
    fireEvent.mouseEnter(temp);
    expect(proposalTile(container, 'w-temp')).toHaveClass('is-hot');
    fireEvent.mouseLeave(temp);
    expect(proposalTile(container, 'w-temp')).not.toHaveClass('is-hot');
    // From the keyboard too.
    act(() => row('Temp folders').focus());
    expect(proposalTile(container, 'w-temp')).toHaveClass('is-hot');
    act(() => row('Temp folders').blur());
    expect(proposalTile(container, 'w-temp')).not.toHaveClass('is-hot');
    fireEvent.pointerMove(proposalTile(container, 'w-browser'), { clientX: 50, clientY: 50 });
    expect(row('Browser caches').closest('.tray-row')).toHaveClass('is-hot');
  });

  it('adds a proposal from a zoomed-in category and opens the tray', async () => {
    const { container } = renderApp(createMockBridge(fixtures.macos, { scanMs: 0 }));
    await scanToMap('Macintosh HD');

    await userEvent.click(proposalTile(container, 'm-archives'));
    const crumbs = screen.getByRole('navigation', { name: 'Map location' });
    expect(within(crumbs).getByText('Developer')).toHaveAttribute('aria-current', 'location');
    expect(screen.queryByRole('complementary', { name: 'Plan' })).toBeNull();

    fireEvent.pointerMove(proposalTile(container, 'm-archives'), { clientX: 50, clientY: 50 });
    expect(screen.getByRole('tooltip')).toHaveTextContent('Click to add it to the plan.');
    await userEvent.click(proposalTile(container, 'm-archives'));
    expect(row('Xcode archives older than a year')).toHaveAttribute('aria-checked', 'true');
    expect(within(tray()).getByRole('heading', { name: 'Delete 10 of 10' })).toBeInTheDocument();
  });

  it('starts the plan with review proposals off unless their rule turns them on', async () => {
    renderApp(createMockBridge(fixtures.macos, { scanMs: 0 }));
    await scanToMap('Macintosh HD');
    await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));

    expect(row('Xcode archives older than a year')).toHaveAttribute('aria-checked', 'false');
    expect(row('Old disk images in Downloads')).toHaveAttribute('aria-checked', 'true');
    expect(row('Xcode DerivedData')).toHaveAttribute('aria-checked', 'true');
    expect(within(tray()).getByRole('heading', { name: 'Delete 9 of 10' })).toBeInTheDocument();
  });
});

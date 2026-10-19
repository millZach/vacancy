import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type Bridge, type MockOptions } from '../bridge';
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

const FAST = 'Fast scan (asks for admin)';
const windows = (options: MockOptions = {}) => createMockBridge(fixtures.windows, { scanMs: 0, uacMs: 0, ...options });
const hud = () => screen.getByRole('status', { name: 'Scan progress' });
const notes = (container: HTMLElement) => plain(container.querySelector('.scan-notes')?.textContent);

/** The tile with this name. */
const tile = (container: HTMLElement, name: string) => {
  const el = [...container.querySelectorAll<HTMLElement>('[data-tile]')].find(
    (t) => t.querySelector('.map-name, .map-region-name, .locked-name')?.textContent === name,
  );
  if (!el) throw new Error(`no tile named ${name}`);
  return el;
};

/** Picks a drive on the plate, then asks for a fast scan of it and waits for the map. */
async function fastScanToMap(drive?: string) {
  if (drive) await userEvent.click(await screen.findByRole('radio', { name: drive }));
  await userEvent.click(await screen.findByRole('button', { name: FAST }));
  await screen.findByLabelText('Map key', undefined, { timeout: 4000 });
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

describe('the scan plate on Windows', () => {
  it('offers a fast scan beside the normal one, with one line saying what admin is for', async () => {
    renderApp(windows());
    expect(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
    expect(screen.getByRole('button', { name: FAST })).toBeEnabled();
    expect(
      screen.getByText(
        'Fast scan reads the drive’s file index directly instead of opening every folder. Windows asks for admin each time, and Vacancy uses it only for that read. Deleting still happens normally, with the same checks.',
      ),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole('radio', { name: 'SanDisk (E:)' }));
    expect(screen.getByText(/^Fast scan reads an NTFS drive’s file index directly\./)).toBeInTheDocument();
  });

  it('offers it on the single drive plate too, and beside Scan again once the drive has a map', async () => {
    const fixture = { ...fixtures.windows, volumes: fixtures.windows.volumes.slice(0, 1) };
    renderApp(createMockBridge(fixture, { scanMs: 0, uacMs: 0 }));
    expect(await screen.findByRole('button', { name: FAST })).toBeEnabled();

    await userEvent.click(screen.getByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByLabelText('Map key');
    await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    expect(screen.getByRole('button', { name: 'Open its map' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Scan again' })).toBeEnabled();
    expect(screen.getByRole('button', { name: FAST })).toBeEnabled();
  });

  it('asks the bridge for a fast scan of the drive on screen, and a normal one from Scan', async () => {
    const bridge = windows();
    const scan = vi.spyOn(bridge, 'scan');
    renderApp(bridge);
    await userEvent.click(await screen.findByRole('button', { name: FAST }));
    expect(scan).toHaveBeenLastCalledWith('C:\\', expect.any(Function), { fast: true });
    await screen.findByLabelText('Map key');

    await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    await userEvent.click(screen.getByRole('button', { name: 'Scan again' }));
    expect(scan).toHaveBeenLastCalledWith('C:\\', expect.any(Function));
  });

  it('keeps the plate up while Windows asks for admin, its buttons waiting, and scans nothing yet', async () => {
    renderApp(windows({ uacMs: 60_000 }));
    await userEvent.click(await screen.findByRole('button', { name: FAST }));

    expect(await screen.findByText('Waiting for Windows’ admin prompt…')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: FAST })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeDisabled();
    expect(screen.queryByRole('status', { name: 'Scan progress' })).toBeNull();
    expect(screen.getByRole('heading', { name: 'Which drive should Vacancy scan?' })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Local Disk (C:)' })).toHaveTextContent('Waiting for admin');
  });
});

describe('the scan plate on a Mac', () => {
  it('is as it was: no fast scan', async () => {
    renderApp(createMockBridge(fixtures.macos, { scanMs: 0 }));
    expect(await screen.findByRole('button', { name: 'Scan Macintosh HD' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: /Fast scan/ })).toBeNull();
    expect(screen.queryByText(/file index/)).toBeNull();
  });
});

describe('a fast scan running', () => {
  it('is tagged in the HUD, reads the file index, then sorts it into categories', async () => {
    renderApp(windows({ scanMs: 4000, uacMs: 0 }));
    await userEvent.click(await screen.findByRole('button', { name: FAST }));

    await waitFor(() => expect(hud()).toHaveTextContent('Reading the file index of Local Disk (C:)'));
    expect(hud()).toHaveClass('is-fast');
    expect(within(hud()).getByText('Fast scan')).toBeInTheDocument();
    expect(screen.getByText('Mapping from the file index')).toBeInTheDocument();
    const bar = within(hud()).getByRole('progressbar', { name: 'Scanned' });
    expect(Number(bar.getAttribute('aria-valuenow'))).toBeLessThanOrEqual(60);

    await waitFor(() => expect(hud()).toHaveTextContent('Sorting Local Disk (C:) into categories'), { timeout: 3000 });
    await waitFor(() => expect(Number(bar.getAttribute('aria-valuenow'))).toBeGreaterThanOrEqual(60));
  });
});

describe('a normal scan in place of a fast one', () => {
  it.each([
    [
      'declined',
      'Local Disk (C:)',
      'Running the normal scan. Windows wasn’t given admin, so Vacancy can’t read the file index. This takes a few minutes.',
      'Normal scan: admin wasn’t given',
    ],
    [
      'failed',
      'Local Disk (C:)',
      'Running the normal scan. Vacancy’s scan helper didn’t start, so it can’t read the file index.',
      'Normal scan: the helper didn’t start',
    ],
    [
      'unreadable',
      'Local Disk (C:)',
      'Running the normal scan. The drive’s file index couldn’t be read (a drive BitLocker has locked, for one).',
      'Normal scan: the index couldn’t be read',
    ],
    [
      'ok',
      'SanDisk (E:)',
      'Running the normal scan. SanDisk (E:) is exFAT, and fast scan only reads NTFS drives. No admin was asked for.',
      'Normal scan: exFAT drive',
    ],
  ] as const)('says why in the HUD and on the map (%s, %s)', async (fast, drive, note, label) => {
    const { container } = renderApp(windows({ fast, scanMs: 1500 }));
    await userEvent.click(await screen.findByRole('radio', { name: drive }));
    await userEvent.click(await screen.findByRole('button', { name: FAST }));

    await waitFor(() => expect(within(hud()).getByRole('note')).toHaveTextContent(note));
    expect(hud()).not.toHaveClass('is-fast');
    expect(within(hud()).queryByText('Fast scan')).toBeNull();

    await screen.findByLabelText('Map key', undefined, { timeout: 4000 });
    expect(notes(container)).toContain(label);
    expect(notes(container)).toContain('folders couldn’t be read');
    expect(notes(container)).not.toContain('Fast scan');
  });
});

describe('a fast scan’s map', () => {
  it('says it was a fast scan and how much is in locked folders', async () => {
    const { container } = renderApp(windows());
    await fastScanToMap();

    expect(notes(container)).toContain('Fast scan, 9 s');
    expect(notes(container)).toContain('26 GB in locked folders, totals only');
    expect(notes(container)).not.toContain('couldn’t be read');
    expect(screen.getByText(/Fast scan, 9 s/)).toHaveAttribute(
      'title',
      'Read from the drive’s file index, with admin, then admin was given back.',
    );
  });

  it('draws the Locked folders block before free space, hatched, its tiles totals only', async () => {
    const { container } = renderApp(windows());
    await fastScanToMap();

    const heads = [...container.querySelectorAll('.map-region-name')].map((h) => h.textContent);
    expect(heads).toContain('Locked folders');
    const block = container.querySelector('.map-region[data-region="locked"]')!;
    expect(block).toHaveClass('is-locked');
    const accounts = tile(container, 'Other accounts on this PC');
    expect(accounts).toHaveClass('map-tile--locked');
    expect(plain(accounts.textContent)).toBe('Other accounts on this PC21.4 GB · totals only');

    fireEvent.pointerMove(accounts, { clientX: 100, clientY: 100 });
    const card = screen.getByRole('tooltip');
    expect(card).toHaveTextContent(
      'Files that belong to another account. Only that account, or Windows, can open them. Fast scan counted 1,204 folders here and shows their total only.',
    );
    expect(card).toHaveTextContent('Protected. Vacancy never changes anything here.');
  });

  it('opens the block to say what it is, and never opens a folder inside it', async () => {
    const bridge = windows();
    const browse = vi.spyOn(bridge, 'browse');
    const { container } = renderApp(bridge);
    await fastScanToMap();

    await userEvent.click(tile(container, 'Other accounts on this PC'));
    const crumbs = screen.getByRole('navigation', { name: 'Map location' });
    expect(within(crumbs).getByText('Locked folders')).toHaveAttribute('aria-current', 'location');
    const about = screen.getByRole('complementary', { name: 'About locked folders' });
    expect(plain(about.textContent)).toContain('25.7 GB in 1,287 locked folders');
    expect(about).toHaveTextContent('no names, nothing to propose, nothing to delete');
    expect(about).toHaveTextContent('Admin from a fast scan is never used to open them.');
    expect(notes(container)).not.toContain('locked folders, totals only');

    // Zoomed in, its tiles do nothing.
    const accounts = tile(container, 'Other accounts on this PC');
    expect(accounts).not.toHaveAttribute('role', 'button');
    await userEvent.click(accounts);
    expect(within(crumbs).getByText('Locked folders')).toHaveAttribute('aria-current', 'location');
    expect(browse).not.toHaveBeenCalled();
  });

  it('counts the file index’s records in Windows & system', async () => {
    const { container } = renderApp(windows());
    await fastScanToMap();

    const records = container.querySelector<HTMLElement>('[data-tile="records"]')!;
    expect(records).toHaveClass('map-tile--records');
    fireEvent.pointerMove(records, { clientX: 100, clientY: 100 });
    expect(screen.getByRole('tooltip')).toHaveTextContent('File system records');
    expect(screen.getByRole('tooltip')).toHaveTextContent('Fast scan reads them; Windows keeps them up to date.');
    expect(() => tile(container, "Space Vacancy can't see")).toThrow();
  });
});

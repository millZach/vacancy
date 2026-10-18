import { act, render, renderHook, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type AdvisorStatus, type Bridge, type PlanSummary } from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { plan } from '../test/run';
import { useAdvisorStatus } from './useAdvisorStatus';

const renderApp = (bridge: Bridge) => render(<BridgeProvider bridge={bridge}><DrivesProvider><App /></DrivesProvider></BridgeProvider>);
const ready = (status: AdvisorStatus) => ({ ...createMockBridge(fixtures.windows, { scanMs: 0 }), advisorStatus: vi.fn().mockResolvedValue(status) });
const openHeader = async () => {
  await userEvent.click(screen.getByRole('button', { name: 'Rules only' }));
  return screen.getByRole('dialog');
};
const assertOff = (dialog: HTMLElement) => {
  const switches = within(dialog).getAllByRole('switch');
  expect(switches).toHaveLength(2);
  for (const control of switches) { expect(control).toBeDisabled(); expect(control).toHaveAttribute('aria-checked', 'false'); }
};

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(DOMRect.fromRect({ width: 1200, height: 700 }));
});

describe('read-only advisor options', () => {
  it.each(['windows', 'macos'] as const)('keeps a delayed %s confirmation out of the advisor keyboard path', async (platform) => {
    const fixture = fixtures[platform];
    const bridge = createMockBridge(fixture, { scanMs: 0 });
    let complete!: (made: PlanSummary) => void;
    vi.spyOn(bridge, 'buildPlan').mockImplementation(() => new Promise<PlanSummary>((resolve) => { complete = resolve; }));
    const run = vi.spyOn(bridge, 'runPlan');
    renderApp(bridge);
    await userEvent.click(await screen.findByRole('button', { name: `Scan ${fixture.volumes[0]!.name}` }));
    await screen.findByLabelText('Map key');
    await userEvent.click(screen.getByRole('button', { name: 'Build a plan' }));
    await userEvent.click(within(screen.getByRole('complementary', { name: 'Plan' })).getByRole('button', { name: /^Free / }));
    const dialog = await openHeader();
    const close = within(dialog).getByRole('button', { name: 'Close AI options' });
    expect(close).toHaveFocus();
    await act(async () => { complete({ ...plan, mountPoint: fixture.volumes[0]!.mountPoint }); await Promise.resolve(); });
    expect(close).toHaveFocus();
    expect(screen.queryByRole('alertdialog', { hidden: true })).toBeNull();
    await userEvent.tab();
    expect(within(dialog).getByRole('button', { name: 'Why does AI stay off?' })).toHaveFocus();
    await userEvent.keyboard('{Enter}');
    expect(within(dialog).getByRole('button', { name: 'Why does AI stay off?' })).toHaveAttribute('aria-expanded', 'true');
    expect(run).not.toHaveBeenCalled();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    const confirmation = screen.getByRole('alertdialog');
    expect(within(confirmation).getByRole('button', { name: 'Go back' })).toHaveFocus();
    await userEvent.keyboard('{Enter}');
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(run).not.toHaveBeenCalled();
  });

  it.each(['windows', 'macos'] as const)('keeps a delayed %s plan error behind the advisor without a confirmation', async (platform) => {
    const fixture = fixtures[platform];
    const bridge = createMockBridge(fixture, { scanMs: 0 });
    let fail!: (error: Error) => void;
    vi.spyOn(bridge, 'buildPlan').mockImplementation(() => new Promise<PlanSummary>((_resolve, reject) => { fail = reject; }));
    const run = vi.spyOn(bridge, 'runPlan');
    renderApp(bridge);
    await userEvent.click(await screen.findByRole('button', { name: `Scan ${fixture.volumes[0]!.name}` }));
    await screen.findByLabelText('Map key');
    await userEvent.click(screen.getByRole('button', { name: 'Build a plan' }));
    await userEvent.click(within(screen.getByRole('complementary', { name: 'Plan' })).getByRole('button', { name: /^Free / }));
    const dialog = await openHeader();
    await act(async () => { fail(new Error('Plan check failed.')); await Promise.resolve(); });
    expect(within(dialog).getByRole('button', { name: 'Close AI options' })).toHaveFocus();
    expect(screen.queryByRole('alertdialog', { hidden: true })).toBeNull();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.getByRole('button', { name: 'Rules only' })).toHaveFocus();
    expect(within(screen.getByRole('complementary', { name: 'Plan' })).getByRole('alert')).toHaveTextContent('Plan check failed.');
    expect(screen.queryByRole('alertdialog', { hidden: true })).toBeNull();
    expect(run).not.toHaveBeenCalled();
  });

  it('uses actual synthetic rows, preserves privacy details, and returns focus to the scan-note trigger', async () => {
    const bridge = createMockBridge(fixtures.windows);
    const scan = vi.spyOn(bridge, 'scan');
    renderApp(bridge);
    const trigger = await screen.findByRole('button', { name: 'AI options' });
    await userEvent.click(trigger);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAccessibleName('Your scan is ready. AI stays off.');
    expect(within(dialog).getByText('Preview: detected · Not available to Vacancy')).toBeInTheDocument();
    expect(within(dialog).getByText('Preview: not detected')).toBeInTheDocument();
    expect(within(dialog).getByText('Fixture preview. No host apps were searched or launched.')).toBeInTheDocument();
    expect(within(dialog).queryByText(/Installed/)).toBeNull();
    assertOff(dialog);
    await userEvent.click(within(dialog).getByRole('button', { name: 'Why does AI stay off?' }));
    expect(within(dialog).getByText(/These apps may include other files or settings/)).toBeInTheDocument();
    expect(within(dialog).getByText('Never the contents of your files.')).toBeInTheDocument();
    expect(document.querySelector('input')).toBeNull();
    await userEvent.click(within(dialog).getByRole('button', { name: 'Continue with built-in rules' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(trigger).toHaveFocus();
    expect(scan).not.toHaveBeenCalled();
  });

  it('reports limited passive discovery rather than assuming installed, absent, version or account state', async () => {
    renderApp(ready({ enabled: false, discovery: 'passive', providers: [
      { provider: 'claudeCode', availability: { state: 'unsupported', reason: 'No supported isolation' } },
      { provider: 'codex', availability: { state: 'notDetected' } },
    ] }));
    await screen.findByText('Detected AI apps can’t be used within Vacancy’s privacy limit.');
    const dialog = await openHeader();
    expect(within(dialog).getByText('Detected · Not available to Vacancy')).toBeInTheDocument();
    expect(within(dialog).getByText('Not detected in this check')).toBeInTheDocument();
    expect(within(dialog).getByText(/Custom locations may be missed; versions and sign-in weren’t checked/)).toBeInTheDocument();
    assertOff(dialog);
  });

  it('shows native-fixture notChecked without inventing a host search', async () => {
    renderApp(ready({ enabled: false, discovery: 'fixture', providers: [
      { provider: 'claudeCode', availability: { state: 'notChecked' } },
      { provider: 'codex', availability: { state: 'notChecked' } },
    ] }));
    await screen.findByText('Preview status only. No apps were checked on this computer.');
    const dialog = await openHeader();
    expect(within(dialog).getAllByText('Not checked in fixture mode')).toHaveLength(2);
    expect(within(dialog).queryByText(/detected/i)).toBeNull();
    assertOff(dialog);
  });

  it('keeps scan available while status loads, then updates the open dialog', async () => {
    let complete!: (status: AdvisorStatus) => void;
    const bridge = { ...createMockBridge(fixtures.windows), advisorStatus: () => new Promise<AdvisorStatus>((resolve) => { complete = resolve; }) };
    renderApp(bridge);
    expect(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
    const dialog = await openHeader();
    expect(within(dialog).getByRole('status')).toHaveTextContent('Checking app availability.');
    assertOff(dialog);
    await act(async () => { complete(fixtures.windows.advisorStatus); await Promise.resolve(); });
    expect(within(dialog).getByText('Preview: detected · Not available to Vacancy')).toBeInTheDocument();
  });

  it('contains focus and Escape, leaving the zoomed map unchanged and returning to its header trigger', async () => {
    const { container } = renderApp(createMockBridge(fixtures.windows, { scanMs: 0 }));
    await userEvent.click(await screen.findByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByLabelText('Map key');
    const category = [...container.querySelectorAll<HTMLElement>('[data-tile]')].find((tile) => tile.querySelector('.map-region-name')?.textContent === 'Apps & games')!;
    await userEvent.click(category);
    const location = within(screen.getByRole('navigation', { name: 'Map location' })).getByText('Apps & games');
    expect(location).toHaveAttribute('aria-current', 'location');
    const trigger = screen.getByRole('button', { name: 'Rules only' });
    const dialog = await openHeader();
    const close = within(dialog).getByRole('button', { name: 'Close AI options' });
    const next = within(dialog).getByRole('button', { name: 'Continue with built-in rules' });
    expect(close).toHaveFocus();
    expect(container.querySelector('.shell')).toHaveAttribute('inert');
    await userEvent.tab({ shift: true });
    expect(next).toHaveFocus();
    await userEvent.tab();
    expect(close).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(container.querySelector('.shell')).not.toHaveAttribute('inert');
    expect(location).toHaveAttribute('aria-current', 'location');
    expect(trigger).toHaveFocus();
  });

  it('handles advisor-only failure and retry without interrupting built-in scanning', async () => {
    const bridge = createMockBridge(fixtures.windows, { scanMs: 0 });
    const status = vi.spyOn(bridge, 'advisorStatus').mockRejectedValueOnce(new Error('private implementation detail')).mockResolvedValue(fixtures.windows.advisorStatus);
    renderApp(bridge);
    await screen.findByText('App availability couldn’t be checked. Built-in cleanups still work.');
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
    const dialog = await openHeader();
    expect(within(dialog).getByRole('alert')).toHaveTextContent('App availability couldn’t be checked.');
    expect(screen.queryByText(/private implementation detail/)).toBeNull();
    assertOff(dialog);
    await userEvent.click(within(dialog).getByRole('button', { name: 'Try again' }));
    await within(dialog).findByText('Preview: not detected');
    expect(status).toHaveBeenCalledTimes(2);
    await userEvent.keyboard('{Escape}');
    await userEvent.click(screen.getByRole('button', { name: 'Scan Local Disk (C:)' }));
    await screen.findByLabelText('Map key');
  });

  it('fails closed for an inconsistent enabled response', async () => {
    renderApp(ready({ ...fixtures.windows.advisorStatus, enabled: true }));
    await screen.findByText('App availability couldn’t be checked. Built-in cleanups still work.');
    assertOff(await openHeader());
  });

  it('ignores a stale response after the bridge changes', async () => {
    let complete!: (status: AdvisorStatus) => void;
    const first = { ...createMockBridge(fixtures.windows), advisorStatus: () => new Promise<AdvisorStatus>((resolve) => { complete = resolve; }) };
    const next = ready(fixtures.macos.advisorStatus);
    let bridge: Bridge = first;
    const { result, rerender } = renderHook(() => useAdvisorStatus(), { wrapper: ({ children }) => <BridgeProvider bridge={bridge}>{children}</BridgeProvider> });
    bridge = next;
    rerender();
    await waitFor(() => expect(result.current.state).toEqual({ phase: 'ready', status: fixtures.macos.advisorStatus }));
    await act(async () => { complete(fixtures.windows.advisorStatus); await Promise.resolve(); });
    expect(result.current.state).toEqual({ phase: 'ready', status: fixtures.macos.advisorStatus });
  });
});

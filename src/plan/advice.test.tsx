import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type RunReport } from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { planTotals } from './totals';

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(DOMRect.fromRect({ width: 1200, height: 700 }));
});

describe.each(['windows', 'macos'] as const)('%s advice reminders', (platform) => {
  const fixture = fixtures[platform];
  const hint = fixture.proposals.find((p) => p.action === 'hint')!;
  const mount = fixture.volumes[0]!.mountPoint;

  async function openPlan() {
    const bridge = createMockBridge(fixture, { scanMs: 0, runMs: 0, fullDiskAccess: 'granted', pastRuns: false, moves: true });
    const run = vi.spyOn(bridge, 'runPlan');
    render(<BridgeProvider bridge={bridge}><DrivesProvider><App /></DrivesProvider></BridgeProvider>);
    await userEvent.click(await screen.findByRole('button', { name: `Scan ${fixture.volumes[0]!.name}` }));
    await screen.findByLabelText('Map key');
    await userEvent.click(screen.getByRole('button', { name: 'Build a plan' }));
    return { bridge, run, tray: screen.getByRole('complementary', { name: 'Plan' }) };
  }

  it('saves an advice-only selection with zero deletion and reopens its explanation from history', async () => {
    const { bridge, run, tray } = await openPlan();
    const openHint = vi.spyOn(bridge, 'openHint');
    expect(within(tray).getByRole('checkbox', { name: `Remind me: ${hint.title}` })).toHaveAttribute('aria-checked', 'false');
    await userEvent.click(within(tray).getByRole('button', { name: hint.title }));
    if (platform === 'windows') {
      expect(openHint).not.toHaveBeenCalled();
      await userEvent.click(within(tray).getByRole('button', { name: 'Open Steam uninstall' }));
      expect(await within(tray).findByRole('alert')).toHaveTextContent('No app was launched.');
      expect(openHint).toHaveBeenCalledExactlyOnceWith(mount, hint.id);
    } else {
      expect(within(tray).queryByRole('button', { name: 'Open Steam uninstall' })).toBeNull();
    }
    for (const box of within(within(tray).getByRole('region', { name: 'Delete' })).getAllByRole('checkbox')) {
      if (box.getAttribute('aria-checked') === 'true') await userEvent.click(box);
    }
    expect(within(tray).getByRole('button', { name: 'Nothing selected yet' })).toBeDisabled();
    await userEvent.click(within(tray).getByRole('checkbox', { name: `Remind me: ${hint.title}` }));
    await userEvent.click(within(tray).getByRole('button', { name: 'Review reminders' }));
    const confirm = await screen.findByRole('alertdialog');
    expect(confirm).toHaveAccessibleName('Save 1 reminder?');
    expect(confirm).toHaveTextContent('Vacancy won’t touch these items or count them as freed space.');
    expect(confirm).not.toHaveTextContent('deleted for good');
    expect(run).not.toHaveBeenCalled();
    await userEvent.click(within(confirm).getByRole('button', { name: 'Save reminders' }));
    await screen.findByRole('heading', { name: 'Reminders saved' });
    const saved: RunReport = await run.mock.results[0]!.value as RunReport;
    expect(saved).toMatchObject({ freedBytes: 0, plannedBytes: 0, deletedItems: 0, movedItems: 0, steps: [], freedByCategory: [], freeChange: 0 });
    expect(saved.leftForYou).toEqual([expect.objectContaining({ id: hint.id, why: hint.why, bytes: 0, hintAction: hint.hintAction })]);
    expect(saved.usedAfter).toBe(saved.usedBefore);
    await userEvent.click(screen.getByRole('button', { name: 'See what changed' }));
    expect(await screen.findByText('1 reminder saved. No files were deleted or moved, and no disk space was freed.')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Skip to recap' }));
    expect(await screen.findByText(hint.why)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Past runs' }));
    await userEvent.click(await screen.findByRole('menuitem'));
    expect(await screen.findByText(hint.why)).toBeInTheDocument();
    expect((await bridge.openRun(saved.id)).leftForYou[0]!.hintAction).toEqual(hint.hintAction);
    if (platform === 'windows') {
      openHint.mockRejectedValueOnce(new Error('Scan this drive again before opening this hint.'));
      await userEvent.click(screen.getByRole('button', { name: 'Open Steam uninstall' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('Scan this drive again before opening this hint.');
    }
  });

  it('keeps advice out of mixed-plan totals and deletion counts', async () => {
    const { bridge, tray } = await openPlan();
    const chosen = fixture.proposals.filter((p) => p.defaultOn).map((p) => p.id);
    const before = planTotals(fixture.proposals, chosen);
    const total = tray.querySelector('.tray-total-num')!.textContent;
    await userEvent.click(within(tray).getByRole('checkbox', { name: `Remind me: ${hint.title}` }));
    expect(tray.querySelector('.tray-total-num')!.textContent).toBe(total);
    expect(planTotals(fixture.proposals, [...chosen, hint.id])).toEqual(before);
    await userEvent.click(within(tray).getByRole('button', { name: /^Free / }));
    expect(await screen.findByRole('alertdialog')).toHaveTextContent('1 reminder will be saved under Left for you.');
    const plan = await bridge.buildPlan(mount, [...chosen, hint.id]);
    const without = await bridge.buildPlan(mount, chosen);
    expect({ bytes: plan.bytes, items: plan.items, places: plan.places, steps: plan.steps }).toEqual({ bytes: without.bytes, items: without.items, places: without.places, steps: without.steps });
    expect(plan.leftForYou).toEqual([hint.id]);
  });

  it('keeps tool instructions default-off, ordered, fixture-safe and intact in the recap and history', async () => {
    const { bridge, run, tray } = await openPlan();
    const guides = fixture.proposals.filter(p => p.action === 'hint' && p.id.startsWith('hint-'));
    expect(guides).toHaveLength(4);
    const launch = vi.spyOn(bridge, 'openGuide');
    for (const guide of guides) {
      const box = within(tray).getByRole('checkbox', { name: `Remind me: ${guide.title}` });
      expect(box).toHaveAttribute('aria-checked', 'false');
      await userEvent.click(within(tray).getByRole('button', { name: guide.title }));
      const row = box.closest('.tray-row')! as HTMLElement;
      expect(row.querySelectorAll('ol > li')).toHaveLength(3);
      const reference = within(row).getByRole('button', { name: /↗/ });
      await userEvent.click(reference);
      expect(await within(row).findByRole('alert')).toHaveTextContent('No app was launched.');
      expect(launch).toHaveBeenCalledTimes(guides.indexOf(guide) + 1);
      await userEvent.click(box);
    }
    for (const box of within(within(tray).getByRole('region', { name: 'Delete' })).getAllByRole('checkbox')) {
      if (box.getAttribute('aria-checked') === 'true') await userEvent.click(box);
    }
    await userEvent.click(within(tray).getByRole('button', { name: 'Review reminders' }));
    const confirm = await screen.findByRole('alertdialog');
    expect(confirm).toHaveAccessibleName('Save 4 reminders?');
    await userEvent.click(within(confirm).getByRole('button', { name: 'Save reminders' }));
    await screen.findByRole('heading', { name: 'Reminders saved' });
    const saved: RunReport = await run.mock.results[0]!.value as RunReport;
    expect(saved).toMatchObject({ freedBytes: 0, plannedBytes: 0, deletedItems: 0, movedItems: 0, steps: [] });
    expect(saved.leftForYou.map(a => a.why)).toEqual(guides.map(g => g.why));
    await userEvent.click(screen.getByRole('button', { name: 'See what changed' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Skip to recap' }));
    for (const toggle of screen.getAllByText('Instructions and reference')) await userEvent.click(toggle);
    for (const guide of guides) {
      expect(screen.getByText(guide.why.split('\n')[1]!.replace(/^\d+\. /, ''))).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole('button', { name: 'Past runs' }));
    await userEvent.click(await screen.findByRole('menuitem'));
    expect((await bridge.openRun(saved.id)).leftForYou.map(a => a.why)).toEqual(guides.map(g => g.why));
  });
});

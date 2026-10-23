import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type Bridge, type MockOptions, type RunListing } from '../bridge';
import { DrivesProvider } from '../drives/DrivesProvider';
import { HistoryMenu } from '../history/HistoryMenu';
import { ConfirmDialog } from '../run/ConfirmDialog';
import { SummaryPanel } from '../summary/Summary';
import { plan, report, stepReport } from '../test/run';

const panel = () => screen.getByRole('complementary', { name: 'Claude' });
const tray = () => screen.getByRole('complementary', { name: 'Plan' });
const header = () => screen.getByRole('banner');

function renderApp(bridge: Bridge) {
  return render(
    <BridgeProvider bridge={bridge}>
      <DrivesProvider>
        <App />
      </DrivesProvider>
    </BridgeProvider>,
  );
}

const windows = (options: MockOptions = {}) =>
  createMockBridge(fixtures.windows, { scanMs: 0, dupsMs: 0, claudeMs: 0, pastRuns: false, ...options });

async function toMap(bridge: Bridge) {
  const view = renderApp(bridge);
  await userEvent.click(await screen.findByRole('button', { name: /^Scan Local Disk|^Scan Macintosh HD/ }));
  await screen.findByLabelText('Map key');
  // The duplicate search ends at once here; wait for it so its answer doesn't land mid-test.
  await waitFor(() => expect(screen.queryByRole('progressbar', { name: 'Finding duplicates' })).toBeNull());
  return view;
}

async function openPlan() {
  await userEvent.click(within(header()).getByRole('button', { name: 'Build a plan' }));
  return tray();
}

/** Presses a job chip and waits for its answer in the log. */
async function job(name: string, kind: string) {
  const before = panel().querySelectorAll(`.cp-entry[data-job="${kind}"] .cp-msg`).length;
  await userEvent.click(within(panel()).getByRole('button', { name, exact: true } as never));
  await waitFor(() => expect(panel().querySelectorAll(`.cp-entry[data-job="${kind}"] .cp-msg`).length).toBe(before + 1));
  return [...panel().querySelectorAll<HTMLElement>(`.cp-entry[data-job="${kind}"]`)].at(-1)!;
}

const checked = (name: string) => within(tray()).getByRole('checkbox', { name: `Include ${name}` }).getAttribute('aria-checked') === 'true';

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    DOMRect.fromRect({ x: 0, y: 0, width: 1200, height: 700 }),
  );
});

describe('the Claude panel’s states', () => {
  it('is off until turned on: the turn-on step, the privacy notice and what always stays hidden', async () => {
    const bridge = windows();
    const enable = vi.spyOn(bridge, 'claudeSetEnabled');
    await toMap(bridge);

    expect(within(panel()).getByRole('heading', { name: 'Turn on Claude?' })).toBeInTheDocument();
    expect(within(panel()).getByText('The contents of any file.')).toBeInTheDocument();
    expect(within(header()).getByText('Rules only')).toBeInTheDocument();
    expect(within(panel()).queryByText('Password managers and wallets')).toBeNull();
    await userEvent.click(within(panel()).getByRole('button', { name: 'What always stays hidden' }));
    expect(within(panel()).getByText('Password managers and wallets')).toBeInTheDocument();
    expect(within(panel()).queryByRole('button', { name: 'Review my plan' })).toBeNull();
    expect(enable).not.toHaveBeenCalled();
  });

  it('checks that Claude Code answers while turning on, then is on with privacy mode on', async () => {
    await toMap(windows({ claudeMs: 300 }));
    await userEvent.click(within(panel()).getByRole('button', { name: 'Turn on Claude' }));

    expect(within(panel()).getByText('Checking that Claude Code answers…')).toBeInTheDocument();
    expect(await within(panel()).findByRole('button', { name: 'Turn off Claude' })).toBeInTheDocument();
    expect(within(panel()).getByRole('switch', { name: 'Privacy mode' })).toHaveAttribute('aria-checked', 'true');
    expect(within(panel()).getByText('On · through your Claude Code')).toBeInTheDocument();
    expect(within(header()).getByText('Claude on')).toBeInTheDocument();
    for (const chip of ['Review my plan', 'Let Claude pick', 'Explain big folders', 'Suggest Tidy up layouts'])
      expect(within(panel()).getByRole('button', { name: chip })).toBeInTheDocument();
  });

  it('says when Claude Code isn’t on this computer, or isn’t signed in, and can check again', async () => {
    const bridge = windows({ claude: 'notfound' });
    const view = await toMap(bridge);
    expect(within(panel()).getByRole('heading', { name: 'Claude Code isn’t on this PC.' })).toBeInTheDocument();
    const status = vi.spyOn(bridge, 'claudeStatus');
    await userEvent.click(within(panel()).getByRole('button', { name: 'Check again' }));
    expect(status).toHaveBeenCalled();
    view.unmount();

    await toMap(windows({ claude: 'signedout' }));
    expect(within(panel()).getByRole('heading', { name: 'Sign in to Claude Code first.' })).toBeInTheDocument();
    expect(within(panel()).getByText('Found · not signed in')).toBeInTheDocument();
  });

  it('works with a Cancel, says when it’s slow, and Cancel ends it with nothing changed', async () => {
    const bridge = windows({ claude: 'slow' });
    const cancel = vi.spyOn(bridge, 'claudeCancel');
    await toMap(bridge);
    await openPlan();
    await userEvent.click(within(panel()).getByRole('button', { name: 'Review my plan' }));

    expect(within(panel()).getByText('Claude is reviewing your plan…')).toBeInTheDocument();
    expect(await within(panel()).findByText('Still working.', {}, { timeout: 4000 })).toBeInTheDocument();
    expect(within(panel()).getByText('On · replies are slow')).toBeInTheDocument();
    expect(within(header()).getByText('Claude is working')).toBeInTheDocument();
    await userEvent.click(within(panel()).getByRole('button', { name: 'Cancel' }));
    expect(within(panel()).getByText('Cancelled. Nothing changed.')).toBeInTheDocument();
    await waitFor(() => expect(cancel).toHaveBeenCalled());
  });

  it('shows Claude’s own words when it fails, and says when it timed out', async () => {
    const view = await toMap(windows({ claude: 'error' }));
    await openPlan();
    await userEvent.click(within(panel()).getByRole('button', { name: 'Review my plan' }));
    expect(await within(panel()).findByRole('alert')).toHaveTextContent('Claude Code stopped with an error. Nothing changed.');
    expect(within(panel()).getByRole('alert')).toHaveTextContent('rate_limit_error');
    expect(within(panel()).getByText('On · the last request failed')).toBeInTheDocument();
    expect(within(panel()).getByRole('button', { name: 'Try again' })).toBeInTheDocument();
    view.unmount();

    await toMap(windows({ claude: 'timeout' }));
    await userEvent.click(within(panel()).getByRole('button', { name: 'Explain big folders' }));
    expect(await within(panel()).findByText(/took too long and was stopped/, {}, { timeout: 4000 })).toBeInTheDocument();
  });

  it('turned off again, keeps Claude’s items in the plan and forgets the conversation', async () => {
    await toMap(windows({ claude: 'on' }));
    await job('Explain big folders', 'explainFolders');
    await openPlan();
    expect(within(tray()).getByText('Android emulator images')).toBeInTheDocument();

    await userEvent.click(within(panel()).getByRole('button', { name: 'Turn off Claude' }));
    expect(await within(panel()).findByRole('heading', { name: 'Turn on Claude?' })).toBeInTheDocument();
    expect(within(tray()).getByText('Android emulator images')).toBeInTheDocument();
    expect(within(tray()).queryByRole('button', { name: 'Ask Claude' })).toBeNull();
    expect(document.querySelector('.map-tile.is-explained')).toBeNull();
  });

  it('folds to a rail, and the header’s control unfolds it', async () => {
    await toMap(windows({ claude: 'on' }));
    await userEvent.click(within(panel()).getByRole('button', { name: 'Fold the Claude panel' }));
    expect(within(panel()).getByRole('button', { name: 'Show Claude' })).toBeInTheDocument();
    await userEvent.click(within(header()).getByRole('button', { name: /Claude on: show the Claude panel/ }));
    expect(within(panel()).getByRole('button', { name: 'Turn off Claude' })).toBeInTheDocument();
  });
});

describe('privacy mode', () => {
  it('asks before it goes off, lists what always stays hidden, and is remembered by the core', async () => {
    const bridge = windows({ claude: 'on' });
    const set = vi.spyOn(bridge, 'claudeSetPrivacy');
    await toMap(bridge);
    const toggle = within(panel()).getByRole('switch', { name: 'Privacy mode' });

    await userEvent.click(toggle);
    const ask = screen.getByRole('alertdialog', { name: 'Let Claude read your files?' });
    expect(within(ask).getByText('Keys and sign-ins')).toBeInTheDocument();
    expect(within(ask).getByText('Browser data')).toBeInTheDocument();
    expect(within(ask).getByRole('button', { name: 'Keep privacy mode on' })).toHaveFocus();
    await userEvent.click(within(ask).getByRole('button', { name: 'Keep privacy mode on' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(set).not.toHaveBeenCalled();
    expect(toggle).toHaveAttribute('aria-checked', 'true');

    await userEvent.click(toggle);
    await userEvent.click(screen.getByRole('button', { name: 'Turn privacy mode off' }));
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'false'));
    expect(set).toHaveBeenCalledWith(false);
    expect(within(header()).getByText('Claude on · reads files')).toBeInTheDocument();

    // Back on needs no confirm.
    await userEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'));
    expect(set).toHaveBeenLastCalledWith(true);
  });

  it('every answer says what it was based on, and offers to ask again once the mode changed', async () => {
    await toMap(windows({ claude: 'on' }));
    await openPlan();
    const review = await job('Review my plan', 'reviewPlan');
    expect(within(review).getByText(/From names, sizes and dates only/)).toBeInTheDocument();

    await userEvent.click(within(panel()).getByRole('switch', { name: 'Privacy mode' }));
    await userEvent.click(screen.getByRole('button', { name: 'Turn privacy mode off' }));
    expect(await within(review).findByRole('button', { name: 'Ask again with privacy mode off' })).toBeInTheDocument();
    const again = await job('Review my plan', 'reviewPlan');
    expect(within(again).getByText(/Claude opened/)).toBeInTheDocument();
    // With privacy mode off the mock reads raytracer\target and flags it too.
    expect(within(again).getAllByText(/raytracer/).length).toBeGreaterThan(0);
  });
});

describe('the jobs', () => {
  it('Review my plan flags risky items with reasons, adds nothing, and Untick it takes one out', async () => {
    const bridge = windows({ claude: 'on' });
    const start = vi.spyOn(bridge, 'claudeStart');
    await toMap(bridge);
    await openPlan();
    const items = within(tray()).getAllByRole('checkbox').length;
    const review = await job('Review my plan', 'reviewPlan');

    expect(start.mock.calls[0]?.[1]).toMatchObject({ job: 'reviewPlan' });
    expect(within(review).getByText(/look like versions you might keep/)).toBeInTheDocument();
    expect(within(tray()).getAllByRole('checkbox')).toHaveLength(items);
    expect(checked('Old installers in Downloads')).toBe(true);
    await userEvent.click(within(review).getAllByRole('button', { name: 'Untick it' })[0]!);
    expect(checked('Old installers in Downloads')).toBe(false);
    expect(within(review).getByText('Out of the plan.')).toBeInTheDocument();
    await userEvent.click(within(review).getByRole('button', { name: 'Keep it' }));
    expect(within(review).getByText('You kept it.')).toBeInTheDocument();
  });

  it('Let Claude pick changes the plan at once, its own items unticked; Keep mine puts it back', async () => {
    const bridge = windows({ claude: 'on' });
    const remove = vi.spyOn(bridge, 'claudeRemoveItems');
    await toMap(bridge);
    await openPlan();
    const total = within(tray()).getByText(/will be freed/).previousSibling?.textContent;
    const pick = await job('Let Claude pick', 'pickPlan');

    // Straight into the plan: the installers are out, the game's reminder in, Claude's items listed but unticked.
    expect(checked('Old installers in Downloads')).toBe(false);
    expect(within(tray()).getByRole('checkbox', { name: 'Remind me: Cyberpunk 2077 (not played for 430 days)' })).toHaveAttribute('aria-checked', 'true');
    expect(checked('Android emulator images')).toBe(false);
    expect(checked('Installers in Public Downloads')).toBe(false);
    expect(within(pick).getByText('Unticked')).toBeInTheDocument();
    expect(within(pick).getAllByText('Added, unticked')).toHaveLength(2);
    expect(within(pick).getAllByText('Left out by Vacancy')).toHaveLength(2);
    expect(within(pick).getByText('Windows Update downloads')).toBeInTheDocument();
    expect(within(pick).getByText(/Your plan/)).toHaveTextContent('75.3 GB');

    await userEvent.click(within(pick).getByRole('button', { name: 'Keep mine' }));
    expect(await within(pick).findByText(/You kept your plan/)).toBeInTheDocument();
    expect(remove).toHaveBeenCalledWith('C:\\', ['claude-android', 'claude-public']);
    expect(checked('Old installers in Downloads')).toBe(true);
    expect(within(tray()).queryByText('Android emulator images')).toBeNull();
    expect(within(tray()).getByText(/will be freed/).previousSibling?.textContent).toBe(total);
  });

  it('Use Claude’s plan keeps its changes, and its items carry “Suggested by Claude” and its note', async () => {
    await toMap(windows({ claude: 'on' }));
    await openPlan();
    const pick = await job('Let Claude pick', 'pickPlan');
    await userEvent.click(within(pick).getByRole('button', { name: 'Use Claude’s plan' }));

    expect(within(pick).getByText(/You’re using my plan/)).toBeInTheDocument();
    expect(checked('Old installers in Downloads')).toBe(false);
    const row = within(tray()).getByText('Android emulator images').closest('.tray-row') as HTMLElement;
    expect(within(row).getByText('Suggested by Claude')).toBeInTheDocument();
    expect(within(row).getByText(/phone images Android Studio’s emulator boots/)).toBeInTheDocument();
    expect(within(row).getByText('Review')).toBeInTheDocument();
  });

  it('Ask Claude: a row’s chat gets a first answer, suggested follow-ups and the composer', async () => {
    const bridge = windows({ claude: 'on' });
    const chat = vi.spyOn(bridge, 'claudeChat');
    await toMap(bridge);
    await openPlan();
    const row = within(tray()).getByText('Rust build output in 12 projects').closest('.tray-row') as HTMLElement;
    await userEvent.click(within(row).getByRole('button', { name: 'Ask Claude' }));

    const entry = await waitFor(() => {
      const e = panel().querySelector<HTMLElement>('.cp-entry[data-chat="Rust build output in 12 projects"]');
      expect(e?.querySelector('.cl-first')).toBeTruthy();
      return e!;
    });
    expect(chat.mock.calls[0]?.[1]).toEqual({ kind: 'proposal', id: 'w-rust' });
    expect(within(entry).getByText(/Compiled output of 12 Rust projects/)).toBeInTheDocument();
    expect(within(panel()).getByRole('button', { name: 'Stop asking about it' })).toBeInTheDocument();

    const input = within(panel()).getByRole('textbox', { name: 'Ask about Rust build output in 12 projects' });
    await userEvent.type(input, 'Can I get it back from the Recycle Bin?{Enter}');
    expect(await within(entry).findByText(/skips the Recycle Bin and the Trash/)).toBeInTheDocument();
    const sent = chat.mock.calls.at(-1)?.[2];
    expect(sent?.at(-1)).toEqual({ from: 'person', text: 'Can I get it back from the Recycle Bin?' });
    expect(sent).toHaveLength(3);
    expect(within(entry).getByRole('button', { name: 'What would you do?' })).toBeInTheDocument();
  });

  it('Explain big folders adds its suggestions unticked, badges the folders, and a click on one asks about it', async () => {
    const view = await toMap(windows({ claude: 'on' }));
    const explain = await job('Explain big folders', 'explainFolders');

    expect(within(explain).getByText(/big folders the rules don’t recognize/)).toBeInTheDocument();
    expect(within(explain).getAllByText(/in the tray unticked/)).toHaveLength(2);
    expect(within(explain).getByText('Windows Update downloads')).toBeInTheDocument();
    const docker = [...view.container.querySelectorAll<HTMLElement>('.map-tile.is-explained')].find(
      (t) => t.querySelector('.map-name')?.textContent === 'Docker',
    );
    expect(docker).toBeTruthy();
    await userEvent.click(docker!);
    await waitFor(() => expect(panel().querySelector('.cp-entry[data-chat="Docker"] .cl-first')).toBeTruthy());

    await openPlan();
    expect(checked('Android emulator images')).toBe(false);
    expect(checked('Installers in Public Downloads')).toBe(false);
  });

  it('Suggest Tidy up layouts offers Claude’s layout beside the built-in one, picked through the core', async () => {
    const bridge = windows({ claude: 'on' });
    const choose = vi.spyOn(bridge, 'claudeChooseLayout');
    await toMap(bridge);
    await openPlan();
    const tidy = await job('Suggest Tidy up layouts', 'tidyLayout');
    const downloads = within(tidy).getByRole('radiogroup', { name: 'Layout for Downloads' });
    expect(within(downloads).getByRole('radio', { name: /Built-in/ })).toHaveAttribute('aria-checked', 'true');

    await userEvent.click(within(downloads).getByRole('radio', { name: /Claude’s/ }));
    await waitFor(() => expect(within(downloads).getByRole('radio', { name: /Claude’s/ })).toHaveAttribute('aria-checked', 'true'));
    expect(choose).toHaveBeenCalledWith('C:\\', 'tidy-downloads', 'claude');
    const row = within(tray()).getByText('Sort Downloads').closest('.tray-row') as HTMLElement;
    expect(row).toHaveTextContent('sorted into Claude’s folders.');
    expect(within(row).getByText('Suggested by Claude')).toBeInTheDocument();
    // Picking a layout changes no tick: Tidy up still starts out of the plan.
    expect(checked('Sort Downloads')).toBe(false);
  });

  it('has no Tidy up layouts on macOS', async () => {
    await toMap(createMockBridge(fixtures.macos, { scanMs: 0, dupsMs: 0, claudeMs: 0, claude: 'on' }));
    expect(within(panel()).getByRole('button', { name: 'Explain big folders' })).toBeInTheDocument();
    expect(within(panel()).queryByRole('button', { name: 'Suggest Tidy up layouts' })).toBeNull();
  });
});

describe('Claude’s marks after the plan', () => {
  it('the confirm dialog names Claude’s items, with its note, and a sort into its layout', () => {
    const withClaude = {
      ...plan,
      moves: 12,
      steps: [
        ...plan.steps,
        { id: 'claude-android', title: 'Android emulator images', category: 'dev' as const, bytes: 4.6e9, items: 3812, places: 1, kind: 'delete' as const, moves: 0, source: 'claude' as const },
        { id: 'tidy-downloads', title: 'Sort Downloads', category: 'downloads' as const, bytes: 0, items: 0, places: 0, kind: 'move' as const, moves: 12, source: 'claude' as const },
      ],
    };
    render(
      <ConfirmDialog
        plan={withClaude}
        platform="windows"
        notes={{ 'claude-android': 'Nothing has started them in 240 days.' }}
        onCancel={() => {}}
        onGo={() => {}}
      />,
    );
    const marks = screen.getByRole('group', { name: 'Suggested by Claude' });
    expect(marks).toHaveTextContent('1 item was suggested by Claude. It passed the Guard and you ticked it.');
    expect(marks).toHaveTextContent('Android emulator images (4.6 GB): Nothing has started them in 240 days.');
    expect(marks).toHaveTextContent('Downloads is sorted into Claude’s folders. Moves are journaled, so Undo still works.');
  });

  it('the confirm dialog says nothing about Claude when it suggested nothing', () => {
    render(<ConfirmDialog plan={plan} platform="windows" onCancel={() => {}} onGo={() => {}} />);
    expect(screen.queryByRole('group', { name: 'Suggested by Claude' })).toBeNull();
  });

  it('the summary lists what Claude suggested', () => {
    const r = {
      ...report,
      steps: [
        ...report.steps,
        stepReport({ id: 'claude-android', title: 'Android emulator images', category: 'dev', why: 'Nothing has started them in 240 days.', freedBytes: 4.6e9, source: 'claude' }),
      ],
    };
    render(<SummaryPanel report={r} os="Windows" onHot={() => {}} onReplay={() => {}} />);
    const sec = screen.getByRole('region', { name: 'Suggested by Claude' });
    expect(sec).toHaveTextContent('Android emulator images');
    expect(sec).toHaveTextContent('Nothing has started them in 240 days.');
    expect(sec).toHaveTextContent('4.6 GB');
  });

  it('Past runs marks a run with Claude’s steps', async () => {
    const bridge = createMockBridge(fixtures.windows, { pastRuns: false });
    const listing: RunListing = {
      id: 'run-claude', mountPoint: 'C:\\', volumeName: 'Local Disk (C:)', volumeId: 'vol-c', startedAt: Date.UTC(2026, 9, 20, 9),
      freedBytes: 4.6e9, deletedItems: 3812, movedItems: 0, cancelled: false, tidy: [], claudeSteps: 2,
    };
    vi.spyOn(bridge, 'listRuns').mockResolvedValue({ unreadable: 0, runs: [listing] });
    render(
      <BridgeProvider bridge={bridge}>
        <HistoryMenu locked={false} onOpen={() => {}} />
      </BridgeProvider>,
    );
    await userEvent.click(screen.getByRole('button', { name: 'Past runs' }));
    const row = await screen.findByRole('menuitem', { name: /2 steps suggested by Claude/ });
    expect(row).toHaveTextContent('Claude');
  });
});

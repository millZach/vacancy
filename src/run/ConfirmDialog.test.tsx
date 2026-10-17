import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { changed, plan } from '../test/run';
import { ConfirmDialog } from './ConfirmDialog';

const plain = (s: string | null | undefined) => (s ?? '').replace(/\u00a0/g, ' ');
const dialog = () => screen.getByRole('alertdialog');

describe('the confirm dialog', () => {
  it('asks once, with the numbers from the plan, and says it skips the Recycle Bin', () => {
    render(<ConfirmDialog plan={plan} platform="windows" onCancel={() => {}} onGo={() => {}} />);

    expect(dialog()).toHaveAccessibleName(/^Permanently delete 27\.3\sGB\?$/);
    expect(plain(dialog().textContent)).toContain(
      '460,428 items in 67 places will be deleted for good. They skip the Recycle Bin, so they can’t be brought back.',
    );
    expect(screen.getByRole('button', { name: 'Go back' })).toHaveFocus();
    expect(screen.getByRole('button', { name: /^Delete 27\.3\sGB permanently$/ })).toBeInTheDocument();
    expect(dialog()).not.toHaveTextContent('moved');
    expect(dialog()).not.toHaveTextContent('changed since the scan');
  });

  it('says Trash on a Mac, and one item in one place', () => {
    const one = { ...plan, bytes: 2.2e9, items: 1, places: 1 };
    render(<ConfirmDialog plan={one} platform="macos" onCancel={() => {}} onGo={() => {}} />);
    expect(plain(dialog().textContent)).toContain('1 item in 1 place will be deleted for good. They skip the Trash');
  });

  it('gives the undoable moves, and quietly the places that changed since the scan', () => {
    render(
      <ConfirmDialog
        plan={{ ...plan, moves: 1_204, refused: [changed, { ...changed, path: 'C:\\y' }] }}
        platform="windows"
        onCancel={() => {}}
        onGo={() => {}}
      />,
    );
    expect(dialog()).toHaveTextContent('1,204 files will be moved. Every move is recorded, so you can undo it from the summary.');
    expect(dialog()).toHaveTextContent('2 places changed since the scan and will be left alone.');
  });

  it('says so when everything changed and there is nothing left to delete', () => {
    const none = { ...plan, bytes: 0, items: 0, places: 0, steps: [], refused: [changed] };
    render(<ConfirmDialog plan={none} platform="windows" onCancel={() => {}} onGo={() => {}} />);
    expect(dialog()).toHaveAccessibleName('Nothing left to delete');
    expect(screen.queryByRole('button', { name: /permanently/ })).toBeNull();
  });

  it('goes back on Go back, Escape and the scrim; deletes only on its button', async () => {
    const onCancel = vi.fn();
    const onGo = vi.fn();
    render(<ConfirmDialog plan={plan} platform="windows" onCancel={onCancel} onGo={onGo} />);

    await userEvent.click(screen.getByRole('button', { name: 'Go back' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    await userEvent.keyboard('{Escape}');
    expect(onCancel).toHaveBeenCalledTimes(2);
    fireEvent.click(dialog().parentElement!);
    expect(onCancel).toHaveBeenCalledTimes(3);
    fireEvent.click(dialog());
    expect(onCancel).toHaveBeenCalledTimes(3);
    expect(onGo).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('button', { name: /permanently$/ }));
    expect(onGo).toHaveBeenCalledTimes(1);
  });

  it('keeps Tab inside', async () => {
    render(<ConfirmDialog plan={plan} platform="windows" onCancel={() => {}} onGo={() => {}} />);
    await userEvent.tab();
    expect(screen.getByRole('button', { name: /permanently$/ })).toHaveFocus();
    await userEvent.tab();
    expect(screen.getByRole('button', { name: 'Go back' })).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(screen.getByRole('button', { name: /permanently$/ })).toHaveFocus();
  });
});

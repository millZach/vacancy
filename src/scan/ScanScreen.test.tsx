import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from '../App';
import { BridgeProvider, createMockBridge, fixtures, type Bridge } from '../bridge';

function renderApp(bridge: Bridge) {
  return render(
    <BridgeProvider bridge={bridge}>
      <App />
    </BridgeProvider>,
  );
}

describe('the scan screen', () => {
  it('shows the system drive on the plate and in the header', async () => {
    renderApp(createMockBridge(fixtures.windows));

    expect(await screen.findByRole('heading', { name: 'Local Disk (C:)' })).toBeInTheDocument();
    expect(screen.getByText('476 GB on DESKTOP-ALEX, 431 GB used')).toBeInTheDocument();
    expect(screen.getByText('431 GB of 476 GB used')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Scan Local Disk (C:)' })).toBeEnabled();
  });

  it('says no AI agent was found, and that scanning only reads', async () => {
    renderApp(createMockBridge(fixtures.windows));

    expect(await screen.findByText('No AI agent found.')).toBeInTheDocument();
    expect(screen.getByText(/Vacancy will use its built-in rules only/)).toBeInTheDocument();
    expect(screen.getByText('Rules only')).toBeInTheDocument();
    expect(screen.getByText('Scanning only reads. Nothing changes until you approve a plan.')).toBeInTheDocument();
  });

  it('splashes when Scan is pressed, and changes nothing else yet', async () => {
    renderApp(createMockBridge(fixtures.windows));
    const scan = await screen.findByRole('button', { name: 'Scan Local Disk (C:)' });

    await userEvent.click(scan);

    expect(screen.getByRole('heading', { name: 'Local Disk (C:)' })).toBeInTheDocument();
    expect(scan).toBeEnabled();
  });

  it('shows a Mac drive, with the header ready for the traffic lights', async () => {
    const { container } = renderApp(createMockBridge(fixtures.macos));

    expect(await screen.findByRole('heading', { name: 'Macintosh HD' })).toBeInTheDocument();
    expect(screen.getByText('494 GB on Alex’s MacBook Air, 460 GB used')).toBeInTheDocument();
    expect(container.querySelector('.shell')).toHaveAttribute('data-chrome', 'overlay');
    expect(container.querySelector('.shell-head')).toHaveAttribute('data-tauri-drag-region', 'deep');
  });

  it('keeps the native title bar on Windows', async () => {
    const { container } = renderApp(createMockBridge(fixtures.windows));

    await screen.findByRole('heading', { name: 'Local Disk (C:)' });
    expect(container.querySelector('.shell')).toHaveAttribute('data-chrome', 'native');
    expect(container.querySelector('[data-tauri-drag-region]')).toBeNull();
  });

  it('says so when the drive cannot be read', async () => {
    renderApp(createMockBridge(fixtures.windows, { fail: 'Access is denied. (os error 5)' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Access is denied. (os error 5)');
    expect(screen.getByRole('heading', { name: 'Can’t read your system drive' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Scan/ })).toBeNull();
  });
});

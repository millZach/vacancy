import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { moveAdviceReport, plan } from '../test/run';
import { RunPanel } from './RunPanel';
import { settleRun, startRun } from './progress';

const movedPlan = { ...plan, bytes: 0, items: 0, places: 0, steps: [], moves: 4, leftForYou: ['w-game'] };
const callbacks = { onHot: () => {}, onStop: () => {}, onSeeChanges: () => {}, onBack: () => {}, onScanAgain: () => {} };

describe('separate move and reminder accounting', () => {
  it('shows actual moves from a zero-deletion report instead of saying no files moved', () => {
    render(<RunPanel run={{ phase: 'ended', plan: movedPlan, report: moveAdviceReport, view: settleRun(startRun(movedPlan), moveAdviceReport) }} hot={null} {...callbacks} />);
    expect(screen.getByRole('heading', { name: 'Done' })).toBeInTheDocument();
    expect(screen.getByText('4 files moved.')).toBeInTheDocument();
    expect(screen.queryByText('No files were deleted or moved.')).toBeNull();
  });

  it('does not promise unchanged files while planned moves are running', () => {
    render(<RunPanel run={{ phase: 'running', plan: movedPlan, view: startRun(movedPlan), stopping: false }} hot={null} {...callbacks} />);
    expect(screen.getByRole('heading', { name: 'Moving files' })).toBeInTheDocument();
    expect(screen.getByText('Moving 4 files. Every move is recorded.')).toBeInTheDocument();
    expect(screen.queryByText('No apps are opened and no files are changed.')).toBeNull();
  });
});

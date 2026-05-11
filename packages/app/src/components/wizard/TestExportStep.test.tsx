import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { TestExportStep } from './TestExportStep.js';

describe('TestExportStep', () => {
  const baseProps = {
    busy: false,
    result: null,
    onTest: vi.fn(),
    onSave: vi.fn(),
    onBack: vi.fn(),
  };

  it('renders the Test export button when no result yet', () => {
    render(<TestExportStep {...baseProps} />);
    const btn = screen.getByTestId('test-export-run');
    expect(btn.textContent).toBe('Test export');
  });

  it('shows the green banner and enables Save on ok', () => {
    render(<TestExportStep {...baseProps} result={{ status: 'ok', detail: 'all good' }} />);
    expect(screen.getByTestId('test-export-banner-ok')).toBeDefined();
    const save = screen.getByTestId('test-export-save') as HTMLButtonElement;
    expect(save.disabled).toBe(false);
    // No "Save anyway" link on green.
    expect(screen.queryByTestId('test-export-save-anyway')).toBeNull();
  });

  it('renders the synthetic-span hints inside the ok banner', () => {
    render(
      <TestExportStep
        {...baseProps}
        result={{ status: 'ok', detail: 'all good' }}
        backendKind="signoz"
      />,
    );
    expect(screen.getByTestId('synthetic-span-hints')).toBeDefined();
    expect(screen.getByTestId('hint-service-name').textContent).toBe('trove-test-export');
    expect(screen.getByTestId('synthetic-span-hints').textContent).toContain('SigNoz Cloud');
  });

  it('does not render synthetic-span hints when the test failed', () => {
    render(<TestExportStep {...baseProps} result={{ status: 'failed', detail: 'bad' }} />);
    expect(screen.queryByTestId('synthetic-span-hints')).toBeNull();
  });

  it('disables Save and shows Save anyway on failed', () => {
    render(
      <TestExportStep
        {...baseProps}
        result={{ status: 'failed', detail: 'Permanent error: 401 Unauthorized' }}
      />,
    );
    expect(screen.getByTestId('test-export-banner-failed')).toBeDefined();
    const save = screen.getByTestId('test-export-save') as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    expect(screen.getByTestId('test-export-save-anyway')).toBeDefined();
  });

  it('disables Save and shows Save anyway on timeout', () => {
    render(
      <TestExportStep
        {...baseProps}
        result={{ status: 'timeout', detail: 'no log line within 5s' }}
      />,
    );
    expect(screen.getByTestId('test-export-banner-timeout')).toBeDefined();
    expect(screen.getByTestId('test-export-save-anyway')).toBeDefined();
  });

  it('shows Test again label after a result has landed', () => {
    render(<TestExportStep {...baseProps} result={{ status: 'ok', detail: 'all good' }} />);
    expect(screen.getByTestId('test-export-run').textContent).toBe('Test again');
  });

  it('disables every button while busy', () => {
    render(<TestExportStep {...baseProps} busy={true} />);
    expect((screen.getByTestId('test-export-run') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('test-export-back') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('test-export-save') as HTMLButtonElement).disabled).toBe(true);
  });

  it('invokes onTest, onSave, and onBack from the right buttons', () => {
    const onTest = vi.fn();
    const onSave = vi.fn();
    const onBack = vi.fn();
    render(
      <TestExportStep
        busy={false}
        result={{ status: 'ok', detail: 'all good' }}
        onTest={onTest}
        onSave={onSave}
        onBack={onBack}
      />,
    );
    fireEvent.click(screen.getByTestId('test-export-run'));
    fireEvent.click(screen.getByTestId('test-export-save'));
    fireEvent.click(screen.getByTestId('test-export-back'));
    expect(onTest).toHaveBeenCalled();
    expect(onSave).toHaveBeenCalled();
    expect(onBack).toHaveBeenCalled();
  });
});

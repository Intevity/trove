import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AutoUpdate } from './AutoUpdate.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe('AutoUpdate', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders unchecked when enabled is false', () => {
    render(<AutoUpdate enabled={false} onToggle={vi.fn()} />);
    const toggle = screen.getByTestId('auto-update-toggle') as HTMLInputElement;
    expect(toggle.checked).toBe(false);
  });

  it('renders checked when enabled is true', () => {
    render(<AutoUpdate enabled={true} onToggle={vi.fn()} />);
    const toggle = screen.getByTestId('auto-update-toggle') as HTMLInputElement;
    expect(toggle.checked).toBe(true);
  });

  it('calls onToggle(true) when an off toggle is clicked', () => {
    const onToggle = vi.fn();
    render(<AutoUpdate enabled={false} onToggle={onToggle} />);
    fireEvent.click(screen.getByTestId('auto-update-toggle'));
    expect(onToggle).toHaveBeenCalledOnce();
    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it('calls onToggle(false) when an on toggle is clicked', () => {
    const onToggle = vi.fn();
    render(<AutoUpdate enabled={true} onToggle={onToggle} />);
    fireEvent.click(screen.getByTestId('auto-update-toggle'));
    expect(onToggle).toHaveBeenCalledOnce();
    expect(onToggle).toHaveBeenCalledWith(false);
  });

  it('check-now button reports update available with version', async () => {
    invokeMock.mockResolvedValueOnce({
      available: true,
      version: '0.6.1',
      current: '0.6.0',
    });
    render(<AutoUpdate enabled={false} onToggle={vi.fn()} />);
    fireEvent.click(screen.getByTestId('auto-update-check-now'));
    await waitFor(() => {
      expect(screen.getByTestId('auto-update-status').textContent).toContain('0.6.1');
    });
    expect(screen.getByTestId('auto-update-status').textContent).toContain('Update available');
    expect(invokeMock).toHaveBeenCalledWith('check_for_updates', undefined);
  });

  it('check-now button reports up-to-date when no update', async () => {
    invokeMock.mockResolvedValueOnce({
      available: false,
      version: null,
      current: '0.6.0',
    });
    render(<AutoUpdate enabled={false} onToggle={vi.fn()} />);
    fireEvent.click(screen.getByTestId('auto-update-check-now'));
    await waitFor(() => {
      expect(screen.getByTestId('auto-update-status').textContent).toContain('up to date');
    });
  });

  it('check-now button surfaces UpdaterCheckFailed reason verbatim', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'updater-check-failed',
      reason: 'network timeout',
    });
    render(<AutoUpdate enabled={false} onToggle={vi.fn()} />);
    fireEvent.click(screen.getByTestId('auto-update-check-now'));
    await waitFor(() => {
      expect(screen.getByTestId('auto-update-status').textContent).toContain('network timeout');
    });
    expect(screen.getByTestId('auto-update-status').textContent).toContain('Check failed');
  });

  it('falls back to a generic message for non-updater IpcError kinds', async () => {
    invokeMock.mockRejectedValueOnce({ kind: 'internal', reason: 'boom' });
    render(<AutoUpdate enabled={false} onToggle={vi.fn()} />);
    fireEvent.click(screen.getByTestId('auto-update-check-now'));
    await waitFor(() => {
      expect(screen.getByTestId('auto-update-status').textContent).toContain('Check failed');
    });
    // The branch should hit the kind-only fallback, not include 'reason' text.
    expect(screen.getByTestId('auto-update-status').textContent).toContain('internal');
  });

  it('disables the check-now button while a check is in flight', async () => {
    let resolve!: (value: unknown) => void;
    invokeMock.mockReturnValueOnce(
      new Promise((r) => {
        resolve = r;
      }),
    );
    render(<AutoUpdate enabled={false} onToggle={vi.fn()} />);
    fireEvent.click(screen.getByTestId('auto-update-check-now'));
    const btn = screen.getByTestId('auto-update-check-now') as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.textContent).toContain('Checking');
    resolve({ available: false, version: null, current: '0.6.0' });
    await waitFor(() => {
      expect(btn.disabled).toBe(false);
    });
  });
});

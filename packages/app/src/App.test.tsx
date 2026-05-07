import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { App } from './App.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe('App', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the Trove header', async () => {
    invokeMock.mockResolvedValue([]);
    render(<App />);
    const header = screen.getByTestId('app-header');
    expect(header.textContent).toBe('Trove');
    // Wait for the detection promise to settle so the test cleans up
    // without an act() warning.
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-empty')).toBeDefined();
    });
  });

  it('shows a loading state until detection completes', async () => {
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          setTimeout(() => resolve([]), 0);
        }),
    );
    render(<App />);
    expect(screen.getByTestId('harness-list-loading')).toBeDefined();
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-empty')).toBeDefined();
    });
  });

  it('renders detected harness rows once detection resolves', async () => {
    invokeMock.mockResolvedValue([
      {
        id: 'claude-code',
        detected: true,
        configPath: '/home/me/.claude/settings.json',
        telemetry: 'off',
        detectionMethod: 'config-dir',
        troveRegionPresent: false,
      },
    ]);
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('harness-row-claude-code')).toBeDefined();
    });
  });

  it('shows an error banner if detection fails', async () => {
    invokeMock.mockRejectedValue({ kind: 'internal', reason: 'boom' });
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-error')).toBeDefined();
    });
  });
});

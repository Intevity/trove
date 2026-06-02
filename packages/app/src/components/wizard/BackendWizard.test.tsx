import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { BackendWizard } from './BackendWizard.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe('BackendWizard', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('flows from preset → creds → test → save', async () => {
    const onComplete = vi.fn();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'add_backend') {
        return Promise.resolve({
          id: '11111111-1111-1111-1111-111111111111',
          backend: {
            kind: 'signoz',
            endpoint: 'ingest.us.signoz.cloud:443',
            ingestionKey: {
              service: 'trove',
              account: 'backend.signoz.ingestion-key.11111111-1111-1111-1111-111111111111',
            },
          },
        });
      }
      if (cmd === 'test_export') {
        return Promise.resolve({ status: 'ok', detail: 'all good' });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    render(<BackendWizard onComplete={onComplete} />);

    // Step 1: pick SigNoz.
    fireEvent.click(screen.getByTestId('preset-signoz'));

    // Step 2: enter the ingestion key (endpoint is pre-filled).
    fireEvent.change(screen.getByTestId('signoz-ingestion-key'), {
      target: { value: 'sk-ingest-test' },
    });
    fireEvent.click(screen.getByTestId('credentials-form-continue'));

    // Step 3: run the test, wait for ok banner.
    fireEvent.click(screen.getByTestId('test-export-run'));
    await waitFor(() => {
      expect(screen.getByTestId('test-export-banner-ok')).toBeDefined();
    });

    // Save → onComplete fires.
    fireEvent.click(screen.getByTestId('test-export-save'));
    expect(onComplete).toHaveBeenCalledTimes(1);

    // Verify the IPC contract: add_backend received the draft;
    // test_export was called with no args.
    const saveCall = invokeMock.mock.calls.find((c) => c[0] === 'add_backend');
    expect(saveCall?.[1]).toEqual({
      draft: {
        kind: 'signoz',
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: 'sk-ingest-test',
      },
      label: undefined,
    });
    expect(invokeMock.mock.calls.find((c) => c[0] === 'test_export')).toBeDefined();
  });

  it('surfaces a failed test result on red and gates Save', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'add_backend') {
        return Promise.resolve({
          id: '11111111-1111-1111-1111-111111111111',
          backend: {
            kind: 'signoz',
            endpoint: 'ingest.us.signoz.cloud:443',
            ingestionKey: {
              service: 'trove',
              account: 'backend.signoz.ingestion-key.11111111-1111-1111-1111-111111111111',
            },
          },
        });
      }
      if (cmd === 'test_export') {
        return Promise.resolve({
          status: 'failed',
          detail: 'collector log surfaced exporter failure: Permanent error',
        });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    render(<BackendWizard onComplete={vi.fn()} />);
    fireEvent.click(screen.getByTestId('preset-signoz'));
    fireEvent.change(screen.getByTestId('signoz-ingestion-key'), {
      target: { value: 'wrong-key' },
    });
    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    fireEvent.click(screen.getByTestId('test-export-run'));

    await waitFor(() => {
      expect(screen.getByTestId('test-export-banner-failed')).toBeDefined();
    });

    expect((screen.getByTestId('test-export-save') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId('test-export-save-anyway')).toBeDefined();
  });

  it('lets the user back out of the credentials step', async () => {
    render(<BackendWizard onComplete={vi.fn()} />);
    fireEvent.click(screen.getByTestId('preset-honeycomb'));
    expect(screen.getByTestId('credentials-form')).toBeDefined();
    fireEvent.click(screen.getByTestId('credentials-form-back'));
    // handleBackToPicker is async (awaits dropPendingInstance), so the
    // picker re-render lands on a microtask after the click returns.
    expect(await screen.findByTestId('preset-picker')).toBeDefined();
  });

  it('surfaces ipc errors raised by add_backend', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'add_backend') {
        return Promise.reject({ kind: 'internal', reason: 'keychain locked' });
      }
      return Promise.reject(new Error(`unexpected command ${cmd}`));
    });

    render(<BackendWizard onComplete={vi.fn()} />);
    fireEvent.click(screen.getByTestId('preset-signoz'));
    fireEvent.change(screen.getByTestId('signoz-ingestion-key'), {
      target: { value: 'sk-ingest-test' },
    });
    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    fireEvent.click(screen.getByTestId('test-export-run'));

    await waitFor(() => {
      expect(screen.getByTestId('backend-wizard-error')).toBeDefined();
    });
    expect(screen.getByTestId('backend-wizard-error').textContent).toMatch(/keychain locked/);
  });
});

import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { App } from './App.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Sprint 6 PR 3 added Tauri-event-driven hooks (collector status,
// metrics snapshot, log tail). The dashboard mounts them on every
// render, so the test environment needs a no-op `listen` so the
// hooks don't crash on import. The hooks ignore parse failures, so
// returning a never-firing listener is the simplest contract.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

/** Default state.json shape: schemaVersion 3, no backend yet. App
 *  swaps in the wizard for this case. The detection-related tests
 *  override `backend` to a SigNoz stub so the dashboard view renders. */
const FRESH_APP_STATE = { schemaVersion: 3, backend: null, harnesses: [] };

const SIGNOZ_STATE = {
  schemaVersion: 3,
  backend: {
    kind: 'signoz' as const,
    region: 'us-east',
    ingestionKey: { service: 'trove', account: 'backend.signoz.ingestion-key' },
  },
  harnesses: [],
};

/** Returns a mock implementation that maps Tauri command names to
 *  pre-baked responses. Unknown commands resolve to `undefined` so
 *  the corresponding Zod schema fails loudly. */
function dispatch(responses: Record<string, unknown | (() => Promise<unknown>)>): () => unknown {
  return (...args: unknown[]) => {
    const [cmd] = args as [string];
    const r = responses[cmd];
    if (typeof r === 'function') return (r as () => Promise<unknown>)();
    return Promise.resolve(r);
  };
}

describe('App', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the Trove header', async () => {
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: SIGNOZ_STATE,
        list_detected_harnesses: [],
      }),
    );
    render(<App />);
    const header = screen.getByTestId('app-header');
    expect(header.textContent).toBe('Trove');
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-empty')).toBeDefined();
    });
  });

  it('shows a loading state until detection completes', async () => {
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: SIGNOZ_STATE,
        list_detected_harnesses: () =>
          new Promise((resolve) => {
            setTimeout(() => resolve([]), 0);
          }),
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-loading')).toBeDefined();
    });
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-empty')).toBeDefined();
    });
  });

  it('renders detected harness rows once detection resolves', async () => {
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: SIGNOZ_STATE,
        list_detected_harnesses: [
          {
            id: 'claude-code',
            detected: true,
            configPath: '/home/me/.claude/settings.json',
            telemetry: 'off',
            detectionMethod: 'config-dir',
            troveRegionPresent: false,
            adapterAvailable: true,
          },
        ],
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('harness-row-claude-code')).toBeDefined();
    });
  });

  it('shows an error banner if detection fails', async () => {
    invokeMock.mockImplementation((...args: unknown[]) => {
      const [cmd] = args as [string];
      if (cmd === 'get_app_state') return Promise.resolve(SIGNOZ_STATE);
      return Promise.reject({ kind: 'internal', reason: 'boom' });
    });
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-error')).toBeDefined();
    });
  });

  it('renders the wizard when no backend is saved yet', async () => {
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: FRESH_APP_STATE,
        list_detected_harnesses: [],
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('backend-wizard')).toBeDefined();
    });
    expect(screen.getByTestId('preset-picker')).toBeDefined();
  });

  it('renders the BackendBanner with the saved backend label', async () => {
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: SIGNOZ_STATE,
        list_detected_harnesses: [],
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('backend-banner')).toBeDefined();
    });
    const banner = screen.getByTestId('backend-banner');
    expect(banner.textContent).toContain('SigNoz Cloud');
    expect(banner.textContent).toContain('us-east');
  });
});

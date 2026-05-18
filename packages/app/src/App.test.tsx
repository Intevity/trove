import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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

const DEFAULT_IDENTITY = {
  enabled: false,
  source: 'auto' as const,
  name: '',
  email: '',
};

const EMPTY_MAPPINGS = {
  schemaVersion: 2 as const,
  metrics: [],
  harnesses: [],
};

/** Default state.json shape: schemaVersion 8, no platforms yet. App
 *  swaps in the wizard for this case. The detection-related tests
 *  override `backends` to a one-element list so the dashboard view
 *  renders. */
const FRESH_APP_STATE = {
  schemaVersion: 8,
  backends: [],
  harnesses: [],
  autoUpdateEnabled: false,
  launchAtStartupEnabled: true,
  identity: DEFAULT_IDENTITY,
  mappings: EMPTY_MAPPINGS,
};

const SIGNOZ_STATE = {
  schemaVersion: 8,
  backends: [
    {
      id: '11111111-1111-1111-1111-111111111111',
      backend: {
        kind: 'signoz' as const,
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: { service: 'trove', account: 'backend.signoz.ingestion-key' },
      },
    },
  ],
  harnesses: [],
  autoUpdateEnabled: false,
  launchAtStartupEnabled: true,
  identity: DEFAULT_IDENTITY,
  mappings: EMPTY_MAPPINGS,
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
    await waitFor(() => {
      expect(screen.getByTestId('app-header').textContent).toBe('Trove');
    });
    // Switch to Harnesses tab — the empty state lives there now.
    await waitFor(() => {
      expect(screen.getByTestId('tab-harnesses')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('tab-harnesses'));
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-empty')).toBeDefined();
    });
  });

  it('shows a loading state until detection completes', async () => {
    let resolveHarnesses!: (value: unknown) => void;
    const pending = new Promise((resolve) => {
      resolveHarnesses = resolve;
    });
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: SIGNOZ_STATE,
        list_detected_harnesses: () => pending,
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('tab-harnesses')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('tab-harnesses'));
    await waitFor(() => {
      expect(screen.getByTestId('harness-list-loading')).toBeDefined();
    });
    resolveHarnesses([]);
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
      expect(screen.getByTestId('tab-harnesses')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('tab-harnesses'));
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
      expect(screen.getByTestId('tab-harnesses')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('tab-harnesses'));
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

  it('lists configured platforms on the Platforms tab', async () => {
    invokeMock.mockImplementation(
      dispatch({
        get_app_state: SIGNOZ_STATE,
        list_detected_harnesses: [],
      }),
    );
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('tab-platforms')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('tab-platforms'));
    await waitFor(() => {
      expect(screen.getByTestId('platforms-list')).toBeDefined();
    });
    const list = screen.getByTestId('platforms-list');
    expect(list.textContent).toContain('SigNoz Cloud');
    expect(list.textContent).toContain('ingest.us.signoz.cloud:443');
  });
});

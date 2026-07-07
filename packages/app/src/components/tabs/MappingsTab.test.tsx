import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AppState, MappingState } from '@trove/shared';

import { MappingsTab } from './MappingsTab.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const baseIdentity = { enabled: false, source: 'auto' as const, name: '', email: '' };

function appStateWithMappings(mappings: MappingState): AppState {
  return {
    schemaVersion: 12,
    backends: [],
    harnesses: [],
    autoUpdateEnabled: false,
    launchAtStartupEnabled: true,
    identity: baseIdentity,
    mappings,
    telemetryObserved: {},
  };
}

const builtinCatalog: MappingState['metrics'] = [
  {
    id: 'events',
    name: 'trove.harness.events',
    kind: 'counter',
    description: '',
    requiredAttributes: ['event.kind'],
    builtin: true,
  },
  {
    id: 'tokens',
    name: 'trove.harness.tokens',
    kind: 'counter',
    description: '',
    requiredAttributes: ['direction'],
    builtin: true,
  },
];

describe('MappingsTab', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the metrics catalog with builtin metrics', () => {
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    expect(screen.getByText('Metrics catalog')).toBeDefined();
    expect(screen.getByText('trove.harness.events')).toBeDefined();
    expect(screen.getByText('trove.harness.tokens')).toBeDefined();
  });

  it('renders one card per harness in the mapping state', () => {
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [
        {
          harnessId: 'claude-code',
          enabled: true,
          sources: [],
          costOverrides: {},
        },
        {
          harnessId: 'cursor-ide',
          enabled: true,
          sources: [],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    // Each harness id renders as a CardTitle in monospace.
    expect(screen.getByText('claude-code')).toBeDefined();
    expect(screen.getByText('cursor-ide')).toBeDefined();
  });

  it('renders the synthesis-row summary with native and target metric', () => {
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [
        {
          harnessId: 'codex-cli',
          enabled: true,
          sources: [
            {
              kind: 'synthesize-from-native',
              nativeMetric: 'codex.session.count',
              targetMetric: 'events',
              attributeMap: {},
              injectAttributes: {},
            },
          ],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    expect(screen.getByText('codex.session.count')).toBeDefined();
    // Catalog name renders for the target; multiple instances allowed.
    const occurrences = screen.getAllByText('trove.harness.events');
    expect(occurrences.length).toBeGreaterThan(0);
  });

  it('renders a hook-rule with null emit as "no emission"', () => {
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [
        {
          harnessId: 'cursor-ide',
          enabled: true,
          sources: [{ kind: 'hook-rule', when: 'beforeSubmitPrompt', emit: null }],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    expect(screen.getByText('no emission')).toBeDefined();
  });

  it('toggling enabled marks the harness card as Modified', () => {
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [
        {
          harnessId: 'aider',
          enabled: true,
          sources: [],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    expect(screen.queryByText('Modified')).toBeNull();
    // The enabled checkbox is the first checkbox inside the harness card.
    const checkbox = screen.getByRole('checkbox');
    fireEvent.click(checkbox);
    // Modified pill appears in the dirty state.
    expect(screen.getAllByText('Modified').length).toBeGreaterThan(0);
  });

  it('Apply sends the full draft to apply_mappings and refreshes', async () => {
    invokeMock.mockResolvedValue(null);
    const onRefresh = vi.fn();
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [
        {
          harnessId: 'aider',
          enabled: true,
          sources: [],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={onRefresh} />);
    // Make the harness dirty.
    fireEvent.click(screen.getByRole('checkbox'));
    // The per-harness Apply button is enabled once dirty.
    const applyButtons = screen.getAllByText('Apply');
    fireEvent.click(applyButtons[0]!);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalled();
    });
    const [command, payload] = invokeMock.mock.calls[0]!;
    expect(command).toBe('apply_mappings');
    expect(payload).toMatchObject({
      mappings: {
        schemaVersion: 2,
        harnesses: [expect.objectContaining({ harnessId: 'aider', enabled: false })],
      },
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('Reset to defaults calls reset_mappings_to_defaults', async () => {
    invokeMock.mockResolvedValue(null);
    const onRefresh = vi.fn();
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={onRefresh} />);
    fireEvent.click(screen.getByText('Reset to defaults'));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('reset_mappings_to_defaults', undefined);
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('surfaces an IPC error in an inline banner', async () => {
    invokeMock.mockRejectedValueOnce({ kind: 'internal', reason: 'bad mapping' });
    const state = appStateWithMappings({
      schemaVersion: 2,
      metrics: builtinCatalog,
      harnesses: [
        {
          harnessId: 'aider',
          enabled: true,
          sources: [],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    fireEvent.click(screen.getByRole('checkbox'));
    const applyButtons = screen.getAllByText('Apply');
    fireEvent.click(applyButtons[0]!);
    await waitFor(() => {
      expect(screen.getByText(/bad mapping/)).toBeDefined();
    });
  });
});

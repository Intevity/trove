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
    schemaVersion: 6,
    backend: null,
    harnesses: [],
    autoUpdateEnabled: false,
    identity: baseIdentity,
    mappings,
  };
}

describe('MappingsTab', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders one card per harness in the mapping state', () => {
    const state = appStateWithMappings({
      schemaVersion: 1,
      harnesses: [
        {
          harnessId: 'claude-code',
          enabled: true,
          sources: [
            {
              kind: 'synthesize-from-native',
              nativeMetric: 'claude_code.token.usage',
              targetMetric: 'tokens',
              attributeMap: { type: 'direction' },
            },
          ],
          costOverrides: {},
        },
        {
          harnessId: 'cursor-ide',
          enabled: true,
          sources: [
            {
              kind: 'hook-rule',
              when: 'afterAgentResponse',
              emit: {
                metric: 'events',
                attributes: { 'event.kind': 'chat.turn' },
              },
            },
          ],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    expect(screen.getByTestId('mapping-card-claude-code')).toBeDefined();
    expect(screen.getByTestId('mapping-card-cursor-ide')).toBeDefined();
  });

  it('renders synthesis row with native and Tier A metric names', () => {
    const state = appStateWithMappings({
      schemaVersion: 1,
      harnesses: [
        {
          harnessId: 'gemini-cli',
          enabled: true,
          sources: [
            {
              kind: 'synthesize-from-native',
              nativeMetric: 'gemini_cli.session.count',
              targetMetric: 'events',
              attributeMap: {},
            },
          ],
          costOverrides: {},
        },
      ],
    });

    render(<MappingsTab appState={state} onAppStateRefresh={vi.fn()} />);
    const card = screen.getByTestId('mapping-card-gemini-cli');
    // The synthesis details summary is rendered; the row itself lives
    // inside it. Confirm the native name and the Tier A name both
    // appear inside this card's DOM tree.
    expect(card.textContent).toContain('gemini_cli.session.count');
    expect(card.textContent).toContain('trove.harness.events');
  });

  it('renders hook-rule row with null emit as "no emission"', () => {
    const state = appStateWithMappings({
      schemaVersion: 1,
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
    const card = screen.getByTestId('mapping-card-cursor-ide');
    expect(card.textContent).toContain('no emission');
  });

  it('clicking the enable checkbox calls apply_mappings with the toggled value', async () => {
    invokeMock.mockResolvedValue(null);
    const onRefresh = vi.fn();
    const state = appStateWithMappings({
      schemaVersion: 1,
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
    fireEvent.click(screen.getByTestId('mapping-enabled-aider'));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalled();
    });
    const [command, payload] = invokeMock.mock.calls[0]!;
    expect(command).toBe('apply_mappings');
    expect(payload).toMatchObject({
      mappings: {
        schemaVersion: 1,
        harnesses: [expect.objectContaining({ harnessId: 'aider', enabled: false })],
      },
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('reset-all button calls reset_mappings_to_defaults', async () => {
    invokeMock.mockResolvedValue(null);
    const onRefresh = vi.fn();
    const state = appStateWithMappings({ schemaVersion: 1, harnesses: [] });

    render(<MappingsTab appState={state} onAppStateRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('mappings-reset-all'));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('reset_mappings_to_defaults', undefined);
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('surfaces an IPC error in the error banner', async () => {
    invokeMock.mockRejectedValueOnce({ kind: 'internal', reason: 'bad mapping' });
    const state = appStateWithMappings({
      schemaVersion: 1,
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
    fireEvent.click(screen.getByTestId('mapping-enabled-aider'));
    await waitFor(() => {
      expect(screen.getByTestId('mappings-error').textContent).toContain('bad mapping');
    });
  });
});

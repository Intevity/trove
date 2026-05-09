import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ApplyOptions, ConflictPayload, ConflictResolutionOutcome } from '@trove/shared';

import { ConflictResolver } from './ConflictResolver.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const SAMPLE_OPTIONS: ApplyOptions = { logUserPrompts: false, customAttributes: {} };

const SAMPLE_PAYLOAD: ConflictPayload = {
  configPath: '/home/me/.claude/settings.json',
  format: 'json',
  originalRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
  currentRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://attacker.example.com"}}',
  theirsRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
  fileBefore: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://attacker.example.com"}}',
  fileAfterIfTakingTheirs:
    '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"},"_trove":{}}',
};

const ORPHAN_PAYLOAD: ConflictPayload = {
  ...SAMPLE_PAYLOAD,
  originalRegionPayload: null,
};

const SAMPLE_PATCH = {
  managedBlockHash: 'a'.repeat(64),
  fileHashAtLastWrite: 'b'.repeat(64),
  format: 'json' as const,
  lastWrittenRegionPayload: SAMPLE_PAYLOAD.theirsRegionPayload,
};

describe('ConflictResolver', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders three panes when originalRegionPayload is present', () => {
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={SAMPLE_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByTestId('conflict-resolver').getAttribute('data-mode')).toBe('three-way');
    expect(screen.getByTestId('conflict-pane-original')).toBeDefined();
    expect(screen.getByTestId('conflict-pane-yours')).toBeDefined();
    expect(screen.getByTestId('conflict-pane-theirs')).toBeDefined();
    expect(screen.queryByTestId('conflict-orphan-notice')).toBeNull();
  });

  it('collapses to two panes plus an orphan notice when originalRegionPayload is null', () => {
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={ORPHAN_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByTestId('conflict-resolver').getAttribute('data-mode')).toBe('two-way');
    expect(screen.queryByTestId('conflict-pane-original')).toBeNull();
    expect(screen.getByTestId('conflict-orphan-notice')).toBeDefined();
    expect(screen.getByTestId('conflict-pane-yours')).toBeDefined();
    expect(screen.getByTestId('conflict-pane-theirs')).toBeDefined();
  });

  it('keep-mine dispatches the right action and reports outcome', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'marked-mine', patch: SAMPLE_PATCH });
    const onResolved = vi.fn<(o: ConflictResolutionOutcome) => void>();
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={SAMPLE_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={onResolved}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('conflict-keep-mine'));
    await waitFor(() => {
      expect(onResolved).toHaveBeenCalledWith({
        status: 'marked-mine',
        patch: SAMPLE_PATCH,
      });
    });
    expect(invokeMock).toHaveBeenCalledWith('resolve_conflict', {
      harnessId: 'claude-code',
      action: { kind: 'keep-mine' },
    });
  });

  it('take-theirs dispatches with options', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'applied', patch: SAMPLE_PATCH });
    const onResolved = vi.fn<(o: ConflictResolutionOutcome) => void>();
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={SAMPLE_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={onResolved}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('conflict-take-theirs'));
    await waitFor(() => {
      expect(onResolved).toHaveBeenCalled();
    });
    expect(invokeMock).toHaveBeenCalledWith('resolve_conflict', {
      harnessId: 'claude-code',
      action: { kind: 'take-theirs', options: SAMPLE_OPTIONS },
    });
  });

  it('merge-manually swaps to the sibling-paths banner with the returned paths', async () => {
    const siblingPaths = {
      original: '/home/me/.claude/settings.json.trove.original',
      theirs: '/home/me/.claude/settings.json.trove.theirs',
      host: '/home/me/.claude/settings.json',
    };
    invokeMock.mockResolvedValueOnce({ status: 'merge-deferred', siblingPaths });
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={SAMPLE_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={() => {}}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('conflict-merge-manually'));
    await waitFor(() => {
      expect(screen.getByTestId('conflict-resolver').getAttribute('data-state')).toBe(
        'merge-deferred',
      );
    });
    expect(screen.getByTestId('conflict-sibling-host').textContent).toBe(siblingPaths.host);
    expect(screen.getByTestId('conflict-sibling-theirs').textContent).toBe(siblingPaths.theirs);
    expect(screen.getByTestId('conflict-sibling-original').textContent).toBe(siblingPaths.original);
  });

  it('renders an action-error banner when resolveConflict throws TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'internal',
      reason: 'state.json missing prior record',
    });
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={SAMPLE_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={() => {}}
        onCancel={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId('conflict-keep-mine'));
    await waitFor(() => {
      expect(screen.getByTestId('conflict-action-error')).toBeDefined();
    });
  });

  it('cancel button fires onCancel without dispatching', () => {
    const onCancel = vi.fn();
    render(
      <ConflictResolver
        harnessId="claude-code"
        conflict={SAMPLE_PAYLOAD}
        options={SAMPLE_OPTIONS}
        onResolved={() => {}}
        onCancel={onCancel}
      />,
    );
    fireEvent.click(screen.getByTestId('conflict-cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

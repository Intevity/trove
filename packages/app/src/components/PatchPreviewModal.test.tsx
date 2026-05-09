import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { PatchPreviewModal } from './PatchPreviewModal.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const FRESH_PREVIEW = {
  configPath: '/home/me/.claude/settings.json',
  format: 'json',
  before: '{}',
  after: '{"_trove":{"managed_keys":["env.OTEL_EXPORTER_OTLP_ENDPOINT"]}}',
  status: 'fresh',
};

describe('PatchPreviewModal', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('shows a loading state while preview resolves', async () => {
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          setTimeout(() => resolve(FRESH_PREVIEW), 0);
        }),
    );
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={() => {}} />);
    expect(screen.getByTestId('patch-preview-loading')).toBeDefined();
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-fresh')).toBeDefined();
    });
  });

  it('renders a fresh preview with the diff', async () => {
    invokeMock.mockResolvedValueOnce(FRESH_PREVIEW);
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-fresh')).toBeDefined();
    });
  });

  it('shows the conflict banner when status is conflict and Apply is enabled to surface the resolver', async () => {
    invokeMock.mockResolvedValueOnce({
      ...FRESH_PREVIEW,
      status: 'conflict',
    });
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-conflict')).toBeDefined();
    });
    // Sprint 8: Apply is enabled in conflict state. Clicking Apply
    // routes through applyPatch which returns RegionConflictDetected,
    // which the modal turns into the ConflictResolver render path.
    const apply = screen.getByTestId('patch-preview-apply') as HTMLButtonElement;
    expect(apply.disabled).toBe(false);
  });

  it('swaps to ConflictResolver when applyPatch returns region-conflict-detected', async () => {
    const conflictPayload = {
      configPath: '/home/me/.claude/settings.json',
      format: 'json',
      originalRegionPayload: '{"a":1}',
      currentRegionPayload: '{"a":2}',
      theirsRegionPayload: '{"a":3}',
      fileBefore: '{"a":2}',
      fileAfterIfTakingTheirs: '{"a":3}',
    };
    invokeMock.mockResolvedValueOnce(FRESH_PREVIEW).mockRejectedValueOnce({
      kind: 'region-conflict-detected',
      conflict: conflictPayload,
    });
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-fresh')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('patch-preview-apply'));
    await waitFor(() => {
      expect(screen.getByTestId('conflict-resolver')).toBeDefined();
    });
    expect(screen.getByTestId('conflict-resolver').getAttribute('data-mode')).toBe('three-way');
  });

  it('shows the idempotent banner when status is idempotent', async () => {
    invokeMock.mockResolvedValueOnce({
      ...FRESH_PREVIEW,
      status: 'idempotent',
    });
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-idempotent')).toBeDefined();
    });
    const apply = screen.getByTestId('patch-preview-apply') as HTMLButtonElement;
    // Re-applying an idempotent state is allowed (button reads "Re-apply").
    expect(apply.disabled).toBe(false);
    expect(apply.textContent).toBe('Re-apply');
  });

  it('calls onApplied after a successful apply', async () => {
    invokeMock.mockResolvedValueOnce(FRESH_PREVIEW).mockResolvedValueOnce({
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json',
    });
    const onApplied = vi.fn();
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={onApplied} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-fresh')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('patch-preview-apply'));
    await waitFor(() => {
      expect(onApplied).toHaveBeenCalled();
    });
  });

  it('surfaces apply errors without closing the modal', async () => {
    invokeMock.mockResolvedValueOnce(FRESH_PREVIEW).mockRejectedValueOnce({
      kind: 'region-conflict',
      path: '/home/me/.claude/settings.json',
    });
    const onApplied = vi.fn();
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={onApplied} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-fresh')).toBeDefined();
    });
    fireEvent.click(screen.getByTestId('patch-preview-apply'));
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-error')).toBeDefined();
    });
    expect(onApplied).not.toHaveBeenCalled();
  });

  it('surfaces preview load errors', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'config-unparseable',
      path: '/home/me/.claude/settings.json',
      reason: 'expected `:`',
    });
    render(<PatchPreviewModal harnessId="claude-code" onClose={() => {}} onApplied={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-error')).toBeDefined();
    });
    // Apply is disabled on load error.
    const apply = screen.getByTestId('patch-preview-apply') as HTMLButtonElement;
    expect(apply.disabled).toBe(true);
  });

  it('calls onClose when the close button is clicked', async () => {
    invokeMock.mockResolvedValueOnce(FRESH_PREVIEW);
    const onClose = vi.fn();
    render(<PatchPreviewModal harnessId="claude-code" onClose={onClose} onApplied={() => {}} />);
    await waitFor(() => {
      expect(screen.getByTestId('patch-preview-fresh')).toBeDefined();
    });
    fireEvent.click(screen.getByLabelText('close-modal'));
    expect(onClose).toHaveBeenCalled();
  });
});

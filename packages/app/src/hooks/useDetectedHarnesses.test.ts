import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useDetectedHarnesses } from './useDetectedHarnesses.js';

const SAMPLE_ROW = {
  id: 'claude-code',
  detected: true,
  configPath: '/home/me/.claude/settings.json',
  telemetry: 'off',
  detectionMethod: 'config-dir',
  troveRegionPresent: false,
  adapterAvailable: true,
};

describe('useDetectedHarnesses', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue([SAMPLE_ROW]);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('runs an initial detection sweep on mount', async () => {
    const { result } = renderHook(() => useDetectedHarnesses({ refreshOnFocus: false }));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(result.current.harnesses).toHaveLength(1);
  });

  it('re-runs detection on window focus, coalesced within the window', async () => {
    vi.useFakeTimers();
    try {
      const { result } = renderHook(() =>
        useDetectedHarnesses({ refreshOnFocus: true, focusCoalesceMs: 2_000 }),
      );
      // Await initial sweep.
      await vi.waitFor(() => expect(result.current.loading).toBe(false));
      expect(invokeMock).toHaveBeenCalledTimes(1);

      // Two rapid focus events within the coalesce window — only one
      // additional sweep should run.
      vi.setSystemTime(new Date(Date.now() + 3_000));
      act(() => {
        window.dispatchEvent(new Event('focus'));
      });
      await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
      act(() => {
        window.dispatchEvent(new Event('focus'));
      });
      // No further IPC call within the coalesce window.
      await vi.advanceTimersByTimeAsync(500);
      expect(invokeMock).toHaveBeenCalledTimes(2);

      // After the coalesce window elapses, the next focus does fire.
      vi.setSystemTime(new Date(Date.now() + 3_000));
      act(() => {
        window.dispatchEvent(new Event('focus'));
      });
      await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(3));
    } finally {
      vi.useRealTimers();
    }
  });

  it('does not bind the focus listener when refreshOnFocus is false', async () => {
    const { result } = renderHook(() => useDetectedHarnesses({ refreshOnFocus: false }));
    await waitFor(() => expect(result.current.loading).toBe(false));
    invokeMock.mockClear();
    act(() => {
      window.dispatchEvent(new Event('focus'));
    });
    // No subsequent invoke even on focus.
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('runs a manual refresh regardless of coalescing', async () => {
    const { result } = renderHook(() =>
      useDetectedHarnesses({ refreshOnFocus: true, focusCoalesceMs: 60_000 }),
    );
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(invokeMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      await result.current.refresh();
    });
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { TroveIpcError, listDetectedHarnesses } from './ipc.js';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe('listDetectedHarnesses', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the parsed response on success', async () => {
    invokeMock.mockResolvedValueOnce([
      {
        id: 'claude-code',
        detected: true,
        configPath: '/home/me/.claude/settings.json',
        telemetry: 'off',
        detectionMethod: 'config-dir',
      },
    ]);
    const result = await listDetectedHarnesses();
    expect(result).toHaveLength(1);
    expect(result[0]?.id).toBe('claude-code');
    expect(invokeMock).toHaveBeenCalledWith('list_detected_harnesses', undefined);
  });

  it('throws TroveIpcError with structured cause on Rust rejection', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'region-conflict',
      path: '/home/me/.claude/settings.json',
    });
    try {
      await listDetectedHarnesses();
      throw new Error('expected rejection');
    } catch (err) {
      expect(err).toBeInstanceOf(TroveIpcError);
      const cause = (err as TroveIpcError).cause;
      expect(cause.kind).toBe('region-conflict');
      if (cause.kind === 'region-conflict') {
        expect(cause.path).toBe('/home/me/.claude/settings.json');
      }
    }
  });

  it('wraps non-IpcError rejections as internal error', async () => {
    invokeMock.mockRejectedValueOnce(new Error('process killed'));
    try {
      await listDetectedHarnesses();
      throw new Error('expected rejection');
    } catch (err) {
      expect(err).toBeInstanceOf(TroveIpcError);
      const cause = (err as TroveIpcError).cause;
      expect(cause.kind).toBe('internal');
      if (cause.kind === 'internal') {
        expect(cause.reason).toBe('process killed');
      }
    }
  });

  it('wraps a non-Error rejection as internal error', async () => {
    invokeMock.mockRejectedValueOnce('something weird');
    try {
      await listDetectedHarnesses();
      throw new Error('expected rejection');
    } catch (err) {
      expect(err).toBeInstanceOf(TroveIpcError);
      const cause = (err as TroveIpcError).cause;
      expect(cause.kind).toBe('internal');
      if (cause.kind === 'internal') {
        expect(cause.reason).toBe('something weird');
      }
    }
  });

  it('rejects when the response shape is wrong', async () => {
    invokeMock.mockResolvedValueOnce([{ id: 'claude-code' /* missing fields */ }]);
    await expect(listDetectedHarnesses()).rejects.toThrow();
  });
});

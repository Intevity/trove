import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  TroveIpcError,
  applyPatch,
  listDetectedHarnesses,
  previewPatch,
  revertPatch,
} from './ipc.js';

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
        troveRegionPresent: false,
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

describe('previewPatch', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('forwards args and parses the response', async () => {
    const expected = {
      configPath: '/home/me/.claude/settings.json',
      format: 'json' as const,
      before: '',
      after: '{"_trove":{}}',
      status: 'fresh' as const,
    };
    invokeMock.mockResolvedValueOnce(expected);
    const result = await previewPatch('claude-code', {
      logUserPrompts: false,
      customAttributes: {},
    });
    expect(result).toEqual(expected);
    expect(invokeMock).toHaveBeenCalledWith('preview_patch', {
      harnessId: 'claude-code',
      options: { logUserPrompts: false, customAttributes: {} },
    });
  });

  it('rethrows region-conflict as TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'region-conflict',
      path: '/home/me/.claude/settings.json',
    });
    await expect(
      previewPatch('claude-code', { logUserPrompts: false, customAttributes: {} }),
    ).rejects.toBeInstanceOf(TroveIpcError);
  });
});

describe('applyPatch', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('parses the TrovePatch response', async () => {
    const expected = {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json' as const,
    };
    invokeMock.mockResolvedValueOnce(expected);
    const result = await applyPatch('claude-code', {
      logUserPrompts: false,
      customAttributes: {},
    });
    expect(result).toEqual(expected);
  });
});

describe('revertPatch', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('resolves when Rust returns null', async () => {
    invokeMock.mockResolvedValueOnce(null);
    await expect(revertPatch('claude-code')).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith('revert_patch', {
      harnessId: 'claude-code',
    });
  });

  it('rejects when Rust returns a non-null payload', async () => {
    invokeMock.mockResolvedValueOnce({});
    await expect(revertPatch('claude-code')).rejects.toThrow();
  });
});

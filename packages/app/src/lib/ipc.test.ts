import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  TroveIpcError,
  addBackend,
  applyPatch,
  checkForUpdates,
  clearBackend,
  getAppState,
  listDetectedHarnesses,
  previewPatch,
  removeBackend,
  resolveConflict,
  revertPatch,
  setAutoUpdateEnabled,
  testExport,
  updateBackend,
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
        adapterAvailable: true,
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
      customAttributes: {},
    });
    expect(result).toEqual(expected);
    expect(invokeMock).toHaveBeenCalledWith('preview_patch', {
      harnessId: 'claude-code',
      options: { customAttributes: {} },
    });
  });

  it('rethrows region-conflict as TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'region-conflict',
      path: '/home/me/.claude/settings.json',
    });
    await expect(previewPatch('claude-code', { customAttributes: {} })).rejects.toBeInstanceOf(
      TroveIpcError,
    );
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
      lastWrittenRegionPayload: '{"env":{"OTEL_FOO":"bar"}}',
    };
    invokeMock.mockResolvedValueOnce(expected);
    const result = await applyPatch('claude-code', {
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

describe('getAppState', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('returns the parsed AppState on success', async () => {
    const expected = {
      schemaVersion: 10 as const,
      backends: [],
      harnesses: [],
      autoUpdateEnabled: false,
      launchAtStartupEnabled: true,
      identity: { enabled: false, source: 'auto' as const, name: '', email: '' },
      mappings: {
        schemaVersion: 2 as const,
        metrics: [],
        harnesses: [],
      },
      telemetryObserved: {},
    };
    invokeMock.mockResolvedValueOnce(expected);
    const result = await getAppState();
    expect(result).toEqual(expected);
    expect(invokeMock).toHaveBeenCalledWith('get_app_state', undefined);
  });

  it('rejects when the response shape is wrong', async () => {
    invokeMock.mockResolvedValueOnce({ schemaVersion: 1, backend: null, harnesses: [] });
    await expect(getAppState()).rejects.toThrow();
  });
});

describe('addBackend', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('forwards the draft and parses the persisted BackendInstance', async () => {
    const draft = {
      kind: 'signoz' as const,
      endpoint: 'ingest.us.signoz.cloud:443',
      ingestionKey: 'raw-secret-DO-NOT-PERSIST',
    };
    const persisted = {
      id: '11111111-1111-1111-1111-111111111111',
      enabled: true,
      backend: {
        kind: 'signoz' as const,
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: {
          service: 'trove',
          account: 'backend.signoz.ingestion-key.11111111-1111-1111-1111-111111111111',
        },
      },
    };
    invokeMock.mockResolvedValueOnce(persisted);
    const result = await addBackend(draft);
    expect(result).toEqual(persisted);
    expect(invokeMock).toHaveBeenCalledWith('add_backend', { draft, label: undefined });
  });

  it('rethrows internal errors as TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({ kind: 'internal', reason: 'keychain locked' });
    await expect(
      addBackend({ kind: 'datadog', site: 'datadoghq.eu', apiKey: 'k' }),
    ).rejects.toBeInstanceOf(TroveIpcError);
  });
});

describe('updateBackend', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('forwards the id + draft and parses the persisted instance', async () => {
    const id = '22222222-2222-2222-2222-222222222222';
    const draft = {
      kind: 'datadog' as const,
      site: 'datadoghq.eu',
      apiKey: 'fresh-key',
    };
    const persisted = {
      id,
      enabled: true,
      backend: {
        kind: 'datadog' as const,
        site: 'datadoghq.eu',
        apiKey: {
          service: 'trove',
          account: `backend.datadog.api-key.${id}`,
        },
      },
    };
    invokeMock.mockResolvedValueOnce(persisted);
    const result = await updateBackend(id, draft);
    expect(result).toEqual(persisted);
    expect(invokeMock).toHaveBeenCalledWith('update_backend', {
      id,
      draft,
      label: undefined,
    });
  });
});

describe('removeBackend', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('resolves on null and forwards the id', async () => {
    invokeMock.mockResolvedValueOnce(null);
    await expect(removeBackend('abc')).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith('remove_backend', { id: 'abc' });
  });
});

describe('clearBackend', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('resolves when Rust returns null', async () => {
    invokeMock.mockResolvedValueOnce(null);
    await expect(clearBackend()).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith('clear_backend', undefined);
  });

  it('rejects when Rust returns a non-null payload', async () => {
    invokeMock.mockResolvedValueOnce({});
    await expect(clearBackend()).rejects.toThrow();
  });
});

describe('testExport', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('parses a green ok result', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'ok', detail: '1 trace exported' });
    const result = await testExport();
    expect(result.status).toBe('ok');
    expect(invokeMock).toHaveBeenCalledWith('test_export', {});
  });

  it('parses a failed result with detail', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'failed', detail: 'Permanent error: 401' });
    const result = await testExport();
    expect(result.status).toBe('failed');
    expect(result.detail).toContain('401');
  });

  it('parses a timeout result', async () => {
    invokeMock.mockResolvedValueOnce({
      status: 'timeout',
      detail: 'no exporter log line within 5s',
    });
    const result = await testExport();
    expect(result.status).toBe('timeout');
  });

  it('rejects an unknown status', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'maybe', detail: '' });
    await expect(testExport()).rejects.toThrow();
  });
});

describe('resolveConflict', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  const sampleApplyOptions = { customAttributes: {} };
  const samplePatch = {
    managedBlockHash: 'a'.repeat(64),
    fileHashAtLastWrite: 'b'.repeat(64),
    format: 'json' as const,
    lastWrittenRegionPayload: '{"a":1}',
  };

  it('forwards a keep-mine action and parses the marked-mine outcome', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'marked-mine', patch: samplePatch });
    const result = await resolveConflict('claude-code', { kind: 'keep-mine' });
    expect(result).toEqual({ status: 'marked-mine', patch: samplePatch });
    expect(invokeMock).toHaveBeenCalledWith('resolve_conflict', {
      harnessId: 'claude-code',
      action: { kind: 'keep-mine' },
    });
  });

  it('forwards a take-theirs action carrying ApplyOptions', async () => {
    invokeMock.mockResolvedValueOnce({ status: 'applied', patch: samplePatch });
    const result = await resolveConflict('claude-code', {
      kind: 'take-theirs',
      options: sampleApplyOptions,
    });
    expect(result).toEqual({ status: 'applied', patch: samplePatch });
    expect(invokeMock).toHaveBeenCalledWith('resolve_conflict', {
      harnessId: 'claude-code',
      action: { kind: 'take-theirs', options: sampleApplyOptions },
    });
  });

  it('forwards a merge-manually action and parses sibling paths', async () => {
    const siblingPaths = {
      original: '/tmp/x.trove.original',
      theirs: '/tmp/x.trove.theirs',
      host: '/tmp/x',
    };
    invokeMock.mockResolvedValueOnce({ status: 'merge-deferred', siblingPaths });
    const result = await resolveConflict('claude-code', {
      kind: 'merge-manually',
      options: sampleApplyOptions,
    });
    expect(result).toEqual({ status: 'merge-deferred', siblingPaths });
  });

  it('rethrows Rust IpcError as TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'internal',
      reason: 'state.json missing prior record',
    });
    await expect(resolveConflict('claude-code', { kind: 'keep-mine' })).rejects.toBeInstanceOf(
      TroveIpcError,
    );
  });
});

describe('setAutoUpdateEnabled', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('forwards the boolean and resolves on success', async () => {
    invokeMock.mockResolvedValueOnce(null);
    await expect(setAutoUpdateEnabled(true)).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith('set_auto_update_enabled', { enabled: true });
  });

  it('forwards false and resolves', async () => {
    invokeMock.mockResolvedValueOnce(null);
    await setAutoUpdateEnabled(false);
    expect(invokeMock).toHaveBeenCalledWith('set_auto_update_enabled', { enabled: false });
  });

  it('rethrows Rust IpcError as TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({ kind: 'internal', reason: 'disk full' });
    await expect(setAutoUpdateEnabled(true)).rejects.toBeInstanceOf(TroveIpcError);
  });
});

describe('checkForUpdates', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('returns the parsed UpdateMetadata on success', async () => {
    const meta = { available: true, version: '0.6.1', current: '0.6.0' };
    invokeMock.mockResolvedValueOnce(meta);
    const result = await checkForUpdates();
    expect(result).toEqual(meta);
    expect(invokeMock).toHaveBeenCalledWith('check_for_updates', undefined);
  });

  it('parses an "up to date" response with null version', async () => {
    const meta = { available: false, version: null, current: '0.6.0' };
    invokeMock.mockResolvedValueOnce(meta);
    expect(await checkForUpdates()).toEqual(meta);
  });

  it('rethrows updater-check-failed as TroveIpcError', async () => {
    invokeMock.mockRejectedValueOnce({
      kind: 'updater-check-failed',
      reason: 'sig mismatch',
    });
    await expect(checkForUpdates()).rejects.toMatchObject({
      cause: { kind: 'updater-check-failed', reason: 'sig mismatch' },
    });
  });
});

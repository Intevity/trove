import { describe, expect, it } from 'vitest';

import {
  ApplyPatchResponse,
  GetCollectorLogTailResponse,
  GetCollectorStatusResponse,
  GetMetricsSnapshotResponse,
  IpcCommandName,
  ListDetectedHarnessesResponse,
  PreviewPatchResponse,
  RevertPatchResponse,
  TauriEventName,
} from './ipc-messages.js';

describe('IpcCommandName', () => {
  it('exposes the list_detected_harnesses Tauri command name', () => {
    expect(IpcCommandName.ListDetectedHarnesses).toBe('list_detected_harnesses');
  });

  it('exposes the patch trio command names', () => {
    expect(IpcCommandName.PreviewPatch).toBe('preview_patch');
    expect(IpcCommandName.ApplyPatch).toBe('apply_patch');
    expect(IpcCommandName.RevertPatch).toBe('revert_patch');
  });
});

describe('PreviewPatchResponse', () => {
  it('parses a fresh preview response', () => {
    const res = {
      configPath: '/home/me/.claude/settings.json',
      format: 'json',
      before: '',
      after: '{"_trove":{}}',
      status: 'fresh',
    };
    expect(PreviewPatchResponse.parse(res)).toEqual(res);
  });
});

describe('ApplyPatchResponse', () => {
  it('parses an apply response carrying both hashes', () => {
    const res = {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json',
    };
    expect(ApplyPatchResponse.parse(res)).toEqual(res);
  });
});

describe('RevertPatchResponse', () => {
  it('parses a null success response', () => {
    expect(RevertPatchResponse.parse(null)).toBeNull();
  });

  it('rejects non-null payloads', () => {
    expect(() => RevertPatchResponse.parse({})).toThrow();
  });
});

describe('ListDetectedHarnessesResponse', () => {
  it('parses an empty array', () => {
    expect(ListDetectedHarnessesResponse.parse([])).toEqual([]);
  });

  it('parses a one-row response', () => {
    const row = {
      id: 'claude-code',
      detected: true,
      configPath: '/home/me/.claude/settings.json',
      telemetry: 'off',
      detectionMethod: 'config-dir',
      troveRegionPresent: false,
      adapterAvailable: true,
    };
    expect(ListDetectedHarnessesResponse.parse([row])).toEqual([row]);
  });

  it('parses a row where the harness is absent', () => {
    const row = {
      id: 'codex-cli',
      detected: false,
      configPath: null,
      telemetry: 'unknown',
      detectionMethod: null,
      troveRegionPresent: false,
      adapterAvailable: true,
    };
    expect(ListDetectedHarnessesResponse.parse([row])).toEqual([row]);
  });

  it('rejects a row with an unknown telemetry status', () => {
    expect(() =>
      ListDetectedHarnessesResponse.parse([
        {
          id: 'claude-code',
          detected: true,
          configPath: '/x',
          telemetry: 'partially-on',
          detectionMethod: 'config-dir',
          troveRegionPresent: false,
          adapterAvailable: true,
        },
      ]),
    ).toThrow();
  });
});

describe('Sprint 6 PR 1 — collector status surface', () => {
  it('exposes the three new command names', () => {
    expect(IpcCommandName.GetCollectorStatus).toBe('get_collector_status');
    expect(IpcCommandName.GetMetricsSnapshot).toBe('get_metrics_snapshot');
    expect(IpcCommandName.GetCollectorLogTail).toBe('get_collector_log_tail');
  });

  it('exposes the Tauri event channel names', () => {
    expect(TauriEventName.CollectorState).toBe('collector-state');
    expect(TauriEventName.MetricsSnapshot).toBe('metrics-snapshot');
    expect(TauriEventName.CollectorLog).toBe('collector-log');
  });

  it('parses GetCollectorStatusResponse', () => {
    const status = {
      state: { kind: 'idle' as const },
      logPath: '/tmp/trove/collector.log',
    };
    expect(GetCollectorStatusResponse.parse(status)).toEqual(status);
  });

  it('parses GetMetricsSnapshotResponse with null', () => {
    expect(GetMetricsSnapshotResponse.parse(null)).toBeNull();
  });

  it('parses GetMetricsSnapshotResponse with a populated snapshot', () => {
    const snap = {
      received: { spans: 3, metricPoints: 0, logRecords: 0 },
      sent: { spans: 3, metricPoints: 0, logRecords: 0 },
      lastSignalMsAgo: 1500,
      scrapedMsAgo: 100,
      unreachable: false,
      overallHealth: 'green' as const,
    };
    expect(GetMetricsSnapshotResponse.parse(snap)).toEqual(snap);
  });

  it('parses GetCollectorLogTailResponse', () => {
    const payload = {
      lines: [{ stream: 'stdout', line: 'startup' }],
      byteOffset: 8,
    };
    expect(GetCollectorLogTailResponse.parse(payload)).toEqual(payload);
  });
});

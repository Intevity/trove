import { describe, expect, it } from 'vitest';

import {
  ApplyOptions,
  AppState,
  Backend,
  BackendDraft,
  CollectorLogLineWire,
  CollectorLogTailResponse,
  CollectorRunState,
  CollectorStatus,
  ConflictState,
  DetectedHarness,
  DetectionMethod,
  HarnessConfig,
  HarnessId,
  IpcError,
  MetricsSnapshotWire,
  OverallHealth,
  PatchFormat,
  PatchPreview,
  PreviewStatus,
  SecretRef,
  SignalCounts,
  TelemetryStatus,
  TestExportResult,
  TrovePatch,
} from './schemas.js';

describe('HarnessId', () => {
  it('accepts each MVP harness identifier', () => {
    for (const id of [
      'claude-code',
      'gemini-cli',
      'codex-cli',
      'qwen-code',
      'opencode',
      'cursor-ide',
      'cursor-cli',
      'cline',
      'aider',
      'copilot-cli',
    ]) {
      expect(HarnessId.parse(id)).toBe(id);
    }
  });

  it('rejects an unknown harness identifier', () => {
    expect(() => HarnessId.parse('zed-ai')).toThrow();
  });
});

describe('SecretRef', () => {
  it('requires non-empty service and account', () => {
    expect(() => SecretRef.parse({ service: '', account: 'x' })).toThrow();
    expect(() => SecretRef.parse({ service: 'x', account: '' })).toThrow();
    expect(SecretRef.parse({ service: 'trove', account: 'signoz' })).toEqual({
      service: 'trove',
      account: 'signoz',
    });
  });
});

describe('Backend', () => {
  const ref: SecretRef = { service: 'trove', account: 'k' };

  it('parses a SigNoz backend', () => {
    const parsed = Backend.parse({ kind: 'signoz', region: 'us', ingestionKey: ref });
    expect(parsed.kind).toBe('signoz');
  });

  it('parses a Honeycomb backend', () => {
    const parsed = Backend.parse({ kind: 'honeycomb', team: ref, dataset: 'main' });
    expect(parsed.kind).toBe('honeycomb');
  });

  it('parses a Grafana Cloud backend', () => {
    const parsed = Backend.parse({
      kind: 'grafana-cloud',
      endpoint: 'https://grafana.example.com',
      auth: ref,
    });
    expect(parsed.kind).toBe('grafana-cloud');
  });

  it('parses a Datadog backend', () => {
    const parsed = Backend.parse({ kind: 'datadog', site: 'datadoghq.com', apiKey: ref });
    expect(parsed.kind).toBe('datadog');
  });

  it('parses an OTLP-generic backend with header refs', () => {
    const parsed = Backend.parse({
      kind: 'otlp-generic',
      endpoint: 'https://otel.example.com',
      protocol: 'grpc',
      headers: { 'x-api-key': ref },
    });
    expect(parsed.kind).toBe('otlp-generic');
  });

  it('parses an otelcol-passthrough backend', () => {
    const parsed = Backend.parse({
      kind: 'otelcol-passthrough',
      endpoint: 'https://collector.example.com',
    });
    expect(parsed.kind).toBe('otelcol-passthrough');
  });

  it('rejects an unknown backend kind', () => {
    expect(() => Backend.parse({ kind: 'newrelic' })).toThrow();
  });

  it('rejects a non-URL endpoint', () => {
    expect(() =>
      Backend.parse({
        kind: 'otelcol-passthrough',
        endpoint: 'not-a-url',
      }),
    ).toThrow();
  });
});

describe('BackendDraft', () => {
  it('parses each variant with raw string secrets', () => {
    expect(
      BackendDraft.parse({ kind: 'signoz', region: 'us', ingestionKey: 'raw-secret' }).kind,
    ).toBe('signoz');
    expect(
      BackendDraft.parse({ kind: 'honeycomb', team: 'raw-secret', dataset: 'main' }).kind,
    ).toBe('honeycomb');
    expect(
      BackendDraft.parse({
        kind: 'grafana-cloud',
        endpoint: 'https://grafana.example.com',
        auth: 'raw-secret',
      }).kind,
    ).toBe('grafana-cloud');
    expect(
      BackendDraft.parse({ kind: 'datadog', site: 'datadoghq.com', apiKey: 'raw-secret' }).kind,
    ).toBe('datadog');
    expect(
      BackendDraft.parse({
        kind: 'otlp-generic',
        endpoint: 'https://otel.example.com',
        protocol: 'http',
        headers: { 'x-api-key': 'raw-secret' },
      }).kind,
    ).toBe('otlp-generic');
    expect(
      BackendDraft.parse({
        kind: 'otelcol-passthrough',
        endpoint: 'https://collector.example.com',
      }).kind,
    ).toBe('otelcol-passthrough');
  });

  it('rejects an empty raw secret', () => {
    expect(() => BackendDraft.parse({ kind: 'signoz', region: 'us', ingestionKey: '' })).toThrow();
  });

  it('rejects an unknown draft kind', () => {
    expect(() => BackendDraft.parse({ kind: 'newrelic' })).toThrow();
  });
});

describe('PatchFormat', () => {
  it('matches the Rust Format enum variants', () => {
    for (const f of ['json', 'jsonc', 'toml', 'yaml']) {
      expect(PatchFormat.parse(f)).toBe(f);
    }
  });

  it('rejects formats Trove does not support', () => {
    expect(() => PatchFormat.parse('xml')).toThrow();
  });
});

describe('TrovePatch', () => {
  it('round-trips a captured patch metadata record', () => {
    const patch: TrovePatch = {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json',
    };
    expect(TrovePatch.parse(patch)).toEqual(patch);
  });

  it('rejects missing required fields', () => {
    expect(() => TrovePatch.parse({ managedBlockHash: 'x', format: 'json' })).toThrow();
    expect(() => TrovePatch.parse({ managedBlockHash: 'x', fileHashAtLastWrite: 'y' })).toThrow();
  });
});

describe('ConflictState', () => {
  it('matches the Rust ConflictState variants', () => {
    for (const s of ['clean', 'user-edited-outside', 'region-removed', 'region-conflict']) {
      expect(ConflictState.parse(s)).toBe(s);
    }
  });

  it('rejects unknown states', () => {
    expect(() => ConflictState.parse('mystery')).toThrow();
  });
});

describe('HarnessConfig', () => {
  const valid: HarnessConfig = {
    id: 'claude-code',
    enabled: true,
    configPath: '/Users/test/.claude/settings.json',
    lastPatchedAt: '2026-05-06T12:00:00.000Z',
    trovePatch: {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json',
    },
    options: { logUserPrompts: false, customAttributes: {} },
  };

  it('parses a complete config with explicit options', () => {
    expect(HarnessConfig.parse(valid)).toEqual(valid);
  });

  it('applies defaults to options when fields are omitted', () => {
    const parsed = HarnessConfig.parse({ ...valid, options: {} });
    expect(parsed.options.logUserPrompts).toBe(false);
    expect(parsed.options.customAttributes).toEqual({});
  });

  it('rejects an invalid lastPatchedAt timestamp', () => {
    expect(() => HarnessConfig.parse({ ...valid, lastPatchedAt: 'yesterday' })).toThrow();
  });

  it('rejects a missing trovePatch', () => {
    const without = { ...valid } as Partial<HarnessConfig>;
    delete without.trovePatch;
    expect(() => HarnessConfig.parse(without)).toThrow();
  });
});

describe('AppState', () => {
  it('parses a minimal v2 state with no backend and no harnesses', () => {
    const parsed = AppState.parse({
      schemaVersion: 2,
      backend: null,
      harnesses: [],
    });
    expect(parsed.schemaVersion).toBe(2);
    expect(parsed.backend).toBeNull();
    expect(parsed.harnesses).toEqual([]);
  });

  it('rejects the legacy schemaVersion 1', () => {
    expect(() =>
      AppState.parse({
        schemaVersion: 1,
        backend: null,
        harnesses: [],
      }),
    ).toThrow();
  });
});

describe('DetectionMethod', () => {
  it('matches the Rust DetectionMethod variants', () => {
    for (const m of ['path-binary', 'config-dir', 'app-bundle']) {
      expect(DetectionMethod.parse(m)).toBe(m);
    }
  });

  it('rejects unknown methods', () => {
    expect(() => DetectionMethod.parse('docker-image')).toThrow();
  });
});

describe('TelemetryStatus', () => {
  it('matches the Rust TelemetryStatus variants', () => {
    for (const s of ['on', 'off', 'unknown']) {
      expect(TelemetryStatus.parse(s)).toBe(s);
    }
  });

  it('rejects unknown statuses', () => {
    expect(() => TelemetryStatus.parse('partially-on')).toThrow();
  });
});

describe('DetectedHarness', () => {
  it('parses a fully detected row', () => {
    const row = {
      id: 'claude-code',
      detected: true,
      configPath: '/home/me/.claude/settings.json',
      telemetry: 'on',
      detectionMethod: 'config-dir',
      troveRegionPresent: false,
      adapterAvailable: true,
    };
    expect(DetectedHarness.parse(row)).toEqual(row);
  });

  it('parses an absent row with null fields', () => {
    const row = {
      id: 'qwen-code',
      detected: false,
      configPath: null,
      telemetry: 'unknown',
      detectionMethod: null,
      troveRegionPresent: false,
      adapterAvailable: true,
    };
    expect(DetectedHarness.parse(row)).toEqual(row);
  });

  it('rejects rows with snake_case keys', () => {
    expect(() =>
      DetectedHarness.parse({
        id: 'claude-code',
        detected: true,
        config_path: '/x',
        telemetry: 'on',
        detection_method: 'config-dir',
        trove_region_present: false,
        adapter_available: true,
      }),
    ).toThrow();
  });

  it('rejects rows missing the adapterAvailable field', () => {
    expect(() =>
      DetectedHarness.parse({
        id: 'cline',
        detected: false,
        configPath: null,
        telemetry: 'unknown',
        detectionMethod: null,
        troveRegionPresent: false,
      }),
    ).toThrow();
  });
});

describe('ApplyOptions', () => {
  it('parses an empty object using defaults', () => {
    const parsed = ApplyOptions.parse({});
    expect(parsed.logUserPrompts).toBe(false);
    expect(parsed.customAttributes).toEqual({});
  });

  it('parses an explicit options object', () => {
    const parsed = ApplyOptions.parse({
      logUserPrompts: true,
      customAttributes: { team: 'platform' },
    });
    expect(parsed.logUserPrompts).toBe(true);
    expect(parsed.customAttributes).toEqual({ team: 'platform' });
  });

  it('rejects non-string custom attribute values', () => {
    expect(() =>
      ApplyOptions.parse({
        customAttributes: { team: 123 as unknown as string },
      }),
    ).toThrow();
  });
});

describe('PreviewStatus', () => {
  it('matches the Rust PreviewStatus variants', () => {
    for (const s of ['fresh', 'idempotent', 'conflict']) {
      expect(PreviewStatus.parse(s)).toBe(s);
    }
  });

  it('rejects unknown statuses', () => {
    expect(() => PreviewStatus.parse('done')).toThrow();
  });
});

describe('PatchPreview', () => {
  it('parses a fresh-status preview', () => {
    const preview = {
      configPath: '/home/me/.claude/settings.json',
      format: 'json',
      before: '',
      after: '{"_trove":{"managed_keys":[],"hash":"x"}}',
      status: 'fresh',
    };
    expect(PatchPreview.parse(preview)).toEqual(preview);
  });

  it('rejects a preview with the wrong format', () => {
    expect(() =>
      PatchPreview.parse({
        configPath: '/x',
        format: 'xml',
        before: '',
        after: '',
        status: 'fresh',
      }),
    ).toThrow();
  });
});

describe('TestExportResult', () => {
  it('parses each of the three terminal statuses', () => {
    expect(TestExportResult.parse({ status: 'ok', detail: 'all good' }).status).toBe('ok');
    expect(TestExportResult.parse({ status: 'failed', detail: 'rejected' }).status).toBe('failed');
    expect(TestExportResult.parse({ status: 'timeout', detail: 'no log line' }).status).toBe(
      'timeout',
    );
  });

  it('rejects unknown statuses', () => {
    expect(() => TestExportResult.parse({ status: 'maybe', detail: '' })).toThrow();
  });
});

describe('IpcError', () => {
  it('parses a config-unparseable error', () => {
    const err = {
      kind: 'config-unparseable',
      path: '/tmp/x',
      reason: 'expected `:`',
    };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses a region-conflict error', () => {
    const err = {
      kind: 'region-conflict',
      path: '/home/me/.claude/settings.json',
    };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses a harness-not-detected error', () => {
    const err = { kind: 'harness-not-detected', id: 'gemini-cli' };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses a harness-not-implemented error', () => {
    const err = { kind: 'harness-not-implemented', id: 'codex-cli' };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses an io error', () => {
    const err = { kind: 'io', path: '/tmp/x', reason: 'permission denied' };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses an internal error', () => {
    const err = { kind: 'internal', reason: 'unexpected' };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('rejects an unknown kind', () => {
    expect(() => IpcError.parse({ kind: 'misc', reason: 'huh' })).toThrow();
  });
});

describe('CollectorRunState', () => {
  it('parses each kebab-case kind', () => {
    expect(CollectorRunState.parse({ kind: 'idle' })).toEqual({ kind: 'idle' });
    expect(CollectorRunState.parse({ kind: 'starting', pid: 1234 })).toEqual({
      kind: 'starting',
      pid: 1234,
    });
    expect(CollectorRunState.parse({ kind: 'running', pid: 1, restarts: 0 })).toEqual({
      kind: 'running',
      pid: 1,
      restarts: 0,
    });
    expect(CollectorRunState.parse({ kind: 'crashed', restarts: 2 })).toEqual({
      kind: 'crashed',
      restarts: 2,
    });
    expect(CollectorRunState.parse({ kind: 'stopping' })).toEqual({ kind: 'stopping' });
    expect(CollectorRunState.parse({ kind: 'stopped' })).toEqual({ kind: 'stopped' });
    expect(CollectorRunState.parse({ kind: 'failed', reason: 'spawn failed' })).toEqual({
      kind: 'failed',
      reason: 'spawn failed',
    });
  });

  it('rejects starting without pid', () => {
    expect(() => CollectorRunState.parse({ kind: 'starting' })).toThrow();
  });

  it('rejects an unknown kind', () => {
    expect(() => CollectorRunState.parse({ kind: 'paused' })).toThrow();
  });
});

describe('CollectorStatus', () => {
  it('round-trips a running state with logPath', () => {
    const status = {
      state: { kind: 'running', pid: 42, restarts: 0 } as const,
      logPath: '/tmp/trove/collector.log',
    };
    expect(CollectorStatus.parse(status)).toEqual(status);
  });

  it('rejects when logPath is missing', () => {
    expect(() => CollectorStatus.parse({ state: { kind: 'idle' } })).toThrow();
  });
});

describe('SignalCounts', () => {
  it('accepts non-negative integer counts', () => {
    expect(SignalCounts.parse({ spans: 1, metricPoints: 2, logRecords: 3 })).toEqual({
      spans: 1,
      metricPoints: 2,
      logRecords: 3,
    });
  });

  it('rejects negatives', () => {
    expect(() => SignalCounts.parse({ spans: -1, metricPoints: 0, logRecords: 0 })).toThrow();
  });
});

describe('OverallHealth', () => {
  it('accepts the three lowercase variants', () => {
    expect(OverallHealth.parse('green')).toBe('green');
    expect(OverallHealth.parse('amber')).toBe('amber');
    expect(OverallHealth.parse('red')).toBe('red');
  });

  it('rejects mixed case', () => {
    expect(() => OverallHealth.parse('Green')).toThrow();
  });
});

describe('MetricsSnapshotWire', () => {
  const empty = {
    received: { spans: 0, metricPoints: 0, logRecords: 0 },
    sent: { spans: 0, metricPoints: 0, logRecords: 0 },
    lastSignalMsAgo: null,
    scrapedMsAgo: 0,
    unreachable: false,
    overallHealth: 'green' as const,
  };

  it('round-trips a fresh snapshot', () => {
    expect(MetricsSnapshotWire.parse(empty)).toEqual(empty);
  });

  it('round-trips a snapshot with last-signal delta', () => {
    const snap = { ...empty, lastSignalMsAgo: 4_500, scrapedMsAgo: 250 };
    expect(MetricsSnapshotWire.parse(snap)).toEqual(snap);
  });

  it('marks unreachable + amber', () => {
    const snap = { ...empty, unreachable: true, overallHealth: 'amber' as const };
    expect(MetricsSnapshotWire.parse(snap)).toEqual(snap);
  });

  it('rejects negative scrapedMsAgo', () => {
    expect(() => MetricsSnapshotWire.parse({ ...empty, scrapedMsAgo: -1 })).toThrow();
  });
});

describe('CollectorLogTailResponse', () => {
  it('accepts a fresh-launch empty response', () => {
    expect(CollectorLogTailResponse.parse({ lines: [], byteOffset: 0 })).toEqual({
      lines: [],
      byteOffset: 0,
    });
  });

  it('accepts a response with multiple lines and a non-zero offset', () => {
    const payload = {
      lines: [
        { stream: 'stdout', line: 'starting OTLP receiver' },
        { stream: 'stderr', line: 'health endpoint up' },
      ],
      byteOffset: 1234,
    };
    expect(CollectorLogTailResponse.parse(payload)).toEqual(payload);
    // Same shape as the live event payload.
    expect(CollectorLogLineWire.parse(payload.lines[0])).toEqual(payload.lines[0]);
  });
});

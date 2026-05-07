import { describe, expect, it } from 'vitest';

import {
  AppState,
  Backend,
  ConflictState,
  DetectedHarness,
  DetectionMethod,
  HarnessConfig,
  HarnessId,
  IpcError,
  PatchFormat,
  SecretRef,
  TelemetryStatus,
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
      }),
    ).toThrow();
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

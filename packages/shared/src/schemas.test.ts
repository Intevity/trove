import { describe, expect, it } from 'vitest';
import { AppState, Backend, HarnessConfig, HarnessId, SecretRef } from './schemas.js';

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

  it('parses an OTLP-generic backend with header refs', () => {
    const parsed = Backend.parse({
      kind: 'otlp-generic',
      endpoint: 'https://otel.example.com',
      protocol: 'grpc',
      headers: { 'x-api-key': ref },
    });
    expect(parsed.kind).toBe('otlp-generic');
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

describe('HarnessConfig', () => {
  it('parses a complete config with default options', () => {
    const parsed = HarnessConfig.parse({
      id: 'claude-code',
      enabled: true,
      configPath: '/Users/test/.claude/settings.json',
      lastPatchedAt: '2026-05-06T12:00:00.000Z',
      trovePatchHash: 'sha256:abc',
      options: {},
    });
    expect(parsed.options.logUserPrompts).toBe(false);
    expect(parsed.options.customAttributes).toEqual({});
  });
});

describe('AppState', () => {
  it('parses a minimal state with no backend and no harnesses', () => {
    const parsed = AppState.parse({
      schemaVersion: 1,
      backend: null,
      harnesses: [],
    });
    expect(parsed.schemaVersion).toBe(1);
    expect(parsed.backend).toBeNull();
    expect(parsed.harnesses).toEqual([]);
  });

  it('rejects a state with the wrong schema version', () => {
    expect(() =>
      AppState.parse({
        schemaVersion: 2,
        backend: null,
        harnesses: [],
      }),
    ).toThrow();
  });
});

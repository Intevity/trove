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
  ConflictAction,
  ConflictPayload,
  ConflictResolutionOutcome,
  ConflictState,
  DetectedHarness,
  DetectionMethod,
  HarnessConfig,
  HarnessId,
  HarnessMapping,
  IpcError,
  MappingSource,
  MetricsSnapshotWire,
  TierAMetric,
  OverallHealth,
  PatchFormat,
  PatchPreview,
  PreviewStatus,
  SecretRef,
  SiblingPaths,
  SignalCounts,
  TelemetryStatus,
  TestExportResult,
  TrovePatch,
  UpdateMetadata,
} from './schemas.js';

describe('HarnessId', () => {
  it('accepts each supported harness identifier', () => {
    for (const id of [
      'claude-code',
      'claude-desktop',
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
    const parsed = Backend.parse({
      kind: 'signoz',
      endpoint: 'ingest.us.signoz.cloud:443',
      ingestionKey: ref,
    });
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
      BackendDraft.parse({
        kind: 'signoz',
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: 'raw-secret',
      }).kind,
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
    expect(() =>
      BackendDraft.parse({
        kind: 'signoz',
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: '',
      }),
    ).toThrow();
  });

  it('rejects an unknown draft kind', () => {
    expect(() => BackendDraft.parse({ kind: 'newrelic' })).toThrow();
  });
});

describe('PatchFormat', () => {
  it('matches the Rust Format enum variants', () => {
    for (const f of ['json', 'jsonc', 'toml', 'yaml', 'shell']) {
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
      lastWrittenRegionPayload: '{"env":{"OTEL_FOO":"bar"}}',
    };
    expect(TrovePatch.parse(patch)).toEqual(patch);
  });

  it('defaults lastWrittenRegionPayload to empty string when missing (schema v2 -> v3 migration)', () => {
    // Sprint 8 added lastWrittenRegionPayload alongside the v3 schema
    // bump. The Zod default() means a v2-shaped record (no payload key)
    // parses cleanly with an empty payload — the same migration the
    // Rust loader applies in app_state::load_from_dir.
    const v2Shape = {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: 'b'.repeat(64),
      format: 'json',
    };
    const parsed = TrovePatch.parse(v2Shape);
    expect(parsed.lastWrittenRegionPayload).toBe('');
  });

  it('rejects missing required fields', () => {
    expect(() => TrovePatch.parse({ managedBlockHash: 'x', format: 'json' })).toThrow();
    expect(() => TrovePatch.parse({ managedBlockHash: 'x', fileHashAtLastWrite: 'y' })).toThrow();
  });

  it('accepts an empty fileHashAtLastWrite for adapterless harnesses', () => {
    // Cline and Claude Desktop don't patch a host file, so their
    // `fileHashAtLastWrite` is persisted as the empty string. The
    // conflict UI keys off `lastWrittenRegionPayload` for those rows.
    const patch = {
      managedBlockHash: 'a'.repeat(64),
      fileHashAtLastWrite: '',
      format: 'json' as const,
      lastWrittenRegionPayload: '{"harness":"claude-desktop"}',
    };
    expect(TrovePatch.parse(patch).fileHashAtLastWrite).toBe('');
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
      lastWrittenRegionPayload: '{"env":{"OTEL_FOO":"bar"}}',
    },
    options: { customAttributes: {} },
  };

  it('parses a complete config with explicit options', () => {
    expect(HarnessConfig.parse(valid)).toEqual(valid);
  });

  it('applies defaults to options when fields are omitted', () => {
    const parsed = HarnessConfig.parse({ ...valid, options: {} });
    expect(parsed.options.customAttributes).toEqual({});
  });

  it('rejects an invalid lastPatchedAt timestamp', () => {
    expect(() => HarnessConfig.parse({ ...valid, lastPatchedAt: 'yesterday' })).toThrow();
  });

  it('accepts RFC 3339 timestamps with explicit offset (Rust chrono::to_rfc3339 format)', () => {
    // chrono::Utc::now().to_rfc3339() emits e.g. "2026-05-12T02:35:56.049769+00:00"
    // — with an explicit +00:00, not the Z suffix. The schema must accept it.
    const offsetForm: HarnessConfig = {
      ...valid,
      lastPatchedAt: '2026-05-12T02:35:56.049769+00:00',
    };
    expect(() => HarnessConfig.parse(offsetForm)).not.toThrow();

    const easternForm: HarnessConfig = {
      ...valid,
      lastPatchedAt: '2026-05-12T02:35:56.049-04:00',
    };
    expect(() => HarnessConfig.parse(easternForm)).not.toThrow();
  });

  it('rejects a missing trovePatch', () => {
    const without = { ...valid } as Partial<HarnessConfig>;
    delete without.trovePatch;
    expect(() => HarnessConfig.parse(without)).toThrow();
  });
});

describe('AppState', () => {
  const defaultIdentity = {
    enabled: false,
    source: 'auto' as const,
    name: '',
    email: '',
  };

  const emptyMappings = {
    schemaVersion: 2 as const,
    metrics: [],
    harnesses: [],
  };

  it('parses a minimal v10 state with default identity off and empty mappings', () => {
    const parsed = AppState.parse({
      schemaVersion: 10,
      backends: [],
      harnesses: [],
      autoUpdateEnabled: false,
      launchAtStartupEnabled: true,
      identity: defaultIdentity,
      mappings: emptyMappings,
    });
    expect(parsed.schemaVersion).toBe(10);
    expect(parsed.backends).toEqual([]);
    expect(parsed.harnesses).toEqual([]);
    expect(parsed.autoUpdateEnabled).toBe(false);
    expect(parsed.launchAtStartupEnabled).toBe(true);
    expect(parsed.identity.enabled).toBe(false);
    expect(parsed.identity.source).toBe('auto');
    expect(parsed.mappings.harnesses).toEqual([]);
  });

  it('parses a v10 state with autoUpdateEnabled true and identity tagging on', () => {
    const parsed = AppState.parse({
      schemaVersion: 10,
      backends: [],
      harnesses: [],
      autoUpdateEnabled: true,
      launchAtStartupEnabled: false,
      identity: {
        enabled: true,
        source: 'manual',
        name: 'Ada Lovelace',
        email: 'ada@example.com',
      },
      mappings: emptyMappings,
    });
    expect(parsed.autoUpdateEnabled).toBe(true);
    expect(parsed.launchAtStartupEnabled).toBe(false);
    expect(parsed.identity.enabled).toBe(true);
    expect(parsed.identity.source).toBe('manual');
    expect(parsed.identity.name).toBe('Ada Lovelace');
    expect(parsed.identity.email).toBe('ada@example.com');
  });

  it('parses a v10 state with populated mapping rows', () => {
    const parsed = AppState.parse({
      schemaVersion: 10,
      backends: [],
      harnesses: [],
      autoUpdateEnabled: false,
      launchAtStartupEnabled: true,
      identity: defaultIdentity,
      mappings: {
        schemaVersion: 2,
        metrics: [],
        harnesses: [
          {
            harnessId: 'gemini-cli',
            enabled: true,
            sources: [
              {
                kind: 'synthesize-from-native',
                nativeMetric: 'gemini_cli.session.count',
                targetMetric: 'events',
                attributeMap: {},
              },
            ],
            costOverrides: {},
          },
        ],
      },
    });
    expect(parsed.mappings.harnesses).toHaveLength(1);
    expect(parsed.mappings.harnesses[0].harnessId).toBe('gemini-cli');
    expect(parsed.mappings.harnesses[0].sources[0].kind).toBe('synthesize-from-native');
  });

  it('rejects when launchAtStartupEnabled is missing', () => {
    expect(() =>
      AppState.parse({
        schemaVersion: 10,
        backends: [],
        harnesses: [],
        autoUpdateEnabled: false,
        identity: defaultIdentity,
        mappings: emptyMappings,
      }),
    ).toThrow();
  });

  it('rejects when autoUpdateEnabled is missing', () => {
    expect(() =>
      AppState.parse({
        schemaVersion: 6,
        backend: null,
        harnesses: [],
        identity: defaultIdentity,
        mappings: emptyMappings,
      }),
    ).toThrow();
  });

  it('rejects when mappings is missing', () => {
    // The Rust loader auto-populates mappings on the wire via
    // serde(default = default_mapping_state). At the IPC boundary,
    // every payload from Rust includes the field — Zod enforcing it
    // catches accidental new-codepath omissions early.
    expect(() =>
      AppState.parse({
        schemaVersion: 6,
        backend: null,
        harnesses: [],
        autoUpdateEnabled: false,
        identity: defaultIdentity,
      }),
    ).toThrow();
  });

  it('rejects the legacy schemaVersion 1', () => {
    expect(() =>
      AppState.parse({
        schemaVersion: 1,
        backend: null,
        harnesses: [],
        autoUpdateEnabled: false,
        identity: defaultIdentity,
        mappings: emptyMappings,
      }),
    ).toThrow();
  });

  it('rejects v2..v9 wire payloads (Rust loader migrates them to v10 before IPC return)', () => {
    // Migrations live in `app_state::load_from_dir`; older payloads
    // should never appear at the IPC boundary, so the Zod literal
    // rejects them outright.
    for (const schemaVersion of [2, 3, 4, 5, 6, 7]) {
      expect(() =>
        AppState.parse({
          schemaVersion,
          backends: [],
          harnesses: [],
          autoUpdateEnabled: false,
          launchAtStartupEnabled: true,
          identity: defaultIdentity,
          mappings: emptyMappings,
        }),
      ).toThrow();
    }
  });
});

describe('TierAMetric and MappingState', () => {
  it('TierAMetric accepts every Tier A bucket name', () => {
    for (const m of ['events', 'tokens', 'cost.usd', 'turn.duration', 'errors']) {
      expect(TierAMetric.parse(m)).toBe(m);
    }
  });

  it('TierAMetric rejects unknown bucket names', () => {
    expect(() => TierAMetric.parse('throughput')).toThrow();
  });

  it('MappingSource round-trips a hook-rule row with null emit', () => {
    const parsed = MappingSource.parse({
      kind: 'hook-rule',
      when: 'beforeSubmitPrompt',
      emit: null,
    });
    expect(parsed.kind).toBe('hook-rule');
    if (parsed.kind === 'hook-rule') {
      expect(parsed.emit).toBeNull();
    }
  });

  it('MappingSource round-trips a synthesize-from-native row', () => {
    const parsed = MappingSource.parse({
      kind: 'synthesize-from-native',
      nativeMetric: 'claude_code.token.usage',
      targetMetric: 'tokens',
      attributeMap: { type: 'direction' },
    });
    expect(parsed.kind).toBe('synthesize-from-native');
    if (parsed.kind === 'synthesize-from-native') {
      expect(parsed.nativeMetric).toBe('claude_code.token.usage');
      expect(parsed.targetMetric).toBe('tokens');
    }
  });

  it('HarnessMapping fills costOverrides with empty default when missing', () => {
    const parsed = HarnessMapping.parse({
      harnessId: 'aider',
      enabled: true,
      sources: [],
    });
    expect(parsed.costOverrides).toEqual({});
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
    expect(parsed.customAttributes).toEqual({});
  });

  it('parses an explicit options object', () => {
    const parsed = ApplyOptions.parse({
      customAttributes: { team: 'platform' },
    });
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

  it('parses an updater-check-failed error', () => {
    const err = { kind: 'updater-check-failed', reason: 'network timeout' };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses a region-conflict-detected error carrying a 3-way payload', () => {
    const err = {
      kind: 'region-conflict-detected',
      conflict: {
        configPath: '/home/me/.claude/settings.json',
        format: 'json',
        originalRegionPayload: '{"a":1}',
        currentRegionPayload: '{"a":2}',
        theirsRegionPayload: '{"a":3}',
        fileBefore: '{"a":2}',
        fileAfterIfTakingTheirs: '{"a":3}',
      },
    };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('parses a region-conflict-detected error in 2-way orphan-block mode', () => {
    const err = {
      kind: 'region-conflict-detected',
      conflict: {
        configPath: '/home/me/.claude/settings.json',
        format: 'json',
        originalRegionPayload: null,
        currentRegionPayload: '{"a":2}',
        theirsRegionPayload: '{"a":3}',
        fileBefore: '{"a":2}',
        fileAfterIfTakingTheirs: '{"a":3}',
      },
    };
    expect(IpcError.parse(err)).toEqual(err);
  });

  it('rejects an unknown kind', () => {
    expect(() => IpcError.parse({ kind: 'misc', reason: 'huh' })).toThrow();
  });
});

describe('ConflictPayload', () => {
  const sample = {
    configPath: '/tmp/.claude/settings.json',
    format: 'json' as const,
    originalRegionPayload: '{"a":1}',
    currentRegionPayload: '{"a":2}',
    theirsRegionPayload: '{"a":3}',
    fileBefore: '{"a":2}',
    fileAfterIfTakingTheirs: '{"a":3}',
  };

  it('round-trips a 3-way payload', () => {
    expect(ConflictPayload.parse(sample)).toEqual(sample);
  });

  it('accepts null originalRegionPayload (orphan-block 2-way fallback)', () => {
    const orphan = { ...sample, originalRegionPayload: null };
    expect(ConflictPayload.parse(orphan)).toEqual(orphan);
  });

  it('rejects when configPath is missing', () => {
    const broken = { ...sample } as Partial<ConflictPayload>;
    delete broken.configPath;
    expect(() => ConflictPayload.parse(broken)).toThrow();
  });
});

describe('ConflictAction', () => {
  it('parses a keep-mine action with no payload', () => {
    expect(ConflictAction.parse({ kind: 'keep-mine' })).toEqual({ kind: 'keep-mine' });
  });

  it('parses a take-theirs action carrying ApplyOptions', () => {
    const action = {
      kind: 'take-theirs' as const,
      options: { customAttributes: {} },
    };
    expect(ConflictAction.parse(action)).toEqual(action);
  });

  it('parses a merge-manually action carrying ApplyOptions', () => {
    const action = {
      kind: 'merge-manually' as const,
      options: { customAttributes: { team: 'platform' } },
    };
    expect(ConflictAction.parse(action)).toEqual(action);
  });

  it('rejects an unknown kind', () => {
    expect(() => ConflictAction.parse({ kind: 'rebase-onto-trove' })).toThrow();
  });
});

describe('SiblingPaths', () => {
  it('round-trips a triple of paths', () => {
    const paths = {
      original: '/tmp/x.trove.original',
      theirs: '/tmp/x.trove.theirs',
      host: '/tmp/x',
    };
    expect(SiblingPaths.parse(paths)).toEqual(paths);
  });

  it('rejects when host is missing', () => {
    expect(() => SiblingPaths.parse({ original: '/tmp/o', theirs: '/tmp/t' })).toThrow();
  });
});

describe('ConflictResolutionOutcome', () => {
  const patch: TrovePatch = {
    managedBlockHash: 'a'.repeat(64),
    fileHashAtLastWrite: 'b'.repeat(64),
    format: 'json',
    lastWrittenRegionPayload: '{"a":1}',
  };

  it('parses an applied outcome with a fresh patch', () => {
    const outcome = { status: 'applied' as const, patch };
    expect(ConflictResolutionOutcome.parse(outcome)).toEqual(outcome);
  });

  it('parses a marked-mine outcome with the user-baselined patch', () => {
    const outcome = { status: 'marked-mine' as const, patch };
    expect(ConflictResolutionOutcome.parse(outcome)).toEqual(outcome);
  });

  it('parses a merge-deferred outcome carrying sibling paths', () => {
    const outcome = {
      status: 'merge-deferred' as const,
      siblingPaths: {
        original: '/tmp/x.trove.original',
        theirs: '/tmp/x.trove.theirs',
        host: '/tmp/x',
      },
    };
    expect(ConflictResolutionOutcome.parse(outcome)).toEqual(outcome);
  });

  it('rejects an unknown status', () => {
    expect(() => ConflictResolutionOutcome.parse({ status: 'rolled-back', patch })).toThrow();
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
    diagObservations: {},
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

describe('UpdateMetadata', () => {
  it('parses an "update available" response', () => {
    const meta = { available: true, version: '0.6.1', current: '0.6.0' };
    expect(UpdateMetadata.parse(meta)).toEqual(meta);
  });

  it('parses an "up to date" response with null version', () => {
    const meta = { available: false, version: null, current: '0.6.0' };
    expect(UpdateMetadata.parse(meta)).toEqual(meta);
  });

  it('rejects when current is missing', () => {
    expect(() => UpdateMetadata.parse({ available: false, version: null })).toThrow();
  });
});

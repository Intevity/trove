import { describe, expect, it } from 'vitest';

import { PRESETS, presetMetadataFor, type PresetKind } from './index.js';

describe('PRESETS', () => {
  it('exposes one entry per BackendDraft kind', () => {
    const expected: PresetKind[] = [
      'signoz',
      'honeycomb',
      'grafana-cloud',
      'datadog',
      'otlp-generic',
      'otelcol-passthrough',
      'new-relic',
      'splunk-observability',
      'dynatrace',
      'elastic',
      'opensearch',
      'openobserve',
      'clickstack',
      'chronosphere',
      'sentry',
    ];
    expect(PRESETS.map((p) => p.kind).sort()).toEqual([...expected].sort());
  });

  it('marks SigNoz as the recommended default and lists it first', () => {
    expect(PRESETS[0]?.kind).toBe('signoz');
    expect(PRESETS[0]?.recommended).toBe(true);
  });

  it('marks every other preset as not recommended', () => {
    const others = PRESETS.filter((p) => p.kind !== 'signoz');
    for (const preset of others) {
      expect(preset.recommended).toBe(false);
    }
  });

  it('every entry has a non-empty label and description', () => {
    for (const preset of PRESETS) {
      expect(preset.label.length).toBeGreaterThan(0);
      expect(preset.description.length).toBeGreaterThan(0);
    }
  });
});

describe('presetMetadataFor', () => {
  it('returns the entry for each known kind', () => {
    for (const preset of PRESETS) {
      expect(presetMetadataFor(preset.kind)).toEqual(preset);
    }
  });

  it('throws for an unknown kind', () => {
    expect(() => presetMetadataFor('newrelic' as PresetKind)).toThrow(/unknown preset kind/);
  });
});
